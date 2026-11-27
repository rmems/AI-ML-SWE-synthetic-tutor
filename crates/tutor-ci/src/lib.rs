//! PR-time data-validation gates.
//!
//! These checks are deliberately small wiring around the canonical schema types
//! and verifier: taxonomy references are compared with `schemas/taxonomy.yaml`,
//! teacher citations are checked fail-closed against `docs/rights.md`, and the
//! verification smoke test invokes `tutor-verify` rather than duplicating its
//! recomputation logic.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tutor_schema::{Answer, GeneratorLanguage, Problem};
use tutor_verify::{ReasonCode, verify};

/// One JSON value read from a committed data file.
#[derive(Debug, Clone)]
pub struct DataRecord {
    /// File the value came from.
    pub path: PathBuf,
    /// One-based line number for JSONL records; `None` for whole-file JSON.
    pub line: Option<usize>,
    /// Raw JSON value. Checks inspect it before typed schema validation so
    /// malformed rows can still produce precise lint diagnostics.
    pub value: Value,
}

impl DataRecord {
    /// Human-readable source location for diagnostics.
    pub fn location(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path.display()),
            None => self.path.display().to_string(),
        }
    }
}

/// Summary of a successful taxonomy lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxonomyLintReport {
    /// Number of JSON records inspected.
    pub records: usize,
    /// Number of taxonomy references inspected.
    pub references: usize,
}

/// Summary of a successful rights gate run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RightsGateReport {
    /// Number of JSON records inspected.
    pub records: usize,
    /// Number of teacher citations inspected.
    pub citations: usize,
}

/// Summary of a successful verification smoke test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationSmokeReport {
    /// ID of the known-good problem that passed verification.
    pub problem_id: String,
    /// Reason code the deliberately wrong mutation produced.
    pub rejection_code: String,
}

/// Read `.json` files as one JSON value and `.jsonl` files as one JSON value
/// per non-empty line.
///
/// Parse and IO failures are returned as diagnostics rather than ignored:
/// unparseable committed data must not slip through because a lint could not
/// inspect it.
pub fn read_records(paths: &[PathBuf]) -> Result<Vec<DataRecord>, Vec<String>> {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let (file_records, file_errors) = read_record_file(path);
        records.extend(file_records);
        errors.extend(file_errors);
    }
    if errors.is_empty() {
        Ok(records)
    } else {
        Err(errors)
    }
}

fn read_record_file(path: &Path) -> (Vec<DataRecord>, Vec<String>) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "{}: cannot read data file: {error}",
                    path.display()
                )],
            );
        }
    };
    if is_jsonl(path) {
        read_jsonl_records(path, &text)
    } else {
        read_json_record(path, &text)
    }
}

fn read_jsonl_records(path: &Path, text: &str) -> (Vec<DataRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_record(path, line, Some(index + 1)) {
            Ok(record) => records.push(record),
            Err(error) => errors.push(error),
        }
    }
    (records, errors)
}

fn read_json_record(path: &Path, text: &str) -> (Vec<DataRecord>, Vec<String>) {
    match parse_record(path, text, None) {
        Ok(record) => (vec![record], Vec::new()),
        Err(error) => (Vec::new(), vec![error]),
    }
}

fn parse_record(path: &Path, text: &str, line: Option<usize>) -> Result<DataRecord, String> {
    match serde_json::from_str(text) {
        Ok(value) => Ok(DataRecord {
            path: path.to_owned(),
            line,
            value,
        }),
        Err(error) => {
            let location = match line {
                Some(line) => format!("{}:{line}", path.display()),
                None => path.display().to_string(),
            };
            Err(format!("{location}: not JSON: {error}"))
        }
    }
}

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

fn taxonomy_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim_start().strip_prefix("- id:"))
        .map(|id| id.trim().to_owned())
        .collect()
}

