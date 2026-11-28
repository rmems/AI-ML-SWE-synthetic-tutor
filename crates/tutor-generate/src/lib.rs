//! Problem generators and the generate→verify pipeline.
//!
//! One [`GeneratorSpec`] per topic family. Each spec declares where its math runs:
//! [`Backend::Rust`] generators draw parameters with `rand_pcg` and compute the
//! answer in-crate; [`Backend::Julia`] generators keep the math in `math-julia/`
//! (symbolic differentiation and step construction are materially simpler there)
//! and are driven over ZMQ — the same `tutor-math/1` REQ/REP bridge as
//! `tutor-wire`, endpoint `TUTOR_MATH_ENDPOINT` or `tcp://127.0.0.1:5557`.
//!
//! Every problem is seeded for replay: the same seed always yields the same
//! problem (Rust draws are pinned to `rand_pcg`; the Julia side pins `Xoshiro`).
//! Answers are claims, not checks — [`run`] verifies each problem on the
//! opposite side of the boundary and writes `verified/` and `rejected/` outputs.

pub mod student;

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tutor_runlog::{
    Counts, GeneratorUse, LockFile, OutputRef, Record, RunLog, RunRecord, SeedUse, Stage,
};
use tutor_schema::{
    Answer, Difficulty, GeneratorLanguage, GeneratorProvenance, MAX_SEED, MisconceptionId,
    Parameters, Problem, Step,
};
use tutor_verify::{Exponent, Rejection, Scalar};
use tutor_wire::{MathClient, WireError, WorkerInfo};

/// Version of the Rust-native binomial generator.
pub const BINOMIAL_VERSION: u32 = 1;

/// The rustc version that built this crate, recorded as `runtime_version`.
pub const RUSTC_VERSION: &str = env!("TUTOR_RUSTC_VERSION");

/// How a generator family is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Parameters and answer are produced in Rust (no Julia needed).
    Rust,
    /// The math stays in `math-julia/`; Rust drives it over ZMQ
    /// (`generate_problem` op of the `tutor-math/1` protocol).
    Julia,
}

/// One problem family's generator contract.
#[derive(Debug, Clone, Copy)]
pub struct GeneratorSpec {
    /// Generator name; also the ZMQ generator name for [`Backend::Julia`].
    pub name: &'static str,
    /// Generator version; bump when output for a seed changes.
    pub version: u32,
    /// Curriculum topic ID every emitted problem carries.
    pub topic: &'static str,
    /// Where the math runs.
    pub backend: Backend,
    /// The seed → answer draw bounds recorded in `difficulty`.
    pub difficulty: Difficulty,
    /// Taxonomy IDs emitted problems may carry.
    pub misconception_ids: &'static [MisconceptionId],
    /// One line stating the Rust/Julia split and the endpoint used.
    pub split: &'static str,
}

