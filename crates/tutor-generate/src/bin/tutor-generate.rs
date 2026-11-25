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
        /// Base seed for stable per-problem attempt seeds.
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
    match Cli::parse().command {
        Command::List => list_generators(),
        Command::Run {
            generators,
            seed_start,
            count,
            endpoint,
            timeout_secs,
            schema,
            out,
        } => run_generation(GenerateArgs {
            generators,
            seed_start,
            count,
            endpoint,
            timeout_secs,
            schema,
            out,
        }),
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

fn list_generators() -> Result<(), Box<dyn std::error::Error>> {
    for generator in GENERATORS {
        let backend = match generator.backend {
            tutor_generate::Backend::Rust => "rust",
            tutor_generate::Backend::Julia => "julia",
        };
        println!(
            "{}\tv{}\t{}\t{}",
            generator.name, generator.version, backend, generator.split
        );
    }
    Ok(())
}

struct GenerateArgs {
    generators: Vec<String>,
    seed_start: u64,
    count: u64,
    endpoint: Option<String>,
    timeout_secs: u64,
    schema: PathBuf,
    out: PathBuf,
}

fn run_generation(args: GenerateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut specs: Vec<_> = if args.generators.is_empty() {
        GENERATORS.iter().collect()
    } else {
        args.generators
            .iter()
            .map(|name| spec(name).ok_or_else(|| format!("unknown generator {name:?}")))
            .collect::<Result<_, String>>()?
    };
    specs.sort_by_key(|spec| spec.name);
    specs.dedup_by_key(|spec| spec.name);

    let endpoint = args
        .endpoint
        .or_else(|| std::env::var("TUTOR_MATH_ENDPOINT").ok())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
    let schema_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args.schema)?)?;
    let report = run(&RunConfig {
        generators: specs,
        seed_start: args.seed_start,
        count: args.count,
        endpoint,
        timeout: Duration::from_secs(args.timeout_secs),
        out_dir: args.out.clone(),
        schema_json,
        schema: args.schema.display().to_string(),
    })?;
    print_generation_report(&report, &args.out);
    if report.total_verified == 0 {
        return Err("no problem verified".into());
    }
    Ok(())
}

fn print_generation_report(report: &tutor_generate::RunReport, out: &Path) {
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

#[derive(Clone, Copy)]
struct SimulationOptions {
    seed_start: u64,
    correct: Option<bool>,
    misconception: Option<MisconceptionId>,
    level: Option<StudentLevel>,
    persistence: Option<Persistence>,
    surface_style: Option<SurfaceStyle>,
}

fn run_simulation(args: SimulateArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.teacher_variety {
        return Err("teacher surface variety is flag-gated but no allowed teacher lane is wired; rerun without --teacher-variety for procedural output".into());
    }
    let options = simulation_options(&args)?;
    let schema_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args.schema)?)?;
    let validator = jsonschema::validator_for(&schema_json)
        .map_err(|error| format!("student-attempt schema does not compile: {error}"))?;
    let text = std::fs::read_to_string(&args.problems)?;
    let batch = simulate_batch(&text, &args, options, &validator)?;
    write_simulation_output(&args, batch)
}

fn simulation_options(
    args: &SimulateArgs,
) -> Result<SimulationOptions, Box<dyn std::error::Error>> {
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
    Ok(SimulationOptions {
        seed_start: args.seed_start,
        correct,
        misconception: args
            .misconception
            .as_deref()
            .map(|value| parse_snake_enum::<MisconceptionId>(value, "misconception"))
            .transpose()?,
        level: args
            .level
            .as_deref()
            .map(|value| parse_snake_enum::<StudentLevel>(value, "level"))
            .transpose()?,
        persistence: args
            .persistence
            .as_deref()
            .map(|value| parse_snake_enum::<Persistence>(value, "persistence"))
            .transpose()?,
        surface_style: args
            .surface_style
            .as_deref()
            .map(|value| parse_snake_enum::<SurfaceStyle>(value, "surface style"))
            .transpose()?,
    })
}

struct SimulationContext<'a> {
    attempts_per_problem: u64,
    options: SimulationOptions,
    validator: &'a jsonschema::Validator,
}

