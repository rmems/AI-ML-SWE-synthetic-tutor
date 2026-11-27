//! The generation run log: one append-only JSONL entry per pipeline run.
//!
//! The dataset-factory equivalent of a study log ([issue
//! #7](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/7)): every
//! run that commits a batch — generation, simulation, and later the
//! assemble/export stages — appends one [`RunLogEntry`] complete enough to
//! reproduce the batch or reject it. Entries are hash-chained
//! (`prev_entry_sha256`) so edited or reordered history is detected by
//! [`RunLog::entries`]; a rerun appends a new entry whose caller-set
//! `replaces` names the run it supersedes, instead of editing the old one.
//! Fields a run could not have known are explicit `null`, documented on
//! each field.
//!
//! The crate is deliberately free of `zmq`/network dependencies so the log
//! core builds and tests everywhere; `tutor-generate` feeds it the Julia
//! worker's version when a worker answers `ping`.
//!
//! Format version [`RUN_LOG_SCHEMA_VERSION`] = 1; the JSON contract is
//! `schemas/run-log.schema.json` (see [`run_log_json_schema`]).

mod entry;
mod log;

pub use entry::{
    Counts, Environment, Gate, GatePauseRecord, GeneratorUse, OutputRef, QaSummary,
    RUN_LOG_SCHEMA_VERSION, Record, RunLogEntry, RunRecord, SeedUse, Stage, TeacherUse,
};
pub use log::{RunLog, RunLogError, new_run_id, sha256_hex};

/// The rustc version that built this crate; recorded as
/// [`Environment::rustc_version`].
pub const RUSTC_VERSION: &str = env!("TUTOR_RUNLOG_RUSTC_VERSION");

/// The cargo version that built this crate; recorded as
/// [`Environment::cargo_version`].
pub const CARGO_VERSION: &str = env!("TUTOR_RUNLOG_CARGO_VERSION");

/// Capture the local toolchain environment for a run entry.
///
/// `git_commit` comes from `git rev-parse HEAD` when the pipeline runs inside
/// a worktree (`null` otherwise); Julia fields are `None` — callers that use
/// the math worker set them from its `ping` reply.
pub fn environment() -> Environment {
    Environment {
        git_commit: git_head(),
        rustc_version: RUSTC_VERSION.to_owned(),
        cargo_version: CARGO_VERSION.to_owned(),
        pipeline_version: env!("CARGO_PKG_VERSION").to_owned(),
        julia_version: None,
        julia_worker_version: None,
    }
}

fn git_head() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!head.is_empty()).then_some(head)
}

