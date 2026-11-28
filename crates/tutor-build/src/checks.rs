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

use tutor_schema::{Answer, DialogueRow, MAX_HINT_LEVEL, Problem, Role, RowSource, TurnKind};

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
    let problem = problems.and_then(|index| index.get(&row.problem_id));
    check_provenance(&row, problems.is_some(), problem)
        .map_err(|(code, detail)| fail(code, detail, Some(value.clone())))?;
    // A row that failed earlier checks must not reserve its `row_id`: a
    // corrected copy later in the input is not a duplicate of the draft.
    if !seen_row_ids.insert(row.row_id.clone()) {
        return Err(fail(
            "duplicate_row_id",
            format!("duplicate row_id {:?}", row.row_id),
            Some(value),
        ));
    }
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
    // Mock readiness candidates remain ineligible even if their outer envelope
    // is removed. No configuration/approval flag can promote mock wording.
    if row
        .provenance
        .generator
        .as_ref()
        .is_some_and(|g| g.name == "mock_teacher_surface")
    {
        return Err((
            "mock_teacher_not_exportable",
            "mock teacher candidates are not approved training/export rows".to_owned(),
        ));
    }
    if row
        .provenance
        .generator
        .as_ref()
        .is_some_and(|g| g.name == "deferred_teacher_surface")
    {
        return Err((
            "teacher_admission_blocked",
            "teacher candidate lacks admitted rights/approval provenance; RM-2500 remains required"
                .to_owned(),
        ));
    }
    check_generated(row, problems_given, problem)?;
    let Some(problem) = problem else {
        // Hand-written row with no matching problem: its provenance is the
        // hand-written record itself.
        return Ok(());
    };
    check_grounding(row, problem)
}

/// A `generated` row's seed, parameters, and verification record live on
/// its problem; without `--problems` (or a matching entry) the row cannot
/// carry full provenance and does not ship.
fn check_generated(
    row: &DialogueRow,
    problems_given: bool,
    problem: Option<&Problem>,
) -> Result<(), (&'static str, String)> {
    if row.provenance.source != RowSource::Generated {
        return Ok(());
    }
    if !problems_given {
        return Err((
            "provenance_missing_problem",
            "generated rows require --problems so seed, parameters, and verification provenance can ship"
                .to_owned(),
        ));
    }
    if problem.is_none() {
        return Err((
            "provenance_missing_problem",
            format!("no problem {:?} in the problems file", row.problem_id),
        ));
    }
    Ok(())
}

