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
fn secant_attempts_require_v2_in_rust_and_json_schema() {
    let mut attempt = correction_attempt();
    let id = MisconceptionId::DerivativesSecantAsTangent;
    attempt.topic = "calculus.derivatives.definition".into();
    attempt.misconception = Some(id);
    attempt.transformation.as_mut().unwrap().misconception = id;
    let schema = jsonschema::validator_for(&student_attempt_json_schema()).unwrap();
    attempt.provenance.taxonomy_version = 1;
    assert!(attempt.validate().is_err());
    assert!(!schema.is_valid(&serde_json::to_value(&attempt).unwrap()));
    attempt.provenance.taxonomy_version = 2;
    attempt.validate().unwrap();
    assert!(schema.is_valid(&serde_json::to_value(&attempt).unwrap()));
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
fn provenance_must_identify_a_versioned_generator() {
    let mut attempt = correction_attempt();
    attempt.provenance.generator.version = 0;
    let error = attempt
        .validate()
        .expect_err("version zero is not replayable provenance");
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

fn json_validator() -> jsonschema::Validator {
    jsonschema::validator_for(&student_attempt_json_schema()).expect("student schema compiles")
}

#[test]
fn generated_schema_accepts_a_valid_correction_attempt() {
    let value = serde_json::to_value(correction_attempt()).expect("attempt serializes");
    let validator = json_validator();
    assert!(
        validator.is_valid(&value),
        "{}",
        validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    );
}

#[test]
fn generated_schema_accepts_a_valid_correct_attempt() {
    let mut attempt = correction_attempt();
    attempt.correct = true;
    attempt.misconception = None;
    attempt.transformation = None;
    attempt.student_turns.truncate(1);
    attempt.validate().expect("correct artifact validates");
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(json_validator().is_valid(&value));
}

#[test]
fn generated_schema_rejects_a_correct_attempt_with_a_label() {
    let mut attempt = correction_attempt();
    attempt.correct = true;
    attempt.student_turns.truncate(1);
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn generated_schema_rejects_the_wrong_retry_count() {
    let mut attempt = correction_attempt();
    attempt.student_turns.truncate(2);
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn generated_schema_rejects_a_mismatched_transformation_label() {
    let mut attempt = correction_attempt();
    attempt.misconception = Some(MisconceptionId::DerivativesProductRule);
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn generated_schema_accepts_null_labels_on_a_confirmation() {
    let mut attempt = correction_attempt();
    attempt.correct = true;
    attempt.misconception = None;
    attempt.transformation = None;
    attempt.student_turns.truncate(1);
    let mut value = serde_json::to_value(attempt).expect("attempt serializes");
    value["misconception"] = serde_json::Value::Null;
    value["transformation"] = serde_json::Value::Null;
    assert!(json_validator().is_valid(&value));
    StudentAttempt::from_json(&serde_json::to_string(&value).expect("serializes"))
        .expect("serde also treats explicit null labels as absent");
}

#[test]
fn generated_schema_rejects_non_attempt_student_turns() {
    let mut answer_request = correction_attempt();
    answer_request.student_turns[0].kind = Some(TurnKind::AnswerRequest);
    let value = serde_json::to_value(answer_request).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));

    let mut tutor_turn = correction_attempt();
    tutor_turn.student_turns[0].role = Role::Tutor;
    tutor_turn.student_turns[0].kind = Some(TurnKind::Diagnosis);
    let value = serde_json::to_value(tutor_turn).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn generated_schema_rejects_empty_ids_and_blank_student_work() {
    let value = serde_json::to_value(correction_attempt()).expect("attempt serializes");
    for field in ["attempt_id", "problem_id"] {
        for empty in ["", "   "] {
            let mut invalid_value = value.clone();
            invalid_value[field] = serde_json::json!(empty);
            assert!(
                !json_validator().is_valid(&invalid_value),
                "empty {field} must not validate"
            );
        }
    }
    let mut blank_work = value;
    blank_work["student_turns"][0]["content"] = serde_json::json!("   ");
    assert!(!json_validator().is_valid(&blank_work));
}

#[test]
fn generated_schema_rejects_a_topic_outside_the_label_taxonomy() {
    let mut attempt = correction_attempt();
    attempt.topic = "calculus.derivatives.product_rule".to_owned();
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn generated_schema_rejects_null_correction_labels() {
    let value = serde_json::to_value(correction_attempt()).expect("attempt serializes");
    for field in ["misconception", "transformation"] {
        let mut null_value = value.clone();
        null_value[field] = serde_json::Value::Null;
        assert!(
            !json_validator().is_valid(&null_value),
            "null {field} must not satisfy the correction branch"
        );
    }
}

#[test]
fn provenance_seed_must_remain_json_exact() {
    let mut attempt = correction_attempt();
    attempt.provenance.seed = tutor_schema::MAX_SEED + 1;
    let error = attempt
        .validate()
        .expect_err("seed above the JSON-safe maximum is invalid");
    assert_eq!(error.code(), "schema_constraint");
    let value = serde_json::to_value(attempt).expect("attempt serializes");
    assert!(!json_validator().is_valid(&value));
}

#[test]
fn rust_non_empty_validation_matches_ecmascript_whitespace_semantics() {
    // JSON Schema regexes follow ECMA-262, whose whitespace escapes treat U+FEFF as
    // whitespace but not U+0085 (NEL); Unicode `White_Space` (Rust's
    // `char::is_whitespace`) does the reverse. Semantic validation must match
    // the schema's ECMAScript semantics exactly.
    let mut feff_id = correction_attempt();
    feff_id.attempt_id = "\u{FEFF}".to_owned();
    assert!(
        feff_id.validate().is_err(),
        "U+FEFF-only IDs are blank as ECMA-262 whitespace"
    );

    let nel_id = {
        let mut attempt = correction_attempt();
        attempt.attempt_id = "\u{0085}".to_owned();
        attempt
    };
    nel_id
        .validate()
        .expect("U+0085 NEL is not ECMAScript whitespace, so the non-empty check passes");

    let mut feff_generator = correction_attempt();
    feff_generator.provenance.generator.name = "\u{FEFF}".to_owned();
    assert!(
        feff_generator.validate().is_err(),
        "U+FEFF-only generator names are blank as ECMA-262 whitespace"
    );

    let mut feff_work = correction_attempt();
    feff_work.student_turns[0].content = "\u{FEFF}".to_owned();
    assert!(
        feff_work.validate().is_err(),
        "U+FEFF-only student work is blank as ECMA-262 whitespace"
    );

    let ascii_whitespace_id = {
        let mut attempt = correction_attempt();
        attempt.attempt_id = " \t\n\u{000B}\u{000C}\r".to_owned();
        attempt
    };
    assert!(
        ascii_whitespace_id.validate().is_err(),
        "ASCII whitespace-only IDs remain blank"
    );
}
