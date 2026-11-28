use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Value, json};
use tutor_build::{checks, input::InputLine};
use tutor_schema::{DialogueRow, Problem};

use crate::{Error, Provider, TeacherConfig, json_sha256, sha256};

// Reuse the existing static schema fixture. This is not a fresh Julia run.
pub(crate) const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");
const PROMPT: &str = include_str!("../../../prompts/tutor-system-v1.txt");
const GENERATOR: &str = "mock_teacher_surface";

/// Run the opt-in offline fixture, validate candidates, then write a NEW directory.
///
/// Returns the report. It always leaves live generation, training and export
/// blocked, even after successful candidate validation. No credentials are read.
pub fn mock_smoke(config: &TeacherConfig, out: &Path) -> Result<Value, Error> {
    config.validate()?;
    if config.provider != Provider::Mock {
        return Err("live teacher generation blocked: RM-2500 exact model/route/vendor evidence and explicit run approval required; live adapter not implemented".into());
    }
    // Verification is separate from drafting and does not consult the teacher.
    let problem = tutor_verify::verify(&Problem::from_json(PROBLEM)?)?;
    let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema())?;
    let problems = BTreeMap::from([(problem.problem_id.clone(), problem.clone())]);
    let mut candidates = String::new();
    for correction in [false, true] {
        let row = draft_fixture(&problem, correction)?;
        // Quality validation is separate from admission; the draft is already
        // quarantined and keeps that marker in every emitted artifact.
        check_draft(&row, &problems, &validator)?;
        let candidate = json!({
            "kind": "mock_teacher_candidate_v1",
            "training_approved": false,
            "export_approved": false,
            "rights_status": "mock_only_not_training",
            "provenance": {
                "teacher": config,
                "fixture_sha256": sha256(PROBLEM.as_bytes()),
                "config_sha256": sha256(&serde_json::to_vec(config)?),
                "dialogue_sha256": json_sha256(&row)?,
                "implementation_version": env!("CARGO_PKG_VERSION"),
                "fixture_version": 1,
                "verification_scope": "rust_recompute_of_static_fixture_not_fresh_julia_run"
            },
            "problem": problem,
            "dialogue": row
        });
        candidates.push_str(&serde_json::to_string(&candidate)?);
        candidates.push('\n');
    }
    let report = json!({
        "kind": "mock_teacher_smoke_report_v1",
        "candidate_count": 2,
        "validation": "schema_policy_grounding_passed",
        "candidates_sha256": sha256(candidates.as_bytes()),
        "live_generation": "blocked",
        "training": "blocked",
        "export": "blocked",
        "rights_gate": "RM-2500 remains required; config validation is not approval"
    });
    crate::publication::publish_artifacts(out, |staged| {
        std::fs::write(staged.join("candidates.jsonl"), candidates)?;
        std::fs::write(
            staged.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    })?;
    Ok(report)
}

pub(crate) fn check_draft(
    row: &DialogueRow,
    problems: &BTreeMap<String, Problem>,
    validator: &jsonschema::Validator,
) -> Result<(), Error> {
    // The production gate rejects mock markers before grounding. Use a private,
    // in-memory copy to exercise those quality checks; never return or write it.
    // The original remains marked and is independently rejected at admission.
    let mut quality_only = row.clone();
    quality_only
        .provenance
        .generator
        .as_mut()
        .ok_or("missing draft generator")?
        .name = "offline_fixture_draft".into();
    let input = InputLine {
        file: "mock-in-memory.json".into(),
        line: None,
        text: serde_json::to_string(&quality_only)?,
    };
    checks::check(&input, Some(problems), validator, &mut HashSet::new())
        .map_err(|rejected| format!("mock candidate validation failed: {}", rejected.code))?;
    Ok(())
}

pub(crate) fn draft_fixture(problem: &Problem, correction: bool) -> Result<DialogueRow, Error> {
    // Only wording is mocked. The fixture's problem, answer, steps and student
    // attempts are fixed. No arbitrary prompt or user data input is accepted.
    let mut turns = vec![json!({"role": "system", "content": PROMPT})];
    if correction {
        turns.extend([
            json!({"role": "student", "kind": "attempt", "content": format!("{} My attempt: write the entire polynomial as (3x^2 - 2x + 5)^1. Applying the power rule directly gives 1*(3x^2 - 2x + 5)^0 = 1, so f'(2) = 1.", problem.statement)}),
            json!({"role": "tutor", "kind": "diagnosis", "content": "You used the entire polynomial as the base. Does the simple power rule assume the base is x itself?"}),
            json!({"role": "student", "kind": "attempt", "content": "I treated the whole polynomial as if it were the variable x."}),
            json!({"role": "tutor", "kind": "hint", "hint_level": 1, "content": "Return to the original sum and apply the power rule separately to its powers of x before evaluating."}),
            json!({"role": "student", "kind": "attempt", "content": "The derivative is 6x - 2, so f'(2) = 10."}),
        ]);
    } else {
        turns.push(json!({"role": "student", "kind": "attempt", "content": format!("{} My attempt: f'(x) = 6x - 2, so f'(2) = 10.", problem.statement)}));
    }
    let closing = if correction {
        "Confirmed: f'(2) = 10. The simple power rule applies directly to powers of x, not to an arbitrary expression used as the base. Try one: differentiate g(x) = x^2 + x at the same point."
    } else {
        "Confirmed: f'(2) = 10. You differentiated each term and substituted correctly. Try a harder variant: differentiate g(x) = x^3 - 2x^2 + x at the same point."
    };
    turns.push(json!({"role": "tutor", "kind": "closing", "reveals_final_answer": true, "content": closing}));
    Ok(DialogueRow::from_value(json!({
        "schema_version": 1,
        "row_id": if correction {"mock-power-correction-v1"} else {"mock-power-confirmation-v1"},
        "row_type": if correction {"correction"} else {"confirmation"},
        "problem_id": problem.problem_id,
        "topic": problem.topic,
        "system_prompt_version": 1,
        "misconceptions": if correction {vec!["derivatives.power_rule_overgeneralised"]} else {vec![]},
        "hint_levels_reached": if correction {vec![1]} else {vec![]},
        "turns": turns,
        "provenance": {"source": "generated", "taxonomy_version": 1,
            "generator": {"name": GENERATOR, "version": 1}},
        "gold": false
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_and_grounding_reject_tampering() {
        let original = Problem::from_json(PROBLEM).unwrap();
        let mut forged = original.clone();
        forged.answer = tutor_schema::Answer::Integer { value: 11 };
        assert!(tutor_verify::verify(&forged).is_err());
        let verified = tutor_verify::verify(&original).unwrap();
        let problems = BTreeMap::from([(verified.problem_id.clone(), verified.clone())]);
        let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema()).unwrap();
        let mut row = draft_fixture(&verified, true).unwrap();
        row.turns[2].content = "The answer is 10.".into();
        assert!(
            check_draft(&row, &problems, &validator)
                .unwrap_err()
                .to_string()
                .contains("early_answer_reveal")
        );
        let mut row = draft_fixture(&verified, false).unwrap();
        row.turns.last_mut().unwrap().content = "The answer is 11.".into();
        assert!(
            check_draft(&row, &problems, &validator)
                .unwrap_err()
                .to_string()
                .contains("verified_answer_mismatch")
        );
    }
}
