//! Stage 2 — decontamination against the held-out eval set.
//!
//! Every `.json` / `.jsonl` record under the `--heldout` directory
//! (default `evals/heldout/`) is fingerprinted the same way candidate rows
//! are in [`dedup`]: all of the record's string leaves joined into one
//! normalized document, plus every `problem_id` it names. A candidate row
//! is rejected when its `problem_id` appears in the held-out set or its
//! document matches a held-out document exactly (`heldout_exact_match`),
//! or when the shingle Jaccard similarity reaches
//! [`NEAR_DUPLICATE_THRESHOLD`](crate::dedup::NEAR_DUPLICATE_THRESHOLD)
//! (`heldout_near_match`).
//!
//! The held-out set is input-only by construction: this module only reads
//! it, and nothing in the crate generates against it or tunes to it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::{BuildError, RejectedRow, Stage, dedup, input};

/// The loaded held-out set: fingerprints of every held-out record plus the
/// files they came from (recorded as inputs in the manifest).
#[derive(Debug)]
pub struct HeldOut {
    /// Fingerprinted held-out records, in sorted-file then line order.
    pub items: Vec<HeldOutItem>,
    /// The `.json` / `.jsonl` files the records were read from.
    pub files: Vec<PathBuf>,
}

/// One held-out record's fingerprints.
#[derive(Debug)]
pub struct HeldOutItem {
    /// File the record was read from, for rejection detail.
    pub origin: String,
    /// Every `problem_id` named anywhere in the record.
    pub problem_ids: BTreeSet<String>,
    /// Normalized-text fingerprint of all the record's string leaves.
    pub fingerprint: dedup::Fingerprint,
    /// Fingerprint of the record's `statement` field alone, when it has one.
    /// A held-out problem record carries metadata a dialogue never has, so
    /// the whole-document comparison can miss an identical statement filed
    /// under a different `problem_id`.
    pub statement: Option<dedup::Fingerprint>,
}

/// Read and fingerprint every held-out record under `dir`.
///
/// # Errors
/// [`BuildError::HeldOut`] when the directory or a record cannot be read or
/// parsed — a corrupt held-out set must fail closed, not silently shrink
/// the decontamination corpus.
pub fn load(dir: &std::path::Path) -> Result<HeldOut, BuildError> {
    let files = input::heldout_files(dir)?;
    let mut items = Vec::new();
    for file in &files {
        let records = load_file(file)?;
        if records.is_empty() {
            // A held-out file with no record looks like a truncated corpus;
            // fail closed rather than silently shrinking it.
            return Err(BuildError::heldout(file, "file contains no records"));
        }
        items.extend(records);
    }
    Ok(HeldOut { items, files })
}

/// One held-out record's text and where it sits in its file.
struct Chunk<'a> {
    /// `"line N: "` for a `.jsonl` record; empty for a whole `.json` file.
    location: String,
    text: &'a str,
}

/// Every record in one held-out file: the whole file for `.json` (one
/// record), each non-blank line for `.jsonl`.
fn load_file(file: &std::path::Path) -> Result<Vec<HeldOutItem>, BuildError> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| BuildError::heldout(file, format!("cannot read: {e}")))?;
    let chunks: Vec<Chunk<'_>> = if is_jsonl(file) {
        text.lines()
            .enumerate()
            .map(|(index, line)| Chunk {
                location: format!("line {}: ", index + 1),
                text: line,
            })
            .collect()
    } else {
        vec![Chunk {
            location: String::new(),
            text: &text,
        }]
    };
    chunks
        .into_iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .map(|chunk| parse_record(file, &chunk))
        .collect()
}

/// Parse and fingerprint one record. A record with no `problem_id` and less
/// text than one shingle (`[]`, `{}`, `{"topic": "calculus"}`) would count
/// as a record yet decontaminate nothing, so it fails closed.
fn parse_record(file: &std::path::Path, chunk: &Chunk<'_>) -> Result<HeldOutItem, BuildError> {
    let location = &chunk.location;
    let value: serde_json::Value = serde_json::from_str(chunk.text)
        .map_err(|e| BuildError::heldout(file, format!("{location}not JSON: {e}")))?;
    let item = HeldOutItem::new(file, &value);
    let words = item
        .fingerprint
        .document
        .split(' ')
        .filter(|w| !w.is_empty())
        .count();
    let has_statement_text = item
        .statement
        .as_ref()
        .is_some_and(|statement| !statement.document.trim().is_empty());
    if item.problem_ids.is_empty() && !has_statement_text && words < dedup::SHINGLE_WORDS {
        return Err(BuildError::heldout(
            file,
            format!(
                "{location}record has no problem_id or statement and too little text ({words} words) to                  decontaminate against"
            ),
        ));
    }
    Ok(item)
}

