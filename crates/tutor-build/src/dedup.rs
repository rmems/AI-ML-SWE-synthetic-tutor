//! Stage 3 — deterministic exact and near-duplicate detection over the
//! problem text and the dialogue text together. It runs after
//! decontamination so a contaminated row can never suppress a clean
//! near-duplicate of it.
//!
//! Each row's *document* is its problem text (the verified problem's
//! `statement` when `--problems` matched, else the `problem_id`) followed by
//! every non-system turn's content. Documents are [`normalize`]d, then:
//!
//! - **exact duplicate** — normalized documents match byte-for-byte;
//! - **near duplicate** — the [Jaccard similarity](https://en.wikipedia.org/wiki/Jaccard_index)
//!   of the two documents' word [`SHINGLE_WORDS`]-shingle sets reaches
//!   [`NEAR_DUPLICATE_THRESHOLD`].
//!
//! Rows are compared in input order against the rows already kept: the first
//! occurrence wins and later duplicates are rejected, which makes the result
//! deterministic for a fixed input set. The comparison is O(n²) over kept
//! rows; candidate batches this crate sees are small enough that the
//! quadratic scan is not a bottleneck.

use std::collections::BTreeSet;

use crate::{RejectedRow, Stage, checks::CheckedRow};

/// Jaccard similarity at or above which two rows count as near duplicates.
pub const NEAR_DUPLICATE_THRESHOLD: f64 = 0.85;

/// Words per shingle in the near-duplicate comparison.
pub const SHINGLE_WORDS: usize = 3;

/// A row's normalized text plus the shingle set derived from it.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    /// The normalized document text.
    pub document: String,
    /// Word [`SHINGLE_WORDS`]-shingle set of `document`.
    pub shingles: BTreeSet<String>,
}

/// A checked row plus its dedup/decontamination fingerprint.
#[derive(Debug)]
pub struct Fingerprinted {
    /// The row that passed stage 1.
    pub checked: CheckedRow,
    /// Normalized-text fingerprint of the row's problem + dialogue text.
    pub fingerprint: Fingerprint,
}

/// Fingerprint a checked row for the decontamination and dedup stages.
pub fn fingerprinted(row: CheckedRow) -> Fingerprinted {
    let fingerprint = fingerprint(&document(&row));
    Fingerprinted {
        checked: row,
        fingerprint,
    }
}

/// Keep rows whose document is neither an exact nor a near duplicate of a
/// kept row; reject the rest with `exact_duplicate` / `near_duplicate`.
pub fn run(candidates: Vec<Fingerprinted>, rejected: &mut Vec<RejectedRow>) -> Vec<Fingerprinted> {
    let mut kept: Vec<Fingerprinted> = Vec::new();
    for row in candidates {
        if let Some(other) = kept
            .iter()
            .find(|k| k.fingerprint.document == row.fingerprint.document)
        {
            rejected.push(row.checked.rejected(
                Stage::Dedup,
                "exact_duplicate",
                format!("same normalized text as row {:?}", other.checked.row.row_id),
            ));
            continue;
        }
        if let Some((other, similarity)) = kept
            .iter()
            .map(|k| {
                (
                    k,
                    jaccard(&k.fingerprint.shingles, &row.fingerprint.shingles),
                )
            })
            .filter(|(_, similarity)| *similarity >= NEAR_DUPLICATE_THRESHOLD)
            .max_by(|a, b| a.1.total_cmp(&b.1))
        {
            rejected.push(row.checked.rejected(
                Stage::Dedup,
                "near_duplicate",
                format!(
                    "{similarity:.3} similar to row {:?} (threshold {NEAR_DUPLICATE_THRESHOLD})",
                    other.checked.row.row_id
                ),
            ));
            continue;
        }
        kept.push(row);
    }
    kept
}

/// The text a row is compared on: problem text plus every non-system turn.
/// The problem statement usually also appears inside the first student
/// turn; stating it explicitly keeps the comparison honest when the row
/// embeds it differently.
pub fn document(row: &CheckedRow) -> String {
    let mut text = match &row.problem {
        Some(problem) => problem.statement.clone(),
        None => row.row.problem_id.clone(),
    };
    for turn in &row.row.turns {
        if turn.role == tutor_schema::Role::System {
            continue;
        }
        text.push('\n');
        text.push_str(&turn.content);
    }
    text
}

/// Normalize `text` for comparison: lowercase ASCII and collapse every run
/// of non-ASCII-alphanumeric characters into one ASCII space. Dropping math
/// operators (`d/dx sin(x^2)` → `d dx sin x 2`) is intentional: near
/// duplicates are about the same tutoring trajectory, not byte-identical
/// rendering.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut separator = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if separator && !out.is_empty() {
                out.push(' ');
            }
            out.push(ch.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    out
}

/// The fingerprint of already-normalized or raw `text`.
pub fn fingerprint(text: &str) -> Fingerprint {
    let document = normalize(text);
    let shingles = shingles(&document);
    Fingerprint { document, shingles }
}

/// Word [`SHINGLE_WORDS`]-shingle set of a normalized document. Documents
/// shorter than one shingle contribute their whole text as a single
/// shingle, so tiny records still compare.
pub fn shingles(document: &str) -> BTreeSet<String> {
    let words: Vec<&str> = document.split(' ').filter(|w| !w.is_empty()).collect();
    if words.len() <= SHINGLE_WORDS {
        return [document.to_owned()].into_iter().collect();
    }
    words
        .windows(SHINGLE_WORDS)
        .map(|window| window.join(" "))
        .collect()
}

/// Jaccard similarity of two shingle sets. Two empty sets are identical
/// (similarity 1).
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 1.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_punctuation_and_case() {
        assert_eq!(
            normalize("Let f(x) = 3x^2. Compute f'(2)."),
            "let f x 3x 2 compute f 2"
        );
        assert_eq!(normalize("  A\n\nB  "), "a b");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn shingles_of_short_documents() {
        assert_eq!(
            shingles("one two"),
            ["one two".to_owned()].into_iter().collect::<BTreeSet<_>>()
        );
        let set = shingles("a b c d");
        assert!(set.contains("a b c") && set.contains("b c d"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn jaccard_scores() {
        let a = shingles("the cat sat on the mat today");
        let b = shingles("the cat sat on the mat today");
        assert_eq!(jaccard(&a, &b), 1.0);
        let c = shingles("completely different words here entirely");
        assert_eq!(jaccard(&a, &c), 0.0);
    }
}