/// The pilot derivative families, in pilot order: power, product, chain rule.
/// `polynomial_derivative_at_point` is the power-rule family's Julia form;
/// `binomial_derivative_at_point` is its Rust-native form.
pub const GENERATORS: &[GeneratorSpec] = &[
    GeneratorSpec {
        name: "polynomial_derivative_at_point",
        version: 1,
        topic: "calculus.derivatives.power_rule",
        backend: Backend::Julia,
        difficulty: Difficulty {
            max_degree: 4,
            coefficient_bound: 9,
            point_bound: 5,
        },
        misconception_ids: &[
            MisconceptionId::DerivativesPowerRuleOvergeneralised,
            MisconceptionId::AlgebraSignError,
        ],
        split: "Julia math (polynomial rendering + derivative) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem at tcp://127.0.0.1:5557 (default; --endpoint/TUTOR_MATH_ENDPOINT overrides).",
    },
    GeneratorSpec {
        name: "binomial_derivative_at_point",
        version: BINOMIAL_VERSION,
        topic: "calculus.derivatives.power_rule",
        backend: Backend::Rust,
        difficulty: Difficulty {
            max_degree: 6,
            coefficient_bound: 9,
            point_bound: 5,
        },
        misconception_ids: &[
            MisconceptionId::DerivativesPowerRuleOvergeneralised,
            MisconceptionId::AlgebraSignError,
        ],
        split: "Pure Rust: rand_pcg draw + checked integer power-rule math in tutor-generate; verified by Julia over ZMQ verify_answer at tcp://127.0.0.1:5557 (default; --endpoint/TUTOR_MATH_ENDPOINT overrides).",
    },
    GeneratorSpec {
        name: "product_rule_at_point",
        version: 1,
        topic: "calculus.derivatives.product_rule",
        backend: Backend::Julia,
        difficulty: Difficulty {
            max_degree: 2,
            coefficient_bound: 9,
            point_bound: 5,
        },
        misconception_ids: &[
            MisconceptionId::DerivativesProductRule,
            MisconceptionId::AlgebraSignError,
        ],
        split: "Julia math (two polynomial draws + symbolic differentiation of each factor) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem at tcp://127.0.0.1:5557 (default; --endpoint/TUTOR_MATH_ENDPOINT overrides).",
    },
    GeneratorSpec {
        name: "chain_rule_at_point",
        version: 1,
        topic: "calculus.derivatives.chain_rule",
        backend: Backend::Julia,
        difficulty: Difficulty {
            max_degree: 2,
            coefficient_bound: 9,
            point_bound: 5,
        },
        misconception_ids: &[
            MisconceptionId::DerivativesChainRule,
            MisconceptionId::AlgebraSignError,
        ],
        split: "Julia math (outer/inner draws, inner evaluation, symbolic differentiation) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem at tcp://127.0.0.1:5557 (default; --endpoint/TUTOR_MATH_ENDPOINT overrides).",
    },
];

/// A spec by name.
pub fn spec(name: &str) -> Option<&'static GeneratorSpec> {
    GENERATORS.iter().find(|g| g.name == name)
}

/// Why a generation or verification step failed.
#[derive(Debug, Error)]
pub enum GenerateError {
    /// Seed outside `0..=MAX_SEED`.
    #[error("seed {0} exceeds {MAX_SEED}")]
    InvalidSeed(u64),
    /// A Julia-backed generator needs a math worker client.
    #[error("generator is Julia-backed but no MathClient was provided")]
    MissingWorker,
    /// The ZMQ call failed.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The computed answer overflowed i64 (parameters were out of draw bounds).
    #[error("answer overflows i64")]
    Overflow,
}

impl GenerateError {
    /// Stable reason code for rejection records.
    pub fn code(&self) -> &str {
        match self {
            Self::InvalidSeed(_) => "generate_invalid_seed",
            Self::MissingWorker => "generate_missing_worker",
            Self::Wire(err) => err.code(),
            Self::Overflow => "generate_overflow",
        }
    }
}

/// Draw `f(x) = a·x^m + b·x^n` (m > n) and `f'(point)` for `seed`, in Rust.
pub fn binomial_derivative_at_point(seed: u64) -> Result<Problem, GenerateError> {
    if seed > MAX_SEED {
        return Err(GenerateError::InvalidSeed(seed));
    }
    let spec = spec("binomial_derivative_at_point").unwrap();
    let mut rng = Pcg64::seed_from_u64(seed);
    let bound = spec.difficulty;
    let a = rng.random_range(-(bound.coefficient_bound as i64)..=bound.coefficient_bound as i64);
    let a = if a == 0 { 1 } else { a };
    let m = rng.random_range(1..=bound.max_degree);
    let b = rng.random_range(-(bound.coefficient_bound as i64)..=bound.coefficient_bound as i64);
    let b = if b == 0 { -1 } else { b };
    let n = rng.random_range(0..m);
    let point = rng.random_range(-(bound.point_bound as i64)..=bound.point_bound as i64);

    // f'(x) = a·m·x^(m-1) + b·n·x^(n-1), checked.
    let term = |coeff: i64, e: u32| -> Option<i64> {
        if e == 0 {
            return Some(0);
        }
        coeff
            .checked_mul(i64::from(e))?
            .checked_mul(point.checked_pow(e - 1)?)
    };
    let value = term(a, m)
        .and_then(|t1| term(b, n).and_then(|t2| t1.checked_add(t2)))
        .ok_or(GenerateError::Overflow)?;

    let dense = tutor_verify::dense_binomial(Scalar(a), Exponent(m), Scalar(b), Exponent(n));
    let derivative = tutor_verify::polynomial_derivative(&dense).ok_or(GenerateError::Overflow)?;
    let statement = format!(
        "Let f(x) = {}. Compute f'({point}).",
        format_binomial(a, m, b, n)
    );
    Ok(Problem {
        schema_version: 1,
        problem_id: format!("{}/v{}/seed-{seed}", spec.name, spec.version),
        topic: spec.topic.to_owned(),
        generator: GeneratorProvenance {
            name: spec.name.to_owned(),
            version: spec.version,
            language: GeneratorLanguage::Rust,
            runtime_version: RUSTC_VERSION.to_owned(),
            seed,
        },
        statement,
        parameters: Parameters::BinomialDerivativeAtPoint { a, m, b, n, point },
        difficulty: spec.difficulty,
        answer: Answer::Integer { value },
        steps: vec![
            Step::DifferentiatePolynomial {
                input: dense,
                output: derivative.clone(),
            },
            Step::EvaluatePolynomial {
                coefficients: derivative,
                point,
                value,
            },
        ],
        misconception_ids: spec.misconception_ids.to_vec(),
        verification: None,
    })
}

