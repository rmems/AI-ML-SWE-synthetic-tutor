//! Stage 3 — decontamination against the held-out eval set.
//!
//! Every `.json` / `.jsonl` record under the `--heldout` directory
//! (default `evals/heldout/`) is fingerprinted the same way candidate rows
//! are in [`dedup`]: all of the record's string leaves joined into one
//! normalized document, plus every `problem_id` it names. A candidate row
//! is rejected when its `problem_id` appears in the held-out set or its
//! document matches a held-out document exactly (`heldout_exact_match`),
//! or when the shingle Jaccard similarity reaches
//! [`NEAR_DUPLICATE_THRESHOLD`](crate::dedup::NEAR_DUPLICATE_THRESHOLD)
//! (`heldout_near_match`).
//!
//! The held-out set is input-only by construction: this module only reads
//! it, and nothing in the crate generates against it or tunes to it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::{BuildError, RejectedRow, Stage, dedup, input};

/// The loaded held-out set: fingerprints of every held-out record plus the
/// files they came from (recorded as inputs in the manifest).
#[derive(Debug)]
pub struct HeldOut {
    /// Fingerprinted held-out records, in sorted-file then line order.
    pub items: Vec<HeldOutItem>,
    /// The `.json` / `.jsonl` files the records were read from.
    pub files: Vec<PathBuf>,
}

/// One held-out record's fingerprints.
#[derive(Debug)]
pub struct HeldOutItem {
    /// File the record was read from, for rejection detail.
    pub origin: String,
    /// Every `problem_id` named anywhere in the record.
    pub problem_ids: BTreeSet<String>,
    /// Normalized-text fingerprint of all the record's string leaves.
    pub fingerprint: dedup::Fingerprint,
}

/// Read and fingerprint every held-out record under `dir`.
///
/// # Errors
/// [`BuildError::HeldOut`] when the directory or a record cannot be read or
/// parsed — a corrupt held-out set must fail closed, not silently shrink
/// the decontamination corpus.
pub fn load(dir: &std::path::Path) -> Result<HeldOut, BuildError> {
    let files = input::heldout_files(dir)?;
    let mut items = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| BuildError::heldout(file, format!("cannot read: {e}")))?;
        for (index, line) in text.lines().enumerate() {
            let chunk = if is_jsonl(file) { line } else { text.as_str() };
            if chunk.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(chunk).map_err(|e| {
                let location = if is_jsonl(file) {
                    format!("line {}: ", index + 1)
                } else {
                    String::new()
                };
                BuildError::heldout(file, format!("{location}not JSON: {e}"))
            })?;
            items.push(HeldOutItem::new(file, &value));
            // For .json files the whole file is one record; stop after it.
            if !is_jsonl(file) {
                break;
            }
        }
    }
    Ok(HeldOut { items, files })
}

impl HeldOutItem {
    fn new(file: &std::path::Path, value: &serde_json::Value) -> Self {
        let mut text = String::new();
        let mut problem_ids = BTreeSet::new();
        collect_strings(value, &mut text, &mut problem_ids);
        Self {
            origin: file.display().to_string(),
            problem_ids,
            fingerprint: dedup::fingerprint(&text),
        }
    }
}

/// Join every string leaf into `text` and collect `problem_id` values.
fn collect_strings(
    value: &serde_json::Value,
    text: &mut String,
    problem_ids: &mut BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "problem_id"
                    && let serde_json::Value::String(id) = child
                {
                    problem_ids.insert(id.clone());
                }
                collect_strings(child, text, problem_ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, text, problem_ids);
            }
        }
        serde_json::Value::String(s) => {
            text.push_str(s);
            text.push(' ');
        }
        _ => {}
    }
}

/// Reject kept rows that match the held-out set; pass the rest through.
pub fn run(
    kept: Vec<dedup::Fingerprinted>,
    heldout: &HeldOut,
    rejected: &mut Vec<RejectedRow>,
) -> Vec<dedup::Fingerprinted> {
    let mut clean = Vec::new();
    for row in kept {
        if let Some((code, detail)) = contamination(&row, heldout) {
            rejected.push(row.checked.rejected(Stage::Decontamination, code, detail));
        } else {
            clean.push(row);
        }
    }
    clean
}

/// The rejection code + detail when `row` contaminates `heldout`, else
/// `None`. Exact matches (problem id or identical normalized text) are
/// checked across the whole set before near matches so the reason code is
/// deterministic.
fn contamination(row: &dedup::Fingerprinted, heldout: &HeldOut) -> Option<(&'static str, String)> {
    let problem_id = &row.checked.row.problem_id;
    if let Some(item) = heldout
        .items
        .iter()
        .find(|item| item.problem_ids.contains(problem_id))
    {
        return Some((
            "heldout_exact_match",
            format!("problem_id {problem_id:?} is held out by {}", item.origin),
        ));
    }
    if let Some(item) = heldout
        .items
        .iter()
        .find(|item| item.fingerprint.document == row.fingerprint.document)
    {
        return Some((
            "heldout_exact_match",
            format!("normalized text matches {}", item.origin),
        ));
    }
    heldout
        .items
        .iter()
        .map(|item| {
            (
                item,
                dedup::jaccard(&item.fingerprint.shingles, &row.fingerprint.shingles),
            )
        })
        .filter(|(_, similarity)| *similarity >= dedup::NEAR_DUPLICATE_THRESHOLD)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(item, similarity)| {
            (
                "heldout_near_match",
                format!(
                    "{similarity:.3} similar to {} (threshold {})",
                    item.origin,
                    dedup::NEAR_DUPLICATE_THRESHOLD
                ),
            )
        })
}

fn is_jsonl(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_strings_gathers_leaves_and_problem_ids() {
        let value = serde_json::json!({
            "problem_id": "p-1",
            "statement": "Compute f'(2).",
            "turns": [{"content": "hello"}],
            "nested": {"problem_id": "p-2"}
        });
        let mut text = String::new();
        let mut ids = BTreeSet::new();
        collect_strings(&value, &mut text, &mut ids);
        assert!(text.contains("Compute f'(2).") && text.contains("hello"));
        assert!(ids.contains("p-1") && ids.contains("p-2"));
    }
}
