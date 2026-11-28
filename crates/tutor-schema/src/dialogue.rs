//! One tutoring dialogue row — the training-data contract of
//! `schemas/dialogue.schema.json`.
//!
//! A row is one complete tutoring trajectory: a versioned system prompt, a
//! student attempt, tutor turns that diagnose before hinting, and a closing
//! turn that confirms the verified answer. [`DialogueRow::validate`] enforces
//! the constraints serde cannot express (turn ordering, hint-ladder levels,
//! single primary misconception, provenance presence, and the rule that no
//! turn before the last tutor turn reveals the final answer). The checked-in
//! JSON Schema is generated from these types with `schemars` — regenerate it
//! with `cargo run -p tutor-schema --example print_dialogue_schema` — and a
//! test in `tests/dialogue.rs` keeps the file, the types, and the gold rows
//! under `datasets/gold/` in sync.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{MisconceptionId, SchemaError, is_valid_topic};

/// Current `schema_version` of [`DialogueRow`].
pub const DIALOGUE_SCHEMA_VERSION: u32 = 1;

/// Current tutor policy version; names `docs/tutor-policy.md` and
/// `prompts/tutor-system-v<VERSION>.txt`.
pub const TUTOR_POLICY_VERSION: u32 = 1;

/// Deepest hint-ladder level (L4, a full worked solution).
pub const MAX_HINT_LEVEL: u8 = 4;

/// One tutoring dialogue row.
///
/// Correction rows diagnose exactly one primary misconception; confirmation
/// rows confirm a correct first attempt and extend it. Both are first-class
/// row types so a model does not learn to invent errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DialogueRow {
    /// Always [`DIALOGUE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable row ID, unique within the dataset family.
    pub row_id: String,
    /// Whether the row corrects a wrong attempt or confirms a correct one.
    pub row_type: RowType,
    /// `problem_id` of the verified problem this dialogue is grounded in.
    pub problem_id: String,
    /// Dotted curriculum topic ID, e.g. `calculus.derivatives.chain_rule`.
    pub topic: String,
    /// Version of the tutor system prompt used as the `system` turn content.
    pub system_prompt_version: u32,
    /// Primary misconception IDs from `schemas/taxonomy.yaml`: exactly one for
    /// a correction row, empty for a confirmation row. Plural so the validator
    /// can flag rows that try to label more than one primary misconception.
    pub misconceptions: Vec<MisconceptionId>,
    /// Hint levels the ladder reached, ascending; e.g. `[1, 2]` when the
    /// dialogue used an L1 nudge and an L2 diagnostic question. Empty when no
    /// hint turn was needed.
    pub hint_levels_reached: Vec<u8>,
    /// The conversation: one `system` turn, then strictly alternating
    /// `student`/`tutor` turns ending on a `tutor` `closing` turn.
    pub turns: Vec<Turn>,
    /// Who produced the row and under which policy/taxonomy versions.
    /// Required on every row.
    pub provenance: DialogueProvenance,
    /// `true` only for hand-written exemplar rows under `datasets/gold/`.
    pub gold: bool,
}

/// The two first-class row types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowType {
    /// Student's first attempt was wrong: diagnose, ladder hints, confirm the
    /// fix. Target share of the pilot mix: 80%.
    Correction,
    /// Student's first attempt was correct: confirm and extend with a harder
    /// variant. Target share of the pilot mix: 20%.
    Confirmation,
}

/// One turn of the dialogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    /// Who speaks.
    pub role: Role,
    /// What they say. For tutor `hint` turns, exactly one ladder level of
    /// help — never more.
    pub content: String,
    /// Turn's function within the policy shape. Required on `student` and
    /// `tutor` turns, absent on the `system` turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TurnKind>,
    /// Ladder level, 1–4. Required on `tutor` `hint` turns, absent elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint_level: Option<u8>,
    /// Whether the turn states the verified final answer. May be `true` only
    /// on an L4 worked-solution `hint` or on the last turn (the tutor
    /// `closing` turn): no other turn reveals the answer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reveals_final_answer: bool,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// Who speaks in a [`Turn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The versioned tutor policy prompt; exactly one, always first.
    System,
    /// The learner: presents the problem and attempts, responds to hints.
    Student,
    /// The tutor: diagnoses, hints one level at a time, closes with
    /// confirmation, the fix rule, and a follow-up.
    Tutor,
}