/// Cross-checks against the verified problem the row names: a verification
/// record, the same topic, a closing turn that asserts the verified answer,
/// and no earlier tutor turn that asserts it.
fn check_grounding(row: &DialogueRow, problem: &Problem) -> Result<(), (&'static str, String)> {
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
    if let Some(index) = early_answer_reveal(row, &problem.answer) {
        return Err((
            "early_answer_reveal",
            format!(
                "tutor turn {index} states the verified answer for problem {:?} before the closing turn",
                problem.problem_id
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

/// The index of the first tutor turn that asserts the verified answer
/// where the policy forbids it — every turn before the closing except an
/// L4 worked-solution `hint`, which may show the answer. `None` when no
/// early turn asserts it.
fn early_answer_reveal(row: &DialogueRow, answer: &Answer) -> Option<usize> {
    let Answer::Integer { value } = answer;
    let last = row.turns.len() - 1;
    row.turns.iter().enumerate().find_map(|(index, turn)| {
        let reveal_allowed = index == last
            || turn.role != Role::Tutor
            || (turn.kind == Some(TurnKind::Hint) && turn.hint_level == Some(MAX_HINT_LEVEL));
        (!reveal_allowed && asserted_ints(&turn.content).contains(value)).then_some(index)
    })
}

/// The closing turn must confirm the verified answer. For the integer
/// answers the schema currently models, the closing's last asserted value
/// (the integer after `=`, `:`, or the word `is`/`equals`) must equal it —
/// an intermediate mention or a signed lookalike (`-10` for `10`) does not
/// count as confirmation.
fn closing_states_answer(row: &DialogueRow, answer: &Answer) -> bool {
    let Some(closing) = row.turns.last() else {
        return false;
    };
    match answer {
        Answer::Integer { value } => asserted_ints(&closing.content).last() == Some(value),
    }
}

/// Every integer `text` asserts: a signed digit token that immediately
/// follows `=`, `:`, or the word `is`/`equals`, in order. Tokens inside a
/// question (`Is f'(1) = 7? Substitute to check.`) are not assertions: a
/// tutor asking the student to verify a value does not confirm it.
fn asserted_ints(text: &str) -> Vec<i64> {
    int_tokens(text)
        .into_iter()
        .filter(|(start, _)| {
            follows_assertion(&text[..*start])
                && !in_question(&text[*start..])
                && !clause_started_as_question(&text[..*start])
        })
        .map(|(_, value)| value)
        .collect()
}

/// Whether the clause that `rest` continues ends in `?`. A clause ends at
/// `?`, `!`, `;`, a newline, or a `.` followed by whitespace or the end of
/// the text (so the decimal point in `7.5` does not end it).
fn in_question(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'?' => return true,
            // A clause boundary: a later question does not undo an
            // assertion made before it.
            b'!' | b'\n' | b';' => return false,
            b'.' if bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace) => return false,
            _ => {}
        }
    }
    false
}

/// Whether the clause containing the token opened as a direct question.
fn clause_started_as_question(prefix: &str) -> bool {
    let start = prefix
        .rmatch_indices(['?', '!', ';', '\n'])
        .next()
        .map_or(0, |(index, _)| index + 1);
    let clause = prefix[start..].trim_start().to_ascii_lowercase();
    [
        "is ",
        "are ",
        "does ",
        "do ",
        "did ",
        "can ",
        "could ",
        "should ",
        "would ",
        "what is ",
        "what are ",
        "what does ",
        "what do ",
        "why is ",
        "why does ",
        "how is ",
        "how does ",
        "how do ",
    ]
    .iter()
    .any(|opening| clause.starts_with(opening))
}

/// Whether `prefix` ends with an assertion marker: `=`, `:`, or the word
/// `is`/`equals` (the formats every shipped closing uses: `f'(2) = 20`,
/// `the answer is 7`, `answer: 12`).
fn follows_assertion(prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    if prefix.ends_with('=') || prefix.ends_with(':') {
        return true;
    }
    prefix
        .rsplit(|c: char| !c.is_ascii_alphabetic())
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("is") || word.eq_ignore_ascii_case("equals"))
}

/// Signed integer tokens in `text` as `(byte index, value)`: a run of ASCII
/// digits with an optional leading `+`/`-`, not glued to other alphanumeric
/// characters on either side — `x10` and `10x` are not the token `10`, and
/// `-20` is not `20`. Tokens that overflow `i64` are dropped.
fn int_tokens(text: &str) -> Vec<(usize, i64)> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some((end, value)) = int_token_at(bytes, index) else {
            index += 1;
            continue;
        };
        if let Some(value) = value {
            tokens.push((index, value));
        }
        index = end;
    }
    tokens
}

/// The standalone integer token starting at `index`, as its end and value
/// (`None` when it overflows `i64`); `None` when no token starts there.
fn int_token_at(bytes: &[u8], index: usize) -> Option<(usize, Option<i64>)> {
    if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
        return None;
    }
    let (negative, digits) = match bytes[index] {
        b'-' => (true, index + 1),
        b'+' => (false, index + 1),
        b'0'..=b'9' => (false, index),
        _ => return None,
    };
    let end = digits_end(bytes, digits);
    if end == digits || bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    let magnitude = parse_magnitude(&bytes[digits..end]);
    Some((end, magnitude.map(|v| if negative { -v } else { v })))
}

/// The index just past the run of ASCII digits starting at `start`.
fn digits_end(bytes: &[u8], start: usize) -> usize {
    bytes
        .get(start..)
        .and_then(|rest| rest.iter().position(|b| !b.is_ascii_digit()))
        .map_or(bytes.len(), |offset| start + offset)
}

