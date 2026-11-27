//! `tutor-validate` — check tutoring dialogue rows against
//! `schemas/dialogue.schema.json` and the policy constraints of
//! [`tutor_schema::DialogueRow`].
//!
//! Each file is validated in two passes: the generated JSON Schema (shape and
//! field-level types), then the row's semantic rules (turn structure, hint
//! ladder, single primary misconception, provenance, no early final answer).
//! `.json` files contain one row; `.jsonl` files contain one row per
//! non-empty line. Exit code is 0 only when every row passes.

use std::collections::HashSet;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use tutor_schema::DialogueRow;

/// Largest row file the CLI reads: bound memory use on bad input.
const MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;

/// Validate tutoring dialogue rows.
#[derive(Parser)]
#[command(name = "tutor-validate", version, about)]
struct Cli {
    /// Dialogue row JSON files to validate.
    #[arg(required = true, value_name = "ROW.json[l]")]
    files: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let schema = tutor_schema::dialogue_json_schema();
    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            eprintln!("generated dialogue schema does not compile: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut failures = 0u32;
    let mut row_ids = HashSet::new();
    for file in &cli.files {
        match validate_file(&validator, file, &mut row_ids) {
            Ok(()) => println!("{}: valid", file.display()),
            Err(reasons) => {
                failures += 1;
                eprintln!("{}: INVALID", file.display());
                for reason in reasons {
                    eprintln!("  - {reason}");
                }
            }
        }
    }
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!("{failures} file(s) failed validation");
        ExitCode::FAILURE
    }
}

fn validate_file(
    validator: &jsonschema::Validator,
    file: &PathBuf,
    row_ids: &mut HashSet<(String, String)>,
) -> Result<(), Vec<String>> {
    if is_jsonl(file) {
        return validate_jsonl_file(validator, file, row_ids);
    }
    // Bounded read: metadata reports zero bytes for pipes, so cap the stream
    // itself instead of checking the file size first.
    let text = match std::fs::File::open(file) {
        Ok(handle) => {
            let mut text = String::new();
            match std::io::Read::take(handle, MAX_ROW_BYTES + 1).read_to_string(&mut text) {
                Ok(_) if text.len() as u64 > MAX_ROW_BYTES => {
                    return Err(vec![format!(
                        "file exceeds {MAX_ROW_BYTES} bytes; rows are single dialogue records"
                    )]);
                }
                Ok(_) => text,
                Err(error) => return Err(vec![format!("cannot read file: {error}")]),
            }
        }
        Err(error) => return Err(vec![format!("cannot read file: {error}")]),
    };
    let key = validate_row_text(validator, &text)?;
    insert_row_key(key, row_ids)
}

fn validate_jsonl_file(
    validator: &jsonschema::Validator,
    file: &PathBuf,
    row_ids: &mut HashSet<(String, String)>,
) -> Result<(), Vec<String>> {
    let handle = match std::fs::File::open(file) {
        Ok(handle) => handle,
        Err(error) => return Err(vec![format!("cannot read file: {error}")]),
    };
    let reader = std::io::BufReader::new(handle);
    let mut state = JsonlValidationState {
        row_ids,
        file_row_ids: HashSet::new(),
        reasons: Vec::new(),
    };
    for (index, line) in reader.lines().enumerate() {
        if !validate_jsonl_line(validator, line, index + 1, &mut state) {
            break;
        }
    }
    if state.reasons.is_empty() {
        Ok(())
    } else {
        Err(state.reasons)
    }
}

struct JsonlValidationState<'a> {
    row_ids: &'a mut HashSet<(String, String)>,
    file_row_ids: HashSet<String>,
    reasons: Vec<String>,
}

fn validate_jsonl_line(
    validator: &jsonschema::Validator,
    line: std::io::Result<String>,
    line_number: usize,
    state: &mut JsonlValidationState<'_>,
) -> bool {
    let line = match line {
        Ok(line) => line,
        Err(error) => {
            state
                .reasons
                .push(format!("line {line_number}: cannot read line: {error}"));
            return false;
        }
    };
    if line.trim().is_empty() {
        return true;
    }
    if line.len() as u64 > MAX_ROW_BYTES {
        state.reasons.push(format!(
            "line {line_number}: row exceeds {MAX_ROW_BYTES} bytes"
        ));
        return true;
    }
    let result = validate_jsonl_row(validator, &line, state);
    if let Err(row_reasons) = result {
        state
            .reasons
            .extend(prefixed_line_errors(row_reasons, line_number));
    }
    true
}

fn prefixed_line_errors(errors: Vec<String>, line_number: usize) -> Vec<String> {
    errors
        .into_iter()
        .map(|reason| format!("line {line_number}: {reason}"))
        .collect()
}

fn is_jsonl(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

fn validate_jsonl_row(
    validator: &jsonschema::Validator,
    line: &str,
    state: &mut JsonlValidationState<'_>,
) -> Result<(), Vec<String>> {
    let key = validate_row_text(validator, line)?;
    if !state.file_row_ids.insert(key.0.clone()) {
        return Err(vec![format!(
            "duplicate row_id {:?} within this JSONL file",
            key.0
        )]);
    }
    insert_row_key(key, state.row_ids)
}

fn validate_row_text(
    validator: &jsonschema::Validator,
    text: &str,
) -> Result<(String, String), Vec<String>> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => return Err(vec![format!("not JSON: {error}")]),
    };
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|error| format!("json schema: {error}"))
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    // Parse the raw text, not the value: `serde_json::Value` silently keeps
    // only the last of duplicate fields.
    match DialogueRow::from_json(text) {
        Ok(row) => Ok((row.row_id, row.problem_id)),
        Err(error) => Err(vec![format!("{} ({})", error, error.code())]),
    }
}

fn insert_row_key(
    key: (String, String),
    row_ids: &mut HashSet<(String, String)>,
) -> Result<(), Vec<String>> {
    if row_ids.insert(key.clone()) {
        Ok(())
    } else {
        Err(vec![format!("duplicate row_id {:?}", key.0)])
    }
}
