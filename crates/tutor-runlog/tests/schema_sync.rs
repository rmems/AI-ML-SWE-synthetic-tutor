//! Keeps `tutor_runlog::RunLogEntry` and `schemas/run-log.schema.json` in line.
//! Regenerate the file with
//! `cargo run -p tutor-runlog --example print_run_log_schema > schemas/run-log.schema.json`.

use serde_json::Value;
use tutor_runlog::run_log_json_schema;

const COMMITTED_SCHEMA: &str = include_str!("../../../schemas/run-log.schema.json");

#[test]
fn committed_schema_matches_generated() {
    let committed: Value =
        serde_json::from_str(COMMITTED_SCHEMA).expect("run-log.schema.json is valid JSON");
    assert_eq!(committed, run_log_json_schema());
}

#[test]
fn schema_enforces_null_semantics() {
    let validator = jsonschema::validator_for(&run_log_json_schema()).expect("schema compiles");
    // Not-yet-known fields may be null but may not disappear.
    let mut entry = serde_json::json!({
        "log_version": 1,
        "recorded_at": "2026-11-27T09:30:00Z",
        "prev_entry_sha256": null,
        "record": {
            "kind": "run",
            "run_id": "run-20261127T093000Z-00000000",
            "stage": "generate",
            "environment": {
                "git_commit": null,
                "rustc_version": "rustc 1.99.1",
                "cargo_version": "cargo 1.99.1",
                "pipeline_version": "0.1.0",
                "julia_version": null,
                "julia_worker_version": null
            },
            "generators": [{
                "name": "binomial_derivative_at_point",
                "version": 1,
                "language": "rust",
                "seeds": {"start": 0, "end": 40, "derivation": null},
                "parameter_ranges": {"max_degree": 6, "coefficient_bound": 9, "point_bound": 5}
            }],
            "counts": {"generated": 40, "verified": 40, "rejected": 0,
                       "rejected_by_reason": {},
                       "deduplicated": null, "decontaminated": null, "exported": null},
            "output": {"directory": "out", "manifest_sha256": null},
            "family": null, "spec": null, "replaces": null, "teacher": null,
            "split_manifest_sha256": null, "qa": null
        }
    });
    assert!(
        validator.is_valid(&entry),
        "canonical entry must validate: {:?}",
        validator
            .iter_errors(&entry)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    entry.as_object_mut().unwrap().remove("recorded_at");
    assert!(!validator.is_valid(&entry), "missing recorded_at must fail");
}