/// Render `a·x^m + b·x^n` (m > n, both coefficients nonzero).
pub fn format_binomial(a: i64, m: u32, b: i64, n: u32) -> String {
    let term = |coeff: i64, e: u32| -> String {
        let power = match e {
            0 => return coeff.abs().to_string(),
            1 => "x".to_owned(),
            _ => format!("x^{e}"),
        };
        if coeff.abs() == 1 {
            power
        } else {
            format!("{}{power}", coeff.abs())
        }
    };
    let first = if a < 0 {
        format!("-{}", term(a, m))
    } else {
        term(a, m)
    };
    let second = if b < 0 {
        format!(" - {}", term(b, n))
    } else {
        format!(" + {}", term(b, n))
    };
    format!("{first}{second}")
}

/// Generate one problem for `spec`/`seed`. Julia-backed specs need `client`;
/// Rust-native specs ignore it.
pub fn generate(
    spec: &GeneratorSpec,
    seed: u64,
    client: Option<&mut MathClient>,
) -> Result<Problem, GenerateError> {
    if seed > MAX_SEED {
        return Err(GenerateError::InvalidSeed(seed));
    }
    match spec.backend {
        Backend::Rust => match spec.name {
            "binomial_derivative_at_point" => binomial_derivative_at_point(seed),
            other => unreachable!("no Rust generator named {other}"),
        },
        Backend::Julia => {
            let client = client.ok_or(GenerateError::MissingWorker)?;
            Ok(client.generate_problem(spec.name, seed)?)
        }
    }
}

/// One rejected candidate, recorded in `rejected/rejections.jsonl`.
#[derive(Debug, Serialize)]
pub struct RejectRecord {
    /// Generator name and seed that produced the rejection.
    pub generator: String,
    /// Seed of the rejected candidate.
    pub seed: u64,
    /// Problem ID when generation got that far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    /// Stage that rejected it: `generate`, `schema`, or `verify`.
    pub stage: &'static str,
    /// Stable reason code.
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
}

/// Counts for one generator inside a [`RunReport`].
#[derive(Debug, Default, Serialize)]
pub struct GeneratorCounts {
    /// Problems produced by the generator.
    pub generated: usize,
    /// Problems that passed verification.
    pub verified: usize,
    /// Problems rejected at any stage.
    pub rejected: usize,
}

/// What a run produced; also serialised to `manifest.json`.
#[derive(Debug, Serialize)]
pub struct RunReport {
    /// Schema file the candidates were validated against.
    pub schema: String,
    /// ZMQ endpoint used for Julia-backed generators and verification.
    pub endpoint: String,
    /// Seed range actually drawn.
    pub seeds: String,
    /// Per-generator and total counts.
    pub generators: BTreeMap<String, GeneratorCounts>,
    /// `total_rejected` broken down by stable reason code.
    pub rejected_by_reason: BTreeMap<String, u64>,
    /// Problems written to `verified/problems.jsonl`.
    pub total_verified: usize,
    /// Rejections written to `rejected/rejections.jsonl`.
    pub total_rejected: usize,
}

