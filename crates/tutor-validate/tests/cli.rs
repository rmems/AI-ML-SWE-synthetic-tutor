//! End-to-end check of the `tutor-validate` binary: gold rows pass, a
//! deliberately broken row fails with a non-zero exit.

use std::io::Write;
use std::path::PathBuf;
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
    let mut row: serde_json::Value = serde_json::from_str(GOLD).expect("gold parses");
    row["turns"][2]["reveals_final_answer"] = serde_json::json!(true);
    row["misconceptions"] = serde_json::json!(["derivatives.chain_rule", "algebra.sign_error"]);
    let path = std::env::temp_dir().join("tutor-validate-broken-row.json");
    std::fs::File::create(&path)
        .expect("create temp row")
        .write_all(row.to_string().as_bytes())
        .expect("write temp row");
    let output = bin().arg(&path).output().expect("runs tutor-validate");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("INVALID"));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
