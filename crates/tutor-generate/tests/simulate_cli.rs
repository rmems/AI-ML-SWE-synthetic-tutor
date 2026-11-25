//! End-to-end check of `tutor-generate simulate`: verified problems in,
//! schema-validated student-attempt artifacts out.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tutor-generate"))
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "tutor-generate-simulate-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn write_problem(path: &std::path::Path) {
    let problem = serde_json::json!({
        "schema_version": 1,
        "problem_id": "chain_rule_at_point/v1/seed-1",
        "topic": "calculus.derivatives.chain_rule",
        "generator": {
            "name": "chain_rule_at_point",
            "version": 1,
            "language": "rust",
            "runtime_version": "test",
            "seed": 1
        },
        "statement": "Let p(x) = x^2 and q(x) = 1 + 2x. If h(x) = p(q(x)), compute h'(1).",
        "parameters": {
            "kind": "chain_rule_at_point",
            "outer": [0, 0, 1],
            "inner": [1, 2],
            "point": 1
        },
        "difficulty": {
            "max_degree": 2,
            "coefficient_bound": 9,
            "point_bound": 5
        },
        "answer": {"kind": "integer", "value": 12},
        "steps": [
            {"kind": "evaluate_polynomial", "coefficients": [1, 2], "point": 1, "value": 3},
            {"kind": "differentiate_polynomial", "input": [0, 0, 1], "output": [0, 2]},
            {"kind": "differentiate_polynomial", "input": [1, 2], "output": [2]},
            {"kind": "evaluate_polynomial", "coefficients": [0, 2], "point": 3, "value": 6},
            {"kind": "evaluate_polynomial", "coefficients": [2], "point": 1, "value": 2},
            {"kind": "multiply", "factors": [6, 2], "value": 12}
        ],
        "misconception_ids": ["derivatives.chain_rule"],
        "verification": {"method": "test", "verifier": "test", "verifier_version": "1"}
    });
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("create temp problems")
        .write_all(problem.to_string().as_bytes())
        .expect("write temp problems");
}

#[test]
fn simulate_cli_emits_schema_valid_attempts() {
    let problems = unique_path("problems.jsonl");
    let out = unique_path("out");
    write_problem(&problems);
    let output = bin()
        .args([
            "simulate",
            "--problems",
            problems.to_str().expect("utf-8 temp path"),
            "--out",
            out.to_str().expect("utf-8 temp path"),
            "--seed-start",
            "7",
            "--attempts-per-problem",
            "2",
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
            "--persistence",
            "corrects_after_l2",
            "--schema",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../schemas/student-attempt.schema.json"
            ),
        ])
        .output()
        .expect("runs tutor-generate simulate");
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = std::fs::read_to_string(out.join("attempts.jsonl")).expect("attempts written");
    let rows: Vec<_> = text.lines().collect();
    assert_eq!(rows.len(), 2);
    let first = tutor_schema::StudentAttempt::from_json(rows[0]).expect("valid artifact");
    let second = tutor_schema::StudentAttempt::from_json(rows[1]).expect("valid artifact");
    assert_ne!(first.attempt_id, second.attempt_id);
    assert_eq!(first.transformation.unwrap().corrupted_value, 6);
    assert!(
        std::fs::read_to_string(out.join("rejections.jsonl"))
            .expect("rejections written")
            .trim()
            .is_empty()
    );
}

#[test]
fn rejected_rerun_preserves_previous_attempts() {
    let problems = unique_path("problems.jsonl");
    let out = unique_path("rerun-out");
    write_problem(&problems);
    let schema = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/student-attempt.schema.json"
    );
    let base_args = [
        "simulate".to_owned(),
        "--problems".to_owned(),
        problems.to_str().expect("utf-8 temp path").to_owned(),
        "--out".to_owned(),
        out.to_str().expect("utf-8 temp path").to_owned(),
        "--mode".to_owned(),
        "correction".to_owned(),
        "--schema".to_owned(),
        schema.to_owned(),
    ];
    let output = bin()
        .args(
            base_args.iter().chain(
                [
                    "--misconception".to_owned(),
                    "derivatives.chain_rule".to_owned(),
                ]
                .iter(),
            ),
        )
        .output()
        .expect("runs successful simulation");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let attempts_path = out.join("attempts.jsonl");
    let before = std::fs::read_to_string(&attempts_path).expect("first attempts written");

    let output = bin()
        .args(
            base_args.iter().chain(
                [
                    "--misconception".to_owned(),
                    "derivatives.quotient_rule".to_owned(),
                ]
                .iter(),
            ),
        )
        .output()
        .expect("runs rejected simulation");
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(&attempts_path).expect("previous attempts remain"),
        before
    );
}

#[test]
fn teacher_variety_fails_closed_until_a_teacher_lane_exists() {
    let problems = unique_path("problems.jsonl");
    let out = unique_path("teacher-out");
    write_problem(&problems);
    let output = bin()
        .args([
            "simulate",
            "--problems",
            problems.to_str().expect("utf-8 temp path"),
            "--out",
            out.to_str().expect("utf-8 temp path"),
            "--teacher-variety",
        ])
        .output()
        .expect("runs tutor-generate simulate");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("teacher surface variety"));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
