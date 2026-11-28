//! Hermes-friendly local command. No live provider execution is implemented.
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tutor_teachers::{
    Error, TeacherConfig, mock_smoke, prepare_request, replay_response, synthetic_response,
};

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
    /// Emit an unsent, versioned fixture request for a later Hermes-owned call.
    PrepareRequest {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Emit a synthetic response without contacting the configured teacher.
    MockResponse {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Verify and validate a saved response; exit 2 means admission remains blocked.
    ReplayResponse {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        response: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
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

fn run() -> Result<ExitCode, Error> {
    match Cli::parse().command {
        Command::PrepareRequest { config, out } => {
            let config = config
                .map(|path| read_config(&path))
                .transpose()?
                .unwrap_or_default();
            prepare_request(&config, &out)?;
            println!(
                "Unsent request prepared. Provider contact requires rights evidence and explicit approval."
            );
        }
        Command::MockResponse { request, out } => synthetic_response(&request, &out)?,
        Command::ReplayResponse {
            request,
            response,
            out,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&replay_response(&request, &response, &out)?)?
            );
            return Ok(ExitCode::from(2));
        }
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
                "Configuration valid (offline mock or pinned DeepSeek through OpenRouter). No endpoint contacted or credential read. Live generation, training and export remain BLOCKED pending RM-2500 exact model/route/vendor evidence, approved generation spec and explicit run approval. Hermes file handoff is available; actual provider routing remains unverified."
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("tutor-teachers: {error}");
            ExitCode::FAILURE
        }
    }
}
