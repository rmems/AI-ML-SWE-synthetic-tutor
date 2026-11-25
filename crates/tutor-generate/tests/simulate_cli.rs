//! End-to-end check of `tutor-generate simulate`: verified problems in,
//! schema-validated student-attempt artifacts out.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

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

fn run_simulation(
    problems: &std::path::Path,
    out: &std::path::Path,
    extra_args: &[&str],
) -> Output {
    let schema = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/student-attempt.schema.json"
    );
    let mut args = vec![
        "simulate".to_owned(),
        "--problems".to_owned(),
        problems.to_str().expect("utf-8 temp path").to_owned(),
        "--out".to_owned(),
        out.to_str().expect("utf-8 temp path").to_owned(),
        "--schema".to_owned(),
        schema.to_owned(),
    ];
    args.extend(extra_args.iter().map(|arg| (*arg).to_owned()));
    bin().args(args).output().expect("runs simulation")
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
fn attempt_seeds_are_stable_across_unrelated_input_lines() {
    let problems = unique_path("problems.jsonl");
    write_problem(&problems);
    let shifted = unique_path("shifted-problems.jsonl");
    let problem_text = std::fs::read_to_string(&problems).expect("read problem");
    std::fs::write(
        &shifted,
        format!(
            "not-json

{problem_text}"
        ),
    )
    .expect("write shifted");

    let first_out = unique_path("stable-out-a");
    let second_out = unique_path("stable-out-b");
    let schema = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/student-attempt.schema.json"
    );
    for (input, out) in [(&problems, &first_out), (&shifted, &second_out)] {
        let output = bin()
            .args([
                "simulate",
                "--problems",
                input.to_str().expect("utf-8 temp path"),
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
                "--schema",
                schema,
            ])
            .output()
            .expect("runs simulation");
        assert!(output.status.success(), "stderr: {}", stderr(&output));
    }
    assert_eq!(
        std::fs::read_to_string(first_out.join("attempts.jsonl")).expect("first attempts"),
        std::fs::read_to_string(second_out.join("attempts.jsonl")).expect("second attempts")
    );
}

#[test]
fn duplicate_problem_ids_are_rejected_instead_of_duplicating_attempt_ids() {
    let problems = unique_path("duplicate-problems.jsonl");
    write_problem(&problems);
    let line = std::fs::read_to_string(&problems).expect("read problem");
    std::fs::write(
        &problems,
        format!(
            "{line}
{line}"
        ),
    )
    .expect("duplicate problem");
    let out = unique_path("duplicate-out");
    let output = bin()
        .args([
            "simulate",
            "--problems",
            problems.to_str().expect("utf-8 temp path"),
            "--out",
            out.to_str().expect("utf-8 temp path"),
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
            "--schema",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../schemas/student-attempt.schema.json"
            ),
        ])
        .output()
        .expect("runs simulation");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let rejections =
        std::fs::read_to_string(out.join("rejections.jsonl")).expect("rejections written");
    assert!(rejections.contains("duplicate_problem"));
    let attempts = std::fs::read_to_string(out.join("attempts.jsonl")).expect("attempts written");
    assert_eq!(attempts.lines().count(), 1);
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
fn a_stale_duplicate_does_not_block_a_valid_copy_of_the_problem() {
    let problems = unique_path("stale-duplicate-problems.jsonl");
    write_problem(&problems);
    let valid = std::fs::read_to_string(&problems).expect("read problem");
    let mut stale: serde_json::Value = serde_json::from_str(&valid).expect("problem parses");
    stale["steps"][5]["value"] = serde_json::json!(13);
    stale["answer"]["value"] = serde_json::json!(13);
    std::fs::write(&problems, format!("{stale}\n{}", valid.trim_end()))
        .expect("write stale duplicate input");
    let out = unique_path("stale-duplicate-out");
    let output = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let attempts = std::fs::read_to_string(out.join("attempts.jsonl")).expect("attempts written");
    assert_eq!(attempts.lines().count(), 1);
    let rejections =
        std::fs::read_to_string(out.join("rejections.jsonl")).expect("rejections written");
    assert!(rejections.contains("does not replay as verified"));
    assert!(!rejections.contains("duplicate_problem"));
}