/// A turn's function in the policy shape.
///
/// `Attempt` and `AnswerRequest` belong to `student` turns; `Diagnosis`,
/// `Hint`, and `Closing` belong to `tutor` turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    /// Student work on the problem, initial or retry.
    Attempt,
    /// Student asks for the answer directly. Policy converts this into the
    /// next rung of the ladder: the next tutor turn must be a `hint` at the
    /// next level (L1 when no hint has been given yet).
    AnswerRequest,
    /// First tutor turn of a correction row: names what the attempt got
    /// right, then surfaces the misconception with a question. Never opens
    /// with the full solution.
    Diagnosis,
    /// One rung of the hint ladder: L1 nudge, L2 targeted diagnostic question,
    /// L3 partial worked step, or L4 full worked solution. Levels climb
    /// contiguously from L1, one rung per turn.
    Hint,
    /// Final tutor turn: confirms against the verified answer, states the fix
    /// rule in one line, sets one similar follow-up problem.
    Closing,
}

/// Who produced the row and under which versions of the policy and taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DialogueProvenance {
    /// Hand-written gold exemplar or pipeline-generated.
    pub source: RowSource,
    /// `version` of `schemas/taxonomy.yaml` the `misconceptions` labels were
    /// taken from. Currently [`crate::TAXONOMY_VERSION`].
    pub taxonomy_version: u32,
    /// Generator identity for `generated` rows. Absent on `hand_written` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<NamedVersion>,
}

/// Whether a row was written by hand or produced by the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowSource {
    /// A hand-written exemplar; required for `gold` rows.
    HandWritten,
    /// Produced by the generation pipeline; must carry `generator`.
    Generated,
}

/// A named, versioned producer (e.g. a dialogue generator).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedVersion {
    /// Producer name.
    pub name: String,
    /// Producer version.
    pub version: u32,
}

fn invalid(field: &'static str, reason: impl Into<String>) -> SchemaError {
    SchemaError::Invalid {
        field,
        reason: reason.into(),
    }
}