/// Where and under what identity a run logs itself to the generation run
/// log (`docs/cadence.md`, `schemas/run-log.schema.json`).
///
/// `None` in [`RunConfig::run_log`] disables logging for library callers; the
/// `tutor-generate` CLI always logs — a batch with no log entry does not
/// exist for release purposes.
pub struct RunLogTarget {
    /// Path of the append-only JSONL run log. The CLI defaults to
    /// `<out>/run-log.jsonl` so the log lives with the batch it describes.
    pub path: PathBuf,
    /// Dataset family the run feeds; `null` in the entry when `None`.
    pub family: Option<String>,
    /// Generation spec reference (issue ref or URL); `null` when `None`.
    pub spec: Option<String>,
    /// Run id of the earlier run this one replaces; `None` records that the
    /// run replaces nothing — the log never infers it, so a rerun that
    /// supersedes an earlier batch must name that run explicitly.
    pub replaces: Option<String>,
}

/// Settings for [`run`].
pub struct RunConfig {
    /// Generator specs to draw from (all [`GENERATORS`] when empty at the CLI).
    /// Duplicate names are drawn once.
    pub generators: Vec<&'static GeneratorSpec>,
    /// First seed drawn, inclusive.
    pub seed_start: u64,
    /// How many seeds per generator.
    pub count: u64,
    /// ZMQ endpoint of the `math-julia` worker.
    pub endpoint: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Output directory: gets `verified/`, `rejected/` and `manifest.json`.
    pub out_dir: PathBuf,
    /// `problem.schema.json` content for the jsonschema pass.
    pub schema_json: serde_json::Value,
    /// Schema identifier recorded in the manifest (usually the file path used).
    pub schema: String,
    /// Run-log destination; `Some` appends a `run` entry after the batch is
    /// committed.
    pub run_log: Option<RunLogTarget>,
}

