//! Keeps `tutor_schema::StudentAttempt` and the generated
//! `schemas/student-attempt.schema.json` in line, and checks artifact-level
//! consistency constraints.

use serde_json::Value;
use tutor_schema::{
    MisconceptionId, NamedVersion, Persistence, PlantedTransformation, Role, StudentAttempt,
    StudentAttemptProvenance, StudentLevel, StudentProfile, SurfaceStyle, TAXONOMY_VERSION,
    TeacherVariety, TeacherVarietyStatus, Turn, TurnKind, student_attempt_json_schema,
};

const COMMITTED_SCHEMA: &str = include_str!("../../../schemas/student-attempt.schema.json");

fn correction_attempt() -> StudentAttempt {
    StudentAttempt {
        schema_version: 1,
        attempt_id: "problem/v1/seed-1/synthetic-student-simulator/v1/seed-2".to_owned(),
        problem_id: "problem/v1/seed-1".to_owned(),
        topic: "calculus.derivatives.chain_rule".to_owned(),
        correct: false,
        misconception: Some(MisconceptionId::DerivativesChainRule),
        profile: StudentProfile {
            level: StudentLevel::Typical,
            surface_style: SurfaceStyle::Terse,
            notation_noise: vec![],
        },
        persistence: Persistence::CorrectsAfterL1,
        transformation: Some(PlantedTransformation {
            misconception: MisconceptionId::DerivativesChainRule,
            step_index: 5,
            step_kind: "multiply".to_owned(),
            correct_value: 12,
            corrupted_value: 6,
            propagates_to_answer: true,
            description: "omit the inner-derivative factor".to_owned(),
        }),
        student_turns: vec![
            Turn {
                role: Role::Student,
                content: "h′(1) = 6".to_owned(),
                kind: Some(TurnKind::Attempt),
                hint_level: None,
                reveals_final_answer: false,
            },
            Turn {
                role: Role::Student,
                content: "I still get 6".to_owned(),
                kind: Some(TurnKind::Attempt),
                hint_level: None,
                reveals_final_answer: false,
            },
            Turn {
                role: Role::Student,
                content: "h′(1) = 12".to_owned(),
                kind: Some(TurnKind::Attempt),
                hint_level: None,
                reveals_final_answer: false,
            },
        ],
        teacher_variety: TeacherVariety {
            requested: false,
            status: TeacherVarietyStatus::Disabled,
            teacher: None,
        },
        provenance: StudentAttemptProvenance {
            generator: NamedVersion {
                name: "synthetic-student-simulator".to_owned(),
                version: 1,
            },
            seed: 2,
            taxonomy_version: TAXONOMY_VERSION,
        },
    }
}

#[test]
fn committed_schema_matches_generated() {
    let committed: Value =
        serde_json::from_str(COMMITTED_SCHEMA).expect("student-attempt schema is valid JSON");
    assert_eq!(committed, student_attempt_json_schema());
}

#[test]
fn correction_attempt_validates_and_round_trips() {
    let attempt = correction_attempt();
    attempt.validate().expect("artifact validates");
    let encoded = serde_json::to_string(&attempt).expect("serializes");
    let decoded = StudentAttempt::from_json(&encoded).expect("round trip validates");
    assert_eq!(decoded, attempt);
}

#[test]
fn correct_attempt_must_not_carry_a_label() {
    let mut attempt = correction_attempt();
    attempt.correct = true;
    attempt.student_turns.truncate(1);
    let error = attempt
        .validate()
        .expect_err("correct labeled row is invalid");
    assert_eq!(error.code(), "schema_constraint");
}

#[test]
fn correction_value_must_actually_be_wrong() {
    let mut attempt = correction_attempt();
    attempt
        .transformation
        .as_mut()
        .expect("correction attempt has a transformation")
        .corrupted_value = 12;
    let error = attempt
        .validate()
        .expect_err("a correction row whose value is correct is invalid");
    assert_eq!(error.code(), "schema_constraint");
}

#[test]
fn persistence_controls_retry_count() {
    let mut attempt = correction_attempt();
    attempt.persistence = Persistence::NeedsL3;
    let error = attempt.validate().expect_err("NeedsL3 needs five turns");
    assert_eq!(error.code(), "schema_constraint");
}

#[test]
fn student_turns_must_use_dialogue_student_shape() {
    let mut attempt = correction_attempt();
    attempt.student_turns[0].kind = Some(TurnKind::AnswerRequest);
    let error = attempt
        .validate()
        .expect_err("non-attempt student turn is invalid");
    assert_eq!(error.code(), "schema_constraint");
}
