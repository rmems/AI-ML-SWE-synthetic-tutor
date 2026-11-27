//! The versioned run-log entry format.
//!
//! Every pipeline stage that commits a dataset batch appends one
//! [`RunLogEntry`] to a JSONL log (see `docs/cadence.md`). `null` is
//! meaningful everywhere it appears: a field the run could not have known
//! (split manifest hash before issue #8, QA verdicts before #21, a teacher on
//! a procedural run) is recorded as `null`, never invented.
//!
//! The JSON contract is `schemas/run-log.schema.json`, generated from these
//! types with `cargo run -p tutor-runlog --example print_run_log_schema` and
//! kept in sync by a test in `crates/tutor-runlog/tests`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tutor_schema::{GeneratorLanguage, MAX_SEED};

use crate::RunLogError;

/// Current `log_version` of [`RunLogEntry`].
pub const RUN_LOG_SCHEMA_VERSION: u32 = 1;

/// One line of the append-only run log.
///
/// The envelope (`log_version`, `recorded_at`, `prev_entry_sha256`) is owned by
/// [`crate::RunLog::append`]; callers supply only the [`Record`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLogEntry {
    /// Always [`RUN_LOG_SCHEMA_VERSION`].
    pub log_version: u32,
    /// When the pipeline appended the entry (RFC 3339 UTC).
    pub recorded_at: String,
    /// SHA-256 (lowercase hex) of the previous entry's serialized JSON line,
    /// or `null` on the first entry. This is the chain link: editing,
    /// reordering, or deleting an earlier entry is detected because the next
    /// entry's `prev_entry_sha256` no longer matches.
    pub prev_entry_sha256: Option<String>,
    /// The recorded event.
    pub record: Record,
}

/// The event a [`RunLogEntry`] records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    /// One pipeline run that committed a batch. Boxed so a pause entry does
    /// not pay for the larger run record.
    Run(Box<RunRecord>),
    /// A quality-gate failure that paused a family's cadence. Pauses are
    /// appended, never folded back into an earlier run entry.
    GatePause(GatePauseRecord),
}

impl Record {
    /// Reject records whose fields cannot have come from a real pipeline
    /// write. Called on every appended entry and on every entry during
    /// verification, so only invariants that hold for any stage belong here.
    pub(crate) fn validate(&self) -> Result<(), RunLogError> {
        match self {
            Record::Run(run) => run.validate(),
            Record::GatePause(pause) => pause.validate(),
        }
    }
}

/// One pipeline run that committed a batch (generated + verified problems,
/// simulated attempts, or a later cadence stage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// Unique id of this run; see [`crate::new_run_id`]. Run ids are never
    /// reused: appending a run id already present in the log fails.
    pub run_id: String,
    /// Dataset family the run fed (the `datasets/<family>/` name). `null`
    /// when the run was not bound to a declared family, e.g. a smoke test.
    pub family: Option<String>,
    /// Generation spec the run ran under (issue ref or URL from the
    /// generation-spec form). `null` when the family has no filed spec yet.
    pub spec: Option<String>,
    /// Pipeline stage that produced the batch.
    pub stage: Stage,
    /// Run id of the earlier run this one replaces; `null` when the run
    /// replaces nothing — a fresh batch, or a rerun the caller did not mark
    /// as a replacement. Never inferred: a rerun that supersedes an earlier
    /// batch names it explicitly (e.g. `--replaces` / `RunConfig::replaces`)
    /// and the replaced entry is never edited. When set, it must name a run
    /// earlier in the same log, so a batch lineage keeps its reruns together.
    pub replaces: Option<String>,
    /// Toolchain provenance for reproduction.
    pub environment: Environment,
    /// Generators/simulators used, with seeds and parameter ranges.
    pub generators: Vec<GeneratorUse>,
    /// Teacher model used; `null` for purely procedural runs (the common
    /// case until teacher lanes ship).
    pub teacher: Option<TeacherUse>,
    /// Outcome counts, all measured from the run's written output — never
    /// estimated.
    pub counts: Counts,
    /// Where the batch was published and its committed manifest digest.
    pub output: OutputRef,
    /// SHA-256 (lowercase hex) of the split manifest of the release this run
    /// fed; `null` until the split step (#8) exists or the run feeds no
    /// release.
    pub split_manifest_sha256: Option<String>,
    /// QA sample size and verdict summary for the batch; `null` until the QA
    /// step (#21) has sampled it.
    pub qa: Option<QaSummary>,
}

