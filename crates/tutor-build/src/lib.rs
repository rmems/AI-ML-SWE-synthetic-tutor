//! Dataset assembly for issue
//! [#8](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/8): the
//! pipeline that turns verified dialogue rows into a split, exportable
//! dataset release.
//!
//! One [`build`] call runs five stages, each implemented in its own module:
//!
//! 1. **Dialogue checks** ([`checks`]) — JSON Schema + policy validation via
//!    `tutor-schema` (hint ladder, no final-answer leak before the closing
//!    turn, single primary misconception, provenance presence), plus a
//!    cross-check against the verified problems file when `--problems` is
//!    given: the referenced problem must exist, carry a verification record,
//!    match the row's topic, and have its answer confirmed in the closing
//!    turn.
//! 2. **Dedup** ([`dedup`]) — exact and near-duplicate rejection on the
//!    normalized problem text and dialogue text.
//! 3. **Decontamination** ([`decontam`]) — exact and near matching against
//!    every record under `evals/heldout/` (or `--heldout`). The held-out set
//!    is input-only: nothing here generates against it or tunes to it.
//! 4. **Splits** ([`splits`]) — deterministic SHA-256 hash assignment to
//!    train / validation / test. With `--previous-manifest` an already
//!    assigned row or problem never moves; new rows fill the next release.
//! 5. **Export** ([`export`]) — standard multi-turn chat JSONL per split
//!    plus the provenance fields the issue requires.
//!
//! Rows that fail any stage go to `rejected/rejected.jsonl` with a stable
//! snake_case reason code — never silently dropped. The split manifest is
//! written last, atomically, and records per-split/per-topic/per-taxonomy
//! counts, per-file SHA-256 and byte length, input hashes, and the tool
//! version; its own SHA-256 is printed and stored in
//! `manifest.json.sha256` so the generation run log (#7) can record it.
//!
//! Assembling `DialogueRow`s from verified problems + student attempts needs
//! tutor-turn content (a teacher model or new templates) and is not part of
//! this crate yet — the input is already-assembled dialogue rows. Hugging
//! Face export is also deferred to #21; the output layout
//! (`train.jsonl`/`validation.jsonl`/`test.jsonl` + manifest) is what a
//! later publish step consumes.

pub mod checks;
pub mod decontam;
pub mod dedup;
pub mod export;
pub mod input;
pub mod manifest;
pub mod splits;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use checks::CheckedRow;
use decontam::HeldOut;
use dedup::Fingerprinted;
use manifest::FileMeta;
use splits::Split;

/// Largest single row file or JSONL line read: bound memory use on bad
/// input, same bound as `tutor-validate`.
pub(crate) const MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;

/// The pipeline stage a [`RejectedRow`] came from, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Stage 1: schema, policy, and provenance checks.
    DialogueChecks,
    /// Stage 2: exact and near-duplicate detection.
    Dedup,
    /// Stage 3: decontamination against the held-out set.
    Decontamination,
}

impl Stage {
    /// The stable snake_case stage name used in rejection records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DialogueChecks => "dialogue_checks",
            Self::Dedup => "dedup",
            Self::Decontamination => "decontamination",
        }
    }
}

/// A row dropped by one pipeline stage. Serialized to
/// `rejected/rejected.jsonl`; the `code` is a stable snake_case reason.
#[derive(Debug, Serialize)]
pub struct RejectedRow {
    /// Input file the row came from.
    pub file: String,
    /// One-based line number for JSONL inputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// `row_id` when the row parsed far enough to have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_id: Option<String>,
    /// Stage that rejected the row.
    pub stage: Stage,
    /// Stable snake_case reason code.
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
    /// The raw row JSON when it parsed, for contrast with accepted rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<serde_json::Value>,
}

