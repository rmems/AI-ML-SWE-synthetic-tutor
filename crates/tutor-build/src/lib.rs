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
//! 2. **Decontamination** ([`decontam`]) — exact and near matching against
//!    every record under `evals/heldout/` (or `--heldout`). The held-out set
//!    is input-only: nothing here generates against it or tunes to it.
//!    Decontamination runs before dedup so a contaminated row cannot keep a
//!    clean near-duplicate of itself out of the release.
//! 3. **Dedup** ([`dedup`]) — exact and near-duplicate rejection on the
//!    normalized problem text and dialogue text.
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
use std::io::Write as _;
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
    /// Stage 2: decontamination against the held-out set.
    Decontamination,
    /// Stage 3: exact and near-duplicate detection.
    Dedup,
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
    /// A frozen row assignment cannot be satisfied together with its
    /// problem's split.
    #[error("conflicting frozen split assignments: {0}")]
    FrozenConflict(String),
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
    let fingerprinted: Vec<Fingerprinted> = checked.into_iter().map(dedup::fingerprinted).collect();
    let clean = decontam::run(fingerprinted, &heldout, &mut rejected);
    let kept = dedup::run(clean, &mut rejected);

    let mut assigner =
        splits::Assigner::load_previous(config.previous_manifest.as_deref(), &config.family)?;
    let by_split = assign_splits(&mut assigner, kept);
    if !assigner.conflicts().is_empty() {
        return Err(BuildError::FrozenConflict(assigner.conflicts().join("; ")));
    }

    let inputs = collect_inputs(config, &heldout)?;
    let outputs = write_outputs(ReleaseBundle {
        config,
        by_split: &by_split,
        rejected: &rejected,
        assigner: &assigner,
        inputs,
    })?;

    Ok(BuildReport {
        accepted: by_split.values().map(Vec::len).sum(),
        rejected: rejected.len(),
        split_counts: outputs.split_counts,
        out_dir: config.out_dir.clone(),
        manifest_path: outputs.manifest_path,
        manifest_sha256: outputs.manifest_sha256,
    })
}

/// Group kept rows by split. Rows a previous release froze are assigned
/// first, so their frozen splits settle their problems before any new row
/// draws one; the outcome does not depend on input order. Rows keep their
/// input order within each split.
fn assign_splits(
    assigner: &mut splits::Assigner,
    kept: Vec<Fingerprinted>,
) -> BTreeMap<Split, Vec<Fingerprinted>> {
    let order = assigner.assignment_order(&kept, |row| row.checked.row.row_id.as_str());
    let mut splits = vec![Split::Train; kept.len()];
    for index in order {
        let row = &kept[index].checked.row;
        splits[index] = assigner.assign(&row.row_id, &row.problem_id);
    }
    let mut by_split: BTreeMap<Split, Vec<Fingerprinted>> = BTreeMap::new();
    for (row, split) in kept.into_iter().zip(splits) {
        by_split.entry(split).or_default().push(row);
    }
    by_split
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
        previous_manifest: config
            .previous_manifest
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

/// Everything `write_outputs` needs to publish one release bundle.
struct ReleaseBundle<'a> {
    config: &'a BuildConfig,
    by_split: &'a BTreeMap<Split, Vec<Fingerprinted>>,
    rejected: &'a [RejectedRow],
    assigner: &'a splits::Assigner,
    inputs: manifest::Inputs,
}

/// Accepted-row tallies for the manifest's `counts` section.
#[derive(Default)]
struct SplitCounts {
    splits: BTreeMap<String, usize>,
    topics: BTreeMap<String, BTreeMap<String, usize>>,
    taxonomy: BTreeMap<String, BTreeMap<String, usize>>,
}

/// Stage split JSONL files and `rejected/rejected.jsonl`, then retract any
/// previous manifest, move the staged files into place, and publish the
/// manifest last (atomically), then its `.sha256` sidecar.
fn write_outputs(bundle: ReleaseBundle<'_>) -> Result<Outputs, BuildError> {
    let _lock = prepare_out_dir(bundle.config)?;
    let out_dir = &bundle.config.out_dir;
    let staged_result = stage_artifacts(&bundle);
    let (artifacts, counts) = match staged_result {
        Ok(staged) => staged,
        Err(error) => {
            discard_staged(out_dir);
            return Err(error);
        }
    };
    // Every artifact is staged: only now retract the previous release's
    // manifest and move the new files into place, so a failed staging
    // write leaves the previous release (files and manifest) intact.
    retract_previous_manifest(out_dir)?;
    for name in artifacts.keys() {
        let path = out_dir.join(name);
        std::fs::rename(staged(&path), &path)
            .map_err(|e| BuildError::output(&path, e.to_string()))?;
    }
    write_manifest(bundle, artifacts, counts)
}