/// The JSON Schema for [`RunLogEntry`], generated from the types.
///
/// Post-processing pins `log_version` to [`RUN_LOG_SCHEMA_VERSION`], which
/// `schemars` cannot express. Regenerate `schemas/run-log.schema.json` with
/// `cargo run -p tutor-runlog --example print_run_log_schema`.
pub fn run_log_json_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(RunLogEntry))
        .expect("generated schema serializes");
    let root = schema.as_object_mut().expect("schema is an object");
    root.insert(
        "$id".to_owned(),
        serde_json::Value::String(
            "https://github.com/rmems/AI-ML-SWE-synthetic-tutor/schemas/run-log.schema.json"
                .to_owned(),
        ),
    );
    root.insert(
        "description".to_owned(),
        serde_json::Value::String(
            "One append-only generation run-log entry (log_version 1). Mirrors \
             tutor_runlog::RunLogEntry; regenerate with `cargo run -p tutor-runlog \
             --example print_run_log_schema`. The schema expresses shape; the \
             hash chain, `replaces` resolution, and run-id uniqueness are \
             enforced by crates/tutor-runlog, not by JSON Schema."
                .to_owned(),
        ),
    );
    schema["properties"]["log_version"] = serde_json::json!({
        "const": RUN_LOG_SCHEMA_VERSION
    });
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_run() -> RunRecord {
        RunRecord {
            run_id: "run-20261127T000000Z-deadbeef".to_owned(),
            family: Some("derivative_misconceptions".to_owned()),
            spec: Some("https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20".to_owned()),
            stage: Stage::Generate,
            replaces: None,
            environment: environment(),
            generators: vec![GeneratorUse {
                name: "binomial_derivative_at_point".to_owned(),
                version: 1,
                language: tutor_schema::GeneratorLanguage::Rust,
                seeds: SeedUse {
                    start: 0,
                    end: Some(40),
                    derivation: None,
                },
                parameter_ranges: BTreeMap::from([
                    ("max_degree".to_owned(), serde_json::json!(6)),
                    ("coefficient_bound".to_owned(), serde_json::json!(9)),
                    ("point_bound".to_owned(), serde_json::json!(5)),
                ]),
            }],
            teacher: None,
            counts: Counts {
                generated: 40,
                verified: 38,
                rejected: 2,
                rejected_by_reason: BTreeMap::from([
                    ("generate_overflow".to_owned(), 1),
                    ("verify_answer_mismatch".to_owned(), 1),
                ]),
                deduplicated: None,
                decontaminated: None,
                exported: None,
            },
            output: OutputRef {
                directory: "out/pilot".to_owned(),
                manifest_sha256: Some(sha256_hex(b"manifest")),
            },
            split_manifest_sha256: None,
            qa: None,
        }
    }

    #[test]
    fn run_entry_round_trips() {
        let entry = RunLogEntry {
            log_version: RUN_LOG_SCHEMA_VERSION,
            recorded_at: "2026-11-27T09:30:00Z".to_owned(),
            prev_entry_sha256: Some(sha256_hex(b"previous line")),
            record: Record::Run(Box::new(sample_run())),
        };
        let json = serde_json::to_string(&entry).expect("serializes");
        let back: RunLogEntry = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(entry, back);
        // Nulls stay explicit rather than vanishing.
        let value = serde_json::to_value(&entry).expect("serializes");
        assert_eq!(
            value["record"]["family"],
            serde_json::json!("derivative_misconceptions")
        );
        assert_eq!(value["record"]["teacher"], serde_json::Value::Null);
        assert_eq!(
            value["record"]["split_manifest_sha256"],
            serde_json::Value::Null
        );
        assert_eq!(
            value["record"]["counts"]["deduplicated"],
            serde_json::Value::Null
        );
        assert_eq!(value["record"]["kind"], serde_json::json!("run"));
    }

    #[test]
    fn gate_pause_round_trips() {
        let entry = RunLogEntry {
            log_version: RUN_LOG_SCHEMA_VERSION,
            recorded_at: "2026-11-27T10:00:00Z".to_owned(),
            prev_entry_sha256: None,
            record: Record::GatePause(GatePauseRecord {
                family: "derivative_misconceptions".to_owned(),
                gate: Gate::Verification,
                run_id: Some("run-20261127T000000Z-deadbeef".to_owned()),
                detail: "verification rejection rate above the family gate".to_owned(),
                evidence: BTreeMap::from([(
                    "rejected_by_reason".to_owned(),
                    serde_json::json!({"verify_answer_mismatch": 12}),
                )]),
            }),
        };
        let json = serde_json::to_string(&entry).expect("serializes");
        let back: RunLogEntry = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(entry, back);
        let value = serde_json::to_value(&entry).expect("serializes");
        assert_eq!(value["record"]["kind"], serde_json::json!("gate_pause"));
        assert_eq!(value["record"]["gate"], serde_json::json!("verification"));
    }

    #[test]
    fn emitted_entries_validate_against_the_schema() {
        let validator = jsonschema::validator_for(&run_log_json_schema()).expect("schema compiles");
        for record in [
            Record::Run(Box::new(sample_run())),
            Record::GatePause(GatePauseRecord {
                family: "derivative_misconceptions".to_owned(),
                gate: Gate::EvalSpotCheck,
                run_id: None,
                detail: "eval spot-check regression".to_owned(),
                evidence: BTreeMap::new(),
            }),
        ] {
            let entry = RunLogEntry {
                log_version: RUN_LOG_SCHEMA_VERSION,
                recorded_at: "2026-11-27T10:00:00Z".to_owned(),
                prev_entry_sha256: None,
                record,
            };
            let value = serde_json::to_value(&entry).expect("serializes");
            assert!(
                validator.is_valid(&value),
                "entry fails run-log schema: {}",
                validator
                    .iter_errors(&value)
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }

    #[test]
    fn environment_captures_toolchain() {
        let env = environment();
        assert!(!env.rustc_version.is_empty());
        assert!(!env.cargo_version.is_empty());
        assert_eq!(env.pipeline_version, env!("CARGO_PKG_VERSION"));
    }
}