/// Lint every taxonomy reference in `data_paths` against the entries in
/// `taxonomy_path`. Deprecated ids remain registered ids; absent ids fail.
pub fn lint_taxonomy(
    taxonomy_path: &Path,
    data_paths: &[PathBuf],
) -> Result<TaxonomyLintReport, Vec<String>> {
    let taxonomy_text = fs::read_to_string(taxonomy_path).map_err(|error| {
        vec![format!(
            "{}: cannot read taxonomy: {error}",
            taxonomy_path.display()
        )]
    })?;
    let mut errors = Vec::new();
    let registered = registered_taxonomy_ids(&taxonomy_text, taxonomy_path, &mut errors);
    let records = read_records_allowing_partial(data_paths, &mut errors);
    let references = inspect_taxonomy_references(&records, &registered, taxonomy_path, &mut errors);

    if errors.is_empty() {
        Ok(TaxonomyLintReport {
            records: records.len(),
            references,
        })
    } else {
        Err(errors)
    }
}

fn registered_taxonomy_ids(
    taxonomy_text: &str,
    taxonomy_path: &Path,
    errors: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut registered = BTreeSet::new();
    for id in taxonomy_ids(taxonomy_text) {
        if id.is_empty() {
            errors.push(format!(
                "{}: taxonomy contains an empty misconception id",
                taxonomy_path.display()
            ));
        } else if !registered.insert(id.clone()) {
            errors.push(format!(
                "{}: duplicate taxonomy id {:?}",
                taxonomy_path.display(),
                id
            ));
        }
    }
    if registered.is_empty() {
        errors.push(format!(
            "{}: taxonomy contains no misconception entries",
            taxonomy_path.display()
        ));
    }
    registered
}

fn read_records_allowing_partial(
    data_paths: &[PathBuf],
    errors: &mut Vec<String>,
) -> Vec<DataRecord> {
    match read_records(data_paths) {
        Ok(records) => records,
        Err(read_errors) => {
            errors.extend(read_errors);
            Vec::new()
        }
    }
}

fn inspect_taxonomy_references(
    records: &[DataRecord],
    registered: &BTreeSet<String>,
    taxonomy_path: &Path,
    errors: &mut Vec<String>,
) -> usize {
    let mut references = 0usize;
    for record in records {
        references += inspect_record_taxonomy_references(record, registered, taxonomy_path, errors);
    }
    references
}

fn inspect_record_taxonomy_references(
    record: &DataRecord,
    registered: &BTreeSet<String>,
    taxonomy_path: &Path,
    errors: &mut Vec<String>,
) -> usize {
    let mut references = 0usize;
    for reference in taxonomy_references(record) {
        match reference {
            Ok(reference) => {
                references += 1;
                if !registered.contains(&reference) {
                    errors.push(format!(
                        "{}: orphan misconception id {reference:?} is not registered in {}",
                        record.location(),
                        taxonomy_path.display()
                    ));
                }
            }
            Err(error) => errors.push(format!("{}: {error}", record.location())),
        }
    }
    references
}

fn taxonomy_references(record: &DataRecord) -> Vec<Result<String, String>> {
    let mut out = Vec::new();
    collect_taxonomy_references(&record.value, "", &mut out);
    out
}

fn collect_taxonomy_references(
    value: &Value,
    json_path: &str,
    out: &mut Vec<Result<String, String>>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{json_path}/{key}");
                collect_direct_taxonomy_reference(key, child, &child_path, out);
                collect_taxonomy_references(child, &child_path, out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_taxonomy_references(child, &format!("{json_path}/{index}"), out);
            }
        }
        _ => {}
    }
}

fn collect_direct_taxonomy_reference(
    key: &str,
    child: &Value,
    child_path: &str,
    out: &mut Vec<Result<String, String>>,
) {
    match key {
        "misconception" => collect_taxonomy_id(child, child_path, out),
        "misconceptions" | "misconception_ids" => {
            collect_taxonomy_id_list(child, child_path, out);
        }
        _ => {}
    }
}

fn collect_taxonomy_id_list(value: &Value, json_path: &str, out: &mut Vec<Result<String, String>>) {
    match value {
        Value::Array(ids) => {
            for (index, id) in ids.iter().enumerate() {
                collect_taxonomy_id(id, &format!("{json_path}/{index}"), out);
            }
        }
        Value::Null => {}
        _ => out.push(Err(format!(
            "{json_path}: taxonomy reference list must be an array"
        ))),
    }
}

fn collect_taxonomy_id(value: &Value, json_path: &str, out: &mut Vec<Result<String, String>>) {
    match value {
        Value::String(id) => out.push(Ok(id.clone())),
        Value::Null => {}
        _ => out.push(Err(format!(
            "{json_path}: taxonomy reference must be a string id"
        ))),
    }
}

