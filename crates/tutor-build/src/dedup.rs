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

/// Math operators kept as their own tokens by [`normalize`]: dropping them
/// would make `x^4 - 3x^2` and `x^4 + 3x^2` (different problems with
/// different answers) the same text.
const MATH_OPERATORS: &[char] = &['+', '-', '*', '/', '^', '=', '<', '>', '\''];

/// Normalize `text` for comparison: lowercase ASCII, fold equivalent
/// Unicode operator glyphs to ASCII (`−` → `-`, `′` → `'`, `×` → `*`, …),
/// keep every math operator (`+ - * / ^ = < >` and `'`) as its own token,
/// and collapse every other run of non-ASCII-alphanumeric characters into
/// one ASCII space (`f′(x) = x^4 − 3x^2.` → `f ' x = x ^ 4 - 3x ^ 2`).
/// Brackets, spacing and prose punctuation are rendering; operators and
/// signs change the math.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars().map(fold_glyph) {
        let Some(operator) = token_kind(ch) else {
            pending_space = true;
            continue;
        };
        if pending_space || operator {
            push_separator(&mut out);
        }
        out.push(ch.to_ascii_lowercase());
        pending_space = operator;
    }
    out
}

/// `Some(is_operator)` for a character [`normalize`] keeps, `None` for a
/// separator.
fn token_kind(ch: char) -> Option<bool> {
    if MATH_OPERATORS.contains(&ch) {
        Some(true)
    } else {
        ch.is_ascii_alphanumeric().then_some(false)
    }
}

/// One space between tokens, never a leading one.
fn push_separator(out: &mut String) {
    if !out.is_empty() {
        out.push(' ');
    }
}

/// The ASCII operator a Unicode math glyph means, else `ch` unchanged.
fn fold_glyph(ch: char) -> char {
    match ch {
        '\u{2212}' | '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}'
        | '\u{FE63}' | '\u{FF0D}' => '-',
        '\u{FF0B}' => '+',
        '\u{00D7}' | '\u{22C5}' | '\u{00B7}' | '\u{2219}' | '\u{2217}' => '*',
        '\u{00F7}' | '\u{2215}' | '\u{2044}' => '/',
        '\u{2032}' | '\u{2019}' | '\u{02B9}' => '\'',
        '\u{FF1D}' => '=',
        _ => ch,
    }
}

/// The math content of a normalized document: its operator and
/// digit-bearing tokens, in order. Two statements whose signatures differ
/// pose different problems however similar their prose is.
pub fn math_signature(document: &str) -> Vec<&str> {
    document
        .split(' ')
        .filter(|token| {
            token
                .chars()
                .any(|ch| ch.is_ascii_digit() || MATH_OPERATORS.contains(&ch))
        })
        .collect()
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
            "let f x = 3x ^ 2 compute f ' 2"
        );
        assert_ne!(
            normalize("x^4 - 3x^2 at 2"),
            normalize("x^4 + 3x^2 at 2"),
            "a sign change is a different problem"
        );
        assert_eq!(normalize("a--b"), "a - - b");
        assert_eq!(
            normalize("Let f(x) = x^4 \u{2212} 3x^2. Compute f\u{2032}(2)."),
            normalize("Let f(x) = x^4 - 3x^2. Compute f'(2).")
        );
        assert_eq!(
            math_signature(&normalize("Let f(x) = 3x^2 - 2x + 5. Compute f'(2).")),
            ["=", "3x", "^", "2", "-", "2x", "+", "5", "'", "2"]
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
