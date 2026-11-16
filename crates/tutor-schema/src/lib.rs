//! Row types for the synthetic tutoring datasets.
//!
//! [`Problem`] mirrors `schemas/problem.schema.json`. Deserialization rejects unknown
//! fields and [`Problem::validate`] enforces the constraints serde cannot express, so a
//! value that passes [`Problem::from_json`] is schema-valid. A test in
//! `tests/schema_sync.rs` keeps these types and the JSON Schema file in line.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current `schema_version` of [`Problem`].
pub const PROBLEM_SCHEMA_VERSION: u32 = 1;

/// Largest generator seed (2^53 - 1), so seeds survive any JSON parser exactly.
pub const MAX_SEED: u64 = (1 << 53) - 1;

/// Most coefficients a polynomial problem may carry.
pub const MAX_COEFFICIENTS: usize = 16;

/// One generated problem with its answer and generator provenance.
///
/// A problem without [`Problem::verification`] is only a candidate. Dataset rows may
/// only be built from problems that `tutor-verify` has checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    /// Always [`PROBLEM_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable ID, unique per (generator, version, seed).
    pub problem_id: String,
    /// Dotted curriculum topic ID, e.g. `calculus.derivatives.power_rule`.
    pub topic: String,
    /// Who generated the problem, and how to replay it.
    pub generator: GeneratorProvenance,
    /// Problem text shown to the student.
    pub statement: String,
    /// Structured parameters the statement was rendered from.
    pub parameters: Parameters,
    /// The generator's answer. Untrusted until verified.
    pub answer: Answer,
    /// Set by an independent verifier; absent on unverified candidates.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub verification: Option<Verification>,
}

// The schema allows `verification` to be absent but not `null`.
fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Generator identity and seed: enough to regenerate the problem exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorProvenance {
    /// Generator name, e.g. `polynomial_derivative_at_point`.
    pub name: String,
    /// Generator version; bumped whenever output for a seed changes.
    pub version: u32,
    /// Implementation language of the generator.
    pub language: GeneratorLanguage,
    /// Runtime version (Julia `VERSION` or rustc version) that produced the problem.
    pub runtime_version: String,
    /// Seed in `0..=MAX_SEED`.
    pub seed: u64,
}

/// Where a generator runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorLanguage {
    /// The `math-julia` worker, reached over ZMQ.
    Julia,
    /// In-process Rust generator.
    Rust,
}

/// Structured problem parameters, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Parameters {
    /// Evaluate `f'(point)` for an integer polynomial `f`.
    PolynomialDerivativeAtPoint {
        /// Coefficients in ascending order of power; the leading one is nonzero.
        coefficients: Vec<i64>,
        /// Point at which to evaluate the derivative.
        point: i64,
    },
}

/// A problem's answer, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    /// An exact integer.
    Integer {
        /// The answer value.
        value: i64,
    },
}

/// Record of an independent check of [`Problem::answer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    /// How the answer was checked, e.g. `exact_integer_recompute`.
    pub method: String,
    /// Verifier name, e.g. `tutor-verify`.
    pub verifier: String,
    /// Verifier version.
    pub verifier_version: String,
}

/// Why a value is not schema-valid.
#[derive(Debug, Error)]
pub enum SchemaError {
    /// Not JSON, wrong types, unknown fields, or missing fields.
    #[error("problem JSON does not match the schema: {0}")]
    Json(#[from] serde_json::Error),
    /// Well-typed but violates a schema constraint.
    #[error("invalid `{field}`: {reason}")]
    Invalid {
        /// JSON path of the offending field.
        field: &'static str,
        /// What is wrong with it.
        reason: String,
    },
}

impl SchemaError {
    /// Stable reason code for rejection logs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Json(_) => "schema_shape",
            Self::Invalid { .. } => "schema_constraint",
        }
    }
}

fn invalid(field: &'static str, reason: impl Into<String>) -> SchemaError {
    SchemaError::Invalid {
        field,
        reason: reason.into(),
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), SchemaError> {
    if value.is_empty() {
        return Err(invalid(field, "must be non-empty"));
    }
    Ok(())
}

/// Whether `topic` is a dotted ID of `[a-z0-9_]+` segments.
pub fn is_valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

impl Problem {
    /// Parse and validate a problem from JSON.
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        let problem: Self = serde_json::from_str(json)?;
        problem.validate()?;
        Ok(problem)
    }

    /// Convert an already-decoded JSON value, then validate it.
    pub fn from_value(value: serde_json::Value) -> Result<Self, SchemaError> {
        let problem: Self = serde_json::from_value(value)?;
        problem.validate()?;
        Ok(problem)
    }

    /// Check the constraints of `problem.schema.json` that the types do not encode.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != PROBLEM_SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                format!(
                    "expected {PROBLEM_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            ));
        }
        require_non_empty("problem_id", &self.problem_id)?;
        if !is_valid_topic(&self.topic) {
            return Err(invalid(
                "topic",
                format!("{:?} is not a dotted [a-z0-9_] ID", self.topic),
            ));
        }
        let generator = &self.generator;
        require_non_empty("generator.name", &generator.name)?;
        if generator.version == 0 {
            return Err(invalid("generator.version", "must be at least 1"));
        }
        require_non_empty("generator.runtime_version", &generator.runtime_version)?;
        if generator.seed > MAX_SEED {
            return Err(invalid(
                "generator.seed",
                format!("{} exceeds {MAX_SEED}", generator.seed),
            ));
        }
        require_non_empty("statement", &self.statement)?;
        match &self.parameters {
            Parameters::PolynomialDerivativeAtPoint { coefficients, .. } => {
                if coefficients.is_empty() || coefficients.len() > MAX_COEFFICIENTS {
                    return Err(invalid(
                        "parameters.coefficients",
                        format!("must have 1..={MAX_COEFFICIENTS} entries"),
                    ));
                }
                if coefficients.last() == Some(&0) {
                    return Err(invalid(
                        "parameters.coefficients",
                        "leading coefficient must be nonzero",
                    ));
                }
            }
        }
        if let Some(verification) = &self.verification {
            require_non_empty("verification.method", &verification.method)?;
            require_non_empty("verification.verifier", &verification.verifier)?;
            require_non_empty(
                "verification.verifier_version",
                &verification.verifier_version,
            )?;
        }
        Ok(())
    }
}
