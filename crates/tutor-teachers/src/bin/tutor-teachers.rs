//! Hermes-friendly local command. No live provider execution is implemented.
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tutor_teachers::{Error, TeacherConfig, mock_smoke};

#[derive(Parser)]
#[command(
    version,
    about = "Offline teacher readiness; never calls a live provider"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate and validate two mock math candidates; never training data.
    MockSmoke {
        /// Optional non-secret JSON configuration; defaults to built-in mock.
        #[arg(long)]
        config: Option<PathBuf>,
        /// New output directory; its parent must exist. Existing paths are refused.
        #[arg(long)]
        out: PathBuf,
    },
    /// Validate future provider settings locally; this does not authorize calls.
    CheckConfig {
        #[arg(long)]
        config: PathBuf,
    },
}

fn read_config(path: &std::path::Path) -> Result<TeacherConfig, Error> {
    TeacherConfig::from_json(&std::fs::read_to_string(path)?)
}

fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Command::MockSmoke { config, out } => {
            let config = config
                .map(|path| read_config(&path))
                .transpose()?
                .unwrap_or_default();
            println!(
                "{}",
                serde_json::to_string_pretty(&mock_smoke(&config, &out)?)?
            );
        }
        Command::CheckConfig { config } => {
            read_config(&config)?;
            println!(
                "Configuration shape valid. No endpoint contacted or credential read. Live generation, training and export remain BLOCKED pending RM-2500 exact model/route/vendor evidence, approved generation spec and explicit run approval. Live adapter not implemented."
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tutor-teachers: {error}");
            ExitCode::FAILURE
        }
    }
}
