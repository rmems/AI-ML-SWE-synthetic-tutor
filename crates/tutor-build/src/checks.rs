//! Stage 1 — dialogue checks.
//!
//! Validation reuses the existing machinery rather than duplicating it: the
//! generated `dialogue.schema.json` contract from
//! [`tutor_schema::dialogue_json_schema`] for shape, then
//! [`DialogueRow::from_json`] on the raw text for the policy constraints
//! serde cannot express (turn alternation, hint-ladder rungs, single primary
//! misconception, no final-answer reveal before the last tutor turn).
//!
//! When `--problems` is given, each row is also cross-checked against the
//! verified problem it names: the problem must exist (required outright for
//! `generated` rows, whose seed/parameters/verification provenance lives on
//! the problem), carry a verification record, share the row's topic, and
//! have its answer confirmed in the closing turn.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use tutor_schema::{Answer, DialogueRow, Problem, RowSource};

use crate::{RejectedRow, Stage, input::InputLine};

/// A row that passed dialogue checks and is ready for dedup.
#[derive(Debug)]
pub struct CheckedRow {
    /// Input file the row came from.
    pub file: PathBuf,
    /// One-based JSONL line number, when applicable.
    pub line: Option<usize>,
    /// The validated row.
    pub row: DialogueRow,
    /// The raw JSON, kept for rejection records downstream.
    pub raw: serde_json::Value,
    /// The verified problem the row is grounded in, when `--problems` named
    /// it. `None` for hand-written rows without a matching problem.
    pub problem: Option<Problem>,
}

impl CheckedRow {
    /// Build a rejection record for this row at a later stage.
    pub fn rejected(&self, stage: Stage, code: &'static str, detail: String) -> RejectedRow {
        RejectedRow {
            file: self.file.display().to_string(),
            line: self.line,
            row_id: Some(self.row.row_id.clone()),
            stage,
            code: code.to_owned(),
            detail,
            row: Some(self.raw.clone()),
        }
    }
}

/// Run the stage-1 checks on one input line.
///
/// `seen_row_ids` carries row IDs across the whole input; a repeat is a
/// `duplicate_row_id` rejection.
pub fn check(
    input: &InputLine,
    problems: Option<&BTreeMap<String, Problem>>,
    validator: &jsonschema::Validator,
    seen_row_ids: &mut HashSet<String>,
) -> Result<CheckedRow, Box<RejectedRow>> {
    let fail = |code: &'static str, detail: String, row: Option<serde_json::Value>| {
        Box::new(RejectedRow {
            file: input.file.display().to_string(),
            line: input.line,
            row_id: row
                .as_ref()
                .and_then(|v| v["row_id"].as_str().map(str::to_owned)),
            stage: Stage::DialogueChecks,
            code: code.to_owned(),
            detail,
            row,
        })
    };
    let value: serde_json::Value = match serde_json::from_str(&input.text) {
        Ok(value) => value,
        Err(error) => {
            return Err(fail("invalid_json", format!("not JSON: {error}"), None));
        }
    };
    // Pass one: the generated JSON Schema (shape and field types).
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|error| error.to_string())
        .collect();
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        return Err(fail("schema_shape", detail, Some(value)));
    }
    // Pass two: policy constraints on the raw text (a `Value` hides
    // duplicate fields), exactly as `tutor-validate` does.
    let row = match DialogueRow::from_json(&input.text) {
        Ok(row) => row,
        Err(error) => {
            let detail = format!("{error} ({})", error.code());
            return Err(fail(error.code(), detail, Some(value)));
        }
    };
    if !seen_row_ids.insert(row.row_id.clone()) {
        return Err(fail(
            "duplicate_row_id",
            format!("duplicate row_id {:?}", row.row_id),
            Some(value),
        ));
    }
    let problem = problems.and_then(|index| index.get(&row.problem_id));
    check_provenance(&row, problems.is_some(), problem)
        .map_err(|(code, detail)| fail(code, detail, Some(value.clone())))?;
    Ok(CheckedRow {
        file: input.file.clone(),
        line: input.line,
        row,
        raw: value,
        problem: problem.cloned(),
    })
}

