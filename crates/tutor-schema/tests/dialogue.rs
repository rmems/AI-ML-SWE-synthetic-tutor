//! Keeps `tutor_schema::DialogueRow`, the generated `schemas/dialogue.schema.json`,
//! and the hand-written gold rows under `datasets/gold/` in line, and checks that
//! broken rows are rejected.

use serde_json::{Value, json};
use tutor_schema::{DIALOGUE_SCHEMA_VERSION, DialogueRow, SchemaError, dialogue_json_schema};

const COMMITTED_SCHEMA: &str = include_str!("../../../schemas/dialogue.schema.json");
const SYSTEM_PROMPT_V1: &str = include_str!("../../../prompts/tutor-system-v1.txt");

const GOLD_ROWS: [&str; 3] = [
    include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json"),
    include_str!("../../../datasets/gold/gold-algebra-sign-error.json"),
    include_str!("../../../datasets/gold/gold-confirmation.json"),
];

fn gold(name: &str) -> Value {
    let file = match name {
        "chain" => GOLD_ROWS[0],
        "sign" => GOLD_ROWS[1],
        "confirmation" => GOLD_ROWS[2],
        _ => panic!("unknown gold row"),
    };
    serde_json::from_str(file).expect("gold row is valid JSON")
}

fn reject(mutate: impl FnOnce(&mut Value)) -> SchemaError {
    let mut value = gold("chain");
    mutate(&mut value);
    DialogueRow::from_value(value).expect_err("mutated gold row must be rejected")
}

#[test]
fn committed_schema_matches_generated() {
    let committed: Value =
        serde_json::from_str(COMMITTED_SCHEMA).expect("dialogue.schema.json is valid JSON");
    assert_eq!(committed, dialogue_json_schema());
}

#[test]
fn gold_rows_are_valid_and_round_trip() {
    for (index, file) in GOLD_ROWS.iter().enumerate() {
        let row = DialogueRow::from_json(file).expect("gold row validates");
        assert!(row.gold);
        assert_eq!(row.schema_version, DIALOGUE_SCHEMA_VERSION);
        let encoded = serde_json::to_value(&row).expect("serializes");
        let decoded: Value = serde_json::from_str(file).expect("json");
        assert_eq!(encoded, decoded, "gold row {index} round-trips");
    }
}

#[test]
fn gold_rows_cover_required_shapes() {
    let chain = DialogueRow::from_json(GOLD_ROWS[0]).unwrap();
    let sign = DialogueRow::from_json(GOLD_ROWS[1]).unwrap();
    let confirmation = DialogueRow::from_json(GOLD_ROWS[2]).unwrap();
    assert_eq!(
        chain.misconceptions,
        [tutor_schema::MisconceptionId::DerivativesChainRule]
    );
    assert_eq!(chain.hint_levels_reached, [1]);
    assert_eq!(
        sign.misconceptions,
        [tutor_schema::MisconceptionId::AlgebraSignError]
    );
    assert_eq!(sign.hint_levels_reached, [1, 2]);
    assert_eq!(confirmation.row_type, tutor_schema::RowType::Confirmation);
    assert!(confirmation.misconceptions.is_empty());
    assert!(confirmation.hint_levels_reached.is_empty());
}

#[test]
fn gold_system_turn_matches_stored_prompt() {
    for file in GOLD_ROWS {
        let row = DialogueRow::from_json(file).expect("gold row validates");
        let system = &row.turns[0];
        assert_eq!(system.role, tutor_schema::Role::System);
        // The stored prompt may check out with CRLF endings on Windows.
        let stored = SYSTEM_PROMPT_V1.replace("\r\n", "\n");
        assert_eq!(system.content.trim_end(), stored.trim_end());
    }
}

#[test]
fn final_answer_before_last_turn_is_rejected() {
    let err = reject(|v| {
        v["turns"][2]["reveals_final_answer"] = json!(true);
    });
    assert_eq!(err.code(), "schema_constraint");
}

