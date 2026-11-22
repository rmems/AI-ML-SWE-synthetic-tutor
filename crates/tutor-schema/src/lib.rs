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

/// Most worked-solution steps a problem may carry.
pub const MAX_STEPS: usize = 32;

/// Most misconception IDs a problem may carry.
pub const MAX_MISCONCEPTION_IDS: usize = 8;

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
    /// Ranges the parameters were drawn from, for difficulty grading.
    pub difficulty: Difficulty,
    /// The generator's answer. Untrusted until verified.
    pub answer: Answer,
    /// Worked solution steps; the last step's value must equal `answer`.
    ///
    /// Steps are machine-replayable: `tutor-verify` recomputes each step's output
    /// from its inputs and checks that every input traces to a declared parameter
    /// or an earlier step's output.
    pub steps: Vec<Step>,
    /// Taxonomy IDs of the misconceptions this problem can carry (from
    /// `schemas/taxonomy.yaml`).
    pub misconception_ids: Vec<MisconceptionId>,
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
    /// Evaluate `f'(point)` for `f(x) = a·x^m + b·x^n` with `m > n`.
    BinomialDerivativeAtPoint {
        /// Coefficient of the `x^m` term; nonzero.
        a: i64,
        /// Exponent of the leading term.
        m: u32,
        /// Coefficient of the `x^n` term; nonzero.
        b: i64,
        /// Exponent of the lower term; `n < m`.
        n: u32,
        /// Point at which to evaluate the derivative.
        point: i64,
    },
    /// Evaluate `f'(point)` for `f(x) = p(x)·q(x)` (product rule).
    ProductRuleAtPoint {
        /// Coefficients of `p` in ascending order of power; leading one nonzero.
        p: Vec<i64>,
        /// Coefficients of `q` in ascending order of power; leading one nonzero.
        q: Vec<i64>,
        /// Point at which to evaluate the derivative.
        point: i64,
    },
    /// Evaluate `f'(point)` for `f(x) = outer(inner(x))` (chain rule).
    ChainRuleAtPoint {
        /// Coefficients of the outer polynomial in ascending order; leading one
        /// nonzero.
        outer: Vec<i64>,
        /// Coefficients of the inner polynomial in ascending order; leading one
        /// nonzero.
        inner: Vec<i64>,
        /// Point at which to evaluate the derivative.
        point: i64,
    },
}

impl Parameters {
    /// The point every parameter kind evaluates at.
    pub fn point(&self) -> &i64 {
        match self {
            Parameters::PolynomialDerivativeAtPoint { point, .. }
            | Parameters::BinomialDerivativeAtPoint { point, .. }
            | Parameters::ProductRuleAtPoint { point, .. }
            | Parameters::ChainRuleAtPoint { point, .. } => point,
        }
    }
}

/// The ranges a generator drew from; recorded so difficulty is comparable
/// across problems of one family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Difficulty {
    /// Largest polynomial degree a draw may reach (per polynomial for
    /// multi-polynomial kinds).
    pub max_degree: u32,
    /// Inclusive bound on drawn coefficient magnitudes.
    pub coefficient_bound: u32,
    /// Inclusive bound on the evaluation point magnitude.
    pub point_bound: u32,
}

impl Difficulty {
    /// Every bound must be positive.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.max_degree == 0 || self.coefficient_bound == 0 || self.point_bound == 0 {
            return Err(invalid(
                "difficulty",
                "max_degree, coefficient_bound and point_bound must be positive",
            ));
        }
        Ok(())
    }
}