/// Provenance and grounding rules: no row ships without full provenance.
fn check_provenance(
    row: &DialogueRow,
    problems_given: bool,
    problem: Option<&Problem>,
) -> Result<(), (&'static str, String)> {
    // A `generated` row's seed, parameters, and verification record live on
    // its problem; without `--problems` (or a matching entry) the row cannot
    // carry full provenance and does not ship.
    if row.provenance.source == RowSource::Generated && !problems_given {
        return Err((
            "provenance_missing_problem",
            "generated rows require --problems so seed, parameters, and verification provenance can ship"
                .to_owned(),
        ));
    }
    if row.provenance.source == RowSource::Generated && problem.is_none() {
        return Err((
            "provenance_missing_problem",
            format!("no problem {:?} in the problems file", row.problem_id),
        ));
    }
    let Some(problem) = problem else {
        // Hand-written row with no matching problem: its provenance is the
        // hand-written record itself.
        return Ok(());
    };
    if problem.verification.is_none() {
        return Err((
            "provenance_unverified_problem",
            format!(
                "problem {:?} carries no verification record",
                problem.problem_id
            ),
        ));
    }
    if problem.topic != row.topic {
        return Err((
            "problem_topic_mismatch",
            format!(
                "row topic {:?} != problem {:?} topic {:?}",
                row.topic, problem.problem_id, problem.topic
            ),
        ));
    }
    if !closing_states_answer(row, &problem.answer) {
        return Err((
            "verified_answer_mismatch",
            format!(
                "closing turn does not state the verified answer for problem {:?}",
                problem.problem_id
            ),
        ));
    }
    Ok(())
}

/// The closing turn must confirm the verified answer. For the integer
/// answers the schema currently models, that means the answer's digits
/// appear in the closing content as a standalone token (not inside a longer
/// number).
fn closing_states_answer(row: &DialogueRow, answer: &Answer) -> bool {
    let Some(closing) = row.turns.last() else {
        return false;
    };
    match answer {
        Answer::Integer { value } => contains_int_token(&closing.content, *value),
    }
}

