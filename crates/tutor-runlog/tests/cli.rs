//! CLI-level checks of `tutor-runlog verify` and `tutor-runlog pause` using
//! temp directories only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tutor_runlog::{
    Counts, GeneratorUse, OutputRef, Record, RunLog, RunLogEntry, RunRecord, SeedUse, Stage,
};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tutor-runlog"))
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "tutor-runlog-cli-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn sample_record(run_id: &str) -> Record {
    Record::Run(Box::new(RunRecord {
        run_id: run_id.to_owned(),
        family: Some("derivative_misconceptions".to_owned()),
        spec: None,
        stage: Stage::Generate,
        replaces: None,
        environment: tutor_runlog::environment(),
        generators: vec![GeneratorUse {
            name: "binomial_derivative_at_point".to_owned(),
            version: 1,
            language: tutor_schema::GeneratorLanguage::Rust,
            seeds: SeedUse {
                start: 0,
                end: Some(3),
                derivation: None,
            },
            parameter_ranges: BTreeMap::new(),
        }],
        teacher: None,
        counts: Counts {
            generated: 3,
            verified: 3,
            rejected: 0,
            rejected_by_reason: BTreeMap::new(),
            deduplicated: None,
            decontaminated: None,
            exported: None,
        },
        output: OutputRef {
            directory: "out".to_owned(),
            manifest_sha256: None,
        },
        split_manifest_sha256: None,
        qa: None,
    }))
}

fn verify(path: &Path) -> Output {
    bin()
        .args(["verify", path.to_str().expect("utf-8 temp path")])
        .output()
        .expect("runs tutor-runlog verify")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn verify_accepts_a_chained_log() {
    let path = unique_path("ok.jsonl");
    let log = RunLog::at(&path);
    log.append(sample_record("run-a")).expect("append");
    log.append(sample_record("run-b")).expect("append");

    let output = verify(&path);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 entries"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn verify_fails_when_the_log_is_missing() {
    let path = unique_path("missing.jsonl");
    let output = verify(&path);
    assert!(
        !output.status.success(),
        "verifying a nonexistent log must fail, stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn verify_fails_on_tampered_history() {
    let path = unique_path("tampered.jsonl");
    let log = RunLog::at(&path);
    log.append(sample_record("run-a")).expect("append");
    log.append(sample_record("run-b")).expect("append");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("run-a", "run-x")).unwrap();

    let output = verify(&path);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("prev_entry_sha256"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn pause_appends_a_gate_pause_entry() {
    let path = unique_path("pause.jsonl");
    let log = RunLog::at(&path);
    log.append(sample_record("run-a")).expect("append");

    let output = bin()
        .args([
            "pause",
            path.to_str().expect("utf-8 temp path"),
            "--family",
            "derivative_misconceptions",
            "--gate",
            "verification",
            "--detail",
            "rejection rate above gate",
            "--run-id",
            "run-a",
            "--evidence",
            "verify_answer_mismatch=12",
            "--evidence",
            "note=\"manual check\"",
        ])
        .output()
        .expect("runs tutor-runlog pause");
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let entries = log.entries().expect("log still verifies");
    assert_eq!(entries.len(), 2);
    let RunLogEntry { record, .. } = &entries[1];
    let Record::GatePause(pause) = record else {
        panic!("expected a gate_pause entry");
    };
    assert_eq!(pause.gate, tutor_runlog::Gate::Verification);
    assert_eq!(pause.run_id.as_deref(), Some("run-a"));
    assert_eq!(
        pause.evidence["verify_answer_mismatch"],
        serde_json::json!(12)
    );
    assert_eq!(pause.evidence["note"], serde_json::json!("manual check"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn pause_rejects_an_unknown_run_id_and_gate() {
    let path = unique_path("bad-pause.jsonl");
    let log = RunLog::at(&path);
    log.append(sample_record("run-a")).expect("append");
    let path_str = path.to_str().expect("utf-8 temp path");

    let unknown_run = bin()
        .args([
            "pause",
            path_str,
            "--family",
            "derivative_misconceptions",
            "--gate",
            "export",
            "--detail",
            "export failed",
            "--run-id",
            "run-nope",
        ])
        .output()
        .expect("runs tutor-runlog pause");
    assert!(!unknown_run.status.success());
    assert!(stderr(&unknown_run).contains("run-nope"));

    let bad_gate = bin()
        .args([
            "pause",
            path_str,
            "--family",
            "derivative_misconceptions",
            "--gate",
            "weekly_review",
            "--detail",
            "not a cadence gate",
        ])
        .output()
        .expect("runs tutor-runlog pause");
    assert!(!bad_gate.status.success());
    assert!(stderr(&bad_gate).contains("unknown gate"));
    assert_eq!(log.entries().unwrap().len(), 1, "no pause was appended");
    let _ = std::fs::remove_file(&path);
}
