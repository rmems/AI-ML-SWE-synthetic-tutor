//! `tutor-generate` — generate seeded problems, verify every answer, emit
//! `verified/problems.jsonl`, `rejected/rejections.jsonl`, and `manifest.json`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tutor_generate::student::{AttemptSpec, simulate};
use tutor_generate::{GENERATORS, RunConfig, run, spec};
use tutor_schema::{MisconceptionId, Persistence, Problem, StudentLevel, SurfaceStyle};
use tutor_wire::DEFAULT_ENDPOINT;

use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;

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
    /// Simulate student attempts from verified/problems.jsonl.
    Simulate {
        /// JSONL file of verified problems.
        #[arg(long)]
        problems: PathBuf,
        /// Output directory for attempts.jsonl, rejections.jsonl, and manifest.json.
        #[arg(long)]
        out: PathBuf,
        /// First attempt seed.
        #[arg(long, default_value_t = 0)]
        seed_start: u64,
        /// Attempts drawn per problem.
        #[arg(long, default_value_t = 1)]
        attempts_per_problem: u64,
        /// correction, confirmation, or the seeded mixed profile-dependent mode.
        #[arg(long, default_value = "mixed")]
        mode: String,
        /// Force one taxonomy ID for correction attempts.
        #[arg(long)]
        misconception: Option<String>,
        /// Force student level: struggling, typical, or strong.
        #[arg(long)]
        level: Option<String>,
        /// Force persistence: corrects_after_l1, corrects_after_l2, or needs_l3.
        #[arg(long)]
        persistence: Option<String>,
        /// Force surface style: terse or verbose.
        #[arg(long)]
        surface_style: Option<String>,
        /// Request teacher surface variety. Currently fails closed because no teacher lane is wired.
        #[arg(long)]
        teacher_variety: bool,
        /// Path to student-attempt.schema.json.
        #[arg(long, default_value = "schemas/student-attempt.schema.json")]
        schema: PathBuf,
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
        Command::Simulate {
            problems,
            out,
            seed_start,
            attempts_per_problem,
            mode,
            misconception,
            level,
            persistence,
            surface_style,
            teacher_variety,
            schema,
        } => run_simulation(SimulateArgs {
            problems,
            out,
            seed_start,
            attempts_per_problem,
            mode,
            misconception,
            level,
            persistence,
            surface_style,
            teacher_variety,
            schema,
        }),
    }
}

struct SimulateArgs {
    problems: PathBuf,
    out: PathBuf,
    seed_start: u64,
    attempts_per_problem: u64,
    mode: String,
    misconception: Option<String>,
    level: Option<String>,
    persistence: Option<String>,
    surface_style: Option<String>,
    teacher_variety: bool,
    schema: PathBuf,
}

fn run_simulation(args: SimulateArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.teacher_variety {
        return Err("teacher surface variety is flag-gated but no allowed teacher lane is wired; rerun without --teacher-variety for procedural output".into());
    }
    let correct = match args.mode.as_str() {
        "mixed" => None,
        "correction" => Some(false),
        "confirmation" => Some(true),
        other => {
            return Err(format!(
                "unknown mode {other:?}; expected mixed, correction, or confirmation"
            )
            .into());
        }
    };
    let misconception = args
        .misconception
        .as_deref()
        .map(|value| parse_snake_enum::<MisconceptionId>(value, "misconception"))
        .transpose()?;
    let level = args
        .level
        .as_deref()
        .map(|value| parse_snake_enum::<StudentLevel>(value, "level"))
        .transpose()?;
    let persistence = args
        .persistence
        .as_deref()
        .map(|value| parse_snake_enum::<Persistence>(value, "persistence"))
        .transpose()?;
    let surface_style = args
        .surface_style
        .as_deref()
        .map(|value| parse_snake_enum::<SurfaceStyle>(value, "surface style"))
        .transpose()?;

    let schema_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args.schema)?)?;
    let validator = jsonschema::validator_for(&schema_json)
        .map_err(|error| format!("student-attempt schema does not compile: {error}"))?;
    let text = std::fs::read_to_string(&args.problems)?;
    let mut attempts = Vec::new();
    let mut rejections = Vec::new();
    let mut next_seed = args.seed_start;
    for (line_number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let problem = match Problem::from_json(line) {
            Ok(problem) => problem,
            Err(error) => {
                rejections.push(serde_json::json!({
                    "line": line_number + 1,
                    "stage": "parse_problem",
                    "detail": error.to_string(),
                }));
                continue;
            }
        };
        for _ in 0..args.attempts_per_problem {
            let seed = next_seed;
            next_seed = next_seed.checked_add(1).ok_or("attempt seed overflow")?;
            let spec = AttemptSpec {
                seed,
                correct,
                misconception,
                level,
                persistence,
                surface_style,
                teacher_variety: false,
            };
            match simulate(&problem, &spec) {
                Ok(attempt) => {
                    let value = serde_json::to_value(&attempt)?;
                    if validator.is_valid(&value) {
                        attempts.push(value);
                    } else {
                        rejections.push(serde_json::json!({
                            "problem_id": problem.problem_id,
                            "seed": seed,
                            "stage": "schema",
                            "detail": validator.iter_errors(&value).map(|error| error.to_string()).take(5).collect::<Vec<_>>().join("; "),
                        }));
                    }
                }
                Err(error) => rejections.push(serde_json::json!({
                    "problem_id": problem.problem_id,
                    "seed": seed,
                    "stage": "simulate",
                    "detail": error.to_string(),
                })),
            }
        }
    }

    std::fs::create_dir_all(&args.out)?;
    write_jsonl(&args.out.join("attempts.jsonl"), attempts.iter())?;
    write_jsonl(&args.out.join("rejections.jsonl"), rejections.iter())?;
    let correct_count = attempts.iter().filter(|row| row["correct"] == true).count();
    let noise_count = attempts
        .iter()
        .filter(|row| {
            row["profile"]["notation_noise"]
                .as_array()
                .is_some_and(|noise| !noise.is_empty())
        })
        .count();
    let manifest = serde_json::json!({
        "simulator": "synthetic-student-simulator",
        "simulator_version": 1,
        "schema": args.schema.display().to_string(),
        "problems": args.problems.display().to_string(),
        "seed_start": args.seed_start,
        "attempts_per_problem": args.attempts_per_problem,
        "mode": args.mode,
        "emitted": attempts.len(),
        "correct": correct_count,
        "correction": attempts.len() - correct_count,
        "notation_noise": noise_count,
        "rejected": rejections.len(),
    });
    std::fs::write(
        args.out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    println!(
        "student attempts: {} emitted ({} correct, {} correction), {} rejected -> {}",
        attempts.len(),
        correct_count,
        attempts.len() - correct_count,
        rejections.len(),
        args.out.display()
    );
    if attempts.is_empty() {
        return Err("no student attempt emitted".into());
    }
    Ok(())
}

fn parse_snake_enum<T>(value: &str, field: &str) -> Result<T, Box<dyn std::error::Error>>
where
    T: DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).expect("string serializes"))
        .map_err(|error| format!("invalid {field} {value:?}: {error}").into())
}

fn write_jsonl<'a>(
    path: &Path,
    rows: impl Iterator<Item = &'a serde_json::Value>,
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    for row in rows {
        writeln!(
            file,
            "{}",
            serde_json::to_string(row).map_err(std::io::Error::other)?
        )?;
    }
    Ok(())
}
