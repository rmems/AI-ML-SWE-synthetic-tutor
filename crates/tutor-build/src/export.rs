//! Stage 5 — export to standard multi-turn chat JSONL.
//!
//! One JSON object per line: `messages` holds the conversation in the usual
//! chat roles (`system` → `system`, `student` → `user`, `tutor` →
//! `assistant`), and `provenance` carries every field issue #8 requires —
//! problem id, generator name + version, seed / parameters, the taxonomy id
//! (or `"none"`), teacher id + rights entry (both `"none"` in this slice:
//! the dialogue schema has no teacher field yet and no teacher lane is
//! wired), the verification method + result, and the QA status.
//!
//! Turn-level policy metadata (`kind`, `hint_level`,
//! `reveals_final_answer`) is not emitted: the exported file is the training
//! view. The full `DialogueRow` stays recoverable from the input files via
//! `row_id`.

use serde::Serialize;

use tutor_schema::{NamedVersion, Role, RowSource, RowType};

use crate::{checks::CheckedRow, dedup::Fingerprinted, splits::Split};

/// One exported row: a standard chat `messages` list plus provenance.
#[derive(Debug, Serialize)]
pub struct ExportRow {
    /// `row_id` of the source `DialogueRow`.
    pub row_id: String,
    /// `correction` or `confirmation`.
    pub row_type: RowType,
    /// Dotted curriculum topic ID.
    pub topic: String,
    /// Which split file this row was written to.
    pub split: Split,
    /// The conversation in standard chat roles.
    pub messages: Vec<Message>,
    /// Every provenance field required by issue #8.
    pub provenance: ExportProvenance,
}

/// One chat message.
#[derive(Debug, Serialize)]
pub struct Message {
    /// `system`, `user`, or `assistant`.
    pub role: &'static str,
    /// The turn content.
    pub content: String,
}

/// The provenance block of an [`ExportRow`]. Fields that genuinely do not
/// exist for a row serialize as `null`; the issue's own wording is kept for
/// `taxonomy_id`, `teacher_id`, and `rights_entry`, which emit `"none"`.
#[derive(Debug, Serialize)]
pub struct ExportProvenance {
    /// `problem_id` the dialogue is grounded in.
    pub problem_id: String,
    /// `hand_written` or `generated`, from the row's provenance.
    pub source: RowSource,
    /// Generator that produced the row (`generated` rows only).
    pub row_generator: Option<NamedVersion>,
    /// Generator that produced the grounding problem, when `--problems` was
    /// given and matched.
    pub problem_generator: Option<NamedVersion>,
    /// Problem generator seed, from the verified problem.
    pub seed: Option<u64>,
    /// Structured problem parameters, from the verified problem.
    pub parameters: Option<serde_json::Value>,
    /// Taxonomy ID of the planted misconception, or `"none"` for a
    /// confirmation row.
    pub taxonomy_id: String,
    /// Teacher model ID when a model generated part of the row, else
    /// `"none"`. Dialogue schema v1 has no teacher field and no teacher
    /// lane is wired, so this is always `"none"` in this slice.
    pub teacher_id: String,
    /// Rights-registry entry permitting training use of the teacher's
    /// output, or `"none"` when no teacher contributed.
    pub rights_entry: String,
    /// How the answer was checked, and the pass/fail record.
    pub verification: VerificationRecord,
    /// `unreviewed`, `sampled_and_passed`, or `human_reviewed`. Only `gold`
    /// exemplars ship as `human_reviewed` — a human authored and checked
    /// them; every other row is `unreviewed` until QA runs.
    pub qa_status: QaStatus,
}

/// Verification provenance on an exported row.
#[derive(Debug, Serialize)]
pub struct VerificationRecord {
    /// The method used (`exact_integer_recompute` from `tutor-verify`, or
    /// `hand_written` for hand-checked exemplar rows).
    pub method: String,
    /// Who ran the check (`tutor-verify`, or `human` for exemplars).
    pub verifier: String,
    /// Verifier version, when recorded on the problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier_version: Option<String>,
    /// `pass` or `fail`. Only `pass` ships: failed rows are rejected before
    /// export.
    pub result: &'static str,
}