/// Generate `count` seeded problems per generator, verify each on the opposite
/// side of the boundary, and write verified problems, rejections, and a
/// manifest under `config.out_dir`.
///
/// A run that verifies nothing writes only rejections and the manifest — it
/// never truncates a `verified/problems.jsonl` left by an earlier successful
/// run in the same directory.
///
/// When `config.run_log` is set, the run appends a `run` entry to the
/// generation run log after the batch is committed — including a run that
/// verified nothing, since a rejected-everything run is still a run. The
/// append is proven out first: the log destination must not alias a batch
/// artifact and the entry (chain, `replaces`, format) must be appendable —
/// a doomed append fails before any earlier batch is replaced, not after.
pub fn run(config: &RunConfig) -> std::io::Result<RunReport> {
    use rayon::prelude::*;

    let schema_validator = jsonschema::validator_for(&config.schema_json)
        .map_err(|err| io_err(format!("schema does not compile: {err}")))?;

    let endpoint = config.endpoint.clone();
    let timeout = config.timeout;
    // Best-effort provenance ping: the Julia version is recorded on the
    // run-log entry when the worker answers. A missing worker costs one
    // (capped) timeout here; candidates then fail per-generator anyway.
    let worker = MathClient::connect(&endpoint, timeout.min(Duration::from_secs(5)))
        .ok()
        .and_then(|mut client| client.ping().ok());
    let seed_end = config
        .seed_start
        .checked_add(config.count)
        .ok_or_else(|| io_err("seed_start + count overflows u64".to_owned()))?;

    let mut seen = std::collections::HashSet::new();
    let work: Vec<(&'static GeneratorSpec, u64)> = config
        .generators
        .iter()
        .filter(|spec| seen.insert(spec.name))
        .flat_map(|spec| (config.seed_start..seed_end).map(|s| (*spec, s)))
        .collect();

    let results: Vec<(String, u64, Result<Problem, RejectRecord>)> = work
        .par_iter()
        .map_init(
            || MathClient::connect(&endpoint, timeout).ok(),
            |client_state, (spec, seed)| {
                (
                    spec.name.to_owned(),
                    *seed,
                    check_candidate(spec, *seed, client_state.as_mut(), &schema_validator),
                )
            },
        )
        .collect();

    let mut verified: Vec<Problem> = Vec::new();
    let mut rejected: Vec<RejectRecord> = Vec::new();
    for (_, _, outcome) in results {
        match outcome {
            Ok(p) => verified.push(p),
            Err(r) => rejected.push(r),
        }
    }
    verified.sort_by(|a, b| a.problem_id.cmp(&b.problem_id));
    rejected.sort_by_key(|a| (a.generator.clone(), a.seed));

    let report = RunReport {
        schema: config.schema.clone(),
        endpoint: config.endpoint.clone(),
        seeds: format!("{}..{}", config.seed_start, seed_end),
        generators: generator_counts(config, &verified, &rejected),
        rejected_by_reason: reason_counts(&rejected),
        total_verified: verified.len(),
        total_rejected: rejected.len(),
    };
    let manifest_text = manifest_text(&report, &verified, &rejected)?;
    let summary = RunSummary {
        report: &report,
        worker: worker.as_ref(),
        manifest_text: &manifest_text,
    };
    publish_run(
        config,
        &summary,
        &RunOutputs {
            verified: &verified,
            rejected: &rejected,
        },
    )?;
    Ok(report)
}

/// Generate, schema-check and verify one candidate. `client` may be `None`
/// when the worker is unreachable: Rust-generated problems then fail
/// verification (`verify_verifier_unavailable`) rather than panic.
fn check_candidate(
    spec: &'static GeneratorSpec,
    seed: u64,
    client: Option<&mut MathClient>,
    schema_validator: &jsonschema::Validator,
) -> Result<Problem, RejectRecord> {
    let fail = |stage: &'static str, problem_id: Option<String>, code: &str, detail: String| {
        RejectRecord {
            generator: spec.name.to_owned(),
            seed,
            problem_id,
            stage,
            code: code.to_owned(),
            detail,
        }
    };
    let mut client = client;
    let problem = generate(spec, seed, client.as_deref_mut())
        .map_err(|err| fail("generate", None, err.code(), err.to_string()))?;
    // The candidate must carry the provenance of the spec that produced it —
    // a mislabeled `language` would route verification to the generator's own
    // side of the boundary.
    let expected_language = match spec.backend {
        Backend::Rust => GeneratorLanguage::Rust,
        Backend::Julia => GeneratorLanguage::Julia,
    };
    if problem.generator.name != spec.name || problem.generator.language != expected_language {
        return Err(fail(
            "generate",
            Some(problem.problem_id.clone()),
            "generate_language_mismatch",
            format!(
                "expected generator {}/{expected_language:?}, got {}/{:?}",
                spec.name, problem.generator.name, problem.generator.language
            ),
        ));
    }
    // Second gate: the JSON Schema file itself, in case the worker's idea of
    // the schema drifted from the repo's.
    let as_json = serde_json::to_value(&problem)
        .map_err(|err| fail("schema", None, "schema_serialize", err.to_string()))?;
    if !schema_validator.is_valid(&as_json) {
        let detail = schema_validator
            .iter_errors(&as_json)
            .map(|e| e.to_string())
            .take(5)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(fail(
            "schema",
            Some(problem.problem_id.clone()),
            "schema_invalid",
            detail,
        ));
    }
    // Verify on the opposite side of the boundary. Rust-generated problems
    // need the worker; without it they reject with verify_verifier_unavailable.
    let outcome = match (problem.generator.language, client) {
        (GeneratorLanguage::Julia, _) => tutor_verify::verify(&problem),
        (GeneratorLanguage::Rust, Some(client)) => tutor_verify::verify_remote(&problem, client),
        (GeneratorLanguage::Rust, None) => Err(Rejection {
            problem_id: problem.problem_id.clone(),
            code: tutor_verify::ReasonCode::VerifierUnavailable,
            detail: "math worker unreachable".to_owned(),
        }),
    };
    outcome.map_err(
        |Rejection {
             problem_id,
             code,
             detail,
         }| fail("verify", Some(problem_id), code.as_str(), detail),
    )
}

