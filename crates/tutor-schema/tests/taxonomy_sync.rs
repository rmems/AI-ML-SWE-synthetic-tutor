//! Keeps `schemas/taxonomy.yaml` and `tutor_schema::MisconceptionId` in line, and
//! checks that every entry carries the fields the issue requires.

use std::collections::BTreeSet;
use tutor_schema::{MisconceptionId, TAXONOMY_VERSION};

const TAXONOMY: &str = include_str!("../../../schemas/taxonomy.yaml");

const REQUIRED_FIELDS: [&str; 8] = [
    "status",
    "error_pattern",
    "bites_at",
    "wrong_attempt",
    "correct_reasoning",
    "diagnostic_question",
    "first_hint",
    "topics",
];

const STATUSES: [&str; 2] = ["active", "deprecated"];

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

/// The value of a top-level (4-space indented) `key:` line in an entry block.
fn top_level_value<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|line| {
        line.strip_prefix("    ")
            .and_then(|rest| rest.strip_prefix(key))
            .and_then(|rest| rest.strip_prefix(':'))
            .map(|rest| rest.trim())
    })
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
    let parsed = entries();
    let ids: Vec<&str> = parsed.iter().map(|(id, _)| id.as_str()).collect();
    let unique: BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "taxonomy.yaml has duplicate ids: {ids:?}"
    );
    let yaml_ids: BTreeSet<String> = parsed.into_iter().map(|(id, _)| id).collect();
    let rust_ids: BTreeSet<String> = MisconceptionId::ALL
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(yaml_ids, rust_ids);
    assert_eq!(yaml_ids.len(), MisconceptionId::ALL.len());
}

#[test]
fn all_ids_are_unique_sorted_and_covered() {
    let strs: Vec<&str> = MisconceptionId::ALL.iter().map(|id| id.as_str()).collect();
    let unique: BTreeSet<&str> = strs.iter().copied().collect();
    assert_eq!(strs.len(), unique.len(), "ALL has duplicate ids");
    let mut sorted = strs.clone();
    sorted.sort_unstable();
    assert_eq!(strs, sorted, "ALL must stay sorted by id");
}

#[test]
fn every_entry_has_required_fields() {
    for (id, block) in entries() {
        for field in REQUIRED_FIELDS {
            assert!(
                top_level_value(&block, field).is_some(),
                "{id} is missing top-level `{field}`"
            );
        }
        let status = top_level_value(&block, "status").expect("status present");
        assert!(
            STATUSES.contains(&status),
            "{id} has unsupported status {status:?}"
        );
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
