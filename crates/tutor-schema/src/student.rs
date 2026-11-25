//! Synthetic student-attempt artifacts produced before dialogue assembly.
//!
//! The artifact is not a tutoring dialogue row: it is the student half of one.
//! Its `student_turns` use the canonical [`Turn`] type from
//! `schemas/dialogue.schema.json`, so assembly can splice them into a
//! [`crate::DialogueRow`] without a representation conversion. The surrounding
//! fields record what the dialogue schema deliberately does not model: the
//! sampled student profile, persistence behaviour, planted transformation, and
//! surface-noise flags.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    MisconceptionId, NamedVersion, Role, SchemaError, TAXONOMY_VERSION, Turn, TurnKind, invalid,
    is_valid_topic,
};

/// Current `schema_version` of [`StudentAttempt`].
pub const STUDENT_ATTEMPT_SCHEMA_VERSION: u32 = 1;

/// One seeded synthetic-student simulation for a verified problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StudentAttempt {
    /// Always [`STUDENT_ATTEMPT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable attempt ID, unique for (problem, simulator version, attempt seed).
    pub attempt_id: String,
    /// Verified problem this attempt is grounded in.
    pub problem_id: String,
    /// Dotted curriculum topic ID copied from the problem.
    pub topic: String,
    /// Whether the opening attempt is fully correct (confirmation-row material).
    pub correct: bool,
    /// The one planted taxonomy ID for correction-row material; absent when
    /// `correct` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misconception: Option<MisconceptionId>,
    /// Sampled student profile controlling completeness and surface form.
    pub profile: StudentProfile,
    /// How the planted error persists across the hint ladder.
    pub persistence: Persistence,
    /// Defined procedural corruption of the verified worked solution; absent
    /// on a correct attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transformation: Option<PlantedTransformation>,
    /// Student turns in exactly the dialogue schema's `Turn` representation.
    /// Every turn is `role: "student", kind: "attempt"`. The first is the
    /// opening attempt; later entries are retries after tutor hint levels.
    pub student_turns: Vec<Turn>,
    /// Whether teacher surface variety was requested and how it was handled.
    pub teacher_variety: TeacherVariety,
    /// Simulator identity, seed, and taxonomy version for replay.
    pub provenance: StudentAttemptProvenance,
}

/// Topic-relative student strength and surface form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StudentProfile {
    /// Topic-relative level controlling error rate and attempt completeness.
    pub level: StudentLevel,
    /// Terse or verbose student voice.
    pub surface_style: SurfaceStyle,
    /// Non-substantive notation slips present in the rendered attempt. Empty
    /// means none; every slip is explicit so it cannot masquerade as the
    /// taxonomy label.
    pub notation_noise: Vec<NotationNoise>,
}

/// Topic-relative student level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StudentLevel {
    /// Shorter, less complete work; in batch generation this level is more
    /// likely to receive a planted misconception.
    Struggling,
    /// Ordinary complete work.
    Typical,
    /// More complete explanatory work; in batch generation this level is more
    /// likely to be correct.
    Strong,
}

/// Surface verbosity of the student's written attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceStyle {
    /// Mostly equations and minimal connective text.
    Terse,
    /// Equations plus short explanations of the chosen step.
    Verbose,
}

/// A non-substantive notation slip. These never change the mathematical claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotationNoise {
    /// One multiplication dot is omitted (for example `2x cos(x)`), with no
    /// change to the intended multiplication.
    DroppedMultiplicationDot,
}

/// How a planted misconception behaves as tutor hints arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    /// After answering the diagnosis, the student corrects the planted error
    /// immediately after the L1 nudge.
    CorrectsAfterL1,
    /// The student repeats the same planted error after L1 and corrects after L2.
    CorrectsAfterL2,
    /// The error is still present after L2; the student corrects only after L3.
    NeedsL3,
}

/// The procedural transformation that planted one taxonomy misconception.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantedTransformation {
    /// The taxonomy ID this transformation implements.
    pub misconception: MisconceptionId,
    /// Zero-based index of the verified worked-solution step where the error bites.
    pub step_index: usize,
    /// The `kind` of that worked-solution step.
    pub step_kind: String,
    /// Verified value at the step/final propagation point.
    pub correct_value: i64,
    /// Value produced by applying exactly this transformation.
    pub corrupted_value: i64,
    /// Whether the corrupted step propagates to the student's final answer.
    pub propagates_to_answer: bool,
    /// Short machine-auditable description of the transformation.
    pub description: String,
}

/// Teacher surface-variety provenance. The procedural core never rewrites through
/// a teacher; teacher lanes fail closed unless an allowed teacher client is wired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TeacherVariety {
    /// Whether the caller requested teacher surface variety.
    pub requested: bool,
    /// Result of the request.
    pub status: TeacherVarietyStatus,
    /// Teacher used when `status` is `applied`; absent for procedural output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teacher: Option<TeacherId>,
}

/// How teacher surface variety was handled for one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeacherVarietyStatus {
    /// Not requested; procedural rendering only.
    Disabled,
    /// Requested but not applied because no allowed teacher lane is wired; rows
    /// carrying this status are procedural output and must not be represented as
    /// teacher-varied rows.
    Unavailable,
    /// An allowed teacher rephrased the already-planted attempt and the result
    /// was rechecked against the labelled transformation.
    Applied,
}