/// Whether `value` appears in `text` as a digit string not glued to more
/// digits on either side.
fn contains_int_token(text: &str, value: i64) -> bool {
    let needle = value.to_string();
    text.match_indices(&needle).any(|(index, _)| {
        let before_ok = text[..index]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_digit());
        let after_ok = text[index + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_digit());
        before_ok && after_ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLD_CHAIN: &str =
        include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json");
    const GOLD_CONFIRM: &str = include_str!("../../../datasets/gold/gold-confirmation.json");
    const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");

    fn line(text: &str) -> InputLine {
        InputLine {
            file: PathBuf::from("rows.jsonl"),
            line: Some(1),
            text: text.to_owned(),
        }
    }

    fn validator() -> jsonschema::Validator {
        jsonschema::validator_for(&tutor_schema::dialogue_json_schema()).unwrap()
    }

    fn problems() -> BTreeMap<String, Problem> {
        let mut problem = Problem::from_json(PROBLEM).unwrap();
        // The shared fixture is unverified; attach the record a verified
        // problems.jsonl would carry.
        problem.verification = Some(tutor_schema::Verification {
            method: "exact_integer_recompute".to_owned(),
            verifier: "tutor-verify".to_owned(),
            verifier_version: "0.1.0".to_owned(),
        });
        BTreeMap::from([(problem.problem_id.clone(), problem)])
    }

    #[test]
    fn hand_written_gold_row_passes() {
        let mut seen = HashSet::new();
        let checked = check(&line(GOLD_CHAIN), None, &validator(), &mut seen).unwrap();
        assert_eq!(checked.row.row_id, "gold-derivatives-chain-rule-0001");
        assert!(checked.problem.is_none());
    }

    #[test]
    fn not_json_is_rejected() {
        let mut seen = HashSet::new();
        let rejected = check(&line("{nope"), None, &validator(), &mut seen).unwrap_err();
        assert_eq!(rejected.code, "invalid_json");
        assert_eq!(rejected.stage, Stage::DialogueChecks);
    }

    #[test]
    fn schema_shape_is_rejected() {
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CHAIN).unwrap();
        row["turns"] = serde_json::json!("not-an-array");
        let mut seen = HashSet::new();
        let rejected = check(&line(&row.to_string()), None, &validator(), &mut seen).unwrap_err();
        assert_eq!(rejected.code, "schema_shape");
    }

    #[test]
    fn policy_violation_is_schema_constraint() {
        // A reveal flag on a diagnosis turn violates the no-early-answer rule.
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CHAIN).unwrap();
        row["turns"][2]["reveals_final_answer"] = serde_json::json!(true);
        let mut seen = HashSet::new();
        let rejected = check(&line(&row.to_string()), None, &validator(), &mut seen).unwrap_err();
        assert_eq!(rejected.code, "schema_constraint");
    }

    #[test]
    fn duplicate_row_id_is_rejected() {
        let mut seen = HashSet::new();
        check(&line(GOLD_CHAIN), None, &validator(), &mut seen).unwrap();
        let rejected = check(&line(GOLD_CHAIN), None, &validator(), &mut seen).unwrap_err();
        assert_eq!(rejected.code, "duplicate_row_id");
    }

    #[test]
    fn generated_row_requires_problems() {
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CONFIRM).unwrap();
        row["gold"] = serde_json::json!(false);
        row["provenance"] = serde_json::json!({
            "source": "generated",
            "taxonomy_version": 1,
            "generator": {"name": "dialogue_writer", "version": 1}
        });
        let mut seen = HashSet::new();
        let rejected = check(&line(&row.to_string()), None, &validator(), &mut seen).unwrap_err();
        assert_eq!(rejected.code, "provenance_missing_problem");
    }

    #[test]
    fn generated_row_checks_against_its_problem() {
        let index = problems();
        let problem = index.values().next().unwrap();
        // A generated confirmation row grounded in the fixture problem: the
        // closing turn confirms the verified answer 10.
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CONFIRM).unwrap();
        row["row_id"] = serde_json::json!("gen-confirm-0001");
        row["problem_id"] = serde_json::json!(problem.problem_id);
        row["gold"] = serde_json::json!(false);
        row["provenance"] = serde_json::json!({
            "source": "generated",
            "taxonomy_version": 1,
            "generator": {"name": "dialogue_writer", "version": 1}
        });
        row["turns"][2]["content"] =
            serde_json::json!("Confirmed: f'(2) = 10. Rule: (x^n)' = n x^(n-1), then substitute.");
        let mut seen = HashSet::new();
        let checked = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap();
        assert_eq!(
            checked.problem.as_ref().unwrap().problem_id,
            problem.problem_id
        );

        // A closing that confirms the wrong answer is rejected.
        row["turns"][2]["content"] = serde_json::json!("Confirmed: f'(2) = 12.");
        let mut seen = HashSet::new();
        let rejected = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "verified_answer_mismatch");
    }

    #[test]
    fn unverified_problem_rejects_the_row() {
        let mut index = problems();
        let problem = index.values_mut().next().unwrap();
        problem.verification = None;
        let problem_id = problem.problem_id.clone();
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CONFIRM).unwrap();
        row["problem_id"] = serde_json::json!(problem_id);
        let mut seen = HashSet::new();
        let rejected = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "provenance_unverified_problem");
    }

    #[test]
    fn contains_int_token_respects_boundaries() {
        assert!(contains_int_token("answer: 10.", 10));
        assert!(contains_int_token("= -4 done", -4));
        assert!(!contains_int_token("answer 100", 10));
        assert!(contains_int_token("x10", 10));
        assert!(!contains_int_token("nothing", 10));
    }
}
