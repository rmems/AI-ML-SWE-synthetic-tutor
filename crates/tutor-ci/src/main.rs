//! CLI for the PR-time data-validation gates.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tutor_ci::{lint_taxonomy, rights_gate, verification_smoke};

/// Run data-validation checks that are not already exposed by `tutor-validate`.
#[derive(Parser)]
#[command(name = "tutor-ci", version, about)]
struct Cli {
    /// Check to run.
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check that every misconception id referenced in data exists in the taxonomy.
    TaxonomyLint {
        /// Path to schemas/taxonomy.yaml.
        #[arg(long, default_value = "schemas/taxonomy.yaml")]
        taxonomy: PathBuf,
        /// JSON or JSONL data/fixture files to inspect.
        #[arg(value_name = "DATA.json")]
        files: Vec<PathBuf>,
    },
    /// Fail closed unless every cited teacher has an allowed rights-registry entry.
    RightsGate {
        /// Path to docs/rights.md.
        #[arg(long, default_value = "docs/rights.md")]
        registry: PathBuf,
        /// JSON or JSONL data/fixture files to inspect.
        #[arg(value_name = "DATA.json")]
        files: Vec<PathBuf>,
    },
    /// Verify a known-good problem fixture and prove a wrong answer is rejected.
    VerifySmoke {
        /// Path to the unverified known-good problem fixture.
        #[arg(
            long,
            default_value = "crates/tutor-schema/tests/fixtures/problem.json"
        )]
        problem: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::TaxonomyLint { taxonomy, files } => match lint_taxonomy(&taxonomy, &files) {
            Ok(report) => {
                println!(
                    "taxonomy lint: ok ({} records, {} references)",
                    report.records, report.references
                );
                ExitCode::SUCCESS
            }
            Err(errors) => fail("taxonomy lint", &errors),
        },
        Command::RightsGate { registry, files } => match rights_gate(&registry, &files) {
            Ok(report) => {
                println!(
                    "rights gate: ok ({} records, {} teacher citations)",
                    report.records, report.citations
                );
                ExitCode::SUCCESS
            }
            Err(errors) => fail("rights gate", &errors),
        },
        Command::VerifySmoke { problem } => match verification_smoke(&problem) {
            Ok(report) => {
                println!(
                    "verification smoke: ok ({} passed; wrong answer rejected with {})",
                    report.problem_id, report.rejection_code
                );
                ExitCode::SUCCESS
            }
            Err(errors) => fail("verification smoke", &errors),
        },
    }
}

fn fail(check: &str, errors: &[String]) -> ExitCode {
    eprintln!("{check}: FAILED");
    for error in errors {
        eprintln!("  - {error}");
    }
    ExitCode::FAILURE
}