/// Write `verified/problems.jsonl`, `rejected/rejections.jsonl` and
/// `manifest.json`. Each file is written to a temporary sibling and renamed
/// into place, so a failed write never leaves a truncated artifact. An empty
/// verified batch leaves an existing `problems.jsonl` untouched so a failed
/// rerun cannot erase a previous batch.
fn emit_outputs(
    config: &RunConfig,
    verified: &[Problem],
    rejected: &[RejectRecord],
    manifest_text: &str,
) -> std::io::Result<()> {
    let verified_dir = config.out_dir.join("verified");
    let rejected_dir = config.out_dir.join("rejected");
    std::fs::create_dir_all(&verified_dir)?;
    std::fs::create_dir_all(&rejected_dir)?;
    if !verified.is_empty() {
        stage_jsonl(
            &verified_dir.join("problems.jsonl"),
            verified
                .iter()
                .map(serde_json::to_value)
                .map(|v| v.unwrap()),
        )?;
    }
    stage_jsonl(
        &rejected_dir.join("rejections.jsonl"),
        rejected
            .iter()
            .map(serde_json::to_value)
            .map(|v| v.unwrap()),
    )?;
    let manifest_path = config.out_dir.join("manifest.json");
    let tmp = config.out_dir.join(MANIFEST_STAGING_FILE);
    std::fs::write(&tmp, manifest_text)?;
    publish(&tmp, &manifest_path)
}

/// Serialize `manifest.json`: the report plus a sha256/bytes entry per
/// artifact file this run commits, so the manifest digest recorded in the
/// run log binds the batch's contents, not just its counts.
fn manifest_text(
    report: &RunReport,
    verified: &[Problem],
    rejected: &[RejectRecord],
) -> std::io::Result<String> {
    let mut artifacts = serde_json::Map::new();
    if !verified.is_empty() {
        artifacts.insert(
            "verified/problems.jsonl".to_owned(),
            jsonl_metadata(
                verified
                    .iter()
                    .map(serde_json::to_value)
                    .map(|v| v.unwrap()),
            ),
        );
    }
    artifacts.insert(
        "rejected/rejections.jsonl".to_owned(),
        jsonl_metadata(
            rejected
                .iter()
                .map(serde_json::to_value)
                .map(|v| v.unwrap()),
        ),
    );
    let mut manifest = serde_json::to_value(report).map_err(|e| io_err(e.to_string()))?;
    manifest["artifacts"] = serde_json::Value::Object(artifacts);
    serde_json::to_string_pretty(&manifest).map_err(|e| io_err(e.to_string()))
}

/// sha256 + byte count of `rows` rendered as JSONL — the same bytes
/// `write_jsonl` emits — so the manifest describes the committed file.
fn jsonl_metadata(rows: impl Iterator<Item = serde_json::Value>) -> serde_json::Value {
    let mut rendered = Vec::new();
    for row in rows {
        rendered.extend_from_slice(serde_json::to_string(&row).unwrap().as_bytes());
        rendered.push(b'\n');
    }
    serde_json::json!({
        "sha256": tutor_runlog::sha256_hex(&rendered),
        "bytes": rendered.len(),
    })
}

/// Write `rows` to a temporary sibling of `path`, then rename it into place.
fn stage_jsonl(path: &Path, rows: impl Iterator<Item = serde_json::Value>) -> std::io::Result<()> {
    // A staging path unique to this run, so concurrent runs sharing an
    // output directory never delete each other's in-flight file.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("jsonl.tmp.{}.{nonce:x}", std::process::id()));
    write_jsonl(&tmp, rows)?;
    publish(&tmp, path)
}

/// Replace `dest` with the fully written `tmp` file.
fn publish(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    // rename does not replace an existing destination on Windows.
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(tmp, dest)
}

/// Rejections grouped by stable reason code.
fn reason_counts(rejected: &[RejectRecord]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for record in rejected {
        *counts.entry(record.code.clone()).or_insert(0) += 1;
    }
    counts
}

/// Staging name the manifest is written to before its atomic rename.
const MANIFEST_STAGING_FILE: &str = "manifest.json.tmp";

/// Lock file serializing `run` publishes into one output directory.
const RUN_LOCK_FILE: &str = ".run.lock";

