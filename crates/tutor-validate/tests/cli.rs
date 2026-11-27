//! End-to-end check of the `tutor-validate` binary: gold rows pass, a
//! deliberately broken row fails with a non-zero exit.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const GOLD: &str = include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json");

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tutor-validate"))
}

fn gold_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../datasets/gold")
        .join(format!("gold-{name}.json"))
}

fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("row");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "tutor-validate-{stem}-{}-{nanos}{extension}",
        std::process::id()
    ))
}

fn write_temp(name: &str, text: &str) -> PathBuf {
    let path = temp_path(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create temp row")
        .write_all(text.as_bytes())
        .expect("write temp row");
    path
}

fn gold_value() -> serde_json::Value {
    serde_json::from_str(GOLD).expect("gold parses")
}

fn compact_jsonl(rows: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&serde_json::to_string(row).unwrap());
        out.push('\n');
    }
    out
}

#[test]
fn gold_rows_pass() {
    let output = bin()
        .arg(gold_path("derivatives-chain-rule"))
        .arg(gold_path("algebra-sign-error"))
        .arg(gold_path("confirmation"))
        .output()
        .expect("runs tutor-validate");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
}

#[test]
fn broken_row_fails() {
    let mut row = gold_value();
    row["turns"][2]["reveals_final_answer"] = serde_json::json!(true);
    row["misconceptions"] = serde_json::json!(["derivatives.chain_rule", "algebra.sign_error"]);
    let path = write_temp("broken-row.json", &row.to_string());
    let output = bin().arg(&path).output().expect("runs tutor-validate");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("INVALID"));
}

#[test]
fn jsonl_rows_pass_line_by_line() {
    let first = gold_value();
    let mut second = gold_value();
    second["row_id"] = serde_json::json!("jsonl-second-row");
    let path = write_temp("rows.jsonl", &compact_jsonl(&[first, second]));
    let output = bin().arg(&path).output().expect("runs tutor-validate");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
}

#[test]
fn duplicate_row_id_in_one_jsonl_file_fails_across_problem_ids() {
    let first = gold_value();
    let mut second = gold_value();
    second["problem_id"] = serde_json::json!("jsonl-different-problem");
    let path = write_temp("duplicate-rows.jsonl", &compact_jsonl(&[first, second]));
    let output = bin().arg(&path).output().expect("runs tutor-validate");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("duplicate row_id"));
}

#[test]
fn reused_row_id_in_different_problem_files_is_allowed() {
    let first = gold_value();
    let mut second = gold_value();
    second["problem_id"] = serde_json::json!("different-family-problem");
    let first_path = write_temp("first-family.json", &first.to_string());
    let second_path = write_temp("second-family.json", &second.to_string());
    let output = bin()
        .arg(&first_path)
        .arg(&second_path)
        .output()
        .expect("runs tutor-validate");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