#[derive(Debug)]
struct TeacherCitation {
    json_path: String,
    value: String,
}

/// Check teacher citations in `data_paths` against the rights registry.
///
/// The gate is fail-closed for rows that cite a teacher: if the registry is
/// absent or unreadable, or the citation matches no `allowed` teacher entry
/// with pinned snapshot evidence, those rows fail. A row set with no teacher
/// citations passes without needing the registry, as required for purely
/// procedural rows.
pub fn rights_gate(
    registry_path: &Path,
    data_paths: &[PathBuf],
) -> Result<RightsGateReport, Vec<String>> {
    let records = read_records(data_paths)?;
    let (citations, errors) = collect_record_teacher_citations(&records);
    if !errors.is_empty() {
        return Err(errors);
    }
    if citations.is_empty() {
        return Ok(RightsGateReport {
            records: records.len(),
            citations: 0,
        });
    }

    let allowed = load_allowed_sources(registry_path)?;
    let errors = denied_citation_errors(&citations, &allowed, registry_path);
    if errors.is_empty() {
        Ok(RightsGateReport {
            records: records.len(),
            citations: citations.len(),
        })
    } else {
        Err(errors)
    }
}

fn collect_record_teacher_citations(
    records: &[DataRecord],
) -> (Vec<(String, TeacherCitation)>, Vec<String>) {
    let mut citations = Vec::new();
    let mut errors = Vec::new();
    for record in records {
        for citation in teacher_citations(record) {
            match citation {
                Ok(citation) => citations.push((record.location(), citation)),
                Err(error) => errors.push(format!("{}: {error}", record.location())),
            }
        }
    }
    (citations, errors)
}

fn load_allowed_sources(registry_path: &Path) -> Result<Vec<AllowedSource>, Vec<String>> {
    let registry = fs::read_to_string(registry_path).map_err(|error| {
        vec![format!(
            "{}: cannot read rights registry while rows cite teachers: {error}",
            registry_path.display()
        )]
    })?;
    let allowed = allowed_sources(&registry);
    if allowed.is_empty() {
        Err(vec![format!(
            "{}: no allowed source entries found while rows cite teachers",
            registry_path.display()
        )])
    } else {
        Ok(allowed)
    }
}

fn denied_citation_errors(
    citations: &[(String, TeacherCitation)],
    allowed: &[AllowedSource],
    registry_path: &Path,
) -> Vec<String> {
    citations
        .iter()
        .filter(|(_, citation)| !citation_is_allowed(&citation.value, allowed))
        .map(|(location, citation)| {
            format!(
                "{location}: teacher {:?} at {} has no allowed entry with pinned snapshot evidence in {}",
                citation.value,
                citation.json_path,
                registry_path.display()
            )
        })
        .collect()
}

fn teacher_citations(record: &DataRecord) -> Vec<Result<TeacherCitation, String>> {
    let mut out = Vec::new();
    collect_teacher_citations(&record.value, "", &mut out);
    out
}

fn collect_teacher_citations(
    value: &Value,
    json_path: &str,
    out: &mut Vec<Result<TeacherCitation, String>>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{json_path}/{key}");
                collect_direct_teacher_citation(key, child, &child_path, out);
                collect_teacher_citations(child, &child_path, out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_teacher_citations(child, &format!("{json_path}/{index}"), out);
            }
        }
        _ => {}
    }
}

fn collect_direct_teacher_citation(
    key: &str,
    child: &Value,
    child_path: &str,
    out: &mut Vec<Result<TeacherCitation, String>>,
) {
    match key {
        "teacher" | "teacher_id" => collect_teacher_value(child, child_path, out),
        "teachers" => collect_teacher_list(child, child_path, out),
        _ => {}
    }
}

fn collect_teacher_list(
    value: &Value,
    json_path: &str,
    out: &mut Vec<Result<TeacherCitation, String>>,
) {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_teacher_value(item, &format!("{json_path}/{index}"), out);
            }
        }
        Value::Null => {}
        _ => out.push(Err(format!("{json_path}: teacher list must be an array"))),
    }
}