/// What a finished run reports to its log entry.
struct RunSummary<'a> {
    report: &'a RunReport,
    worker: Option<&'a WorkerInfo>,
    /// The manifest text about to be committed.
    manifest_text: &'a str,
}

fn log_err(err: tutor_runlog::RunLogError) -> std::io::Error {
    io_err(format!("run log append failed: {err}"))
}

/// The rows a run publishes.
struct RunOutputs<'a> {
    verified: &'a [Problem],
    rejected: &'a [RejectRecord],
}

/// Publish a finished run and append its log entry.
///
/// One run publishes into an output directory at a time (`.run.lock`), so
/// two runs cannot interleave their artifact and manifest renames. With a
/// run log, the destination is checked before anything is created (it must
/// not alias, or sit under, a batch artifact or staging file — a misplaced
/// `--run-log` would overwrite the output it should only describe), then
/// the log's append lock is taken and the entry — existing chain,
/// `replaces` resolution, record format — is validated before any artifact
/// is replaced: a bad path or a `replaces` naming no earlier run fails
/// while the previous batch is intact. The log is opened for append before
/// publication as well, so an unwritable destination cannot strand a
/// committed batch merely because the open fails. The lock is held through
/// the append, so nothing can change the log between the check and the entry.
fn publish_run(
    config: &RunConfig,
    summary: &RunSummary<'_>,
    outputs: &RunOutputs<'_>,
) -> std::io::Result<()> {
    let _out_lock = LockFile::acquire(&config.out_dir.join(RUN_LOCK_FILE)).map_err(|err| {
        io_err(format!(
            "output directory {}: {err}",
            config.out_dir.display()
        ))
    })?;
    let emit = || {
        emit_outputs(
            config,
            outputs.verified,
            outputs.rejected,
            summary.manifest_text,
        )
    };
    let Some(target) = config.run_log.as_ref() else {
        return emit();
    };
    tutor_runlog::ensure_log_path_is_clear(&target.path, &reserved_run_paths(config))
        .map_err(log_err)?;
    let log = RunLog::at(&target.path);
    let mut locked = log.lock().map_err(log_err)?;
    let record = run_record(config, target, summary);
    locked.check(&record).map_err(log_err)?;
    locked.open_append().map_err(log_err)?;
    emit()?;
    locked.append(record).map_err(log_err)?;
    Ok(())
}

/// Paths a run log may not take: the batch's artifacts, staging and lock
/// files, the output directory itself, and the schema it validates with.
fn reserved_run_paths(config: &RunConfig) -> [PathBuf; 9] {
    [
        config.out_dir.clone(),
        config.out_dir.join("verified"),
        config.out_dir.join("rejected"),
        config.out_dir.join("verified/problems.jsonl"),
        config.out_dir.join("rejected/rejections.jsonl"),
        config.out_dir.join("manifest.json"),
        config.out_dir.join(MANIFEST_STAGING_FILE),
        config.out_dir.join(RUN_LOCK_FILE),
        PathBuf::from(&config.schema),
    ]
}

/// This run's entry for the generation run log: counts from the report
/// (never estimated), the manifest digest of the text about to be committed,
/// and Julia versions from the worker's `ping` reply when it answered.
fn run_record(config: &RunConfig, target: &RunLogTarget, summary: &RunSummary<'_>) -> Record {
    let RunSummary {
        report,
        worker,
        manifest_text,
    } = *summary;
    let mut environment = tutor_runlog::environment();
    if let Some(info) = worker {
        environment.julia_version = Some(info.julia_version.clone());
        environment.julia_worker_version = Some(info.worker_version.clone());
    }
    // Overflow was checked in `run`.
    let seed_end = config.seed_start + config.count;
    let mut seen = std::collections::HashSet::new();
    let mut generators: Vec<&'static GeneratorSpec> = config
        .generators
        .iter()
        .filter(|spec| seen.insert(spec.name))
        .copied()
        .collect();
    generators.sort_by_key(|spec| spec.name);
    let generators = generators
        .into_iter()
        .map(|spec| GeneratorUse {
            name: spec.name.to_owned(),
            version: spec.version,
            language: match spec.backend {
                Backend::Rust => GeneratorLanguage::Rust,
                Backend::Julia => GeneratorLanguage::Julia,
            },
            seeds: SeedUse {
                start: config.seed_start,
                end: Some(seed_end),
                derivation: None,
            },
            parameter_ranges: parameter_ranges(spec),
        })
        .collect();
    Record::Run(Box::new(RunRecord {
        run_id: tutor_runlog::new_run_id(),
        family: target.family.clone(),
        spec: target.spec.clone(),
        stage: Stage::Generate,
        replaces: target.replaces.clone(),
        environment,
        generators,
        teacher: None,
        counts: Counts {
            generated: report.generators.values().map(|c| c.generated as u64).sum(),
            verified: report.total_verified as u64,
            rejected: report.total_rejected as u64,
            rejected_by_reason: report.rejected_by_reason.clone(),
            deduplicated: None,
            decontaminated: None,
            exported: None,
        },
        output: OutputRef {
            directory: config.out_dir.display().to_string(),
            manifest_sha256: Some(tutor_runlog::sha256_hex(manifest_text.as_bytes())),
        },
        split_manifest_sha256: None,
        qa: None,
    }))
}

