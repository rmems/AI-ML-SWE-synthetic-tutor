//! `tutor-generate` — generate seeded problems, verify every answer, emit
//! `verified/problems.jsonl`, `rejected/rejections.jsonl`, and `manifest.json`.

use atomicwrites::{AllowOverwrite, AtomicFile};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tutor_generate::student::{AttemptSpec, SIMULATOR_NAME, SIMULATOR_VERSION, simulate};
use tutor_generate::{GENERATORS, RunConfig, run, spec};
use tutor_runlog::{Counts, GeneratorUse, OutputRef, Record, RunLog, RunRecord, SeedUse, Stage};
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
        /// Path of the append-only generation run log; defaults to
        /// `<out>/run-log.jsonl`.
        #[arg(long)]
        run_log: Option<PathBuf>,
        /// Dataset family this run feeds, recorded in the run log.
        #[arg(long)]
        family: Option<String>,
        /// Generation spec reference (issue ref or URL) recorded in the run log.
        #[arg(long)]
        spec: Option<String>,
        /// Run id of the earlier run this one replaces (a rerun marker);
        /// omit for a run that replaces nothing.
        #[arg(long)]
        replaces: Option<String>,
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
        /// Path of the append-only generation run log; defaults to
        /// `<out>/run-log.jsonl`.
        #[arg(long)]
        run_log: Option<PathBuf>,
        /// Dataset family this run feeds, recorded in the run log.
        #[arg(long)]
        family: Option<String>,
        /// Generation spec reference (issue ref or URL) recorded in the run log.
        #[arg(long)]
        spec: Option<String>,
        /// Run id of the earlier run this one replaces (a rerun marker);
        /// omit for a run that replaces nothing.
        #[arg(long)]
        replaces: Option<String>,
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
            run_log,
            family,
            spec,
            replaces,
        } => run_generation(GenerateArgs {
            generators,
            seed_start,
            count,
            endpoint,
            timeout_secs,
            schema,
            out,
            run_log,
            family,
            spec,
            replaces,
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
            run_log,
            family,
            spec,
            replaces,
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
            run_log,
            family,
            spec,
            replaces,
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
    run_log: Option<PathBuf>,
    family: Option<String>,
    spec: Option<String>,
    replaces: Option<String>,
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
        run_log: Some(tutor_generate::RunLogTarget {
            path: args
                .run_log
                .unwrap_or_else(|| args.out.join("run-log.jsonl")),
            family: args.family,
            spec: args.spec,
            replaces: args.replaces,
        }),
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
    run_log: Option<PathBuf>,
    family: Option<String>,
    spec: Option<String>,
    replaces: Option<String>,
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

    let counts = simulation_counts(&batch);
    // Calculate the complete bundle's digests before touching the output
    // directory. The data files are individually replaced atomically, and the
    // manifest is the commit record: it is written last and carries their
    // hashes and sizes so a consumer can detect an interrupted multi-file
    // publish.
    let bundle_artifacts = serde_json::json!({
        "attempts.jsonl": jsonl_metadata(batch.attempts.iter())?,
        "rejections.jsonl": jsonl_metadata(batch.rejections.iter())?,
    });
    let manifest_text = simulation_manifest_text(args, &batch, counts, &bundle_artifacts)?;
    let publication = PublicationBundle {
        batch: &batch,
        artifacts: &bundle_artifacts,
        manifest_text: &manifest_text,
    };

    std::fs::create_dir_all(&args.out)?;
    let _lock = OutputLock::acquire(&args.out)?;
    // Take the log's append lock and prove the run-log write can commit
    // before any artifact is replaced: the destination must not alias a
    // batch file, and the append — chain, `replaces`, record format — must
    // validate against the log as it stands. A bad --run-log or a mistyped
    // --replaces fails here, while the previous bundle is still intact. The
    // lock is held through the append, so no other writer can change the
    // log between this check and the entry.
    let log_path = run_log_path(args);
    tutor_runlog::ensure_log_path_is_clear(&log_path, &reserved_log_paths(args))?;
    let record = simulation_run_record(args, &batch, &manifest_text);
    let log = RunLog::at(&log_path);
    let locked = log.lock()?;
    locked.check(&record)?;

    let existing = ensure_simulation_output(&args.out)?;
    publish_bundle(&args.out, &publication, &existing)?;
    locked.append(record)?;
    println!(
        "student attempts: {} emitted ({} correct, {} correction), {} rejected -> {}",
        batch.attempts.len(),
        counts.correct,
        batch.attempts.len() - counts.correct,
        batch.rejections.len(),
        args.out.display()
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct SimulationCounts {
    correct: usize,
    notation_noise: usize,
}

struct PublicationBundle<'a> {
    batch: &'a SimulationBatch,
    artifacts: &'a serde_json::Value,
    manifest_text: &'a str,
}

fn simulation_counts(batch: &SimulationBatch) -> SimulationCounts {
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
    SimulationCounts {
        correct: correct_count,
        notation_noise: noise_count,
    }
}

fn simulation_manifest_text(
    args: &SimulateArgs,
    batch: &SimulationBatch,
    counts: SimulationCounts,
    bundle_artifacts: &serde_json::Value,
) -> Result<String, serde_json::Error> {
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
        "correct": counts.correct,
        "correction": batch.attempts.len() - counts.correct,
        "notation_noise": counts.notation_noise,
        "rejected": batch.rejections.len(),
        "artifacts": bundle_artifacts,
    });
    serde_json::to_string_pretty(&manifest)
}

