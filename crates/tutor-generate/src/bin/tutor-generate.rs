//! `tutor-generate` — generate seeded problems, verify every answer, emit
//! `verified/problems.jsonl`, `rejected/rejections.jsonl`, and `manifest.json`.

use atomicwrites::{AllowOverwrite, AtomicFile};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tutor_generate::student::{AttemptSpec, SIMULATOR_NAME, SIMULATOR_VERSION, simulate};
use tutor_generate::{GENERATORS, RunConfig, run, spec};
use tutor_schema::{MAX_SEED, MisconceptionId, Persistence, Problem, StudentLevel, SurfaceStyle};
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
    if args.seed_start > MAX_SEED {
        return Err(format!(
            "--seed-start {} exceeds the JSON-safe maximum {MAX_SEED}",
            args.seed_start
        )
        .into());
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
    let mut hasher = Sha256::new();
    hasher.update(seed_start.to_le_bytes());
    hasher.update(problem_id.as_bytes());
    hasher.update([0xff]);
    hasher.update(attempt_index.to_le_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("SHA-256 has eight bytes")) & MAX_SEED
}

struct SimulationBatch {
    attempts: Vec<serde_json::Value>,
    rejections: Vec<serde_json::Value>,
    seen_problem_ids: HashSet<String>,
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
        seen_problem_ids: HashSet::new(),
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
        if self.seen_problem_ids.contains(&problem.problem_id) {
            self.rejections.push(serde_json::json!({
                "problem_id": problem.problem_id,
                "stage": "duplicate_problem",
                "detail": "duplicate problem ID in simulation input",
            }));
            return Ok(());
        }
        let attempts_before = self.attempts.len();
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
        if self.attempts.len() > attempts_before {
            self.seen_problem_ids.insert(problem.problem_id.clone());
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
    if batch.attempts.is_empty() {
        for rejection in &batch.rejections {
            eprintln!("rejected: {rejection}");
        }
        return Err(
            "no student attempt emitted; existing simulation artifacts were left unchanged".into(),
        );
    }

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

    // Serialize the complete bundle before touching the output directory. The
    // data files are individually replaced atomically, and the manifest is the
    // commit record: it is written last and carries their hashes and sizes so a
    // consumer can detect an interrupted multi-file publish.
    let attempts_text = jsonl_text(batch.attempts.iter())?;
    let rejections_text = jsonl_text(batch.rejections.iter())?;
    let manifest = serde_json::json!({
        "simulator": SIMULATOR_NAME,
        "simulator_version": SIMULATOR_VERSION,
        "schema": args.schema.display().to_string(),
        "problems": args.problems.display().to_string(),
        "seed_start": args.seed_start,
        "attempts_per_problem": args.attempts_per_problem,
        "mode": args.mode,
        "misconception": args.misconception,
        "level": args.level,
        "persistence": args.persistence,
        "surface_style": args.surface_style,
        "emitted": batch.attempts.len(),
        "correct": correct_count,
        "correction": batch.attempts.len() - correct_count,
        "notation_noise": noise_count,
        "rejected": batch.rejections.len(),
        "artifacts": {
            "attempts.jsonl": artifact_metadata(&attempts_text),
            "rejections.jsonl": artifact_metadata(&rejections_text),
        },
    });
    let manifest_text = serde_json::to_string_pretty(&manifest)?;

    std::fs::create_dir_all(&args.out)?;
    ensure_simulation_output(&args.out)?;
    let _lock = OutputLock::acquire(&args.out)?;
    write_text_atomic(&args.out.join("attempts.jsonl"), &attempts_text)?;
    write_text_atomic(&args.out.join("rejections.jsonl"), &rejections_text)?;
    write_text_atomic(&args.out.join("manifest.json"), &manifest_text)?;
    println!(
        "student attempts: {} emitted ({} correct, {} correction), {} rejected -> {}",
        batch.attempts.len(),
        correct_count,
        batch.attempts.len() - correct_count,
        batch.rejections.len(),
        args.out.display()
    );
    Ok(())
}

fn jsonl_text<'a>(
    rows: impl Iterator<Item = &'a serde_json::Value>,
) -> Result<String, serde_json::Error> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&serde_json::to_string(row)?);
        text.push('\n');
    }
    Ok(text)
}

fn artifact_metadata(text: &str) -> serde_json::Value {
    let digest = Sha256::digest(text.as_bytes());
    serde_json::json!({
        "sha256": format!("{digest:x}"),
        "bytes": text.len(),
    })
}

struct OutputLock {
    path: PathBuf,
}

impl OutputLock {
    fn acquire(out: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = out.join(".simulate.lock");
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!(
                    "{} is already locked by another simulation; if that simulation stopped, remove {}",
                    out.display(),
                    path.display()
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        };
        writeln!(file, "pid {}", std::process::id())?;
        Ok(Self { path })
    }
}

impl Drop for OutputLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn parse_snake_enum<T>(value: &str, field: &str) -> Result<T, Box<dyn std::error::Error>>
where
    T: DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).expect("string serializes"))
        .map_err(|error| format!("invalid {field} {value:?}: {error}").into())
}

fn ensure_simulation_output(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    const OUTPUT_MARKER: &str = ".synthetic-student-simulator-output";
    validate_existing_manifest(out)?;
    let marker = out.join(OUTPUT_MARKER);
    if marker.exists() {
        return Ok(());
    }
    if has_unowned_artifacts(out) {
        return Err(format!(
            "{} contains artifact files without simulator provenance; choose a separate output directory",
            out.display()
        )
        .into());
    }
    std::fs::write(&marker, "synthetic-student-simulator output directory\n")?;
    Ok(())
}

fn validate_existing_manifest(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = out.join("manifest.json");
    if !manifest.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&manifest)?;
    let existing: serde_json::Value = serde_json::from_str(&text)?;
    if existing.get("simulator") == Some(&serde_json::json!(SIMULATOR_NAME)) {
        return Ok(());
    }
    Err(format!(
        "{} already contains a non-simulator manifest; choose a separate simulation output directory",
        manifest.display()
    )
    .into())
}

fn has_unowned_artifacts(out: &Path) -> bool {
    !out.join("manifest.json").exists()
        && ["attempts.jsonl", "rejections.jsonl"]
            .iter()
            .any(|artifact| out.join(artifact).exists())
}

fn write_text_atomic(path: &Path, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    AtomicFile::new_with_tmpdir(path, AllowOverwrite, parent)
        .write(|file| file.write_all(text.as_bytes()))
        .map_err(|error| {
            std::io::Error::other(format!(
                "atomic write to {} failed: {error:?}",
                path.display()
            ))
        })?;
    Ok(())
}