#[test]
fn successful_rerun_replaces_the_bundle_and_cleans_its_lock() {
    let problems = unique_path("rerun-problems.jsonl");
    let out = unique_path("successful-rerun-out");
    write_problem(&problems);
    let correction = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
        ],
    );
    assert!(
        correction.status.success(),
        "stderr: {}",
        stderr(&correction)
    );
    let incorrect = std::fs::read_to_string(out.join("attempts.jsonl")).expect("first attempts");

    let confirmation = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(
        confirmation.status.success(),
        "stderr: {}",
        stderr(&confirmation)
    );
    let correct =
        std::fs::read_to_string(out.join("attempts.jsonl")).expect("replacement attempts");
    assert_ne!(incorrect, correct);
    assert!(
        !out.join(".simulate.lock").exists(),
        "output lock is cleaned up"
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("manifest.json")).expect("manifest written"),
    )
    .expect("manifest parses");
    let digest = Sha256::digest(correct.as_bytes());
    assert_eq!(
        manifest["artifacts"]["attempts.jsonl"]["sha256"],
        serde_json::json!(format!("{digest:x}"))
    );
    assert_eq!(
        manifest["artifacts"]["attempts.jsonl"]["bytes"],
        serde_json::json!(correct.len())
    );
}

#[test]
fn an_existing_output_lock_blocks_a_rerun_before_replacing_artifacts() {
    let problems = unique_path("locked-problems.jsonl");
    let out = unique_path("locked-out");
    write_problem(&problems);
    let first = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
        ],
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));
    let before = std::fs::read_to_string(out.join("attempts.jsonl")).expect("first attempts");
    std::fs::write(out.join(".simulate.lock"), "pid test").expect("write test lock");

    let second = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(!second.status.success());
    assert!(stderr(&second).contains("already locked"));
    assert_eq!(
        std::fs::read_to_string(out.join("attempts.jsonl")).expect("attempts remain"),
        before
    );
}

fn artifact_metadata(bytes: &[u8]) -> serde_json::Value {
    let digest = Sha256::digest(bytes);
    serde_json::json!({
        "sha256": format!("{digest:x}"),
        "bytes": bytes.len(),
    })
}

fn write_pending_record(out: &std::path::Path, attempts: &[u8], rejections: &[u8]) {
    let pending = serde_json::json!({
        "simulator": "synthetic-student-simulator",
        "artifacts": {
            "attempts.jsonl": artifact_metadata(attempts),
            "rejections.jsonl": artifact_metadata(rejections),
        }
    });
    std::fs::write(out.join(".simulate-pending.json"), pending.to_string())
        .expect("write pending record");
}

#[test]
fn foreign_artifact_files_without_a_pending_record_are_refused() {
    let problems = unique_path("foreign-problems.jsonl");
    let out = unique_path("foreign-out");
    write_problem(&problems);
    std::fs::create_dir_all(&out).expect("create output directory");
    std::fs::write(out.join("attempts.jsonl"), "foreign\n").expect("write foreign artifact");

    let output = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("without simulator provenance"));
    assert_eq!(
        std::fs::read_to_string(out.join("attempts.jsonl")).expect("foreign artifact remains"),
        "foreign\n"
    );
}

#[test]
fn a_matching_pending_record_allows_replacing_an_interrupted_bundle() {
    let problems = unique_path("pending-problems.jsonl");
    let out = unique_path("pending-out");
    write_problem(&problems);
    std::fs::create_dir_all(&out).expect("create output directory");
    std::fs::write(out.join("attempts.jsonl"), "partial\n").expect("write partial artifact");
    write_pending_record(&out, b"partial\n", b"");

    let output = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_ne!(
        std::fs::read_to_string(out.join("attempts.jsonl")).expect("replacement attempts"),
        "partial\n"
    );
    assert!(
        !out.join(".simulate-pending.json").exists(),
        "pending record is cleared after the manifest commits"
    );
}

#[test]
fn artifacts_that_do_not_match_the_pending_record_are_refused() {
    let problems = unique_path("mismatched-problems.jsonl");
    let out = unique_path("mismatched-out");
    write_problem(&problems);
    std::fs::create_dir_all(&out).expect("create output directory");
    std::fs::write(out.join("attempts.jsonl"), "changed\n").expect("write changed artifact");
    write_pending_record(&out, b"partial\n", b"");

    let output = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("does not match the interrupted simulation record"));
    assert_eq!(
        std::fs::read_to_string(out.join("attempts.jsonl")).expect("artifact remains"),
        "changed\n"
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