/// Write every split export and the rejection log to their staging names.
fn stage_artifacts(
    bundle: &ReleaseBundle<'_>,
) -> Result<(BTreeMap<String, manifest::Artifact>, SplitCounts), BuildError> {
    let (mut artifacts, counts) = write_split_files(bundle)?;
    let rejected_meta = write_jsonl(
        &staged(&bundle.config.out_dir.join(REJECTED_ARTIFACT)),
        bundle.rejected.iter().map(serde_json::to_value),
    )?;
    artifacts.insert(
        REJECTED_ARTIFACT.to_owned(),
        manifest::Artifact {
            sha256: rejected_meta.sha256,
            bytes: rejected_meta.bytes,
            rows: bundle.rejected.len(),
        },
    );
    Ok((artifacts, counts))
}

/// The rejection log, relative to the output directory.
const REJECTED_ARTIFACT: &str = "rejected/rejected.jsonl";

/// The staging name an artifact is written to before it is moved into place.
fn staged(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".staging");
    PathBuf::from(name)
}

/// Best-effort removal of staged artifacts after a failed build.
fn discard_staged(out_dir: &Path) {
    let names = Split::ALL
        .iter()
        .map(|split| format!("{}.jsonl", split.as_str()))
        .chain([REJECTED_ARTIFACT.to_owned()]);
    for name in names {
        let _ = std::fs::remove_file(staged(&out_dir.join(name)));
    }
}

/// Create the output directory and claim it for this build: refuse a
/// `manifest.json` written by another producer, refuse overlap with the
/// held-out input tree, and hold a `.tutor-build.lock` file so a second
/// concurrent build of the same directory fails fast instead of
/// interleaving artifacts. The lock is advisory: a crashed build leaves it
/// behind and it must be removed by hand.
fn prepare_out_dir(config: &BuildConfig) -> Result<LockGuard, BuildError> {
    std::fs::create_dir_all(&config.out_dir)
        .map_err(|e| BuildError::output(&config.out_dir, e.to_string()))?;
    let rejected_dir = config.out_dir.join("rejected");
    std::fs::create_dir_all(&rejected_dir)
        .map_err(|e| BuildError::output(&rejected_dir, e.to_string()))?;

    let out = std::fs::canonicalize(&config.out_dir)
        .map_err(|e| BuildError::output(&config.out_dir, e.to_string()))?;
    let heldout = std::fs::canonicalize(&config.heldout_dir)
        .map_err(|e| BuildError::output(&config.heldout_dir, e.to_string()))?;
    if out.starts_with(&heldout) || heldout.starts_with(&out) {
        return Err(BuildError::output(
            &config.out_dir,
            "output directory overlaps the held-out tree, which is input-only",
        ));
    }

    let manifest_path = config.out_dir.join("manifest.json");
    if let Ok(text) = std::fs::read_to_string(&manifest_path) {
        let owned = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|doc| doc["tool"]["name"].as_str().map(str::to_owned))
            .is_some_and(|name| name == "tutor-build");
        if !owned {
            return Err(BuildError::output(
                &manifest_path,
                "exists and was not written by tutor-build; refusing to overwrite it",
            ));
        }
    }

    ensure_previous_manifest_survives(config, &manifest_path)?;

    let lock_path = config.out_dir.join(".tutor-build.lock");
    match std::fs::File::create_new(&lock_path) {
        Ok(_) => Ok(LockGuard(lock_path)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(BuildError::output(
            &lock_path,
            "another tutor-build run may be writing here; remove the stale lock file if none is",
        )),
        Err(error) => Err(BuildError::output(&lock_path, error.to_string())),
    }
}

/// Refuse a `--previous-manifest` that is the manifest this build is about
/// to replace: the new release would record the old release's digest at a
/// path that then holds only the new manifest, so its provenance could
/// never be verified. Keep the previous release's manifest at its own path.
fn ensure_previous_manifest_survives(
    config: &BuildConfig,
    manifest_path: &Path,
) -> Result<(), BuildError> {
    let Some(previous) = config.previous_manifest.as_deref() else {
        return Ok(());
    };
    let same = match (
        std::fs::canonicalize(previous),
        std::fs::canonicalize(manifest_path),
    ) {
        (Ok(previous), Ok(target)) => previous == target,
        _ => false,
    };
    if same {
        return Err(BuildError::output(
            manifest_path,
            "is also --previous-manifest; this build would overwrite the manifest it records as \
             its frozen input, so copy the previous release's manifest to its own path first",
        ));
    }
    Ok(())
}

