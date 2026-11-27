//! Integration tests for the PR-time data-validation gates.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tutor_ci::{lint_taxonomy, read_records, rights_gate, verification_smoke};

const TAXONOMY: &str = include_str!("../../../schemas/taxonomy.yaml");
const RIGHTS: &str = include_str!("../../../docs/rights.md");
const GOLD: &str = include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json");
const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");

fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("fixture");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "tutor-ci-{stem}-{}-{nanos}{extension}",
        std::process::id()
    ))
}

fn write_temp(name: &str, text: &str) -> PathBuf {
    let path = temp_path(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create temp fixture");
    file.write_all(text.as_bytes()).expect("write temp fixture");
    path
}

fn teacher_row_json(teacher: &str) -> String {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["teacher_variety"] = serde_json::json!({
        "requested": true,
        "status": "applied",
        "teacher": teacher
    });
    value.to_string()
}

fn write_teacher_fixture(name: &str, teacher: &str) -> (PathBuf, PathBuf) {
    (
        write_temp("rights.md", RIGHTS),
        write_temp(name, &teacher_row_json(teacher)),
    )
}

fn assert_teacher_rejected(teacher: &str, context: &str, expected_error: &str) {
    let (registry, row) = write_teacher_fixture("teacher.json", teacher);
    let errors = rights_gate(&registry, &[row]).expect_err(context);
    assert!(errors.iter().any(|error| error.contains(expected_error)));
}

fn assert_taxonomy_row_error(value: &Value, context: &str, expected_error: &str) {
    let taxonomy = write_temp("taxonomy.yaml", TAXONOMY);
    let row = write_temp(&format!("{context}.json"), &value.to_string());
    let errors = lint_taxonomy(&taxonomy, &[row]).expect_err(context);
    assert!(errors.iter().any(|error| error.contains(expected_error)));
}

#[test]
fn taxonomy_lint_accepts_registered_ids() {
    let taxonomy = write_temp("taxonomy.yaml", TAXONOMY);
    let row = write_temp("row.json", GOLD);
    let problem = write_temp("problem.json", PROBLEM);
    let report = lint_taxonomy(&taxonomy, &[row, problem]).expect("registered ids pass");
    assert_eq!(report.records, 2);
    assert_eq!(report.references, 3);
}

#[test]
fn taxonomy_lint_rejects_orphan_ids() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["misconceptions"] = serde_json::json!(["derivatives.not_registered"]);
    assert_taxonomy_row_error(&value, "orphan id fails", "orphan misconception id");
}

#[test]
fn taxonomy_lint_reads_references_nested_inside_arrays() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["nested"] = serde_json::json!([
        {"misconception": "derivatives.not_registered"}
    ]);
    assert_taxonomy_row_error(&value, "nested orphan id fails", "orphan misconception id");
}

#[test]
fn taxonomy_lint_rejects_null_list_entries() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["misconceptions"] = serde_json::json!(["derivatives.chain_rule", null]);
    assert_taxonomy_row_error(
        &value,
        "null list entry fails",
        "list entries must not be null",
    );
}

#[test]
fn rights_gate_passes_teacherless_rows_without_registry() {
    let row = write_temp("teacherless.json", GOLD);
    let missing = temp_path("missing-rights.md");
    let report = rights_gate(&missing, &[row]).expect("no citations, no registry needed");
    assert_eq!(report.citations, 0);
}

#[test]
fn rights_gate_accepts_allowed_teacher_citations() {
    let (registry, row) = write_teacher_fixture("teacher.json", "deepseek_v41_flash");
    let report = rights_gate(&registry, &[row]).expect("allowed teacher passes");
    assert_eq!(report.citations, 1);
}

#[test]
fn rights_gate_fails_closed_when_registry_is_absent() {
    let row = write_temp("teacher.json", &teacher_row_json("deepseek_v41_flash"));
    let missing = temp_path("missing-rights.md");
    let errors = rights_gate(&missing, &[row]).expect_err("citation without registry fails");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cannot read rights registry"))
    );
}

#[test]
fn rights_gate_rejects_parked_or_unregistered_teacher() {
    assert_teacher_rejected(
        "nemotron_3_ultra_550b_a55b",
        "parked teacher fails",
        "has no allowed entry",
    );
}

#[test]
fn rights_gate_accepts_mimo_exact_checkpoint_with_revision() {
    let (registry, row) = write_teacher_fixture(
        "mimo-teacher.json",
        "XiaomiMiMo/MiMo-V2.6-Flash-RL@f657ba2f7d24c9adc5730675baaa2d34de878a49",
    );
    let report = rights_gate(&registry, &[row]).expect("pinned checkpoint passes");
    assert_eq!(report.citations, 1);
}

#[test]
fn rights_gate_rejects_mimo_bare_or_mismatched_revision() {
    for (index, teacher) in [
        "XiaomiMiMo/MiMo-V2.6-Flash-RL",
        "XiaomiMiMo/MiMo-V2.6-Flash-RL@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ]
    .into_iter()
    .enumerate()
    {
        let (registry, row) = write_teacher_fixture(&format!("mimo-bad-{index}.json"), teacher);
        assert!(
            rights_gate(&registry, &[row]).is_err(),
            "{teacher} must not pass the checkpoint-pinned gate"
        );
    }
}

#[test]
fn rights_gate_rejects_allowed_heading_without_terms_evidence() {
    let row = write_temp(
        "teacher-no-evidence.json",
        &teacher_row_json("deepseek_v41_flash"),
    );
    let missing_evidence = RIGHTS.replacen("Snapshot pinned 2026-11-24.", "", 1);
    let registry = write_temp("rights-no-evidence.md", &missing_evidence);
    assert!(rights_gate(&registry, &[row]).is_err());
}

#[test]
fn rights_gate_rejects_teacher_name_embedded_in_unregistered_source() {
    assert_teacher_rejected(
        "unregistered-copy-of-deepseek-v4.1-flash",
        "embedded name fails",
        "has no allowed entry",
    );
}

#[test]
fn verification_smoke_passes_and_rejects_wrong_answer() {
    let problem = write_temp("problem.json", PROBLEM);
    let report = verification_smoke(&problem).expect("smoke passes");
    assert_eq!(
        report.problem_id,
        "polynomial_derivative_at_point/v1/seed-7"
    );
    assert_eq!(report.rejection_code, "verify_answer_mismatch");
}

#[test]
fn jsonl_records_are_read_line_by_line() {
    let first: Value = serde_json::from_str(GOLD).unwrap();
    let mut second = first.clone();
    second["row_id"] = Value::String("jsonl-second".to_owned());
    let jsonl = format!(
        "{}\n{}\n",
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    let path = write_temp("rows.jsonl", &jsonl);
    let records = read_records(&[path]).expect("jsonl parses");
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].line, Some(2));
}