impl RunRecord {
    fn validate(&self) -> Result<(), RunLogError> {
        non_empty("run_id", &self.run_id)?;
        optional_non_empty("family", &self.family)?;
        optional_non_empty("spec", &self.spec)?;
        if self.generators.is_empty() {
            return Err(invalid("generators", "a run must name its generators"));
        }
        for generator in &self.generators {
            generator.validate()?;
        }
        if let Some(teacher) = &self.teacher {
            non_empty("teacher.name", &teacher.name)?;
            non_empty("teacher.version", &teacher.version)?;
            non_empty("teacher.rights_entry", &teacher.rights_entry)?;
        }
        self.counts.validate()?;
        non_empty("output.directory", &self.output.directory)?;
        if let Some(sha) = &self.output.manifest_sha256 {
            sha256_field("output.manifest_sha256", sha)?;
        }
        if let Some(sha) = &self.split_manifest_sha256 {
            sha256_field("split_manifest_sha256", sha)?;
        }
        if let Some(qa) = &self.qa {
            let verdicts = checked_total(qa.verdicts.values())
                .ok_or_else(|| invalid("qa.verdicts", "verdict counts overflow u64"))?;
            if verdicts != qa.sample_size {
                return Err(invalid(
                    "qa.verdicts",
                    format!(
                        "verdict counts ({verdicts}) must sum to sample_size ({})",
                        qa.sample_size
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// A quality-gate failure that paused a family's cadence.
///
/// Written when a cadence step fails its gate (rejection rates, QA verdicts,
/// eval spot-check, export checks). The family stays paused until a fix lands
/// as a new run; the pause entry is the recorded evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GatePauseRecord {
    /// Dataset family whose cadence is paused (the `datasets/<family>/` name).
    pub family: String,
    /// The gate that failed.
    pub gate: Gate,
    /// Run id whose failure paused the family; `null` when the gate failed
    /// outside a logged run (e.g. a review of committed data). When set it
    /// must name a run earlier in the same log.
    pub run_id: Option<String>,
    /// Why the gate failed, in human terms.
    pub detail: String,
    /// Machine-readable evidence backing the pause: reason-code counts,
    /// verdict summaries, file digests — whatever the gate measured. Never
    /// empty: a pause without evidence cannot be reviewed.
    #[schemars(extend("minProperties" = 1))]
    pub evidence: BTreeMap<String, serde_json::Value>,
}

impl GatePauseRecord {
    fn validate(&self) -> Result<(), RunLogError> {
        non_empty("family", &self.family)?;
        non_empty("detail", &self.detail)?;
        if self.evidence.is_empty() {
            return Err(invalid(
                "evidence",
                "a gate pause must carry the evidence for the failed gate",
            ));
        }
        Ok(())
    }
}

/// Pipeline stage a run belongs to.
///
/// Generation bundles its answer-verification gate (`tutor-generate run`
/// verifies every candidate before writing `verified/`), so `generate` is the
/// first batch-producing stage; `simulate` follows (#19). `assemble` and
/// `export` are declared for the dedup/split (#8) and release (#21) steps so
/// the cadence's later stages can log under the same format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Problem generation + independent verification (`tutor-generate run`).
    Generate,
    /// Synthetic student attempt simulation (`tutor-generate simulate`).
    Simulate,
    /// Dedup, decontamination, and frozen splits (#8, not yet built).
    Assemble,
    /// Dataset release/export (#21, not yet built).
    Export,
}

/// A cadence quality gate, in the order a release walks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Problem generation and its embedded independent verification.
    Generation,
    /// Independent verification / replay of produced rows.
    Verification,
    /// Sampled human QA verdicts (#21).
    QaSample,
    /// Eval spot-check against the held-out set.
    EvalSpotCheck,
    /// Export/release checks (#21).
    Export,
}

/// Toolchain provenance of the pipeline that wrote a run entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// `git rev-parse HEAD` of the pipeline source checkout, captured when
    /// `tutor-runlog` was built; `null` when the pipeline was built outside
    /// a git worktree or git was unavailable.
    pub git_commit: Option<String>,
    /// `rustc --version` that built the pipeline.
    pub rustc_version: String,
    /// `cargo --version` that built the pipeline.
    pub cargo_version: String,
    /// Workspace version of the pipeline crates.
    pub pipeline_version: String,
    /// Julia `VERSION` reported by the math worker's `ping` reply; `null`
    /// when the run used no Julia-backed step or the worker reported none.
    pub julia_version: Option<String>,
    /// TutorMath worker package version from the same `ping`; `null` under
    /// the same conditions as `julia_version`.
    pub julia_worker_version: Option<String>,
}

/// One generator/simulator a run drew from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratorUse {
    /// Generator or simulator name, e.g. `binomial_derivative_at_point` or
    /// `synthetic-student-simulator`.
    pub name: String,
    /// Its version; bumped when output for a seed changes.
    pub version: u32,
    /// Implementation language of the tool.
    pub language: GeneratorLanguage,
    /// Seeds the run drew from.
    pub seeds: SeedUse,
    /// Parameter ranges the tool declared for its draws, keyed by the tool's
    /// own bound fields (a generator records its `Difficulty` bounds; the
    /// simulator records `attempts_per_problem`, `mode`, and any pinned
    /// profile fields). Keys and values are the tool's own names, so this is
    /// a free-form object checked by convention, not by this schema.
    pub parameter_ranges: BTreeMap<String, serde_json::Value>,
}

impl GeneratorUse {
    fn validate(&self) -> Result<(), RunLogError> {
        non_empty("generators[].name", &self.name)?;
        self.seeds.validate()
    }
}

/// The seeds one generator/simulator drew from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SeedUse {
    /// First seed of the caller's stream.
    pub start: u64,
    /// One past the last seed of a contiguous draw; `null` when the tool
    /// derives per-item seeds — see `derivation`.
    pub end: Option<u64>,
    /// How per-item seeds were derived from `start` (e.g.
    /// `sha256(seed_start, problem_id, attempt_index)`); `null` means the
    /// contiguous range `start..end` was drawn directly. Exactly one of
    /// `end`/`derivation` must be set.
    pub derivation: Option<String>,
}

impl SeedUse {
    fn validate(&self) -> Result<(), RunLogError> {
        if self.start > MAX_SEED {
            return Err(invalid(
                "generators[].seeds.start",
                format!("exceeds the JSON-safe maximum {MAX_SEED}"),
            ));
        }
        match (self.end, self.derivation.as_deref()) {
            // `end` is exclusive: the last drawn seed may be MAX_SEED.
            (Some(end), None) if end > MAX_SEED + 1 => Err(invalid(
                "generators[].seeds.end",
                format!("exceeds the JSON-safe exclusive bound {}", MAX_SEED + 1),
            )),
            (Some(end), None) if end > self.start => Ok(()),
            (Some(_), None) => Err(invalid(
                "generators[].seeds.end",
                "a contiguous range must satisfy end > start",
            )),
            (None, Some(derivation)) => non_empty("generators[].seeds.derivation", derivation),
            (None, None) => Err(invalid(
                "generators[].seeds",
                "must set either `end` (contiguous draw) or `derivation`",
            )),
            (Some(_), Some(_)) => Err(invalid(
                "generators[].seeds",
                "set `end` or `derivation`, not both",
            )),
        }
    }
}

/// A teacher model used by the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TeacherUse {
    /// Teacher name, e.g. `DeepSeek V4.1 Flash`.
    pub name: String,
    /// Exact pinned version: API version string or immutable checkpoint
    /// revision, per the registry entry.
    pub version: String,
    /// Rights-registry entry authorizing the use — the `docs/rights.md`
    /// heading (e.g. `1` / `DeepSeek V4.1 Flash`). The entry must be
    /// `allowed`; a run naming a teacher without one is not release-legal.
    pub rights_entry: String,
}