fn publish_bundle(
    out: &Path,
    publication: &PublicationBundle<'_>,
    existing: &[(&'static str, serde_json::Value)],
) -> Result<(), Box<dyn std::error::Error>> {
    let pending_text = pending_publish_text(publication.artifacts, existing)?;
    write_text_atomic(&pending_publish_path(out), &pending_text)?;
    write_jsonl_atomic(
        &out.join("attempts.jsonl"),
        publication.batch.attempts.iter(),
    )?;
    write_jsonl_atomic(
        &out.join("rejections.jsonl"),
        publication.batch.rejections.iter(),
    )?;
    write_text_atomic(&out.join("manifest.json"), publication.manifest_text)?;
    let _ = std::fs::remove_file(pending_publish_path(out));
    Ok(())
}

/// Where this simulation's run-log entry is appended (`--run-log` or
/// `<out>/run-log.jsonl`).
fn run_log_path(args: &SimulateArgs) -> PathBuf {
    args.run_log
        .clone()
        .unwrap_or_else(|| args.out.join("run-log.jsonl"))
}

/// Files a run log must never overwrite: the bundle artifacts, the
/// publication-control files, and this simulation's input.
fn reserved_log_paths(args: &SimulateArgs) -> Vec<PathBuf> {
    ARTIFACT_FILES
        .iter()
        .map(|name| args.out.join(name))
        .chain([
            args.out.clone(),
            args.out.join("manifest.json"),
            pending_publish_path(&args.out),
            args.out.join(LOCK_FILE),
            args.problems.clone(),
        ])
        .collect()
}

/// This simulation's entry for the generation run log: counts from the batch
/// being committed, and the manifest digest of the text about to publish.
fn simulation_run_record(
    args: &SimulateArgs,
    batch: &SimulationBatch,
    manifest_text: &str,
) -> Record {
    Record::Run(Box::new(RunRecord {
        run_id: tutor_runlog::new_run_id(),
        family: args.family.clone(),
        spec: args.spec.clone(),
        stage: Stage::Simulate,
        replaces: args.replaces.clone(),
        environment: tutor_runlog::environment(),
        generators: vec![GeneratorUse {
            name: SIMULATOR_NAME.to_owned(),
            version: SIMULATOR_VERSION,
            language: tutor_schema::GeneratorLanguage::Rust,
            seeds: SeedUse {
                start: args.seed_start,
                end: None,
                derivation: Some(
                    "sha256(seed_start, problem_id, attempt_index) & MAX_SEED".to_owned(),
                ),
            },
            parameter_ranges: simulation_parameter_ranges(args),
        }],
        teacher: None,
        counts: simulation_record_counts(batch),
        output: OutputRef {
            directory: args.out.display().to_string(),
            manifest_sha256: Some(tutor_runlog::sha256_hex(manifest_text.as_bytes())),
        },
        split_manifest_sha256: None,
        qa: None,
    }))
}

/// Outcome counts for the entry, measured from the batch being committed.
fn simulation_record_counts(batch: &SimulationBatch) -> Counts {
    let mut rejected_by_reason = std::collections::BTreeMap::new();
    for rejection in &batch.rejections {
        let reason = rejection["stage"].as_str().unwrap_or("unknown");
        *rejected_by_reason.entry(reason.to_owned()).or_insert(0_u64) += 1;
    }
    // Candidates that produced an attempt row: emitted + schema-stage rejects.
    let produced = batch.attempts.len()
        + batch
            .rejections
            .iter()
            .filter(|rejection| rejection["stage"] == "schema")
            .count();
    Counts {
        generated: produced as u64,
        verified: batch.attempts.len() as u64,
        rejected: batch.rejections.len() as u64,
        rejected_by_reason,
        deduplicated: None,
        decontaminated: None,
        exported: None,
    }
}

/// The pinned options the simulator drew under, as the entry's
/// `parameter_ranges`.
fn simulation_parameter_ranges(
    args: &SimulateArgs,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    std::collections::BTreeMap::from([
        (
            "attempts_per_problem".to_owned(),
            serde_json::json!(args.attempts_per_problem),
        ),
        ("mode".to_owned(), serde_json::json!(args.mode)),
        (
            "misconception".to_owned(),
            serde_json::json!(args.misconception),
        ),
        ("level".to_owned(), serde_json::json!(args.level)),
        (
            "persistence".to_owned(),
            serde_json::json!(args.persistence),
        ),
        (
            "surface_style".to_owned(),
            serde_json::json!(args.surface_style),
        ),
    ])
}

fn pending_publish_text(
    bundle_artifacts: &serde_json::Value,
    existing: &[(&'static str, serde_json::Value)],
) -> Result<String, serde_json::Error> {
    let mut replaces = serde_json::Map::new();
    for (artifact, metadata) in existing {
        if bundle_artifacts[*artifact] != *metadata {
            replaces.insert((*artifact).to_owned(), metadata.clone());
        }
    }
    serde_json::to_string(&serde_json::json!({
        "simulator": SIMULATOR_NAME,
        "artifacts": bundle_artifacts,
        "replaces": replaces,
    }))
}

fn jsonl_metadata<'a>(
    rows: impl Iterator<Item = &'a serde_json::Value>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    for row in rows {
        let mut line = serde_json::to_string(row)?;
        line.push('\n');
        hasher.update(line.as_bytes());
        bytes += line.len() as u64;
    }
    let digest = hasher.finalize();
    Ok(serde_json::json!({
        "sha256": format!("{digest:x}"),
        "bytes": bytes,
    }))
}

struct OutputLock {
    path: PathBuf,
}

const LOCK_FILE: &str = ".simulate.lock";

impl OutputLock {
    fn acquire(out: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = out.join(LOCK_FILE);
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
        let lock = Self { path };
        let result = writeln!(file, "pid {}", std::process::id());
        drop(file);
        result?;
        Ok(lock)
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

const ARTIFACT_FILES: [&str; 2] = ["attempts.jsonl", "rejections.jsonl"];

fn pending_publish_path(out: &Path) -> PathBuf {
    out.join(".simulate-pending.json")
}

fn ensure_simulation_output(
    out: &Path,
) -> Result<Vec<(&'static str, serde_json::Value)>, Box<dyn std::error::Error>> {
    let manifest = read_simulator_manifest(out)?;
    let existing = existing_artifact_metadata(out)?;
    let pending = read_pending_publish(out)?;
    validate_existing_artifacts(out, &existing, manifest.as_ref(), pending.as_ref())?;
    Ok(existing)
}

fn read_simulator_manifest(
    out: &Path,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    let manifest = out.join("manifest.json");
    if !manifest.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&manifest)?;
    let existing: serde_json::Value = serde_json::from_str(&text)?;
    if existing.get("simulator") == Some(&serde_json::json!(SIMULATOR_NAME)) {
        return Ok(Some(existing));
    }
    Err(format!(
        "{} already contains a non-simulator manifest; choose a separate simulation output directory",
        manifest.display()
    )
    .into())
}

fn existing_artifact_metadata(
    out: &Path,
) -> Result<Vec<(&'static str, serde_json::Value)>, Box<dyn std::error::Error>> {
    let mut existing = Vec::new();
    for artifact in ARTIFACT_FILES {
        let path = out.join(artifact);
        if path.exists() {
            existing.push((artifact, file_artifact_metadata(&path)?));
        }
    }
    Ok(existing)
}

fn read_pending_publish(
    out: &Path,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    let pending_path = pending_publish_path(out);
    if !pending_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&pending_path)?;
    let pending: serde_json::Value = serde_json::from_str(&text)?;
    if pending.get("simulator") == Some(&serde_json::json!(SIMULATOR_NAME)) {
        return Ok(Some(pending));
    }
    Err(format!(
        "{} is not a simulator pending-publish record; choose a separate output directory",
        pending_path.display()
    )
    .into())
}

fn validate_existing_artifacts(
    out: &Path,
    existing: &[(&'static str, serde_json::Value)],
    manifest: Option<&serde_json::Value>,
    pending: Option<&serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    if existing.is_empty() {
        return Ok(());
    }
    if manifest.is_none() && pending.is_none() {
        return Err(format!(
            "{} contains artifact files without simulator provenance; choose a separate output directory",
            out.display()
        )
        .into());
    }
    for (artifact, actual) in existing {
        if !artifact_has_provenance(artifact, actual, manifest, pending) {
            return Err(artifact_provenance_error(out, artifact, manifest.is_some()));
        }
    }
    Ok(())
}

fn artifact_has_provenance(
    artifact: &str,
    actual: &serde_json::Value,
    manifest: Option<&serde_json::Value>,
    pending: Option<&serde_json::Value>,
) -> bool {
    let matches_manifest = manifest.is_some_and(|record| record["artifacts"][artifact] == *actual);
    let matches_pending = pending.is_some_and(|record| {
        record["artifacts"][artifact] == *actual || record["replaces"][artifact] == *actual
    });
    matches_manifest || matches_pending
}

fn artifact_provenance_error(
    out: &Path,
    artifact: &str,
    has_manifest: bool,
) -> Box<dyn std::error::Error> {
    let provenance = if has_manifest {
        "simulator artifact provenance"
    } else {
        "the interrupted simulation record"
    };
    format!(
        "{} does not match {provenance}; choose a separate output directory",
        out.join(artifact).display()
    )
    .into()
}

fn file_artifact_metadata(path: &Path) -> std::io::Result<serde_json::Value> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    let digest = hasher.finalize();
    Ok(serde_json::json!({
        "sha256": format!("{digest:x}"),
        "bytes": bytes,
    }))
}

fn write_jsonl_atomic<'a>(
    path: &Path,
    rows: impl Iterator<Item = &'a serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    atomic_write_with(path, |file| {
        for row in rows {
            let mut line = serde_json::to_string(row).map_err(std::io::Error::other)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
        }
        Ok(())
    })
}

fn write_text_atomic(path: &Path, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    atomic_write_with(path, |file| file.write_all(text.as_bytes()))
}

fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    AtomicFile::new_with_tmpdir(path, AllowOverwrite, parent)
        .write(write)
        .map_err(|error| {
            std::io::Error::other(format!(
                "atomic write to {} failed: {error:?}",
                path.display()
            ))
        })?;
    Ok(())
}
