//! End-to-end check of the generation run log: `tutor-generate run` and
//! `tutor-generate simulate` each append a chained, schema-valid entry for the
//! batch they committed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use tutor_runlog::{Record, RunLog, Stage};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tutor-generate"))
}

/// A unique scratch path under the system temp dir, removed (file or
/// directory tree) when the guard drops — including when a test panics.
struct TempPath(PathBuf);

impl std::ops::Deref for TempPath {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TempPath {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unique_path(name: &str) -> TempPath {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    TempPath(std::env::temp_dir().join(format!(
        "tutor-generate-runlog-{name}-{}-{nanos}",
        std::process::id()
    )))
}

fn run_log_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/run-log.schema.json"))
            .expect("run-log schema parses");
    jsonschema::validator_for(&schema).expect("run-log schema compiles")
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

fn run_id_of(entry: &tutor_runlog::RunLogEntry) -> &str {
    let Record::Run(run) = &entry.record else {
        panic!("expected a run entry");
    };
    &run.run_id
}

#[test]
fn simulate_appends_a_chained_schema_valid_entry() {
    let problems = unique_path("problems.jsonl");
    let out = unique_path("simulate-out");
    write_problem(&problems);
    let output = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "confirmation",
            "--family",
            "derivative_misconceptions",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let log_path = out.join("run-log.jsonl");
    let entries = RunLog::at(&log_path).entries().expect("log verifies");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.prev_entry_sha256, None);
    let Record::Run(run) = &entry.record else {
        panic!("expected a run entry");
    };
    assert_eq!(
        (
            run.stage,
            run.family.as_deref(),
            run.replaces.as_deref(),
            run.generators[0].name.as_str(),
        ),
        (
            Stage::Simulate,
            Some("derivative_misconceptions"),
            None,
            "synthetic-student-simulator",
        )
    );
    assert_eq!(
        (
            run.counts.generated,
            run.counts.verified,
            run.counts.rejected
        ),
        (1, 1, 0)
    );
    // Fields a simulation could not have known stay null; the toolchain is
    // always recorded.
    let unknown_are_null =
        run.teacher.is_none() && run.split_manifest_sha256.is_none() && run.qa.is_none();
    assert!(unknown_are_null && !run.environment.rustc_version.is_empty());
    // The entry's manifest digest matches the committed manifest.
    let manifest_sha =
        tutor_runlog::sha256_hex(&std::fs::read(out.join("manifest.json")).expect("manifest"));
    assert_eq!(
        run.output.manifest_sha256.as_deref(),
        Some(manifest_sha.as_str())
    );
    // The committed entry validates against schemas/run-log.schema.json.
    let value = serde_json::to_value(entry).expect("entry serializes");
    let validator = run_log_schema();
    assert!(
        validator.is_valid(&value),
        "entry fails run-log schema: {:?}",
        validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_rerun_replaces_nothing_unless_replaces_is_passed() {
    let problems = unique_path("rerun-problems.jsonl");
    let out = unique_path("rerun-out");
    write_problem(&problems);
    for extra in [
        vec!["--mode", "confirmation"],
        vec![
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
        ],
    ] {
        let output = run_simulation(&problems, &out, &extra);
        assert!(output.status.success(), "stderr: {}", stderr(&output));
    }
    let entries = RunLog::at(out.join("run-log.jsonl"))
        .entries()
        .expect("log verifies");
    assert_eq!(entries.len(), 2);
    // A rerun of the same stage is a new batch, not a replacement: without
    // --replaces the entry records that it replaces nothing.
    let Record::Run(second) = &entries[1].record else {
        panic!("expected a run entry");
    };
    assert_eq!(second.replaces, None);

    // An explicit --replaces names the earlier run and is recorded.
    let first_run_id = run_id_of(&entries[0]).to_owned();
    let output = run_simulation(
        &problems,
        &out,
        &["--mode", "confirmation", "--replaces", &first_run_id],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let entries = RunLog::at(out.join("run-log.jsonl"))
        .entries()
        .expect("log verifies");
    assert_eq!(entries.len(), 3);
    let Record::Run(third) = &entries[2].record else {
        panic!("expected a run entry");
    };
    assert_eq!(third.replaces.as_deref(), Some(first_run_id.as_str()));
}

/// Arguments for a `run` pinned to a closed endpoint so the outcome never
/// depends on whether a math worker happens to be running: every candidate
/// rejects with `verify_verifier_unavailable`.
fn run_args(out: &std::path::Path) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--generator".to_owned(),
        "binomial_derivative_at_point".to_owned(),
        "--count".to_owned(),
        "1".to_owned(),
        "--timeout-secs".to_owned(),
        "1".to_owned(),
        "--endpoint".to_owned(),
        "tcp://127.0.0.1:1".to_owned(),
        "--out".to_owned(),
        out.to_str().expect("utf-8 temp path").to_owned(),
        "--schema".to_owned(),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/problem.schema.json"
        )
        .to_owned(),
    ]
}

#[test]
fn a_fully_rejected_run_still_leaves_a_log_entry() {
    let out = unique_path("rejected-run-out");
    // No math worker answers on the pinned closed endpoint: the binomial
    // generator produces a problem but remote verification times out, so the
    // run commits only rejections + manifest and the CLI fails — the entry is
    // still appended because a rejected-everything run is still a run.
    let output = bin()
        .args(run_args(&out))
        .output()
        .expect("runs tutor-generate run");
    assert!(!output.status.success(), "run without worker must fail");

    let entries = RunLog::at(out.join("run-log.jsonl"))
        .entries()
        .expect("log verifies");
    assert_eq!(entries.len(), 1);
    let Record::Run(run) = &entries[0].record else {
        panic!("expected a run entry");
    };
    assert_eq!(
        (
            run.stage,
            run.counts.generated,
            run.counts.verified,
            run.counts.rejected,
        ),
        (Stage::Generate, 1, 0, 1)
    );
    assert_eq!(
        run.counts.rejected_by_reason["verify_verifier_unavailable"],
        1
    );
    let generator = &run.generators[0];
    assert_eq!(
        (
            generator.name.as_str(),
            generator.language,
            run.environment.julia_version.as_deref(),
        ),
        (
            "binomial_derivative_at_point",
            tutor_schema::GeneratorLanguage::Rust,
            None,
        )
    );
}

#[test]
fn a_rerun_with_an_unknown_replaces_leaves_the_bundle_untouched() {
    let problems = unique_path("doomed-replaces-problems.jsonl");
    let out = unique_path("doomed-replaces-out");
    write_problem(&problems);
    let output = run_simulation(&problems, &out, &["--mode", "confirmation"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let manifest = std::fs::read(out.join("manifest.json")).expect("manifest");
    let attempts = std::fs::read(out.join("attempts.jsonl")).expect("attempts");
    let log = std::fs::read(out.join("run-log.jsonl")).expect("run log");

    let output = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "correction",
            "--misconception",
            "derivatives.chain_rule",
            "--replaces",
            "run-nope",
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("run-nope"),
        "stderr: {}",
        stderr(&output)
    );
    // The doomed rerun never touched the batch it would have replaced.
    assert_eq!(std::fs::read(out.join("manifest.json")).unwrap(), manifest);
    assert_eq!(std::fs::read(out.join("attempts.jsonl")).unwrap(), attempts);
    assert_eq!(std::fs::read(out.join("run-log.jsonl")).unwrap(), log);
}

#[test]
fn a_run_log_aliasing_an_output_artifact_is_rejected() {
    let problems = unique_path("alias-problems.jsonl");
    let out = unique_path("alias-out");
    write_problem(&problems);
    // Zero rejections: without the guard the log write would land inside the
    // empty committed rejections file and corrupt it after publication.
    let rejections = out.join("rejections.jsonl");
    let output = run_simulation(
        &problems,
        &out,
        &[
            "--mode",
            "confirmation",
            "--run-log",
            rejections.to_str().expect("utf-8 temp path"),
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("aliases the batch artifact"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!rejections.exists(), "no bundle was published");
}

#[test]
fn a_run_log_on_a_run_staging_or_lock_file_is_rejected() {
    for name in [
        "manifest.json.tmp",
        ".run.lock",
        // Nested under a reserved file: the log's lock would otherwise
        // create a directory where the manifest staging file must go.
        "manifest.json.tmp/run-log.jsonl",
    ] {
        let out = unique_path("staging-alias-out");
        let log = out.join(name);
        let mut args = run_args(&out);
        args.extend([
            "--run-log".to_owned(),
            log.to_str().expect("utf-8 temp path").to_owned(),
        ]);
        let output = bin().args(args).output().expect("runs tutor-generate run");
        assert!(
            !output.status.success() && stderr(&output).contains("the batch artifact"),
            "{name}: stderr: {}",
            stderr(&output)
        );
        let untouched =
            !out.join("manifest.json").exists() && !out.join("manifest.json.tmp").is_dir();
        assert!(
            untouched,
            "{name}: nothing was published and no directory was created at the staging path"
        );
    }
}

#[test]
fn a_held_output_lock_blocks_a_run() {
    let out = unique_path("locked-run-out");
    std::fs::create_dir_all(&*out).unwrap();
    std::fs::write(out.join(".run.lock"), "pid 0\n").unwrap();
    let output = bin()
        .args(run_args(&out))
        .output()
        .expect("runs tutor-generate run");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("held by another writer"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!out.join("manifest.json").exists(), "nothing was published");
}

#[test]
fn the_generation_manifest_binds_artifact_contents() {
    let out = unique_path("manifest-binding-out");
    // The closed endpoint rejects every candidate, but the manifest and
    // rejections file are still committed.
    let output = bin()
        .args(run_args(&out))
        .output()
        .expect("runs tutor-generate run");
    assert!(!output.status.success());

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("manifest.json")).expect("manifest written"),
    )
    .expect("manifest parses");
    let rejections = std::fs::read(out.join("rejected/rejections.jsonl")).expect("rejections");
    let digest = Sha256::digest(&rejections);
    assert_eq!(
        manifest["artifacts"]["rejected/rejections.jsonl"]["sha256"],
        serde_json::json!(format!("{digest:x}"))
    );
    assert_eq!(
        manifest["artifacts"]["rejected/rejections.jsonl"]["bytes"],
        serde_json::json!(rejections.len())
    );
    // No verified batch was committed, so none is described.
    assert!(manifest["artifacts"]["verified/problems.jsonl"].is_null());
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