/// What `tutor-build build` needs: everything that produces one release.
#[derive(Debug)]
pub struct BuildConfig {
    /// Dataset family name; also the default output folder
    /// `datasets/<family>/`.
    pub family: String,
    /// Release identifier recorded in the manifest (e.g. `v1`,
    /// `2026-11-27`). Split assignments are frozen per release.
    pub release: String,
    /// Dialogue row files (`.json` one row per file, `.jsonl` one per line).
    pub rows: Vec<PathBuf>,
    /// Verified `problems.jsonl` grounding generated rows. Required whenever
    /// generated rows are among the inputs.
    pub problems: Option<PathBuf>,
    /// Directory of held-out eval records used for decontamination only.
    pub heldout_dir: PathBuf,
    /// Output directory for splits, `rejected/`, and the manifest.
    pub out_dir: PathBuf,
    /// A manifest written by an earlier `tutor-build` run for the same
    /// family; its assignments freeze rows that were already split.
    pub previous_manifest: Option<PathBuf>,
}

/// Summary of a finished build, printed by the CLI.
#[derive(Debug)]
pub struct BuildReport {
    /// Rows that passed every stage and were exported.
    pub accepted: usize,
    /// Rows rejected with a reason code at some stage.
    pub rejected: usize,
    /// Accepted row count per split name (`train`, `validation`, `test`).
    pub split_counts: BTreeMap<String, usize>,
    /// Output directory the artifacts were written to.
    pub out_dir: PathBuf,
    /// Path of the split manifest (written last, atomically).
    pub manifest_path: PathBuf,
    /// SHA-256 of the split manifest, also stored next to it as
    /// `manifest.json.sha256` for the generation run log (#7).
    pub manifest_sha256: String,
}