/// A generator's declared draw bounds as the entry's `parameter_ranges`.
fn parameter_ranges(spec: &GeneratorSpec) -> BTreeMap<String, serde_json::Value> {
    serde_json::to_value(spec.difficulty)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|map| map.into_iter().collect())
        .unwrap_or_default()
}

/// Per-generator generated/verified/rejected counts for the manifest.
fn generator_counts(
    config: &RunConfig,
    verified: &[Problem],
    rejected: &[RejectRecord],
) -> BTreeMap<String, GeneratorCounts> {
    let mut generators: BTreeMap<String, GeneratorCounts> = BTreeMap::new();
    for spec in &config.generators {
        generators.insert(spec.name.to_owned(), GeneratorCounts::default());
    }
    for p in verified {
        let counts = generators.entry(p.generator.name.clone()).or_default();
        counts.generated += 1;
        counts.verified += 1;
    }
    for r in rejected {
        let counts = generators.entry(r.generator.clone()).or_default();
        // Only candidates that produced a problem count as generated.
        counts.generated += usize::from(r.problem_id.is_some());
        counts.rejected += 1;
    }
    generators
}

fn io_err(message: String) -> std::io::Error {
    std::io::Error::other(message)
}

fn write_jsonl(path: &Path, rows: impl Iterator<Item = serde_json::Value>) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    for row in rows {
        writeln!(file, "{}", serde_json::to_string(&row).unwrap())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomial_is_seeded_and_replayable() {
        let a = binomial_derivative_at_point(7).unwrap();
        let b = binomial_derivative_at_point(7).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.problem_id, "binomial_derivative_at_point/v1/seed-7");
        assert_eq!(a.generator.language, GeneratorLanguage::Rust);
        assert!(tutor_verify::verify(&a).is_ok());
        let c = binomial_derivative_at_point(8).unwrap();
        assert_ne!(a.parameters, c.parameters);
    }

    #[test]
    fn binomial_respects_draw_bounds() {
        for seed in 0..200 {
            let p = binomial_derivative_at_point(seed).unwrap();
            let Parameters::BinomialDerivativeAtPoint { a, m, b, n, point } = p.parameters else {
                panic!("wrong kind");
            };
            assert!((1..=9).contains(&a.abs()));
            assert!((1..=9).contains(&b.abs()));
            assert!((1..=6).contains(&m));
            assert!(n < m);
            assert!(point.abs() <= 5);
            assert_eq!(
                p.difficulty,
                Difficulty {
                    max_degree: 6,
                    coefficient_bound: 9,
                    point_bound: 5
                }
            );
        }
    }

    #[test]
    fn binomial_seed_out_of_range() {
        assert!(matches!(
            binomial_derivative_at_point(MAX_SEED + 1),
            Err(GenerateError::InvalidSeed(_))
        ));
    }

    #[test]
    fn format_binomial_renders() {
        assert_eq!(format_binomial(3, 4, -2, 2), "3x^4 - 2x^2");
        assert_eq!(format_binomial(-1, 2, 1, 0), "-x^2 + 1");
        assert_eq!(format_binomial(2, 1, 5, 0), "2x + 5");
    }
}
