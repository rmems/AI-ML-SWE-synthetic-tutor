//! `tutor-validate` — check tutoring dialogue rows against
//! `schemas/dialogue.schema.json` and the policy constraints of
//! [`tutor_schema::DialogueRow`].
//!
//! Each file is validated in two passes: the generated JSON Schema (shape and
//! field-level types), then the row's semantic rules (turn structure, hint
//! ladder, single primary misconception, provenance, no early final answer).
//! Exit code is 0 only when every file passes.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tutor_schema::DialogueRow;

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
    for file in &cli.files {
        match validate_file(&validator, file) {
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

fn validate_file(validator: &jsonschema::Validator, file: &PathBuf) -> Result<(), Vec<String>> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
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
    match DialogueRow::from_value(value) {
        Ok(_) => Ok(()),
        Err(error) => Err(vec![format!("{} ({})", error, error.code())]),
    }
}
