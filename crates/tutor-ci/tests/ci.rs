//! Integration tests for the PR-time data-validation gates.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde_json::Value;
use tutor_ci::{lint_taxonomy, read_records, rights_gate, verification_smoke};

const TAXONOMY: &str = include_str!("../../../schemas/taxonomy.yaml");
const RIGHTS: &str = include_str!("../../../docs/rights.md");
const GOLD: &str = include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json");
const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");

#[derive(Clone, Copy)]
enum FixtureKind {
    Json,
    Jsonl,
    Yaml,
    Md,
}

impl FixtureKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Yaml => "yaml",
            Self::Md => "md",
        }
    }
}

fn temp_path(kind: FixtureKind) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "tutor-ci-fixture-{}-{nanos}.{}",
        std::process::id(),
        kind.extension()
    ))
}

fn write_temp(kind: FixtureKind, text: &str) -> PathBuf {
    let path = temp_path(kind);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create temp fixture");
    file.write_all(text.as_bytes()).expect("write temp fixture");
    path
}

struct Teacher<'a>(&'a str);

fn teacher_row_json(teacher: Teacher<'_>) -> String {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["teacher_variety"] = serde_json::json!({
        "requested": true,
        "status": "applied",
        "teacher": teacher.0
    });
    value.to_string()
}

fn write_teacher_fixture(teacher: Teacher<'_>) -> (PathBuf, PathBuf) {
    (
        write_temp(FixtureKind::Md, RIGHTS),
        write_temp(FixtureKind::Json, &teacher_row_json(teacher)),
    )
}

struct TeacherRejection<'a> {
    teacher: Teacher<'a>,
    context: &'static str,
    expected_error: &'static str,
}

fn assert_teacher_rejected(case: TeacherRejection<'_>) {
    let (registry, row) = write_teacher_fixture(case.teacher);
    let errors = rights_gate(&registry, &[row]).expect_err(case.context);
    assert!(
        errors
            .iter()
            .any(|error| error.contains(case.expected_error))
    );
}

struct TaxonomyCase<'a> {
    value: &'a Value,
    context: &'static str,
    expected_error: &'static str,
}

fn assert_taxonomy_row_error(case: TaxonomyCase<'_>) {
    let taxonomy = write_temp(FixtureKind::Yaml, TAXONOMY);
    let row = write_temp(FixtureKind::Json, &case.value.to_string());
    let errors = lint_taxonomy(&taxonomy, &[row]).expect_err(case.context);
    assert!(
        errors
            .iter()
            .any(|error| error.contains(case.expected_error))
    );
}

#[test]
fn taxonomy_lint_accepts_registered_ids() {
    let taxonomy = write_temp(FixtureKind::Yaml, TAXONOMY);
    let row = write_temp(FixtureKind::Json, GOLD);
    let problem = write_temp(FixtureKind::Json, PROBLEM);
    let report = lint_taxonomy(&taxonomy, &[row, problem]).expect("registered ids pass");
    assert_eq!(report.records, 2);
    assert_eq!(report.references, 3);
}

#[test]
fn taxonomy_lint_rejects_orphan_ids() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["misconceptions"] = serde_json::json!(["derivatives.not_registered"]);
    assert_taxonomy_row_error(TaxonomyCase {
        value: &value,
        context: "orphan id fails",
        expected_error: "orphan misconception id",
    });
}

#[test]
fn taxonomy_lint_reads_references_nested_inside_arrays() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["nested"] = serde_json::json!([
        {"misconception": "derivatives.not_registered"}
    ]);
    assert_taxonomy_row_error(TaxonomyCase {
        value: &value,
        context: "nested orphan id fails",
        expected_error: "orphan misconception id",
    });
}

#[test]
fn taxonomy_lint_rejects_null_list_entries() {
    let mut value: Value = serde_json::from_str(GOLD).unwrap();
    value["misconceptions"] = serde_json::json!(["derivatives.chain_rule", null]);
    assert_taxonomy_row_error(TaxonomyCase {
        value: &value,
        context: "null list entry fails",
        expected_error: "list entries must not be null",
    });
}

#[test]
fn rights_gate_passes_teacherless_rows_without_registry() {
    let row = write_temp(FixtureKind::Json, GOLD);
    let missing = temp_path(FixtureKind::Md);
    let report = rights_gate(&missing, &[row]).expect("no citations, no registry needed");
    assert_eq!(report.citations, 0);
}

#[test]
fn rights_gate_accepts_allowed_teacher_citations() {
    let (registry, row) = write_teacher_fixture(Teacher("deepseek_v41_flash"));
    let report = rights_gate(&registry, &[row]).expect("allowed teacher passes");
    assert_eq!(report.citations, 1);
}

#[test]
fn rights_gate_fails_closed_when_registry_is_absent() {
    let row = write_temp(
        FixtureKind::Json,
        &teacher_row_json(Teacher("deepseek_v41_flash")),
    );
    let missing = temp_path(FixtureKind::Md);
    let errors = rights_gate(&missing, &[row]).expect_err("citation without registry fails");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cannot read rights registry"))
    );
}

#[test]
fn rights_gate_rejects_parked_or_unregistered_teacher() {
    assert_teacher_rejected(TeacherRejection {
        teacher: Teacher("nemotron_3_ultra_550b_a55b"),
        context: "parked teacher fails",
        expected_error: "has no allowed entry",
    });
}

#[test]
fn rights_gate_accepts_mimo_exact_checkpoint_with_revision() {
    let (registry, row) = write_teacher_fixture(Teacher(
        "XiaomiMiMo/MiMo-V2.6-Flash-RL@f657ba2f7d24c9adc5730675baaa2d34de878a49",
    ));
    let report = rights_gate(&registry, &[row]).expect("pinned checkpoint passes");
    assert_eq!(report.citations, 1);
}

#[test]
fn rights_gate_rejects_mimo_bare_or_mismatched_revision() {
    for teacher in [
        "XiaomiMiMo/MiMo-V2.6-Flash-RL",
        "XiaomiMiMo/MiMo-V2.6-Flash-RL@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ]
    .into_iter()
    {
        let (registry, row) = write_teacher_fixture(Teacher(teacher));
        assert!(
            rights_gate(&registry, &[row]).is_err(),
            "{teacher} must not pass the checkpoint-pinned gate"
        );
    }
}

#[test]
fn rights_gate_rejects_allowed_heading_without_terms_evidence() {
    let row = write_temp(
        FixtureKind::Json,
        &teacher_row_json(Teacher("deepseek_v41_flash")),
    );
    let missing_evidence = RIGHTS.replacen("Snapshot pinned 2026-11-24.", "", 1);
    let registry = write_temp(FixtureKind::Md, &missing_evidence);
    assert!(rights_gate(&registry, &[row]).is_err());
}

#[test]
fn rights_gate_rejects_teacher_name_embedded_in_unregistered_source() {
    assert_teacher_rejected(TeacherRejection {
        teacher: Teacher("unregistered-copy-of-deepseek-v4.1-flash"),
        context: "embedded name fails",
        expected_error: "has no allowed entry",
    });
}

#[test]
fn verification_smoke_passes_and_rejects_wrong_answer() {
    let problem = write_temp(FixtureKind::Json, PROBLEM);
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
    let path = write_temp(FixtureKind::Jsonl, &jsonl);
    let records = read_records(&[path]).expect("jsonl parses");
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].line, Some(2));
}