fn collect_teacher_value(
    value: &Value,
    json_path: &str,
    out: &mut Vec<Result<TeacherCitation, String>>,
) {
    match value {
        Value::String(teacher) => out.push(Ok(TeacherCitation {
            json_path: json_path.to_owned(),
            value: teacher.clone(),
        })),
        Value::Null => {}
        Value::Object(map) => {
            let mut found = false;
            for key in ["id", "name", "model", "checkpoint"] {
                if let Some(Value::String(teacher)) = map.get(key) {
                    found = true;
                    out.push(Ok(TeacherCitation {
                        json_path: format!("{json_path}/{key}"),
                        value: teacher.clone(),
                    }));
                }
            }
            if !found {
                out.push(Err(format!(
                    "{json_path}: teacher object must name an id, name, model, or checkpoint"
                )));
            }
        }
        _ => out.push(Err(format!(
            "{json_path}: teacher citation must be a string or object"
        ))),
    }
}

#[derive(Debug)]
struct AllowedSource {
    aliases: BTreeSet<String>,
    checkpoint_pins: BTreeSet<String>,
    has_snapshot: bool,
}

fn allowed_sources(markdown: &str) -> Vec<AllowedSource> {
    let mut sources = Vec::new();
    for (heading, body) in entry_sections(markdown) {
        if !heading.contains("`allowed`") {
            continue;
        }
        let Some(name) = entry_name(&heading) else {
            continue;
        };
        let mut aliases = BTreeSet::new();
        aliases.insert(normalize(&name));
        let without_parenthetical = strip_parentheticals(&name);
        aliases.insert(normalize(&without_parenthetical));
        aliases.retain(|alias| !alias.is_empty());
        let checkpoint_pins = exact_checkpoint_pins(&body)
            .into_iter()
            .map(|(checkpoint, revision)| normalize(&format!("{checkpoint}{revision}")))
            .collect();
        sources.push(AllowedSource {
            aliases,
            checkpoint_pins,
            has_snapshot: has_terms_evidence(&body),
        });
    }
    sources
}

fn exact_checkpoint_pins(body: &str) -> Vec<(String, String)> {
    body.split("Exact checkpoint:")
        .skip(1)
        .filter_map(|after_label| {
            let after_first_backtick = after_label.split_once('`')?.1;
            let (checkpoint, rest) = after_first_backtick.split_once('`')?;
            let after_revision_backtick = rest.split_once('`')?.1;
            let (revision, _) = after_revision_backtick.split_once('`')?;
            Some((checkpoint.to_owned(), revision.to_owned()))
        })
        .collect()
}

fn has_terms_evidence(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("clause relied on:") && body.contains("snapshot pinned")
}