impl DialogueRow {
    /// Parse and validate a dialogue row from JSON.
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        let row: Self = serde_json::from_str(json)?;
        row.validate()?;
        Ok(row)
    }

    /// Convert an already-decoded JSON value, then validate it.
    pub fn from_value(value: serde_json::Value) -> Result<Self, SchemaError> {
        let row: Self = serde_json::from_value(value)?;
        row.validate()?;
        Ok(row)
    }

    /// Check the constraints of `dialogue.schema.json` that the types do not
    /// encode: turn shape, hint-ladder order, single primary misconception,
    /// provenance, and that no turn before the last reveals the final answer.
    ///
    /// # Errors
    /// Returns [`SchemaError::Invalid`] naming the offending field.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != DIALOGUE_SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                format!(
                    "expected {DIALOGUE_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            ));
        }
        require_non_empty("row_id", &self.row_id)?;
        require_non_empty("problem_id", &self.problem_id)?;
        if !is_valid_topic(&self.topic) {
            return Err(invalid(
                "topic",
                format!("{:?} is not a dotted [a-z0-9_] ID", self.topic),
            ));
        }
        if self.system_prompt_version != TUTOR_POLICY_VERSION {
            return Err(invalid(
                "system_prompt_version",
                format!(
                    "expected {TUTOR_POLICY_VERSION}, got {}",
                    self.system_prompt_version
                ),
            ));
        }
        self.validate_misconceptions()?;
        if self
            .misconceptions
            .iter()
            .any(|id| id.introduced_for_topic(&self.topic) > self.provenance.taxonomy_version)
        {
            return Err(invalid(
                "provenance.taxonomy_version",
                "predates the row's misconception label",
            ));
        }

        self.validate_topic()?;
        self.validate_provenance()?;
        let levels = self.validate_turns()?;
        if levels != self.hint_levels_reached {
            return Err(invalid(
                "hint_levels_reached",
                format!(
                    "declared {:?} but the hint turns reach {:?}",
                    self.hint_levels_reached, levels
                ),
            ));
        }
        Ok(())
    }

    fn validate_misconceptions(&self) -> Result<(), SchemaError> {
        if self.misconceptions.len() > 1 {
            return Err(invalid(
                "misconceptions",
                "more than one primary misconception; a second error is a new row",
            ));
        }
        match self.row_type {
            RowType::Correction if self.misconceptions.is_empty() => Err(invalid(
                "misconceptions",
                "correction row must name exactly one primary misconception",
            )),
            RowType::Confirmation if !self.misconceptions.is_empty() => Err(invalid(
                "misconceptions",
                "confirmation row must not name a misconception; the attempt was correct",
            )),
            _ => Ok(()),
        }
    }

    /// A correction row's `topic` must be one of its misconception's listed
    /// taxonomy topics; a confirmation row names no misconception, so any
    /// well-formed topic passes.
    fn validate_topic(&self) -> Result<(), SchemaError> {
        if self.row_type != RowType::Correction {
            return Ok(());
        }
        // `validate_misconceptions` has already required exactly one id.
        let misconception = self.misconceptions[0];
        if !misconception.topics().contains(&self.topic.as_str()) {
            return Err(invalid(
                "topic",
                format!(
                    "{:?} is not a listed topic for {}",
                    self.topic,
                    misconception.as_str()
                ),
            ));
        }
        Ok(())
    }

    fn validate_provenance(&self) -> Result<(), SchemaError> {
        let provenance = &self.provenance;
        // Rows recorded under any supported taxonomy version stay valid: an
        // id never changes meaning, and deprecated ids remain parseable.
        if provenance.taxonomy_version == 0 || provenance.taxonomy_version > crate::TAXONOMY_VERSION
        {
            return Err(invalid(
                "provenance.taxonomy_version",
                format!(
                    "expected a supported version 1..={}, got {}",
                    crate::TAXONOMY_VERSION,
                    provenance.taxonomy_version
                ),
            ));
        }
        match provenance.source {
            RowSource::HandWritten => {
                if provenance.generator.is_some() {
                    return Err(invalid(
                        "provenance.generator",
                        "hand_written rows carry no generator",
                    ));
                }
            }
            RowSource::Generated => {
                let generator = provenance.generator.as_ref().ok_or_else(|| {
                    invalid(
                        "provenance.generator",
                        "generated rows must name their generator",
                    )
                })?;
                require_non_empty("provenance.generator.name", &generator.name)?;
                if generator.version == 0 {
                    return Err(invalid(
                        "provenance.generator.version",
                        "must be at least 1",
                    ));
                }
            }
        }
        if self.gold && provenance.source != RowSource::HandWritten {
            return Err(invalid(
                "gold",
                "gold rows must have provenance.source \"hand_written\"",
            ));
        }
        Ok(())
    }

    /// Check turn structure and return the ladder levels reached, ascending.
    fn validate_turns(&self) -> Result<Vec<u8>, SchemaError> {
        let turns = &self.turns;
        if turns.len() < 3 {
            return Err(invalid(
                "turns",
                "a dialogue needs at least a system, a student, and a tutor turn",
            ));
        }
        if turns[0].role != Role::System {
            return Err(invalid("turns", "first turn must be the system prompt"));
        }
        Self::validate_turn_order(turns)?;
        Self::validate_answer_requests(turns)?;
        Self::validate_closing(turns)?;
        self.validate_row_shape(turns)?;
        let last = turns.len() - 1;
        for (index, turn) in turns.iter().enumerate() {
            Self::validate_turn(index, turn, last)?;
        }
        Self::collect_hint_levels(turns)
    }

    /// Student and tutor turns must strictly alternate after the system turn.
    fn validate_turn_order(turns: &[Turn]) -> Result<(), SchemaError> {
        for (index, turn) in turns.iter().enumerate().skip(1) {
            let expected = if index % 2 == 1 {
                Role::Student
            } else {
                Role::Tutor
            };
            if turn.role != expected {
                return Err(invalid(
                    "turns",
                    "student and tutor turns must alternate after the system turn",
                ));
            }
        }
        Ok(())
    }

    /// Ladder levels climb contiguously from L1, one rung per hint turn.
    fn collect_hint_levels(turns: &[Turn]) -> Result<Vec<u8>, SchemaError> {
        let mut levels: Vec<u8> = Vec::new();
        for turn in turns {
            if turn.kind != Some(TurnKind::Hint) {
                continue;
            }
            // `validate_turn` has already bounded the level to 1..=MAX_HINT_LEVEL.
            let level = turn.hint_level.unwrap_or(0);
            let expected = levels.len() as u8 + 1;
            if level != expected {
                return Err(invalid(
                    "turns",
                    "hint levels must climb one rung at a time, starting at level 1",
                ));
            }
            levels.push(level);
        }
        Ok(levels)
    }

    /// A direct answer request converts to the next rung of the ladder — L1
    /// when no hint has been given yet — never to the answer.
    fn validate_answer_requests(turns: &[Turn]) -> Result<(), SchemaError> {
        for (index, turn) in turns.iter().enumerate() {
            if turn.kind != Some(TurnKind::AnswerRequest) {
                continue;
            }
            let expected = turns[..index]
                .iter()
                .filter(|earlier| earlier.kind == Some(TurnKind::Hint))
                .count()
                + 1;
            let converts = turns.get(index + 1).is_some_and(|response| {
                response.kind == Some(TurnKind::Hint)
                    && u8::try_from(expected).ok() == response.hint_level
            });
            if !converts {
                return Err(invalid(
                    "turns",
                    "a direct answer request converts to the next rung of the hint ladder, never to the answer",
                ));
            }
        }
        Ok(())
    }

    /// The last turn is the tutor's closing turn, confirming the student's
    /// final attempt against the verified answer.
    fn validate_closing(turns: &[Turn]) -> Result<(), SchemaError> {
        let last = turns.len() - 1;
        let closing = &turns[last];
        if closing.role != Role::Tutor || closing.kind != Some(TurnKind::Closing) {
            return Err(invalid(
                "turns",
                "the last turn must be the tutor's closing turn",
            ));
        }
        if !closing.reveals_final_answer {
            return Err(invalid(
                "turns",
                "the closing turn must confirm the verified answer",
            ));
        }
        let before = &turns[last - 1];
        if before.role != Role::Student || before.kind != Some(TurnKind::Attempt) {
            return Err(invalid(
                "turns",
                "the closing turn must follow a final student attempt",
            ));
        }
        Ok(())
    }

    /// Row-type shape: a correction row whose first student turn is an
    /// `attempt` opens with a diagnosis turn (an `answer_request` opening
    /// converts to an L1 hint instead); confirmation rows are exactly
    /// system + attempt + closing.
    fn validate_row_shape(&self, turns: &[Turn]) -> Result<(), SchemaError> {
        match self.row_type {
            RowType::Correction
                if turns[1].kind == Some(TurnKind::Attempt)
                    && turns[2].kind != Some(TurnKind::Diagnosis) =>
            {
                Err(invalid(
                    "turns",
                    "a correction row's first tutor turn must be a diagnosis",
                ))
            }
            RowType::Confirmation if turns.len() != 3 => Err(invalid(
                "turns",
                "a confirmation row is system + student attempt + tutor closing",
            )),
            _ => Ok(()),
        }
    }

    fn validate_turn(index: usize, turn: &Turn, last: usize) -> Result<(), SchemaError> {
        require_non_empty("turns.content", &turn.content)?;
        Self::validate_kind(turn)?;
        Self::validate_hint_level(turn)?;
        Self::validate_reveal(index, turn, last)?;
        if turn.kind == Some(TurnKind::Closing) && index != last {
            return Err(invalid("turns", "only the final turn is a closing turn"));
        }
        Ok(())
    }

    /// The turn's `kind` must match its `role`.
    fn validate_kind(turn: &Turn) -> Result<(), SchemaError> {
        let kind_ok = match turn.role {
            Role::System => turn.kind.is_none(),
            Role::Student => matches!(turn.kind, Some(TurnKind::Attempt | TurnKind::AnswerRequest)),
            Role::Tutor => matches!(
                turn.kind,
                Some(TurnKind::Diagnosis | TurnKind::Hint | TurnKind::Closing)
            ),
        };
        if !kind_ok {
            return Err(invalid(
                "turns.kind",
                "system turns carry no kind; student turns are attempt or answer_request; tutor turns are diagnosis, hint, or closing",
            ));
        }
        Ok(())
    }

    /// `hint_level` is required on tutor hint turns, in range, absent elsewhere.
    fn validate_hint_level(turn: &Turn) -> Result<(), SchemaError> {
        let is_hint = turn.kind == Some(TurnKind::Hint);
        match (turn.hint_level, is_hint) {
            (Some(level), true) if !(1..=MAX_HINT_LEVEL).contains(&level) => Err(invalid(
                "turns.hint_level",
                format!("hint level must be 1..={MAX_HINT_LEVEL}"),
            )),
            (Some(_), false) => Err(invalid(
                "turns.hint_level",
                "only tutor hint turns carry a hint_level",
            )),
            (None, true) => Err(invalid(
                "turns.hint_level",
                "hint turns must declare their ladder level",
            )),
            _ => Ok(()),
        }
    }

    /// `reveals_final_answer` may be set only on an L4 worked solution or on
    /// the closing turn.
    fn validate_reveal(index: usize, turn: &Turn, last: usize) -> Result<(), SchemaError> {
        if !turn.reveals_final_answer {
            return Ok(());
        }
        let allowed = (index == last && turn.kind == Some(TurnKind::Closing))
            || (turn.kind == Some(TurnKind::Hint) && turn.hint_level == Some(MAX_HINT_LEVEL));
        if !allowed {
            return Err(invalid(
                "turns.reveals_final_answer",
                "the final answer may only appear on an L4 worked solution or the closing turn",
            ));
        }
        Ok(())
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), SchemaError> {
    if value.trim().is_empty() {
        return Err(SchemaError::Invalid {
            field,
            reason: "must be non-empty".to_owned(),
        });
    }
    Ok(())
}