impl HeldOutItem {
    fn new(file: &std::path::Path, value: &serde_json::Value) -> Self {
        let mut text = String::new();
        let mut problem_ids = BTreeSet::new();
        collect_strings(value, &mut text, &mut problem_ids);
        Self {
            origin: file.display().to_string(),
            problem_ids,
            fingerprint: dedup::fingerprint(&text),
            statement: value
                .get("statement")
                .and_then(serde_json::Value::as_str)
                .map(dedup::fingerprint),
        }
    }
}

/// Join every string leaf into `text` and collect `problem_id` values.
fn collect_strings(
    value: &serde_json::Value,
    text: &mut String,
    problem_ids: &mut BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "problem_id"
                    && let serde_json::Value::String(id) = child
                {
                    problem_ids.insert(id.clone());
                }
                collect_strings(child, text, problem_ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, text, problem_ids);
            }
        }
        serde_json::Value::String(s) => {
            text.push_str(s);
            text.push(' ');
        }
        _ => {}
    }
}

/// Whether `item` and the candidate both state a problem and the two
/// statements' math differs (a flipped sign, another coefficient or
/// evaluation point). Shingle similarity only sees the tokens near a change,
/// so a long prompt with one sign flipped can still score above the
/// near-match threshold; such a pair is a different problem, not a leak.
fn poses_different_math(item: &HeldOutItem, candidate: Option<&dedup::Fingerprint>) -> bool {
    match (&item.statement, candidate) {
        (Some(held), Some(candidate)) => {
            dedup::math_signature(&held.document) != dedup::math_signature(&candidate.document)
        }
        _ => false,
    }
}

/// Reject kept rows that match the held-out set; pass the rest through.
pub fn run(
    kept: Vec<dedup::Fingerprinted>,
    heldout: &HeldOut,
    rejected: &mut Vec<RejectedRow>,
) -> Vec<dedup::Fingerprinted> {
    let mut clean = Vec::new();
    for row in kept {
        if let Some((code, detail)) = contamination(&row, heldout) {
            rejected.push(row.checked.rejected(Stage::Decontamination, code, detail));
        } else {
            clean.push(row);
        }
    }
    clean
}

/// The rejection code + detail when `row` contaminates `heldout`, else
/// `None`. Exact matches (problem id or identical normalized text) are
/// checked across the whole set before near matches so the reason code is
/// deterministic.
fn contamination(row: &dedup::Fingerprinted, heldout: &HeldOut) -> Option<(&'static str, String)> {
    let problem_id = &row.checked.row.problem_id;
    if let Some(item) = heldout
        .items
        .iter()
        .find(|item| item.problem_ids.contains(problem_id))
    {
        return Some((
            "heldout_exact_match",
            format!("problem_id {problem_id:?} is held out by {}", item.origin),
        ));
    }
    if let Some(item) = heldout
        .items
        .iter()
        .find(|item| item.fingerprint.document == row.fingerprint.document)
    {
        return Some((
            "heldout_exact_match",
            format!("normalized text matches {}", item.origin),
        ));
    }
    // An identical problem statement under a different `problem_id` still
    // contaminates; compare statements directly when both records have one.
    let statement = row
        .checked
        .problem
        .as_ref()
        .map(|problem| dedup::fingerprint(&problem.statement));
    if let Some(fingerprint) = &statement
        && let Some(item) = heldout.items.iter().find(|item| {
            item.statement
                .as_ref()
                .is_some_and(|s| s.document == fingerprint.document)
        })
    {
        return Some((
            "heldout_exact_match",
            format!("problem statement matches {}", item.origin),
        ));
    }
    heldout
        .items
        .iter()
        .filter(|item| !poses_different_math(item, statement.as_ref()))
        .map(|item| {
            let mut similarity =
                dedup::jaccard(&item.fingerprint.shingles, &row.fingerprint.shingles);
            if let (Some(held), Some(candidate)) = (&item.statement, &statement) {
                similarity = similarity.max(dedup::jaccard(&held.shingles, &candidate.shingles));
            }
            (item, similarity)
        })
        .filter(|(_, similarity)| *similarity >= dedup::NEAR_DUPLICATE_THRESHOLD)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(item, similarity)| {
            (
                "heldout_near_match",
                format!(
                    "{similarity:.3} similar to {} (threshold {})",
                    item.origin,
                    dedup::NEAR_DUPLICATE_THRESHOLD
                ),
            )
        })
}