/// The value of an ASCII digit run, or `None` when it overflows `i64`.
fn parse_magnitude(digits: &[u8]) -> Option<i64> {
    digits.iter().try_fold(0_i64, |value, digit| {
        value.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
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

        // A signed lookalike is not the answer either.
        row["turns"][2]["content"] = serde_json::json!("Confirmed: f'(2) = -10.");
        let mut seen = HashSet::new();
        let rejected = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "verified_answer_mismatch");

        // An intermediate mention does not confirm a different result.
        row["turns"][2]["content"] =
            serde_json::json!("The intermediate value is 10, so the derivative is 12.");
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
    fn unflagged_early_answer_reveal_is_rejected() {
        // A correction row grounded in the fixture problem (answer 10): the
        // diagnosis asserts the answer without the reveal flag.
        let index = problems();
        let problem = index.values().next().unwrap();
        let fixture = include_str!("../tests/fixtures/rows/valid.jsonl");
        let mut row: serde_json::Value =
            serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
        row["problem_id"] = serde_json::json!(problem.problem_id);
        row["turns"][2]["content"] = serde_json::json!("The answer is 10, you forgot the +1 term.");
        row["turns"][6]["content"] = serde_json::json!("Confirmed: f'(1) = 10.");
        let mut seen = HashSet::new();
        let rejected = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "early_answer_reveal");
    }

    #[test]
    fn rejected_row_does_not_reserve_its_row_id() {
        let index = problems();
        let mut row: serde_json::Value = serde_json::from_str(GOLD_CONFIRM).unwrap();
        row["row_id"] = serde_json::json!("draft-1");
        row["problem_id"] = serde_json::json!("missing-problem");
        row["gold"] = serde_json::json!(false);
        row["provenance"] = serde_json::json!({
            "source": "generated",
            "taxonomy_version": 1,
            "generator": {"name": "dialogue_writer", "version": 1}
        });
        let mut seen = HashSet::new();
        let rejected = check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "provenance_missing_problem");

        // The corrected copy under the same row_id is not a duplicate.
        row["problem_id"] = serde_json::json!("gold-pd-power-rule-0001");
        row["provenance"] = serde_json::json!({"source": "hand_written", "taxonomy_version": 1});
        check(
            &line(&row.to_string()),
            Some(&index),
            &validator(),
            &mut seen,
        )
        .unwrap();
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
    fn int_tokens_respect_boundaries_and_signs() {
        assert_eq!(
            int_tokens("answer: 10. x10 10x -4 100"),
            [(8, 10), (20, -4), (23, 100)]
        );
        assert!(int_tokens("nothing").is_empty());
        assert_eq!(int_tokens("= -20"), [(2, -20)]);
    }

    #[test]
    fn asserted_ints_find_assertions() {
        assert_eq!(asserted_ints("Confirmed: f'(2) = 20."), [20]);
        assert_eq!(asserted_ints("The answer is 7, you forgot a term."), [7]);
        assert_eq!(asserted_ints("answer: 12"), [12]);
        assert_eq!(asserted_ints("value is 10, so the result is 12"), [10, 12]);
        // `x10` is not an assertion of 10, and a bare mention asserts nothing.
        assert!(asserted_ints("x10").is_empty());
        assert!(asserted_ints("it could be 10").is_empty());
        // A verification question is not an assertion; the same equation
        // stated affirmatively is.
        assert!(asserted_ints("Is f'(1) = 7? Substitute 1 to check.").is_empty());
        assert_eq!(asserted_ints("So f'(1) = 7. Substitute 1 to check."), [7]);
        assert!(asserted_ints("You got = 7.5, is that right?").is_empty());
        assert_eq!(asserted_ints("f'(1) = 7\nDoes that match?"), [7]);
        assert_eq!(asserted_ints("The answer is 7; can you explain why?"), [7]);
        assert!(asserted_ints("Is f'(1) = 7; if not, what went wrong?").is_empty());
    }
}