#[test]
fn more_than_one_primary_misconception_is_rejected() {
    let err = reject(|v| {
        v["misconceptions"] = json!(["derivatives.chain_rule", "algebra.sign_error"]);
    });
    assert_eq!(err.code(), "schema_constraint");
}

#[test]
fn missing_provenance_is_rejected() {
    let err = reject(|v| {
        v.as_object_mut().unwrap().remove("provenance");
    });
    assert_eq!(err.code(), "schema_shape");
}

#[test]
fn constraint_violations_are_rejected() {
    let cases: Vec<Box<dyn FnOnce(&mut Value)>> = vec![
        Box::new(|v| v["schema_version"] = json!(0)),
        Box::new(|v| v["topic"] = json!("Calculus")),
        Box::new(|v| v["system_prompt_version"] = json!(0)),
        // Correction row with no misconception.
        Box::new(|v| v["misconceptions"] = json!([])),
        // Confirmation row carrying a misconception.
        Box::new(|v| {
            v["row_type"] = json!("confirmation");
            v["misconceptions"] = json!(["derivatives.chain_rule"]);
        }),
        // Declared hint levels disagree with the hint turns.
        Box::new(|v| v["hint_levels_reached"] = json!([1, 2])),
        // Repeated hint level.
        Box::new(|v| {
            v["hint_levels_reached"] = json!([1, 1]);
            v["turns"][4]["hint_level"] = json!(1);
        }),
        // Hint level out of range.
        Box::new(|v| {
            v["hint_levels_reached"] = json!([5]);
            v["turns"][4]["hint_level"] = json!(5);
        }),
        // Student and tutor turns out of order.
        Box::new(|v| {
            v["turns"].as_array_mut().unwrap().swap(1, 2);
        }),
        // Last turn is not the tutor's closing turn.
        Box::new(|v| {
            v["turns"].as_array_mut().unwrap().push(json!({
                "role": "student", "kind": "attempt", "content": "x"
            }));
        }),
        // Closing turn does not confirm the answer.
        Box::new(|v| {
            v["turns"].as_array_mut().unwrap().pop();
            v["turns"].as_array_mut().unwrap().push(json!({
                "role": "tutor", "kind": "closing", "content": "Try d/dx sin(3x²)."
            }));
        }),
        // Answer request met with an L2 hint, not an L1 start.
        Box::new(|v| {
            let turns = v["turns"].as_array_mut().unwrap();
            turns[3]["kind"] = json!("answer_request");
            turns[4]["hint_level"] = json!(2);
            v["hint_levels_reached"] = json!([2]);
        }),
        // Generated row without a generator.
        Box::new(|v| {
            v["gold"] = json!(false);
            v["provenance"]["source"] = json!("generated");
        }),
        // Gold row not marked hand_written.
        Box::new(|v| {
            v["provenance"]["source"] = json!("generated");
            v["provenance"]["generator"] = json!({"name": "g", "version": 1});
        }),
    ];
    for mutate in cases {
        assert_eq!(reject(mutate).code(), "schema_constraint");
    }
}

#[test]
fn shape_violations_are_rejected() {
    let cases: Vec<Box<dyn FnOnce(&mut Value)>> = vec![
        Box::new(|v| v["extra"] = json!(1)),
        Box::new(|v| v["turns"][0]["extra"] = json!(1)),
        Box::new(|v| v["provenance"]["extra"] = json!(1)),
        Box::new(|v| v["row_type"] = json!("essay")),
        Box::new(|v| v["misconceptions"] = json!(["derivatives.not_a_real_id"])),
        Box::new(|v| v["turns"][0]["role"] = json!("user")),
        Box::new(|v| v["provenance"]["source"] = json!("scraped")),
    ];
    for mutate in cases {
        assert_eq!(reject(mutate).code(), "schema_shape");
    }
}