/// Outcome counts of a run, measured from its written output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// Rows/candidates the run produced before its output gate.
    pub generated: u64,
    /// Rows that passed the run's output gate (independent verification for
    /// `generate`, artifact schema validation for `simulate`).
    pub verified: u64,
    /// Candidates the run rejected at any of its gates.
    pub rejected: u64,
    /// `rejected` broken down by stable reason code (rejection `code` for
    /// `generate`, rejection `stage` for `simulate`).
    pub rejected_by_reason: BTreeMap<String, u64>,
    /// Rows removed by deduplication; `null` when the run's stage does not
    /// dedup (the split step, #8, owns that count).
    pub deduplicated: Option<u64>,
    /// Rows removed by decontamination against the held-out eval set; `null`
    /// until the split step (#8) exists.
    pub decontaminated: Option<u64>,
    /// Rows exported to the release; `null` until export (#21) exists.
    pub exported: Option<u64>,
}

impl Counts {
    fn validate(&self) -> Result<(), RunLogError> {
        let accounted = checked_total(self.rejected_by_reason.values())
            .ok_or_else(|| invalid("counts.rejected_by_reason", "reason counts overflow u64"))?;
        if accounted != self.rejected {
            return Err(invalid(
                "counts.rejected_by_reason",
                format!(
                    "reason codes ({accounted}) must sum to rejected ({})",
                    self.rejected
                ),
            ));
        }
        Ok(())
    }
}