/// QA status values per the issue: unreviewed, sampled-and-passed, or
/// human-reviewed.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaStatus {
    /// No QA pass yet; the default for pipeline rows.
    Unreviewed,
    /// A sampled QA review passed.
    SampledAndPassed,
    /// A human reviewed the row. Hand-written gold exemplars ship as such.
    HumanReviewed,
}

/// Convert a kept row into its export form for `split`.
pub fn to_export(row: &Fingerprinted, split: Split) -> ExportRow {
    let checked = &row.checked;
    let messages = checked
        .row
        .turns
        .iter()
        .map(|turn| Message {
            role: match turn.role {
                Role::System => "system",
                Role::Student => "user",
                Role::Tutor => "assistant",
            },
            content: turn.content.clone(),
        })
        .collect();
    ExportRow {
        row_id: checked.row.row_id.clone(),
        row_type: checked.row.row_type,
        topic: checked.row.topic.clone(),
        split,
        messages,
        provenance: provenance(checked),
    }
}

/// The taxonomy id of a row's primary misconception, or `"none"`.
pub fn taxonomy_id(row: &CheckedRow) -> String {
    row.row
        .misconceptions
        .first()
        .map_or_else(|| "none".to_owned(), |id| id.as_str().to_owned())
}

fn provenance(row: &CheckedRow) -> ExportProvenance {
    let problem = row.problem.as_ref();
    let verification = match problem.and_then(|p| p.verification.as_ref()) {
        Some(v) => VerificationRecord {
            method: v.method.clone(),
            verifier: v.verifier.clone(),
            verifier_version: Some(v.verifier_version.clone()),
            result: "pass",
        },
        None => VerificationRecord {
            method: "hand_written".to_owned(),
            verifier: "human".to_owned(),
            verifier_version: None,
            result: "pass",
        },
    };
    let qa_status = if row.row.gold {
        QaStatus::HumanReviewed
    } else {
        QaStatus::Unreviewed
    };
    ExportProvenance {
        problem_id: row.row.problem_id.clone(),
        source: row.row.provenance.source,
        row_generator: row.row.provenance.generator.clone(),
        problem_generator: problem.map(|p| NamedVersion {
            name: p.generator.name.clone(),
            version: p.generator.version,
        }),
        seed: problem.map(|p| p.generator.seed),
        parameters: problem.and_then(|p| serde_json::to_value(&p.parameters).ok()),
        taxonomy_id: taxonomy_id(row),
        teacher_id: "none".to_owned(),
        rights_entry: "none".to_owned(),
        verification,
        qa_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputLine;
    use std::collections::HashSet;
    use std::path::PathBuf;

    const GOLD_CHAIN: &str =
        include_str!("../../../datasets/gold/gold-derivatives-chain-rule.json");
    const GOLD_CONFIRM: &str = include_str!("../../../datasets/gold/gold-confirmation.json");

    fn checked(text: &str) -> CheckedRow {
        let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema()).unwrap();
        let line = InputLine {
            file: PathBuf::from("rows.jsonl"),
            line: Some(1),
            text: text.to_owned(),
        };
        crate::checks::check(&line, None, &validator, &mut HashSet::new()).unwrap()
    }

    #[test]
    fn export_maps_roles_and_provenance() {
        let row = checked(GOLD_CHAIN);
        let fp = Fingerprinted {
            checked: row,
            fingerprint: crate::dedup::fingerprint("x"),
        };
        let export = to_export(&fp, Split::Train);
        let roles: Vec<_> = export.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [
                "system",
                "user",
                "assistant",
                "user",
                "assistant",
                "user",
                "assistant"
            ]
        );
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["provenance"]["taxonomy_id"], "derivatives.chain_rule");
        assert_eq!(json["provenance"]["teacher_id"], "none");
        assert_eq!(json["provenance"]["verification"]["method"], "hand_written");
        assert_eq!(json["provenance"]["qa_status"], "human_reviewed");
        assert_eq!(json["split"], "train");
    }

    #[test]
    fn confirmation_row_has_none_taxonomy() {
        let row = checked(GOLD_CONFIRM);
        assert_eq!(taxonomy_id(&row), "none");
    }
}
