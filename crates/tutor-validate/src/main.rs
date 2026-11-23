//! `tutor-validate` — check tutoring dialogue rows against
//! `schemas/dialogue.schema.json` and the policy constraints of
//! [`tutor_schema::DialogueRow`].
//!
//! Each file is validated in two passes: the generated JSON Schema (shape and
//! field-level types), then the row's semantic rules (turn structure, hint
//! ladder, single primary misconception, provenance, no early final answer).
//! Exit code is 0 only when every file passes.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
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
    #[arg(required = true, value_name = "ROW.json")]
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
    // Row IDs are only unique within a dataset family; rows carry no family
    // field, so key on the (row_id, problem_id) pair — a cross-family reuse
    // of an id always grounds a different problem.
    let mut row_ids: HashSet<(String, String)> = HashSet::new();
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
    let value: serde_json::Value = match serde_json::from_str(&text) {
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
    match DialogueRow::from_json(&text) {
        Ok(row) if row_ids.insert((row.row_id.clone(), row.problem_id.clone())) => Ok(()),
        Ok(row) => Err(vec![format!("duplicate row_id {:?}", row.row_id)]),
        Err(error) => Err(vec![format!("{} ({})", error, error.code())]),
    }
}