/// The JSON Schema for [`DialogueRow`], generated from the types above.
///
/// Post-processing pins `schema_version` to [`DIALOGUE_SCHEMA_VERSION`] and
/// `topic` to the dotted-ID pattern, which `schemars` cannot express.
pub fn dialogue_json_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(DialogueRow))
        .expect("generated schema serializes");
    let root = schema.as_object_mut().expect("schema is an object");
    root.insert(
        "$id".to_owned(),
        serde_json::Value::String(
            "https://github.com/rmems/AI-ML-SWE-synthetic-tutor/schemas/dialogue.schema.json"
                .to_owned(),
        ),
    );
    root.insert(
        "description".to_owned(),
        serde_json::Value::String(
            "One tutoring dialogue row (schema_version 1). Mirrors tutor_schema::DialogueRow; \
             regenerate with `cargo run -p tutor-schema --example print_dialogue_schema`. \
             The schema expresses shape; crates/tutor-schema and crates/tutor-validate enforce \
             the turn-level policy constraints."
                .to_owned(),
        ),
    );
    schema["properties"]["schema_version"] = serde_json::json!({
        "const": DIALOGUE_SCHEMA_VERSION
    });
    schema["properties"]["topic"] = serde_json::json!({
        "description": "Dotted curriculum topic ID.",
        "type": "string",
        "pattern": "^[a-z0-9_]+(\\.[a-z0-9_]+)*$"
    });
    schema
}