fn is_jsonl(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckedRow;
    use tutor_schema::{DialogueRow, Problem};

    const GOLD_CONFIRM: &str = include_str!("../../../datasets/gold/gold-confirmation.json");
    const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");

    /// A candidate row grounded in the fixture problem.
    fn candidate() -> dedup::Fingerprinted {
        let row = DialogueRow::from_json(GOLD_CONFIRM).unwrap();
        let problem = Problem::from_json(PROBLEM).unwrap();
        let checked = CheckedRow {
            file: PathBuf::from("rows.jsonl"),
            line: None,
            row,
            raw: serde_json::Value::Null,
            problem: Some(problem),
        };
        let fingerprint = dedup::fingerprint(&dedup::document(&checked));
        dedup::Fingerprinted {
            checked,
            fingerprint,
        }
    }

    fn heldout_with(value: serde_json::Value) -> HeldOut {
        HeldOut {
            items: vec![HeldOutItem::new(
                std::path::Path::new("heldout.json"),
                &value,
            )],
            files: vec![],
        }
    }

    #[test]
    fn identical_statement_under_different_id_contaminates() {
        let row = candidate();
        let statement = &row.checked.problem.as_ref().unwrap().statement;
        // The whole dialogue document barely overlaps a problem record's
        // metadata, so only the statement-level check can catch this.
        let heldout = heldout_with(serde_json::json!({
            "problem_id": "different-id",
            "statement": statement,
            "generator": {"name": "other", "version": 1}
        }));
        let (code, _) = contamination(&row, &heldout).expect("statement leak is flagged");
        assert_eq!(code, "heldout_exact_match");
    }

    #[test]
    fn a_sign_flipped_statement_does_not_contaminate() {
        let row = candidate();
        let statement = &row.checked.problem.as_ref().unwrap().statement;
        // `3x^2 + 2x + 5` is a different problem (f'(2) = 14, not 10); it
        // must clear both the exact and the near statement checks.
        let flipped = statement.replace("- 2x", "+ 2x");
        assert_ne!(&flipped, statement);
        let heldout = heldout_with(serde_json::json!({
            "problem_id": "different-id",
            "statement": flipped,
        }));
        assert_eq!(contamination(&row, &heldout), None);
    }

    #[test]
    fn placeholder_heldout_records_fail_closed() {
        let dir =
            std::env::temp_dir().join(format!("tutor-build-placeholder-{}", std::process::id()));
        for (name, text) in [
            ("empty-array.json", "[]"),
            ("empty-object.jsonl", "{}\n"),
            ("metadata-only.json", r#"{"topic": "calculus"}"#),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), text).unwrap();
            assert!(
                matches!(load(&dir), Err(BuildError::HeldOut { .. })),
                "{name} must not pass as a held-out record"
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn a_long_sign_flipped_statement_does_not_near_match() {
        let row = candidate();
        let statement = row.checked.problem.as_ref().unwrap().statement.clone();
        // Pad both statements with the same long prose so one flipped sign
        // leaves most shingles shared.
        let prose = " Show every step of your work, state which differentiation rule \
                     you apply at each step, and simplify the result fully before \
                     substituting the evaluation point into the derivative.";
        let mut long_row = row;
        long_row.checked.problem.as_mut().unwrap().statement = format!("{statement}{prose}");
        let flipped = format!("{}{prose}", statement.replace("- 2x", "+ 2x"));
        let heldout = heldout_with(serde_json::json!({
            "problem_id": "different-id",
            "statement": flipped,
        }));
        assert_eq!(contamination(&long_row, &heldout), None);
    }

    #[test]
    fn unicode_operator_glyphs_still_match() {
        let row = candidate();
        let statement = &row.checked.problem.as_ref().unwrap().statement;
        let unicode = statement.replace('-', "\u{2212}").replace('\'', "\u{2032}");
        assert_ne!(&unicode, statement);
        let heldout = heldout_with(serde_json::json!({
            "problem_id": "different-id",
            "statement": unicode,
        }));
        let (code, _) = contamination(&row, &heldout).expect("same problem in Unicode notation");
        assert_eq!(code, "heldout_exact_match");
    }

    #[test]
    fn unrelated_record_does_not_contaminate() {
        let row = candidate();
        let heldout = heldout_with(serde_json::json!({
            "problem_id": "different-id",
            "statement": "Prove that the sum of angles in a triangle is 180 degrees."
        }));
        assert!(contamination(&row, &heldout).is_none());
    }

    #[test]
    fn collect_strings_gathers_leaves_and_problem_ids() {
        let value = serde_json::json!({
            "problem_id": "p-1",
            "statement": "Compute f'(2).",
            "turns": [{"content": "hello"}],
            "nested": {"problem_id": "p-2"}
        });
        let mut text = String::new();
        let mut ids = BTreeSet::new();
        collect_strings(&value, &mut text, &mut ids);
        assert!(text.contains("Compute f'(2).") && text.contains("hello"));
        assert!(ids.contains("p-1") && ids.contains("p-2"));
    }
}
