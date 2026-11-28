//! `tutor-runlog` — verify and extend the append-only generation run log.
//!
//! The pipeline appends run entries programmatically; this CLI is for the two
//! operations a human or CI gate performs directly: checking a log's hash
//! chain (`verify`) and recording a quality-gate pause (`pause`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use tutor_runlog::{Gate, GatePauseRecord, Record, RunLog};

#[derive(Parser)]
#[command(version, about = "Verify and append to the generation run log")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify a run log's append-only hash chain, references, and format.
    Verify {
        /// Path to the run-log.jsonl file.
        log: PathBuf,
    },
    /// Record a quality-gate failure that pauses a family's cadence.
    ///
    /// The pause is appended like any other entry; the family resumes when a
    /// fix lands as a new run (whose `replaces` names the failed run).
    Pause(PauseArgs),
}

#[derive(Args)]
struct PauseArgs {
    /// Path to the run-log.jsonl file.
    log: PathBuf,
    /// Dataset family whose cadence is paused (the datasets/<family>/ name).
    #[arg(long)]
    family: String,
    /// Gate that failed: generation, verification, qa_sample,
    /// eval_spot_check, or export.
    #[arg(long)]
    gate: String,
    /// Human-readable failure detail.
    #[arg(long)]
    detail: String,
    /// Run id whose failure triggered the pause; must name an earlier run
    /// in this log when given.
    #[arg(long)]
    run_id: Option<String>,
    /// Evidence as key=value pairs (value parsed as JSON when it is valid
    /// JSON, else kept as a string); repeatable.
    #[arg(long = "evidence", value_name = "KEY=VALUE")]
    evidence: Vec<String>,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Verify { log } => match RunLog::at(&log).entries_strict() {
            Ok(entries) => {
                println!("run log {}: ok ({} entries)", log.display(), entries.len());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("run log {}: FAILED: {error}", log.display());
                ExitCode::FAILURE
            }
        },
        Command::Pause(args) => {
            let log = args.log.clone();
            match pause(&args) {
                Ok(()) => {
                    println!(
                        "paused {} at gate {} -> {}",
                        args.family,
                        args.gate,
                        log.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("run log {}: FAILED: {error}", log.display());
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn pause(args: &PauseArgs) -> Result<(), Box<dyn std::error::Error>> {
    let gate: Gate = serde_json::from_str(&serde_json::to_string(&args.gate)?)
        .map_err(|_| format!("unknown gate {:?}; expected generation, verification, qa_sample, eval_spot_check, or export", args.gate))?;
    let mut map = BTreeMap::new();
    for pair in &args.evidence {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("evidence {pair:?} is not key=value"))?;
        if key.is_empty() {
            return Err(format!("evidence {pair:?} has an empty key").into());
        }
        map.insert(
            key.to_owned(),
            serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.to_owned())),
        );
    }
    RunLog::at(&args.log).append(Record::GatePause(GatePauseRecord {
        family: args.family.clone(),
        gate,
        run_id: args.run_id.clone(),
        detail: args.detail.clone(),
        evidence: map,
    }))?;
    Ok(())
}
