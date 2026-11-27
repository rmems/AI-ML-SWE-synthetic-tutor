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
    let entry = serde_json::json!({
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
    let mut missing_required = entry.clone();
    missing_required
        .as_object_mut()
        .unwrap()
        .remove("recorded_at");
    assert!(
        !validator.is_valid(&missing_required),
        "missing recorded_at must fail"
    );
    // Nullable fields are emitted as explicit `null`, never omitted.
    let mut missing_link = entry.clone();
    missing_link
        .as_object_mut()
        .unwrap()
        .remove("prev_entry_sha256");
    assert!(
        !validator.is_valid(&missing_link),
        "missing prev_entry_sha256 must fail"
    );
    let mut missing_nullable = entry.clone();
    missing_nullable["record"]
        .as_object_mut()
        .unwrap()
        .remove("split_manifest_sha256");
    assert!(
        !validator.is_valid(&missing_nullable),
        "missing split_manifest_sha256 must fail"
    );
}

#[test]
fn schema_requires_gate_pause_evidence() {
    let validator = jsonschema::validator_for(&run_log_json_schema()).expect("schema compiles");
    let pause = |evidence: Value| {
        serde_json::json!({
            "log_version": 1,
            "recorded_at": "2026-11-27T09:30:00Z",
            "prev_entry_sha256": null,
            "record": {
                "kind": "gate_pause",
                "family": "derivative_misconceptions",
                "gate": "verification",
                "run_id": null,
                "detail": "rejection rate above the family gate",
                "evidence": evidence
            }
        })
    };
    assert!(validator.is_valid(&pause(serde_json::json!({"verify_answer_mismatch": 12}))));
    // The runtime validator rejects an empty map; the schema must agree.
    assert!(!validator.is_valid(&pause(serde_json::json!({}))));
}

#[test]
fn schema_caps_seed_bounds_like_the_runtime() {
    let schema = run_log_json_schema();
    let seed_use = &schema["$defs"]["SeedUse"]["properties"];
    // Same bounds `SeedUse::validate` enforces: start <= MAX_SEED and an
    // exclusive end <= MAX_SEED + 1.
    assert_eq!(
        seed_use["start"]["maximum"],
        serde_json::json!(9_007_199_254_740_991_u64)
    );
    assert_eq!(
        seed_use["end"]["maximum"],
        serde_json::json!(9_007_199_254_740_992_u64)
    );
    let validator = jsonschema::validator_for(&seed_use["end"]).expect("end schema compiles");
    assert!(validator.is_valid(&serde_json::json!(9_007_199_254_740_992_u64)));
    assert!(!validator.is_valid(&serde_json::json!(9_007_199_254_740_993_u64)));
}

#[test]
fn schema_encodes_the_two_seed_forms() {
    let schema = run_log_json_schema();
    let validator =
        jsonschema::validator_for(&schema["$defs"]["SeedUse"]).expect("schema compiles");
    let seed = |end: Value, derivation: Value| serde_json::json!({"start": 7, "end": end, "derivation": derivation});
    assert!(validator.is_valid(&seed(serde_json::json!(9), Value::Null)));
    assert!(validator.is_valid(&seed(Value::Null, serde_json::json!("sha256(seed, id)"))));
    for (name, end, derivation) in [
        ("both null", Value::Null, Value::Null),
        (
            "both populated",
            serde_json::json!(9),
            serde_json::json!("sha256(seed, id)"),
        ),
        ("blank derivation", Value::Null, serde_json::json!("  ")),
    ] {
        assert!(
            !validator.is_valid(&seed(end, derivation)),
            "{name} must fail schema validation"
        );
    }
}

#[test]
fn schema_pins_timestamp_and_digest_formats() {
    let schema = run_log_json_schema();
    let timestamp = jsonschema::validator_for(&schema["properties"]["recorded_at"])
        .expect("timestamp schema compiles");
    assert!(timestamp.is_valid(&serde_json::json!("2026-11-27T09:30:00Z")));
    assert!(timestamp.is_valid(&serde_json::json!("2026-11-27T09:30:00.123Z")));
    assert!(!timestamp.is_valid(&serde_json::json!("yesterday")));
    assert!(!timestamp.is_valid(&serde_json::json!("2026-11-27T09:30:00+00:00")));

    let digest = jsonschema::validator_for(&schema["properties"]["prev_entry_sha256"])
        .expect("digest schema compiles");
    assert!(digest.is_valid(&serde_json::json!("a".repeat(64))));
    assert!(digest.is_valid(&Value::Null));
    assert!(!digest.is_valid(&serde_json::json!("A".repeat(64))));
    assert!(!digest.is_valid(&serde_json::json!("not-a-digest")));
}