/// Derive a stable attempt seed from the caller's seed stream, problem ID, and
/// per-problem attempt index, so unrelated input lines do not reseed a problem.
fn stable_attempt_seed(seed_start: u64, problem_id: &str, attempt_index: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in seed_start
        .to_le_bytes()
        .into_iter()
        .chain(problem_id.as_bytes().iter().copied())
        .chain([0xff])
        .chain(attempt_index.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

struct SimulationBatch {
    attempts: Vec<serde_json::Value>,
    rejections: Vec<serde_json::Value>,
}

fn simulate_batch(
    text: &str,
    args: &SimulateArgs,
    options: SimulationOptions,
    validator: &jsonschema::Validator,
) -> Result<SimulationBatch, Box<dyn std::error::Error>> {
    let mut batch = SimulationBatch {
        attempts: Vec::new(),
        rejections: Vec::new(),
    };
    let context = SimulationContext {
        attempts_per_problem: args.attempts_per_problem,
        options,
        validator,
    };
    for (line_number, line) in text.lines().enumerate() {
        batch.add_problem_line(line_number, line, &context)?;
    }
    Ok(batch)
}

impl SimulationBatch {
    fn add_problem_line(
        &mut self,
        line_number: usize,
        line: &str,
        context: &SimulationContext<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let problem = match Problem::from_json(line) {
            Ok(problem) => problem,
            Err(error) => {
                self.rejections.push(serde_json::json!({
                    "line": line_number + 1,
                    "stage": "parse_problem",
                    "detail": error.to_string(),
                }));
                return Ok(());
            }
        };
        self.add_problem_attempts(&problem, context)
    }

    fn add_problem_attempts(
        &mut self,
        problem: &Problem,
        context: &SimulationContext<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for attempt_index in 0..context.attempts_per_problem {
            let seed = stable_attempt_seed(
                context.options.seed_start,
                &problem.problem_id,
                attempt_index,
            );
            let spec = AttemptSpec {
                seed,
                correct: context.options.correct,
                misconception: context.options.misconception,
                level: context.options.level,
                persistence: context.options.persistence,
                surface_style: context.options.surface_style,
                teacher_variety: false,
            };
            self.add_attempt(problem, spec, context.validator)?;
        }
        Ok(())
    }

    fn add_attempt(
        &mut self,
        problem: &Problem,
        spec: AttemptSpec,
        validator: &jsonschema::Validator,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let seed = spec.seed;
        let attempt = match simulate(problem, &spec) {
            Ok(attempt) => attempt,
            Err(error) => {
                self.rejections.push(serde_json::json!({
                    "problem_id": problem.problem_id,
                    "seed": seed,
                    "stage": "simulate",
                    "detail": error.to_string(),
                }));
                return Ok(());
            }
        };
        let value = serde_json::to_value(&attempt)?;
        if validator.is_valid(&value) {
            self.attempts.push(value);
        } else {
            let detail = validator
                .iter_errors(&value)
                .map(|error| error.to_string())
                .take(5)
                .collect::<Vec<_>>()
                .join("; ");
            self.rejections.push(serde_json::json!({
                "problem_id": problem.problem_id,
                "seed": seed,
                "stage": "schema",
                "detail": detail,
            }));
        }
        Ok(())
    }
}

fn write_simulation_output(
    args: &SimulateArgs,
    batch: SimulationBatch,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&args.out)?;
    stage_jsonl(&args.out.join("attempts.jsonl"), batch.attempts.iter())?;
    stage_jsonl(&args.out.join("rejections.jsonl"), batch.rejections.iter())?;

    let correct_count = batch
        .attempts
        .iter()
        .filter(|row| row["correct"] == true)
        .count();
    let noise_count = batch
        .attempts
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
        "emitted": batch.attempts.len(),
        "correct": correct_count,
        "correction": batch.attempts.len() - correct_count,
        "notation_noise": noise_count,
        "rejected": batch.rejections.len(),
    });
    write_text_atomic(
        &args.out.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest)?,
    )?;
    println!(
        "student attempts: {} emitted ({} correct, {} correction), {} rejected -> {}",
        batch.attempts.len(),
        correct_count,
        batch.attempts.len() - correct_count,
        batch.rejections.len(),
        args.out.display()
    );
    if batch.attempts.is_empty() {
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

fn stage_jsonl<'a>(
    path: &Path,
    rows: impl Iterator<Item = &'a serde_json::Value>,
) -> std::io::Result<()> {
    let tmp = staging_path(path, "jsonl");
    write_jsonl(&tmp, rows)?;
    publish(&tmp, path)
}

fn write_text_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = staging_path(path, "json");
    std::fs::write(&tmp, text)?;
    publish(&tmp, path)
}

fn staging_path(path: &Path, extension: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_extension(format!("{extension}.tmp.{}.{nonce:x}", std::process::id()))
}

fn publish(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    // rename does not replace an existing destination on Windows.
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(tmp, dest)
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