/// Teachers named by the rights registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeacherId {
    /// DeepSeek V4.1 Flash.
    DeepseekV41Flash,
    /// MiMo V2.6 Flash at the pinned rights-registry revision.
    MimoV26Flash,
}

/// Replay provenance for a student attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StudentAttemptProvenance {
    /// Simulator name/version.
    pub generator: NamedVersion,
    /// Attempt-generation seed.
    pub seed: u64,
    /// Taxonomy version used for the misconception label.
    pub taxonomy_version: u32,
}

impl StudentAttempt {
    /// Parse and validate one student-attempt artifact.
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        let attempt: Self = serde_json::from_str(json)?;
        attempt.validate()?;
        Ok(attempt)
    }

    /// Convert an already decoded JSON value, then validate it.
    pub fn from_value(value: serde_json::Value) -> Result<Self, SchemaError> {
        let attempt: Self = serde_json::from_value(value)?;
        attempt.validate()?;
        Ok(attempt)
    }

    /// Check consistency constraints serde cannot express.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != STUDENT_ATTEMPT_SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                format!(
                    "expected {STUDENT_ATTEMPT_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            ));
        }
        require_non_empty("attempt_id", &self.attempt_id)?;
        require_non_empty("problem_id", &self.problem_id)?;
        if !is_valid_topic(&self.topic) {
            return Err(invalid(
                "topic",
                format!("{:?} is not a dotted ID", self.topic),
            ));
        }
        if self.provenance.taxonomy_version == 0
            || self.provenance.taxonomy_version > TAXONOMY_VERSION
        {
            return Err(invalid(
                "provenance.taxonomy_version",
                format!("expected 1..={TAXONOMY_VERSION}"),
            ));
        }
        self.validate_label()?;
        self.validate_turns()?;
        self.validate_teacher_variety()
    }

    fn validate_label(&self) -> Result<(), SchemaError> {
        match (self.correct, self.misconception, &self.transformation) {
            (true, None, None) => Ok(()),
            (false, Some(id), Some(transformation)) => {
                if transformation.misconception != id {
                    return Err(invalid(
                        "transformation.misconception",
                        "must match the attempt's planted misconception",
                    ));
                }
                if !id.topics().contains(&self.topic.as_str()) {
                    return Err(invalid(
                        "topic",
                        format!("{:?} is not a listed topic for {}", self.topic, id.as_str()),
                    ));
                }
                Ok(())
            }
            _ => Err(invalid(
                "misconception",
                "correct attempts carry no label or transformation; incorrect attempts carry exactly one of each",
            )),
        }
    }

    fn validate_turns(&self) -> Result<(), SchemaError> {
        let expected = if self.correct {
            1
        } else {
            match self.persistence {
                Persistence::CorrectsAfterL1 => 3,
                Persistence::CorrectsAfterL2 => 4,
                Persistence::NeedsL3 => 5,
            }
        };
        if self.student_turns.len() != expected {
            return Err(invalid(
                "student_turns",
                format!(
                    "expected {expected} turn(s) for this correctness/persistence, got {}",
                    self.student_turns.len()
                ),
            ));
        }
        for turn in &self.student_turns {
            if turn.role != Role::Student || turn.kind != Some(TurnKind::Attempt) {
                return Err(invalid(
                    "student_turns",
                    "every turn must be a dialogue-schema student attempt turn",
                ));
            }
            require_non_empty("student_turns.content", &turn.content)?;
            if turn.hint_level.is_some() || turn.reveals_final_answer {
                return Err(invalid(
                    "student_turns",
                    "student attempts carry no hint level and never reveal the verified answer flag",
                ));
            }
        }
        Ok(())
    }

    fn validate_teacher_variety(&self) -> Result<(), SchemaError> {
        match (
            self.teacher_variety.requested,
            self.teacher_variety.status,
            self.teacher_variety.teacher,
        ) {
            (false, TeacherVarietyStatus::Disabled, None)
            | (true, TeacherVarietyStatus::Unavailable, None)
            | (true, TeacherVarietyStatus::Applied, Some(_)) => Ok(()),
            _ => Err(invalid(
                "teacher_variety",
                "disabled/unavailable procedural rows carry no teacher; applied rows must name one requested teacher",
            )),
        }
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), SchemaError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "must be non-empty"));
    }
    Ok(())
}

/// JSON Schema for [`StudentAttempt`].
pub fn student_attempt_json_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(StudentAttempt))
        .expect("generated schema serializes");
    let root = schema.as_object_mut().expect("schema is an object");
    root.insert(
        "$id".to_owned(),
        serde_json::Value::String(
            "https://github.com/rmems/AI-ML-SWE-synthetic-tutor/schemas/student-attempt.schema.json"
                .to_owned(),
        ),
    );
    root.insert(
        "description".to_owned(),
        serde_json::Value::String(
            "One synthetic student-attempt artifact (schema_version 1). Student turns use the canonical dialogue-schema Turn shape; assembly splices them into DialogueRow. Regenerate with `cargo run -p tutor-schema --example print_student_attempt_schema`."
                .to_owned(),
        ),
    );
    schema["properties"]["schema_version"] = serde_json::json!({
        "const": STUDENT_ATTEMPT_SCHEMA_VERSION
    });
    schema["properties"]["topic"] = serde_json::json!({
        "description": "Dotted curriculum topic ID.",
        "type": "string",
        "pattern": "^[a-z0-9_]+(\\.[a-z0-9_]+)*$"
    });
    schema
}
