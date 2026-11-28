//! `tutor-build` — one command from verified dialogue rows to a split,
//! exported dataset release (issue #8).
//!
//! ```sh
//! tutor-build build --family derivatives-pilot --release v1 \
//!   --rows dialogue-rows.jsonl --problems verified/problems.jsonl \
//!   --out datasets/derivatives-pilot
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tutor_build::{BuildConfig, BuildReport, build};

/// Assemble, split, and export a tutoring dataset release.
#[derive(Parser)]
#[command(name = "tutor-build", version, about)]
struct Cli {
    /// What to do.
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check, dedup, decontaminate, split, and export a family release.
    Build {
        /// Dataset family name; the default output is `datasets/<family>/`.
        #[arg(long)]
        family: String,
        /// Release identifier recorded in the manifest (e.g. v1). Split
        /// assignments are frozen per release.
        #[arg(long)]
        release: String,
        /// Dialogue row files: `.json` (one row) or `.jsonl` (one per line).
        #[arg(long, required = true, num_args = 1.., value_name = "ROWS.json[l]")]
        rows: Vec<PathBuf>,
        /// Verified problems JSONL grounding the rows. Required when any
        /// `generated` row is among the inputs.
        #[arg(long)]
        problems: Option<PathBuf>,
        /// Held-out eval directory for decontamination (input only).
        #[arg(long, default_value = "evals/heldout")]
        heldout: PathBuf,
        /// Output directory (default `datasets/<family>/`, which is
        /// gitignored).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Manifest from an earlier release of this family; its assignments
        /// freeze rows and problems that were already split.
        #[arg(long)]
        previous_manifest: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let Cli {
        command:
            Command::Build {
                family,
                release,
                rows,
                problems,
                heldout,
                out,
                previous_manifest,
            },
    } = Cli::parse();
    if let Err(reason) = valid_family(&family) {
        eprintln!("invalid --family {family:?}: {reason}");
        return ExitCode::FAILURE;
    }
    let out_dir = out.unwrap_or_else(|| PathBuf::from("datasets").join(&family));
    let config = BuildConfig {
        family,
        release,
        rows,
        problems,
        heldout_dir: heldout,
        out_dir,
        previous_manifest,
    };
    match build(&config) {
        Ok(report) => {
            print_report(&report);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("tutor-build: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Family names become directory and (later, #21) Hugging Face repo names:
/// `[a-z0-9][a-z0-9_-]*`.
fn valid_family(family: &str) -> Result<(), String> {
    let mut chars = family.chars();
    let Some(first) = chars.next() else {
        return Err("must not be empty".to_owned());
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err("must start with [a-z0-9]".to_owned());
    }
    if !chars.all(valid_family_char) {
        return Err("only [a-z0-9_-] characters are allowed".to_owned());
    }
    Ok(())
}

/// One allowed `family` character after the first: `[a-z0-9_-]`.
fn valid_family_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
}

fn print_report(report: &BuildReport) {
    println!(
        "{}: {} accepted, {} rejected",
        report.out_dir.display(),
        report.accepted,
        report.rejected
    );
    for (split, count) in &report.split_counts {
        println!("  {split}: {count}");
    }
    if report.accepted == 0 {
        eprintln!("warning: no rows survived the pipeline; split files are empty");
    }
    println!("manifest: {}", report.manifest_path.display());
    println!("manifest sha256: {}", report.manifest_sha256);
}
