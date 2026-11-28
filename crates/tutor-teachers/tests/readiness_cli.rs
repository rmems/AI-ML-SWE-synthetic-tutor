//! Exercise the public command, deterministic artifacts and release refusal.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use tutor_build::{checks, input::InputLine};
use tutor_schema::Problem;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("tutor-readiness-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tutor-teachers"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn mock_is_repeatable_validated_and_never_exportable() {
    let temp = Temp::new();
    let a = temp.0.join("a");
    let b = temp.0.join("b");
    for out in [&a, &b] {
        let result = cli(&["mock-smoke", "--out", out.to_str().unwrap()]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for file in ["candidates.jsonl", "report.json"] {
        assert_eq!(
            std::fs::read(a.join(file)).unwrap(),
            std::fs::read(b.join(file)).unwrap()
        );
    }
    let bytes = std::fs::read(a.join("candidates.jsonl")).unwrap();
    let report: Value =
        serde_json::from_slice(&std::fs::read(a.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["candidates_sha256"], tutor_teachers::sha256(&bytes));
    assert_eq!(report["candidate_count"], 2);
    let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema()).unwrap();
    for text in std::str::from_utf8(&bytes).unwrap().lines() {
        let candidate: Value = serde_json::from_str(text).unwrap();
        assert_eq!(candidate["training_approved"], false);
        assert_eq!(candidate["export_approved"], false);
        assert!(!validator.is_valid(&candidate));
        let problem = Problem::from_value(candidate["problem"].clone()).unwrap();
        assert_eq!(
            problem.verification.as_ref().unwrap().method,
            tutor_verify::RUST_RECOMPUTE
        );
        let problems = BTreeMap::from([(problem.problem_id.clone(), problem)]);
        let row = &candidate["dialogue"];
        assert!(validator.is_valid(row));
        let input = InputLine {
            file: "extracted.json".into(),
            line: None,
            text: row.to_string(),
        };
        let err =
            checks::check(&input, Some(&problems), &validator, &mut HashSet::new()).unwrap_err();
        assert_eq!(err.code, "mock_teacher_not_exportable");
    }
    let repeat = cli(&["mock-smoke", "--out", a.to_str().unwrap()]);
    assert!(!repeat.status.success());
    assert_eq!(bytes, std::fs::read(a.join("candidates.jsonl")).unwrap());
}

#[test]
fn deepseek_config_is_check_only_and_cannot_generate() {
    let temp = Temp::new();
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../configs/teacher.deepseek.example.json");
    let result = cli(&["check-config", "--config", config.to_str().unwrap()]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("BLOCKED"));
    let out = temp.0.join("must-not-exist");
    let result = cli(&[
        "mock-smoke",
        "--config",
        config.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("live teacher generation blocked"));
    assert!(!out.exists());
}

#[test]
fn invalid_config_and_attempts_to_supply_keys_fail_without_echo() {
    let base: Value = serde_json::from_str(include_str!(
        "../../../configs/teacher.deepseek.example.json"
    ))
    .unwrap();
    for (field, value) in [
        ("api_key", "never-echo-this-fixture-value"),
        ("provider", "unlisted"),
        ("api_key_env", "not a variable name"),
        (
            "endpoint",
            "https://user:never-echo-this-fixture-value@example.invalid/v1",
        ),
        (
            "endpoint",
            "https://example.invalid/v1?key=never-echo-this-fixture-value",
        ),
        ("endpoint", "http://example.invalid/v1"),
        ("endpoint", "https://example.invalid/v1#fragment"),
        ("model", ""),
    ] {
        let temp = Temp::new();
        let mut config = base.clone();
        config[field] = value.into();
        let path = temp.0.join("invalid.json");
        std::fs::write(&path, config.to_string()).unwrap();
        let result = cli(&["check-config", "--config", path.to_str().unwrap()]);
        assert!(!result.status.success(), "field {field}");
        assert!(!String::from_utf8_lossy(&result.stderr).contains("never-echo-this-fixture-value"));
    }
}