/// Remove a previous release's manifest and sidecar once the new artifacts
/// are staged and before any of them is moved into place. The manifest is
/// the commit record of a bundle, so while the new files replace the old
/// ones the directory has no manifest at all — a reader sees "no complete
/// release" rather than new files under the old release's manifest. The new
/// manifest is published last.
fn retract_previous_manifest(out_dir: &Path) -> Result<(), BuildError> {
    for name in ["manifest.json", "manifest.json.sha256"] {
        let path = out_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(BuildError::output(&path, error.to_string())),
        }
    }
    Ok(())
}

/// Deletes the build's lock file when the write finishes or fails.
struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write one `<split>.jsonl` export per split and return the artifact
/// digests plus the accepted-row tallies.
fn write_split_files(
    bundle: &ReleaseBundle<'_>,
) -> Result<(BTreeMap<String, manifest::Artifact>, SplitCounts), BuildError> {
    let mut artifacts = BTreeMap::new();
    let mut counts = SplitCounts::default();
    for split in Split::ALL {
        let rows = bundle
            .by_split
            .get(&split)
            .map_or(&[] as &[Fingerprinted], Vec::as_slice);
        let mut export_rows: Vec<_> = rows.iter().map(|r| export::to_export(r, split)).collect();
        export_rows.sort_by(|a, b| a.row_id.cmp(&b.row_id));
        count_rows(rows, split, &mut counts);
        let name = format!("{}.jsonl", split.as_str());
        let meta = write_jsonl(
            &staged(&bundle.config.out_dir.join(&name)),
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
        counts
            .splits
            .insert(split.as_str().to_owned(), export_rows.len());
    }
    Ok((artifacts, counts))
}

/// Assemble the manifest, write it atomically, then its `.sha256` sidecar.
fn write_manifest(
    bundle: ReleaseBundle<'_>,
    artifacts: BTreeMap<String, manifest::Artifact>,
    counts: SplitCounts,
) -> Result<Outputs, BuildError> {
    let document = manifest::SplitManifest::new(
        bundle.config,
        manifest::ManifestData {
            inputs: bundle.inputs,
            artifacts,
            counts: manifest::Counts::of(
                counts.splits.clone(),
                counts.topics,
                counts.taxonomy,
                bundle.rejected,
            ),
            assignments: bundle.assigner.assignments(),
        },
    );
    let manifest_text = serde_json::to_string_pretty(&document)
        .map_err(|e| BuildError::output(&bundle.config.out_dir, e.to_string()))?;
    let manifest_path = bundle.config.out_dir.join("manifest.json");
    manifest::write_atomic(&manifest_path, manifest_text.as_bytes())
        .map_err(|e| BuildError::output(&manifest_path, e.to_string()))?;
    let digest = format!("{:x}", Sha256::digest(manifest_text.as_bytes()));
    let sidecar = bundle.config.out_dir.join("manifest.json.sha256");
    manifest::write_atomic(&sidecar, format!("{digest}  manifest.json\n").as_bytes())
        .map_err(|e| BuildError::output(&sidecar, e.to_string()))?;

    Ok(Outputs {
        split_counts: counts.splits,
        manifest_path,
        manifest_sha256: digest,
    })
}

/// Tally accepted rows per topic and per taxonomy id (`none` for
/// confirmation rows).
fn count_rows(rows: &[Fingerprinted], split: Split, counts: &mut SplitCounts) {
    for row in rows {
        *counts
            .topics
            .entry(row.checked.row.topic.clone())
            .or_default()
            .entry(split.as_str().to_owned())
            .or_insert(0) += 1;
        *counts
            .taxonomy
            .entry(export::taxonomy_id(&row.checked))
            .or_default()
            .entry(split.as_str().to_owned())
            .or_insert(0) += 1;
    }
}

/// Serialize `rows` as JSONL, stream them through an atomic temporary file,
/// and return the file's SHA-256 and byte length. The artifact is never
/// held in memory in full.
fn write_jsonl(
    path: &Path,
    rows: impl Iterator<Item = serde_json::Result<serde_json::Value>>,
) -> Result<manifest::FileDigest, BuildError> {
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    manifest::write_atomic_with(path, |file| {
        for row in rows {
            let value = row.map_err(std::io::Error::other)?;
            let mut line = serde_json::to_string(&value).map_err(std::io::Error::other)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
            hasher.update(line.as_bytes());
            bytes += line.len() as u64;
        }
        Ok(())
    })
    .map_err(|e| BuildError::output(path, e.to_string()))?;
    Ok(manifest::FileDigest {
        sha256: format!("{:x}", hasher.finalize()),
        bytes,
    })
}
