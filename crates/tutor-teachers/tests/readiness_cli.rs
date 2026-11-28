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

fn handoff_fixture(temp: &Temp) -> (PathBuf, PathBuf) {
    let request = temp.0.join("request.json");
    let response = temp.0.join("response.json");
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../configs/teacher.deepseek.example.json");
    assert!(
        cli(&[
            "prepare-request",
            "--config",
            config.to_str().unwrap(),
            "--out",
            request.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        cli(&[
            "mock-response",
            "--request",
            request.to_str().unwrap(),
            "--out",
            response.to_str().unwrap()
        ])
        .status
        .success()
    );
    (request, response)
}

#[test]
fn openrouter_pins_and_options_are_bound_without_execution_claims() {
    let temp = Temp::new();
    let (request, response) = handoff_fixture(&temp);
    let req: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    assert_eq!(req["payload"]["version"], 2);
    assert_eq!(
        req["payload"]["teacher"]["model"],
        "deepseek/deepseek-v4.1-flash-1017"
    );
    assert_eq!(
        req["payload"]["teacher"]["endpoint"],
        "https://openrouter.ai/api/v1"
    );
    assert_eq!(
        req["payload"]["teacher"]["api_key_env"],
        "OPENROUTER_API_KEY"
    );
    assert_eq!(
        req["payload"]["openrouter_provider"],
        serde_json::json!({"only":["deepseek"],"allow_fallbacks":false})
    );
    let out = temp.0.join("replay");
    assert_eq!(replay(&request, &response, &out).status.code(), Some(2));
    let candidate: Value =
        serde_json::from_slice(&std::fs::read(out.join("candidate.json")).unwrap()).unwrap();
    assert_eq!(
        candidate["provenance"]["intended_openrouter_provider"],
        req["payload"]["openrouter_provider"]
    );
    assert_eq!(
        candidate["provenance"]["provider_execution_verified"],
        false
    );
    assert_eq!(candidate["provenance"]["routing_execution_verified"], false);
}

#[test]
fn openrouter_rejects_other_pins_and_altered_routing() {
    let temp = Temp::new();
    let (request, response) = handoff_fixture(&temp);
    let req: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    for field in [
        "model",
        "endpoint",
        "route",
        "serving_vendor",
        "api_key_env",
    ] {
        let mut config = req["payload"]["teacher"].clone();
        config[field] = "different".into();
        let path = temp.0.join("wrong-pin.json");
        std::fs::write(&path, config.to_string()).unwrap();
        assert!(
            !cli(&["check-config", "--config", path.to_str().unwrap()])
                .status
                .success()
        );
    }
    let mut changed = req.clone();
    changed["payload"]["openrouter_provider"]["allow_fallbacks"] = true.into();
    std::fs::write(&request, changed.to_string()).unwrap();
    assert_eq!(
        replay(&request, &response, &temp.0.join("bad-routing"))
            .status
            .code(),
        Some(1)
    );
    changed = req;
    changed["payload"]["openrouter_provider"]["only"] = serde_json::json!(["different"]);
    std::fs::write(&request, changed.to_string()).unwrap();
    assert_eq!(
        replay(&request, &response, &temp.0.join("bad-provider"))
            .status
            .code(),
        Some(1)
    );
}

fn replay(request: &std::path::Path, response: &std::path::Path, out: &std::path::Path) -> Output {
    cli(&[
        "replay-response",
        "--request",
        request.to_str().unwrap(),
        "--response",
        response.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
}

fn assert_replay_provenance(
    candidate: &Value,
    req: &Value,
    request: &std::path::Path,
    response: &std::path::Path,
) {
    assert_eq!(candidate["provenance"]["request_id"], req["request_id"]);
    assert_eq!(
        candidate["provenance"]["request_sha256"],
        tutor_teachers::sha256(&std::fs::read(request).unwrap())
    );
    assert_eq!(
        candidate["provenance"]["response_sha256"],
        tutor_teachers::sha256(&std::fs::read(response).unwrap())
    );
    assert_eq!(candidate["provenance"]["response_origin"], "synthetic_mock");
    assert_eq!(candidate["problem"], req["payload"]["problem"]);
    for index in [0, 1, 3, 5, 6] {
        assert_eq!(
            candidate["dialogue"]["turns"][index],
            req["payload"]["dialogue"]["turns"][index]
        );
    }
    assert_ne!(
        candidate["dialogue"]["turns"][2],
        req["payload"]["dialogue"]["turns"][2]
    );
}

fn assert_teacher_admission_blocked(candidate: &Value) {
    let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema()).unwrap();
    let problem = Problem::from_value(candidate["problem"].clone()).unwrap();
    let problems = BTreeMap::from([(problem.problem_id.clone(), problem)]);
    let input = InputLine {
        file: "extracted.json".into(),
        line: None,
        text: candidate["dialogue"].to_string(),
    };
    assert_eq!(
        checks::check(&input, Some(&problems), &validator, &mut HashSet::new())
            .unwrap_err()
            .code,
        "teacher_admission_blocked"
    );
}

#[test]
fn hermes_handoff_replays_and_admission_stays_blocked() {
    let temp = Temp::new();
    let (request, response) = handoff_fixture(&temp);
    let req: Value = serde_json::from_str(&std::fs::read_to_string(&request).unwrap()).unwrap();
    assert_eq!(req["payload"]["send_authorized"], false);
    let mut outputs = Vec::new();
    for dir in ["a", "b"] {
        let out = temp.0.join(dir);
        let result = replay(&request, &response, &out);
        assert_eq!(
            result.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let bytes = std::fs::read(out.join("candidate.json")).unwrap();
        let report: Value =
            serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
        assert_eq!(report["admitted"], false);
        assert_eq!(report["admission_code"], "teacher_admission_blocked");
        assert_eq!(report["candidate_sha256"], tutor_teachers::sha256(&bytes));
        let candidate: Value = serde_json::from_slice(&bytes).unwrap();
        assert_replay_provenance(&candidate, &req, &request, &response);
        assert_teacher_admission_blocked(&candidate);
        outputs.push(bytes);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(
        replay(&request, &response, &temp.0.join("a")).status.code(),
        Some(1)
    );
    assert!(
        !cli(&["prepare-request", "--out", request.to_str().unwrap()])
            .status
            .success()
    );

    // A synthetic fixture exercising the recorded-response parser, not a real call.
    let mut recorded: Value = serde_json::from_slice(&std::fs::read(&response).unwrap()).unwrap();
    recorded["origin"] = "recorded".into();
    std::fs::write(&response, recorded.to_string()).unwrap();
    assert_eq!(
        replay(&request, &response, &temp.0.join("recorded"))
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn replay_rejects_mismatches_invalid_wording_and_approval_claims() {
    let temp = Temp::new();
    let (request, response) = handoff_fixture(&temp);
    let base: Value = serde_json::from_slice(&std::fs::read(&response).unwrap()).unwrap();
    for (pointer, value) in [
        ("/version", serde_json::json!(99)),
        ("/request_id", serde_json::json!("different")),
        ("/teacher/model", serde_json::json!("different-model")),
        ("/teacher/route", serde_json::json!("different-route")),
        ("/replacements/0/turn_index", serde_json::json!(1)),
        ("/replacements/1/turn_index", serde_json::json!(2)),
        (
            "/replacements/0/content",
            serde_json::json!("The answer is 10."),
        ),
        ("/replacements/0/content", serde_json::json!("")),
        ("/replacements", serde_json::json!([])),
    ] {
        let mut invalid = base.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        std::fs::write(&response, invalid.to_string()).unwrap();
        let out = temp.0.join("must-not-exist");
        assert_eq!(
            replay(&request, &response, &out).status.code(),
            Some(1),
            "{pointer}"
        );
        assert!(!out.exists());
    }
    let mut invalid = base.clone();
    invalid["rights_approved"] = true.into();
    std::fs::write(&response, invalid.to_string()).unwrap();
    assert_eq!(
        replay(&request, &response, &temp.0.join("approval"))
            .status
            .code(),
        Some(1)
    );
    std::fs::write(&response, "x".repeat(65537)).unwrap();
    assert_eq!(
        replay(&request, &response, &temp.0.join("oversized"))
            .status
            .code(),
        Some(1)
    );
    std::fs::write(&response, base.to_string()).unwrap();
    let original: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    for pointer in ["/payload/send_authorized", "/payload/problem/answer/value"] {
        let mut invalid = original.clone();
        *invalid.pointer_mut(pointer).unwrap() = if pointer.ends_with("value") {
            serde_json::json!(11)
        } else {
            serde_json::json!(true)
        };
        std::fs::write(&request, invalid.to_string()).unwrap();
        assert_eq!(
            replay(&request, &response, &temp.0.join("forged-request"))
                .status
                .code(),
            Some(1)
        );
    }
}
