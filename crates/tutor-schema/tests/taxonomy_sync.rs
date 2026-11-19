//! Keeps `schemas/taxonomy.yaml` and `tutor_schema::MisconceptionId` in line, and
//! checks that every entry carries the fields the issue requires.

use std::collections::BTreeSet;
use tutor_schema::{MisconceptionId, TAXONOMY_VERSION};

const TAXONOMY: &str = include_str!("../../../schemas/taxonomy.yaml");

const REQUIRED_FIELDS: [&str; 7] = [
    "error_pattern",
    "bites_at",
    "wrong_attempt",
    "correct_reasoning",
    "diagnostic_question",
    "first_hint",
    "topics",
];

/// `(id, block)` pairs for each `misconceptions:` list item.
fn entries() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in TAXONOMY.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- id:") {
            if let Some(done) = current.take() {
                out.push(done);
            }
            current = Some((rest.trim().to_owned(), String::new()));
        } else if let Some((_, block)) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    if let Some(done) = current {
        out.push(done);
    }
    out
}

#[test]
fn taxonomy_version_matches() {
    let version = TAXONOMY
        .lines()
        .find_map(|line| line.trim().strip_prefix("version:"))
        .expect("taxonomy.yaml has a version field")
        .trim()
        .parse::<u32>()
        .expect("version is an integer");
    assert_eq!(version, TAXONOMY_VERSION);
}

#[test]
fn yaml_ids_match_misconception_id_all() {
    let yaml_ids: BTreeSet<String> = entries().into_iter().map(|(id, _)| id).collect();
    let rust_ids: BTreeSet<String> = MisconceptionId::ALL
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(yaml_ids, rust_ids);
    assert_eq!(yaml_ids.len(), MisconceptionId::ALL.len());
}

#[test]
fn every_entry_has_required_fields() {
    for (id, block) in entries() {
        for field in REQUIRED_FIELDS {
            assert!(
                block.contains(&format!("    {field}:")),
                "{id} is missing `{field}`"
            );
        }
    }
}

#[test]
fn every_entry_lists_two_topics() {
    for (id, block) in entries() {
        let topics = block
            .split("    topics:")
            .nth(1)
            .expect("topics section")
            .lines()
            .skip_while(|line| line.trim().is_empty())
            .take_while(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("- ") && !trimmed.starts_with("- id:")
            })
            .count();
        assert!(topics >= 2, "{id} lists fewer than 2 topics");
    }
}

#[test]
fn ids_serde_round_trip_and_reject_unknown() {
    for id in MisconceptionId::ALL {
        let json = serde_json::to_string(id).expect("serializes");
        assert_eq!(json, format!("\"{}\"", id.as_str()));
        let parsed: MisconceptionId = serde_json::from_str(&json).expect("deserializes own id");
        assert_eq!(parsed, *id);
    }
    assert!(serde_json::from_str::<MisconceptionId>("\"algebra.not_a_thing\"").is_err());
    assert!(serde_json::from_str::<MisconceptionId>("\"algebra:sign-error\"").is_err());
}
