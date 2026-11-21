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

/// Current `version` of the misconception taxonomy in `schemas/taxonomy.yaml`.
pub const TAXONOMY_VERSION: u32 = 1;

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

/// Stable misconception taxonomy ID, e.g. `derivatives.chain_rule`.
///
/// Serialized as the dotted ID string used in `schemas/taxonomy.yaml`, so a
/// dataset row's primary misconception label validates against the same list the
/// simulator, families, and eval read. IDs are permanent once published: a renamed
/// or split entry gets a new variant and the old one stays parseable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MisconceptionId {
    /// `algebra.distribution_error`.
    #[serde(rename = "algebra.distribution_error")]
    AlgebraDistributionError,
    /// `algebra.exponent_rule`.
    #[serde(rename = "algebra.exponent_rule")]
    AlgebraExponentRule,
    /// `algebra.like_terms`.
    #[serde(rename = "algebra.like_terms")]
    AlgebraLikeTerms,
    /// `algebra.sign_error`.
    #[serde(rename = "algebra.sign_error")]
    AlgebraSignError,
    /// `derivatives.chain_rule`.
    #[serde(rename = "derivatives.chain_rule")]
    DerivativesChainRule,
    /// `derivatives.implicit_differentiation`.
    #[serde(rename = "derivatives.implicit_differentiation")]
    DerivativesImplicitDifferentiation,
    /// `derivatives.power_rule_overgeneralised`.
    #[serde(rename = "derivatives.power_rule_overgeneralised")]
    DerivativesPowerRuleOvergeneralised,
    /// `derivatives.product_rule`.
    #[serde(rename = "derivatives.product_rule")]
    DerivativesProductRule,
    /// `derivatives.quotient_rule`.
    #[serde(rename = "derivatives.quotient_rule")]
    DerivativesQuotientRule,
    /// `functions.composition_order`.
    #[serde(rename = "functions.composition_order")]
    FunctionsCompositionOrder,
    /// `functions.domain_range`.
    #[serde(rename = "functions.domain_range")]
    FunctionsDomainRange,
    /// `functions.inverse_not_reciprocal`.
    #[serde(rename = "functions.inverse_not_reciprocal")]
    FunctionsInverseNotReciprocal,
    /// `integrals.antiderivative_exponent`.
    #[serde(rename = "integrals.antiderivative_exponent")]
    IntegralsAntiderivativeExponent,
    /// `integrals.constant_of_integration`.
    #[serde(rename = "integrals.constant_of_integration")]
    IntegralsConstantOfIntegration,
    /// `integrals.setup_confusion`.
    #[serde(rename = "integrals.setup_confusion")]
    IntegralsSetupConfusion,
    /// `limits.divide_by_zero_form`.
    #[serde(rename = "limits.divide_by_zero_form")]
    LimitsDivideByZeroForm,
    /// `limits.infinity_indeterminate_form`.
    #[serde(rename = "limits.infinity_indeterminate_form")]
    LimitsInfinityIndeterminateForm,
    /// `limits.one_sided_limit`.
    #[serde(rename = "limits.one_sided_limit")]
    LimitsOneSidedLimit,
    /// `trig.identity_confusion`.
    #[serde(rename = "trig.identity_confusion")]
    TrigIdentityConfusion,
    /// `trig.unit_circle`.
    #[serde(rename = "trig.unit_circle")]
    TrigUnitCircle,
}

impl MisconceptionId {
    /// Every taxonomy ID, sorted by ID string.
    pub const ALL: &'static [Self] = &[
        Self::AlgebraDistributionError,
        Self::AlgebraExponentRule,
        Self::AlgebraLikeTerms,
        Self::AlgebraSignError,
        Self::DerivativesChainRule,
        Self::DerivativesImplicitDifferentiation,
        Self::DerivativesPowerRuleOvergeneralised,
        Self::DerivativesProductRule,
        Self::DerivativesQuotientRule,
        Self::FunctionsCompositionOrder,
        Self::FunctionsDomainRange,
        Self::FunctionsInverseNotReciprocal,
        Self::IntegralsAntiderivativeExponent,
        Self::IntegralsConstantOfIntegration,
        Self::IntegralsSetupConfusion,
        Self::LimitsDivideByZeroForm,
        Self::LimitsInfinityIndeterminateForm,
        Self::LimitsOneSidedLimit,
        Self::TrigIdentityConfusion,
        Self::TrigUnitCircle,
    ];

    /// The dotted taxonomy ID, e.g. `derivatives.chain_rule`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlgebraDistributionError => "algebra.distribution_error",
            Self::AlgebraExponentRule => "algebra.exponent_rule",
            Self::AlgebraLikeTerms => "algebra.like_terms",
            Self::AlgebraSignError => "algebra.sign_error",
            Self::DerivativesChainRule => "derivatives.chain_rule",
            Self::DerivativesImplicitDifferentiation => "derivatives.implicit_differentiation",
            Self::DerivativesPowerRuleOvergeneralised => "derivatives.power_rule_overgeneralised",
            Self::DerivativesProductRule => "derivatives.product_rule",
            Self::DerivativesQuotientRule => "derivatives.quotient_rule",
            Self::FunctionsCompositionOrder => "functions.composition_order",
            Self::FunctionsDomainRange => "functions.domain_range",
            Self::FunctionsInverseNotReciprocal => "functions.inverse_not_reciprocal",
            Self::IntegralsAntiderivativeExponent => "integrals.antiderivative_exponent",
            Self::IntegralsConstantOfIntegration => "integrals.constant_of_integration",
            Self::IntegralsSetupConfusion => "integrals.setup_confusion",
            Self::LimitsDivideByZeroForm => "limits.divide_by_zero_form",
            Self::LimitsInfinityIndeterminateForm => "limits.infinity_indeterminate_form",
            Self::LimitsOneSidedLimit => "limits.one_sided_limit",
            Self::TrigIdentityConfusion => "trig.identity_confusion",
            Self::TrigUnitCircle => "trig.unit_circle",
        }
    }
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