/// Where a run's batch was published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputRef {
    /// Directory the batch was committed to, as invoked.
    pub directory: String,
    /// SHA-256 (lowercase hex) of the committed `manifest.json`; `null` when
    /// the stage writes no manifest.
    pub manifest_sha256: Option<String>,
}

/// QA sampling outcome for a batch (#21).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QaSummary {
    /// Rows sampled for review.
    pub sample_size: u64,
    /// Verdict label → count, e.g. `{"pass": 9, "fail": 1}`. Every sampled
    /// row carries exactly one verdict, so the counts sum to `sample_size`.
    pub verdicts: BTreeMap<String, u64>,
}

fn non_empty(field: &'static str, value: &str) -> Result<(), RunLogError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "must be non-empty"));
    }
    Ok(())
}

fn optional_non_empty(field: &'static str, value: &Option<String>) -> Result<(), RunLogError> {
    if let Some(value) = value {
        non_empty(field, value)?;
    }
    Ok(())
}

fn sha256_field(field: &'static str, value: &str) -> Result<(), RunLogError> {
    if value.len() == 64 && value.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(invalid(field, "must be a lowercase-hex SHA-256 digest"))
    }
}

/// `0-9` or `a-f`: the digits of a lowercase-hex digest.
fn is_lower_hex(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
}

/// Sum of `values`, or `None` when it overflows `u64`.
fn checked_total<'a>(mut values: impl Iterator<Item = &'a u64>) -> Option<u64> {
    values.try_fold(0_u64, |total, value| total.checked_add(*value))
}

fn invalid(field: &'static str, reason: impl Into<String>) -> RunLogError {
    RunLogError::Invalid {
        field,
        reason: reason.into(),
    }
}