/// One worked-solution step, tagged by `kind`.
///
/// Steps chain: a verifier replays a problem by recomputing each step's output
/// from its inputs, and every polynomial input must equal a declared parameter
/// polynomial or the output of an earlier [`Step::DifferentiatePolynomial`],
/// every integer operand an earlier [`Step::EvaluatePolynomial`],
/// [`Step::Multiply`], or [`Step::Add`] output, or a declared integer parameter.
/// The final step's value is the claimed answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Differentiate a polynomial (ascending-order coefficients) term by term.
    DifferentiatePolynomial {
        /// Coefficients of the polynomial to differentiate; leading one nonzero.
        input: Vec<i64>,
        /// Coefficients of its derivative; leading one nonzero unless constant.
        output: Vec<i64>,
    },
    /// Evaluate a polynomial (ascending-order coefficients) at a point.
    EvaluatePolynomial {
        /// Coefficients of the polynomial; leading one nonzero.
        coefficients: Vec<i64>,
        /// Point at which to evaluate.
        point: i64,
        /// The value.
        value: i64,
    },
    /// Multiply integer factors drawn from earlier step outputs or parameters.
    Multiply {
        /// The factors, at least two.
        factors: Vec<i64>,
        /// Their product.
        value: i64,
    },
    /// Add integer addends drawn from earlier step outputs or parameters.
    Add {
        /// The addends, at least two.
        addends: Vec<i64>,
        /// Their sum.
        value: i64,
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

fn check_coefficients(field: &str, coefficients: &[i64]) -> Result<(), SchemaError> {
    // Field names are static per call site; only parameter arrays use the
    // parameters.* prefix while step arrays use steps[i].*
    if coefficients.is_empty() || coefficients.len() > MAX_COEFFICIENTS {
        return Err(SchemaError::Invalid {
            field: "parameters",
            reason: format!("{field} must have 1..={MAX_COEFFICIENTS} entries"),
        });
    }
    if coefficients.last() == Some(&0) {
        return Err(SchemaError::Invalid {
            field: "parameters",
            reason: format!("{field} leading coefficient must be nonzero"),
        });
    }
    Ok(())
}

/// Same as [`check_coefficients`] but permits the all-zero vector: a worked
/// step may evaluate the zero polynomial (the derivative of a constant).
fn check_step_coefficients(field: &str, coefficients: &[i64]) -> Result<(), SchemaError> {
    if coefficients.is_empty() || coefficients.len() > MAX_COEFFICIENTS {
        return Err(SchemaError::Invalid {
            field: "parameters",
            reason: format!("{field} must have 1..={MAX_COEFFICIENTS} entries"),
        });
    }
    if coefficients.last() == Some(&0) && coefficients.iter().any(|c| *c != 0) {
        return Err(SchemaError::Invalid {
            field: "parameters",
            reason: format!("{field} leading coefficient must be nonzero"),
        });
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
        self.validate_parameters()?;
        self.difficulty.validate()?;
        self.validate_difficulty_bounds()?;
        self.validate_steps()?;
        if self.misconception_ids.is_empty() || self.misconception_ids.len() > MAX_MISCONCEPTION_IDS
        {
            return Err(invalid(
                "misconception_ids",
                format!("must have 1..={MAX_MISCONCEPTION_IDS} entries"),
            ));
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

    fn validate_parameters(&self) -> Result<(), SchemaError> {
        match &self.parameters {
            Parameters::PolynomialDerivativeAtPoint { coefficients, .. } => {
                check_coefficients("parameters.coefficients", coefficients)
            }
            Parameters::BinomialDerivativeAtPoint { a, m, b, n, .. } => {
                validate_binomial(*a, *m, *b, *n)
            }
            Parameters::ProductRuleAtPoint { p, q, .. } => {
                check_coefficients("parameters.p", p)?;
                check_coefficients("parameters.q", q)
            }
            Parameters::ChainRuleAtPoint { outer, inner, .. } => {
                check_coefficients("parameters.outer", outer)?;
                check_coefficients("parameters.inner", inner)
            }
        }
    }

    /// Parameters must fit inside the `difficulty` bounds they declare.
    fn validate_difficulty_bounds(&self) -> Result<(), SchemaError> {
        let d = &self.difficulty;
        match &self.parameters {
            Parameters::PolynomialDerivativeAtPoint { coefficients, .. } => {
                check_polynomial_bounds("parameters.coefficients", coefficients, d)?;
            }
            Parameters::BinomialDerivativeAtPoint { a, m, b, point, .. } => {
                return check_binomial_bounds(*a, *m, *b, *point, d);
            }
            Parameters::ProductRuleAtPoint { p, q, .. } => {
                check_polynomial_bounds("parameters.p", p, d)?;
                check_polynomial_bounds("parameters.q", q, d)?;
            }
            Parameters::ChainRuleAtPoint { outer, inner, .. } => {
                check_polynomial_bounds("parameters.outer", outer, d)?;
                check_polynomial_bounds("parameters.inner", inner, d)?;
            }
        }
        check_point_bound(*self.parameters.point(), d)
    }

    fn validate_steps(&self) -> Result<(), SchemaError> {
        if self.steps.is_empty() || self.steps.len() > MAX_STEPS {
            return Err(invalid(
                "steps",
                format!("must have 1..={MAX_STEPS} entries"),
            ));
        }
        for (i, step) in self.steps.iter().enumerate() {
            validate_step(i, step)?;
        }
        Ok(())
    }
}

/// A parameter polynomial's degree and coefficient magnitudes must fit the
/// declared difficulty bounds.
fn check_polynomial_bounds(
    field: &str,
    coefficients: &[i64],
    d: &Difficulty,
) -> Result<(), SchemaError> {
    if coefficients.len().saturating_sub(1) > d.max_degree as usize {
        return Err(invalid(
            "parameters",
            format!(
                "{field} degree exceeds difficulty.max_degree {}",
                d.max_degree
            ),
        ));
    }
    for c in coefficients {
        if c.unsigned_abs() > u64::from(d.coefficient_bound) {
            return Err(invalid(
                "parameters",
                format!(
                    "{field} coefficient {c} exceeds difficulty.coefficient_bound {}",
                    d.coefficient_bound
                ),
            ));
        }
    }
    Ok(())
}

/// The binomial's exponent and coefficients must fit the declared difficulty
/// bounds.
fn check_binomial_bounds(
    a: i64,
    m: u32,
    b: i64,
    point: i64,
    d: &Difficulty,
) -> Result<(), SchemaError> {
    if m > d.max_degree {
        return Err(invalid(
            "parameters",
            format!(
                "exponent m={m} exceeds difficulty.max_degree {}",
                d.max_degree
            ),
        ));
    }
    for (name, c) in [("a", a), ("b", b)] {
        if c.unsigned_abs() > u64::from(d.coefficient_bound) {
            return Err(invalid(
                "parameters",
                format!(
                    "{name}={c} exceeds difficulty.coefficient_bound {}",
                    d.coefficient_bound
                ),
            ));
        }
    }
    check_point_bound(point, d)
}

/// The evaluation point must fit the declared difficulty bound.
fn check_point_bound(point: i64, d: &Difficulty) -> Result<(), SchemaError> {
    if point.unsigned_abs() > u64::from(d.point_bound) {
        return Err(invalid(
            "parameters",
            format!(
                "point {point} exceeds difficulty.point_bound {}",
                d.point_bound
            ),
        ));
    }
    Ok(())
}

fn validate_binomial(a: i64, m: u32, b: i64, n: u32) -> Result<(), SchemaError> {
    if a == 0 || b == 0 {
        return Err(invalid(
            "parameters",
            "binomial coefficients must be nonzero",
        ));
    }
    if m <= n {
        return Err(invalid(
            "parameters",
            format!("leading exponent {m} must exceed the lower exponent {n}"),
        ));
    }
    if m as usize >= MAX_COEFFICIENTS {
        return Err(invalid(
            "parameters.m",
            format!("exponent must be below {MAX_COEFFICIENTS}"),
        ));
    }
    Ok(())
}

fn validate_step(i: usize, step: &Step) -> Result<(), SchemaError> {
    match step {
        Step::DifferentiatePolynomial { input, output } => {
            check_coefficients(&format!("steps[{i}].input"), input)?;
            if output.is_empty() || output.len() > MAX_COEFFICIENTS {
                return Err(invalid(
                    "steps",
                    format!("steps[{i}].output must have 1..={MAX_COEFFICIENTS} entries"),
                ));
            }
        }
        Step::EvaluatePolynomial { coefficients, .. } => {
            check_step_coefficients(&format!("steps[{i}].coefficients"), coefficients)?;
        }
        Step::Multiply { factors, .. } => {
            if !(2..=MAX_COEFFICIENTS).contains(&factors.len()) {
                return Err(invalid(
                    "steps",
                    format!("steps[{i}].factors must have 2..={MAX_COEFFICIENTS} entries"),
                ));
            }
        }
        Step::Add { addends, .. } => {
            if !(2..=MAX_COEFFICIENTS).contains(&addends.len()) {
                return Err(invalid(
                    "steps",
                    format!("steps[{i}].addends must have 2..={MAX_COEFFICIENTS} entries"),
                ));
            }
        }
    }
    Ok(())
}