fn entry_sections(markdown: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut heading: Option<String> = None;
    let mut body = String::new();
    for line in markdown.lines() {
        if line.starts_with("### ") {
            if let Some(previous) = heading.take() {
                sections.push((previous, std::mem::take(&mut body)));
            }
            heading = Some(line.trim_start_matches('#').trim().to_owned());
        } else if heading.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(previous) = heading {
        sections.push((previous, body));
    }
    sections
}

fn entry_name(heading: &str) -> Option<String> {
    let (name, _) = heading
        .split_once('—')
        .or_else(|| heading.split_once(" - "))?;
    let mut name = name.trim();
    if let Some((ordinal, rest)) = name.split_once(". ")
        && ordinal.chars().all(|c| c.is_ascii_digit())
    {
        name = rest.trim();
    }
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

fn strip_parentheticals(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn citation_is_allowed(citation: &str, allowed: &[AllowedSource]) -> bool {
    let citation = normalize(citation);
    allowed.iter().any(|source| {
        source.has_snapshot
            && if source.checkpoint_pins.is_empty() {
                source.aliases.contains(&citation)
            } else {
                source.checkpoint_pins.contains(&citation)
            }
    })
}

/// Run the verification smoke test over the committed known-good problem
/// fixture. The good fixture must pass `tutor-verify`; a one-off mutation of
/// its answer must be rejected with `verify_answer_mismatch`.
pub fn verification_smoke(problem_path: &Path) -> Result<VerificationSmokeReport, Vec<String>> {
    let text = fs::read_to_string(problem_path).map_err(|error| {
        vec![format!(
            "{}: cannot read problem fixture: {error}",
            problem_path.display()
        )]
    })?;
    let problem = Problem::from_json(&text).map_err(|error| {
        vec![format!(
            "{}: problem fixture is not schema-valid: {error}",
            problem_path.display()
        )]
    })?;
    if problem.generator.language != GeneratorLanguage::Julia {
        return Err(vec![format!(
            "{}: smoke fixture must be Julia-generated so the in-process Rust verifier is on the opposite side of the boundary",
            problem_path.display()
        )]);
    }

    let verified = verify(&problem).map_err(|rejection| {
        vec![format!(
            "{}: known-good problem was rejected: {rejection}",
            problem_path.display()
        )]
    })?;

    let mut wrong = problem.clone();
    let Answer::Integer { value } = &mut wrong.answer;
    *value = value.checked_add(1).unwrap_or(*value - 1);
    match verify(&wrong) {
        Err(rejection) if rejection.code == ReasonCode::AnswerMismatch => {
            Ok(VerificationSmokeReport {
                problem_id: verified.problem_id,
                rejection_code: rejection.code.as_str().to_owned(),
            })
        }
        Err(rejection) => Err(vec![format!(
            "{}: wrong-answer mutation was rejected with {} instead of verify_answer_mismatch",
            problem_path.display(),
            rejection.code.as_str()
        )]),
        Ok(_) => Err(vec![format!(
            "{}: wrong-answer mutation unexpectedly passed verification",
            problem_path.display()
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        let taxonomy = write_temp("taxonomy.yaml", TAXONOMY);
        let row = write_temp("orphan.json", &value.to_string());
        let errors = lint_taxonomy(&taxonomy, &[row]).expect_err("orphan id fails");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("orphan misconception id"))
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
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "deepseek_v41_flash"
        });
        let registry = write_temp("rights.md", RIGHTS);
        let row = write_temp("teacher.json", &value.to_string());
        let report = rights_gate(&registry, &[row]).expect("allowed teacher passes");
        assert_eq!(report.citations, 1);
    }

    #[test]
    fn rights_gate_fails_closed_when_registry_is_absent() {
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "deepseek_v41_flash"
        });
        let row = write_temp("teacher.json", &value.to_string());
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
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "nemotron_3_ultra_550b_a55b"
        });
        let registry = write_temp("rights.md", RIGHTS);
        let row = write_temp("teacher.json", &value.to_string());
        let errors = rights_gate(&registry, &[row]).expect_err("parked teacher fails");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("has no allowed entry"))
        );
    }

    #[test]
    fn rights_gate_accepts_mimo_exact_checkpoint_with_revision() {
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "XiaomiMiMo/MiMo-V2.6-Flash-RL@f657ba2f7d24c9adc5730675baaa2d34de878a49"
        });
        let registry = write_temp("rights.md", RIGHTS);
        let row = write_temp("mimo-teacher.json", &value.to_string());
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
            let mut value: Value = serde_json::from_str(GOLD).unwrap();
            value["teacher_variety"] = serde_json::json!({
                "requested": true,
                "status": "applied",
                "teacher": teacher
            });
            let registry = write_temp("rights.md", RIGHTS);
            let row = write_temp(&format!("mimo-bad-{index}.json"), &value.to_string());
            assert!(
                rights_gate(&registry, &[row]).is_err(),
                "{teacher} must not pass the checkpoint-pinned gate"
            );
        }
    }

    #[test]
    fn rights_gate_rejects_allowed_heading_without_terms_evidence() {
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "deepseek_v41_flash"
        });
        let missing_evidence = RIGHTS.replacen("Snapshot pinned 2026-11-24.", "", 1);
        let registry = write_temp("rights-no-evidence.md", &missing_evidence);
        let row = write_temp("teacher-no-evidence.json", &value.to_string());
        assert!(rights_gate(&registry, &[row]).is_err());
    }

    #[test]
    fn rights_gate_rejects_teacher_name_embedded_in_unregistered_source() {
        let mut value: Value = serde_json::from_str(GOLD).unwrap();
        value["teacher_variety"] = serde_json::json!({
            "requested": true,
            "status": "applied",
            "teacher": "unregistered-copy-of-deepseek-v4.1-flash"
        });
        let registry = write_temp("rights.md", RIGHTS);
        let row = write_temp("teacher.json", &value.to_string());
        let errors = rights_gate(&registry, &[row]).expect_err("embedded name fails");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("has no allowed entry"))
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
}
