//! Keeps `tutor_schema::DialogueRow`, the generated `schemas/dialogue.schema.json`,
//! and the hand-written gold rows under `datasets/gold/` in line, and checks that
//! broken rows are rejected.

use serde_json::{Value, json};
use tutor_schema::{DIALOGUE_SCHEMA_VERSION, DialogueRow, SchemaError, dialogue_json_schema};

const COMMITTED_SCHEMA: &str = include_str!("../../../schemas/dialogue.schema.json");
const SYSTEM_PROMPT_V1: &str = include_str!("../../../prompts/tutor-system-v1.txt");

type Mutation = Box<dyn FnOnce(&mut Value)>;

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
fn secant_labels_require_the_version_that_defined_the_topic_mapping() {
    for id in [
        "derivatives.secant_as_tangent",
        "derivatives.undefined_quotient_implies_no_limit",
        "limits.divide_by_zero_form",
    ] {
        let mut row = gold("chain");
        row["topic"] = json!("calculus.derivatives.definition");
        row["misconceptions"] = json!([id]);
        row["provenance"]["taxonomy_version"] = json!(1);
        assert!(
            DialogueRow::from_value(row.clone())
                .unwrap_err()
                .to_string()
                .contains("predates the row's misconception label")
        );
        row["provenance"]["taxonomy_version"] = json!(2);
        DialogueRow::from_value(row).expect("v2 secant topic mapping is valid");
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
fn answer_requests_convert_to_the_next_ladder_rung() {
    // Student opens by asking for the answer: the first tutor turn is an L1
    // hint, not a diagnosis, and the ladder then climbs from there.
    let mut opening = gold("sign");
    let turns = opening["turns"].as_array_mut().unwrap();
    turns[1]["kind"] = json!("answer_request");
    turns[2]["kind"] = json!("hint");
    turns[2]["hint_level"] = json!(1);
    turns[4]["hint_level"] = json!(2);
    turns[6]["hint_level"] = json!(3);
    opening["hint_levels_reached"] = json!([1, 2, 3]);
    DialogueRow::from_value(opening).expect("opening answer request converts to L1");

    // A request after an L1 hint converts to the next rung (L2), not L1 again
    // and not the answer.
    let mut mid_ladder = gold("sign");
    mid_ladder["turns"][5]["kind"] = json!("answer_request");
    DialogueRow::from_value(mid_ladder).expect("mid-ladder request converts to L2");
}

#[test]
fn l4_worked_solution_may_reveal_the_answer() {
    let row = json!({
        "schema_version": 1,
        "row_id": "test-l4-0001",
        "row_type": "correction",
        "problem_id": "p-1",
        "topic": "algebra.linear_equations",
        "system_prompt_version": 1,
        "misconceptions": ["algebra.sign_error"],
        "hint_levels_reached": [1, 2, 3, 4],
        "turns": [
            {"role": "system", "content": "prompt"},
            {"role": "student", "kind": "attempt", "content": "x + 1 = 3, x = 1"},
            {"role": "tutor", "kind": "diagnosis", "content": "Check the sign."},
            {"role": "student", "kind": "attempt", "content": "still x = 1"},
            {"role": "tutor", "kind": "hint", "hint_level": 1, "content": "Look at the subtraction."},
            {"role": "student", "kind": "attempt", "content": "x = 1?"},
            {"role": "tutor", "kind": "hint", "hint_level": 2, "content": "What is 3 minus 1?"},
            {"role": "student", "kind": "attempt", "content": "2?"},
            {"role": "tutor", "kind": "hint", "hint_level": 3, "content": "So x = 3 - 1."},
            {"role": "student", "kind": "attempt", "content": "x = 2?"},
            {"role": "tutor", "kind": "hint", "hint_level": 4, "content": "x + 1 = 3 gives x = 3 - 1 = 2.", "reveals_final_answer": true},
            {"role": "student", "kind": "attempt", "content": "x = 2"},
            {"role": "tutor", "kind": "closing", "content": "Confirmed: x = 2.", "reveals_final_answer": true}
        ],
        "provenance": {
            "source": "generated",
            "taxonomy_version": 1,
            "generator": {"name": "dialogue-gen", "version": 1}
        },
        "gold": false
    });
    DialogueRow::from_value(row).expect("an L4 worked solution may reveal the answer");
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
    let cases: Vec<Mutation> = vec![
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
        // Ladder skips L1 (declared levels stay consistent).
        Box::new(|v| {
            v["hint_levels_reached"] = json!([2]);
            v["turns"][4]["hint_level"] = json!(2);
        }),
        // Hint level out of range.
        Box::new(|v| {
            v["hint_levels_reached"] = json!([5]);
            v["turns"][4]["hint_level"] = json!(5);
        }),
        // Tutor closes mid-dialogue and again at the end.
        Box::new(|v| {
            v["turns"][4]["kind"] = json!("closing");
        }),
        // Unknown prompt version.
        Box::new(|v| v["system_prompt_version"] = json!(2)),
        // Taxonomy version newer than supported.
        Box::new(|v| {
            v["provenance"]["taxonomy_version"] = json!(tutor_schema::TAXONOMY_VERSION + 1);
        }),
        // Topic not among the misconception's taxonomy topics.
        Box::new(|v| v["topic"] = json!("algebra.polynomial_expansion")),
        // Whitespace-only turn content.
        Box::new(|v| v["turns"][1]["content"] = json!("   ")),
        // Student and tutor turns out of order (first tutor turn stays a
        // diagnosis, so only alternation is broken).
        Box::new(|v| {
            v["turns"].as_array_mut().unwrap().swap(3, 4);
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
    let cases: Vec<Mutation> = vec![
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
