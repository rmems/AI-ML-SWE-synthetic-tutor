//! `tutor-generate` — generate seeded problems, verify every answer, emit
//! `verified/problems.jsonl`, `rejected/rejections.jsonl`, and `manifest.json`.

use std::path::PathBuf;
use std::time::Duration;
use tutor_generate::{GENERATORS, RunConfig, run, spec};
use tutor_wire::DEFAULT_ENDPOINT;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Generate seeded math problems with verified answers")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List registered generator specs (backend, bounds, Rust/Julia split).
    List,
    /// Generate + verify problems into an output directory.
    Run {
        /// Generator names; omit to run all registered generators.
        #[arg(long = "generator")]
        generators: Vec<String>,
        /// First seed drawn (inclusive).
        #[arg(long, default_value_t = 0)]
        seed_start: u64,
        /// Seeds per generator.
        #[arg(long, default_value_t = 40)]
        count: u64,
        /// ZMQ endpoint of the math-julia worker
        /// (default: $TUTOR_MATH_ENDPOINT or the built-in endpoint).
        #[arg(long)]
        endpoint: Option<String>,
        /// Per-request timeout in seconds.
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
        /// Path to problem.schema.json.
        #[arg(long, default_value = "schemas/problem.schema.json")]
        schema: PathBuf,
        /// Output directory.
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::List => {
            for g in GENERATORS {
                let backend = match g.backend {
                    tutor_generate::Backend::Rust => "rust",
                    tutor_generate::Backend::Julia => "julia",
                };
                println!("{}\tv{}\t{}\t{}", g.name, g.version, backend, g.split);
            }
            Ok(())
        }
        Command::Run {
            generators,
            seed_start,
            count,
            endpoint,
            timeout_secs,
            schema,
            out,
        } => {
            let mut specs: Vec<_> = if generators.is_empty() {
                GENERATORS.iter().collect()
            } else {
                generators
                    .iter()
                    .map(|name| spec(name).ok_or_else(|| format!("unknown generator {name:?}")))
                    .collect::<Result<_, String>>()?
            };
            specs.sort_by_key(|s| s.name);
            specs.dedup_by_key(|s| s.name);
            let endpoint = endpoint.or_else(|| std::env::var("TUTOR_MATH_ENDPOINT").ok());
            let endpoint = endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
            let schema_json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&schema)?)?;
            let report = run(&RunConfig {
                generators: specs,
                seed_start,
                count,
                endpoint,
                timeout: Duration::from_secs(timeout_secs),
                out_dir: out.clone(),
                schema_json,
                schema: schema.display().to_string(),
            })?;
            for (name, counts) in &report.generators {
                println!(
                    "{name}: {} generated, {} verified, {} rejected",
                    counts.generated, counts.verified, counts.rejected
                );
            }
            println!(
                "total: {} verified, {} rejected -> {}",
                report.total_verified,
                report.total_rejected,
                out.display()
            );
            if report.total_verified == 0 {
                return Err("no problem verified".into());
            }
            Ok(())
        }
    }
}
