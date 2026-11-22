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

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tutor_schema::{
    Answer, Difficulty, GeneratorLanguage, GeneratorProvenance, MAX_SEED, MisconceptionId,
    Parameters, Problem, Step,
};
use tutor_verify::Rejection;
use tutor_wire::{MathClient, WireError};

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
        split: "Julia math (polynomial rendering + derivative) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem (endpoint TUTOR_MATH_ENDPOINT).",
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
        split: "Pure Rust: rand_pcg draw + checked integer power-rule math in tutor-generate; verified by Julia over ZMQ verify_answer.",
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
        split: "Julia math (two polynomial draws + symbolic differentiation of each factor) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem.",
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
        split: "Julia math (outer/inner draws, inner evaluation, symbolic differentiation) in math-julia/src/generators.jl; Rust drives it over ZMQ generate_problem.",
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

    let dense = tutor_verify::dense_binomial(a, m, b, n);
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
    /// Problems written to `verified/problems.jsonl`.
    pub total_verified: usize,
    /// Rejections written to `rejected/rejections.jsonl`.
    pub total_rejected: usize,
}

/// Settings for [`run`].
pub struct RunConfig {
    /// Generator specs to draw from (all [`GENERATORS`] when empty at the CLI).
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
}

/// Generate `count` seeded problems per generator, verify each on the opposite
/// side of the boundary, and write verified problems, rejections, and a
/// manifest under `config.out_dir`.
pub fn run(config: &RunConfig) -> std::io::Result<RunReport> {
    use rayon::prelude::*;

    let schema_validator = jsonschema::validator_for(&config.schema_json)
        .map_err(|err| io_err(format!("schema does not compile: {err}")))?;

    let endpoint = config.endpoint.clone();
    let timeout = config.timeout;
    let schema_json = config.schema_json.clone();

    let work: Vec<(&'static GeneratorSpec, u64)> = config
        .generators
        .iter()
        .flat_map(|spec| (config.seed_start..config.seed_start + config.count).map(|s| (*spec, s)))
        .collect();

    let results: Vec<(String, u64, Result<Problem, RejectRecord>)> = work
        .par_iter()
        .map_init(
            || MathClient::connect(&endpoint, timeout).ok(),
            |client_state, (spec, seed)| {
                let fail = |stage: &'static str,
                            problem_id: Option<String>,
                            code: &str,
                            detail: String| {
                    RejectRecord {
                        generator: spec.name.to_owned(),
                        seed: *seed,
                        problem_id,
                        stage,
                        code: code.to_owned(),
                        detail,
                    }
                };
                let outcome = (|| {
                    let problem = generate(spec, *seed, client_state.as_mut())
                        .map_err(|err| fail("generate", None, err.code(), err.to_string()))?;
                    // Second gate: the JSON Schema file itself, in case the
                    // worker's idea of the schema drifted from the repo's.
                    let as_json = serde_json::to_value(&problem)
                        .map_err(|err| fail("schema", None, "schema_serialize", err.to_string()))?;
                    if !schema_validator.is_valid(&as_json) {
                        let detail = schema_validator
                            .iter_errors(&as_json)
                            .map(|e| e.to_string())
                            .take(5)
                            .collect::<Vec<_>>()
                            .join("; ");
                        let _ = &schema_json;
                        return Err(fail(
                            "schema",
                            Some(problem.problem_id.clone()),
                            "schema_invalid",
                            detail,
                        ));
                    }
                    match tutor_verify::verify_auto(&problem, client_state.as_mut().unwrap()) {
                        Ok(verified) => Ok(verified),
                        Err(Rejection {
                            problem_id,
                            code,
                            detail,
                        }) => Err(fail("verify", Some(problem_id), code.as_str(), detail)),
                    }
                })();
                (spec.name.to_owned(), *seed, outcome)
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

    let verified_dir = config.out_dir.join("verified");
    let rejected_dir = config.out_dir.join("rejected");
    std::fs::create_dir_all(&verified_dir)?;
    std::fs::create_dir_all(&rejected_dir)?;
    write_jsonl(
        &verified_dir.join("problems.jsonl"),
        verified.iter().map(|p| serde_json::to_value(p).unwrap()),
    )?;
    write_jsonl(
        &rejected_dir.join("rejections.jsonl"),
        rejected
            .iter()
            .map(serde_json::to_value)
            .map(|r| r.unwrap()),
    )?;

    let mut generators: BTreeMap<String, GeneratorCounts> = BTreeMap::new();
    for spec in &config.generators {
        generators.insert(spec.name.to_owned(), GeneratorCounts::default());
    }
    for p in &verified {
        let counts = generators.entry(p.generator.name.clone()).or_default();
        counts.generated += 1;
        counts.verified += 1;
    }
    for r in &rejected {
        let counts = generators.entry(r.generator.clone()).or_default();
        counts.generated += 1;
        counts.rejected += 1;
    }

    let report = RunReport {
        schema: "schemas/problem.schema.json".to_owned(),
        endpoint: config.endpoint.clone(),
        seeds: format!(
            "{}..{}",
            config.seed_start,
            config.seed_start + config.count
        ),
        generators,
        total_verified: verified.len(),
        total_rejected: rejected.len(),
    };
    let manifest = serde_json::to_string_pretty(&report).map_err(|e| io_err(e.to_string()))?;
    std::fs::write(config.out_dir.join("manifest.json"), manifest)?;
    Ok(report)
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