/// Why a build failed outright (as opposed to per-row rejections).
#[derive(Debug, Error)]
pub enum BuildError {
    /// An input file could not be read or is not a `.json`/`.jsonl` file.
    #[error("{path}: {reason}")]
    Input {
        /// The offending path.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The problems file is malformed at a specific line.
    #[error("{path}:{line}: {reason}")]
    ProblemLine {
        /// The problems file.
        path: PathBuf,
        /// One-based line number.
        line: usize,
        /// What is wrong with the problem record.
        reason: String,
    },
    /// The held-out directory could not be read.
    #[error("held-out directory {path}: {reason}")]
    HeldOut {
        /// The `--heldout` directory.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The `--previous-manifest` file is not usable.
    #[error("previous manifest {path}: {reason}")]
    PreviousManifest {
        /// The `--previous-manifest` file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The generated dialogue JSON Schema does not compile.
    #[error("generated dialogue schema does not compile: {0}")]
    SchemaCompile(String),
    /// Writing an output artifact failed.
    #[error("cannot write {path}: {reason}")]
    Output {
        /// The artifact path.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
}

impl BuildError {
    pub(crate) fn input(path: &Path, reason: impl Into<String>) -> Self {
        Self::Input {
            path: path.to_owned(),
            reason: reason.into(),
        }
    }

    pub(crate) fn output(path: &Path, reason: impl Into<String>) -> Self {
        Self::Output {
            path: path.to_owned(),
            reason: reason.into(),
        }
    }

    pub(crate) fn heldout(path: &Path, reason: impl Into<String>) -> Self {
        Self::HeldOut {
            path: path.to_owned(),
            reason: reason.into(),
        }
    }

    pub(crate) fn io(path: &Path, error: std::io::Error) -> Self {
        Self::input(path, error.to_string())
    }
}

/// Run the full pipeline: check, dedup, decontaminate, split, export, and
/// write the split manifest for one family release.
///
/// # Errors
/// Returns [`BuildError`] for unreadable inputs, a malformed problems or
/// previous-manifest file, an unusable held-out directory, or a failed
/// output write. Per-row failures never abort the build; they are recorded
/// in `rejected/rejected.jsonl`.
pub fn build(config: &BuildConfig) -> Result<BuildReport, BuildError> {
    let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema())
        .map_err(|error| BuildError::SchemaCompile(error.to_string()))?;
    let problems = config
        .problems
        .as_deref()
        .map(input::load_problems)
        .transpose()?;
    let heldout = decontam::load(&config.heldout_dir)?;
    let rows = input::load_rows(&config.rows)?;

    let (checked, mut rejected) = run_checks(&rows, problems.as_ref(), &validator);
    let kept = dedup::run(checked, &mut rejected);
    let clean = decontam::run(kept, &heldout, &mut rejected);

    let mut assigner =
        splits::Assigner::load_previous(config.previous_manifest.as_deref(), &config.family)?;
    let mut by_split: BTreeMap<Split, Vec<Fingerprinted>> = BTreeMap::new();
    for row in clean {
        let split = assigner.assign(&row.checked.row.row_id, &row.checked.row.problem_id);
        by_split.entry(split).or_default().push(row);
    }

    let inputs = collect_inputs(config, &heldout)?;
    let outputs = write_outputs(config, &by_split, &rejected, &assigner, inputs)?;

    Ok(BuildReport {
        accepted: by_split.values().map(Vec::len).sum(),
        rejected: rejected.len(),
        split_counts: outputs.split_counts,
        out_dir: config.out_dir.clone(),
        manifest_path: outputs.manifest_path,
        manifest_sha256: outputs.manifest_sha256,
    })
}

/// Stage 1 for every input line, preserving input order.
fn run_checks(
    rows: &[input::InputLine],
    problems: Option<&BTreeMap<String, tutor_schema::Problem>>,
    validator: &jsonschema::Validator,
) -> (Vec<CheckedRow>, Vec<RejectedRow>) {
    let mut checked = Vec::new();
    let mut rejected = Vec::new();
    let mut seen_row_ids = HashSet::new();
    for line in rows {
        match checks::check(line, problems, validator, &mut seen_row_ids) {
            Ok(row) => checked.push(row),
            Err(rejection) => rejected.push(*rejection),
        }
    }
    (checked, rejected)
}

/// SHA-256 + byte length of every input file for the manifest.
fn collect_inputs(config: &BuildConfig, heldout: &HeldOut) -> Result<manifest::Inputs, BuildError> {
    let file_meta = |path: &Path| FileMeta::of(path).map_err(|e| BuildError::io(path, e));
    Ok(manifest::Inputs {
        rows: config
            .rows
            .iter()
            .map(|p| file_meta(p))
            .collect::<Result<_, _>>()?,
        problems: config
            .problems
            .iter()
            .map(|p| file_meta(p))
            .collect::<Result<_, _>>()?,
        heldout: heldout
            .files
            .iter()
            .map(|p| file_meta(p))
            .collect::<Result<_, _>>()?,
    })
}

struct Outputs {
    split_counts: BTreeMap<String, usize>,
    manifest_path: PathBuf,
    manifest_sha256: String,
}

/// Write split JSONL files, `rejected/rejected.jsonl`, then the manifest
/// last (atomically), then its `.sha256` sidecar.
fn write_outputs(
    config: &BuildConfig,
    by_split: &BTreeMap<Split, Vec<Fingerprinted>>,
    rejected: &[RejectedRow],
    assigner: &splits::Assigner,
    inputs: manifest::Inputs,
) -> Result<Outputs, BuildError> {
    std::fs::create_dir_all(&config.out_dir)
        .map_err(|e| BuildError::output(&config.out_dir, e.to_string()))?;
    let rejected_dir = config.out_dir.join("rejected");
    std::fs::create_dir_all(&rejected_dir)
        .map_err(|e| BuildError::output(&rejected_dir, e.to_string()))?;

    let mut artifacts: BTreeMap<String, manifest::Artifact> = BTreeMap::new();
    let mut split_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut topics: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut taxonomy: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for split in Split::ALL {
        let rows = by_split
            .get(&split)
            .map_or(&[] as &[Fingerprinted], Vec::as_slice);
        let mut export_rows: Vec<_> = rows.iter().map(|r| export::to_export(r, split)).collect();
        export_rows.sort_by(|a, b| a.row_id.cmp(&b.row_id));
        count_rows(rows, split, &mut topics, &mut taxonomy);
        let name = format!("{}.jsonl", split.as_str());
        let meta = write_jsonl(
            &config.out_dir.join(&name),
            export_rows.iter().map(serde_json::to_value),
        )?;
        artifacts.insert(
            name,
            manifest::Artifact {
                sha256: meta.sha256,
                bytes: meta.bytes,
                rows: export_rows.len(),
            },
        );
        split_counts.insert(split.as_str().to_owned(), export_rows.len());
    }

    let rejected_meta = write_jsonl(
        &rejected_dir.join("rejected.jsonl"),
        rejected.iter().map(serde_json::to_value),
    )?;
    artifacts.insert(
        "rejected/rejected.jsonl".to_owned(),
        manifest::Artifact {
            sha256: rejected_meta.sha256,
            bytes: rejected_meta.bytes,
            rows: rejected.len(),
        },
    );

    let document = manifest::SplitManifest::new(
        config,
        inputs,
        artifacts,
        manifest::Counts::of(split_counts.clone(), topics, taxonomy, rejected),
        assigner.assignments(),
    );
    let manifest_text = serde_json::to_string_pretty(&document)
        .map_err(|e| BuildError::output(&config.out_dir, e.to_string()))?;
    let manifest_path = config.out_dir.join("manifest.json");
    manifest::write_atomic(&manifest_path, manifest_text.as_bytes())
        .map_err(|e| BuildError::output(&manifest_path, e.to_string()))?;
    let digest = format!("{:x}", Sha256::digest(manifest_text.as_bytes()));
    let sidecar = config.out_dir.join("manifest.json.sha256");
    manifest::write_atomic(&sidecar, format!("{digest}  manifest.json\n").as_bytes())
        .map_err(|e| BuildError::output(&sidecar, e.to_string()))?;

    Ok(Outputs {
        split_counts,
        manifest_path,
        manifest_sha256: digest,
    })
}

/// Tally accepted rows per topic and per taxonomy id (`none` for
/// confirmation rows).
fn count_rows(
    rows: &[Fingerprinted],
    split: Split,
    topics: &mut BTreeMap<String, BTreeMap<String, usize>>,
    taxonomy: &mut BTreeMap<String, BTreeMap<String, usize>>,
) {
    for row in rows {
        *topics
            .entry(row.checked.row.topic.clone())
            .or_default()
            .entry(split.as_str().to_owned())
            .or_insert(0) += 1;
        *taxonomy
            .entry(export::taxonomy_id(&row.checked))
            .or_default()
            .entry(split.as_str().to_owned())
            .or_insert(0) += 1;
    }
}

/// Serialize `rows` as JSONL, write atomically, and return its SHA-256 and
/// byte length.
fn write_jsonl(
    path: &Path,
    rows: impl Iterator<Item = serde_json::Result<serde_json::Value>>,
) -> Result<manifest::FileDigest, BuildError> {
    let mut text = String::new();
    let mut hasher = Sha256::new();
    for row in rows {
        let value = row.map_err(|e| BuildError::output(path, e.to_string()))?;
        let mut line =
            serde_json::to_string(&value).map_err(|e| BuildError::output(path, e.to_string()))?;
        line.push('\n');
        hasher.update(line.as_bytes());
        text.push_str(&line);
    }
    manifest::write_atomic(path, text.as_bytes())
        .map_err(|e| BuildError::output(path, e.to_string()))?;
    Ok(manifest::FileDigest {
        sha256: format!("{:x}", hasher.finalize()),
        bytes: text.len() as u64,
    })
}
