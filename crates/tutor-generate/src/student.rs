//! Procedural synthetic-student simulator.
//!
//! The simulator starts from a *verified* [`Problem`] and either renders its
//! verified solution as a correct student attempt, or applies one taxonomy
//! misconception as a defined transformation of the worked step where that
//! misconception bites. It never plants a random wrong answer: unsupported
//! taxonomy/problem pairs fail closed instead of producing a mislabeled row.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64;
use thiserror::Error;
use tutor_schema::{
    MisconceptionId, NamedVersion, NotationNoise, Parameters, Persistence, PlantedTransformation,
    Problem, Role, STUDENT_ATTEMPT_SCHEMA_VERSION, Step, StudentAttempt, StudentAttemptProvenance,
    StudentLevel, StudentProfile, SurfaceStyle, TAXONOMY_VERSION, TeacherVariety,
    TeacherVarietyStatus, Turn, TurnKind,
};

/// Simulator name written to provenance.
pub const SIMULATOR_NAME: &str = "synthetic-student-simulator";

/// Simulator version; bump when output for a seed changes.
pub const SIMULATOR_VERSION: u32 = 1;

/// Explicit inputs for one attempt. `None` profile fields are drawn from `seed`.
#[derive(Debug, Clone)]
pub struct AttemptSpec {
    /// Attempt-generation seed.
    pub seed: u64,
    /// Whether this is correct confirmation-row material. `None` draws a
    /// level-dependent mix: struggling students err more often than strong ones.
    pub correct: Option<bool>,
    /// Requested misconception. `None` selects the first transformation the
    /// problem can support from its declared taxonomy IDs.
    pub misconception: Option<MisconceptionId>,
    /// Requested student level.
    pub level: Option<StudentLevel>,
    /// Requested persistence behaviour.
    pub persistence: Option<Persistence>,
    /// Requested surface style.
    pub surface_style: Option<SurfaceStyle>,
    /// Request teacher surface variety. Teacher lanes are deliberately not
    /// wired here; `true` fails closed rather than silently mislabeling
    /// procedural output as teacher-varied.
    pub teacher_variety: bool,
}

impl AttemptSpec {
    /// A specification using only `seed` for deterministic draws.
    pub fn seeded(seed: u64) -> Self {
        Self {
            seed,
            correct: None,
            misconception: None,
            level: None,
            persistence: None,
            surface_style: None,
            teacher_variety: false,
        }
    }
}

/// Why an attempt cannot be emitted.
#[derive(Debug, Error)]
pub enum StudentError {
    /// The input problem has not passed independent verification.
    #[error("problem {0} has no verification record")]
    UnverifiedProblem(String),
    /// The requested taxonomy ID is not declared for this problem.
    #[error("{} is not declared for problem {}", .misconception.as_str(), .problem_id)]
    UndeclaredMisconception {
        /// Problem ID.
        problem_id: String,
        /// Requested taxonomy ID.
        misconception: MisconceptionId,
    },
    /// The taxonomy definition cannot bite on this problem's mathematical shape.
    #[error("{} cannot be planted faithfully on {}: {}", .misconception.as_str(), .problem_id, .reason)]
    IncompatibleMisconception {
        /// Problem ID.
        problem_id: String,
        /// Requested taxonomy ID.
        misconception: MisconceptionId,
        /// Why no exact transformation exists.
        reason: String,
    },
    /// Integer arithmetic overflow while constructing the corrupted attempt.
    #[error("attempt arithmetic overflows i64 for {0}")]
    Overflow(String),
    /// Teacher variety was requested but no allowed teacher client is wired.
    #[error(
        "teacher surface variety requested, but no allowed teacher lane is wired; procedural output was not mislabeled"
    )]
    TeacherVarietyUnavailable,
    /// The constructed artifact failed its own validation.
    #[error("constructed attempt is invalid: {0}")]
    InvalidAttempt(#[from] tutor_schema::SchemaError),
}

/// Draw profile fields and simulate one seeded student attempt.
pub fn simulate(problem: &Problem, spec: &AttemptSpec) -> Result<StudentAttempt, StudentError> {
    if spec.teacher_variety {
        return Err(StudentError::TeacherVarietyUnavailable);
    }
    if problem.verification.is_none() {
        return Err(StudentError::UnverifiedProblem(problem.problem_id.clone()));
    }

    let mut rng = Pcg64::seed_from_u64(spec.seed);
    let level = spec.level.unwrap_or_else(|| match rng.random_range(0..3) {
        0 => StudentLevel::Struggling,
        1 => StudentLevel::Typical,
        _ => StudentLevel::Strong,
    });
    let correct = spec.correct.unwrap_or_else(|| {
        let correct_probability = match level {
            StudentLevel::Struggling => 0.10,
            StudentLevel::Typical => 0.20,
            StudentLevel::Strong => 0.30,
        };
        rng.random_bool(correct_probability)
    });
    let persistence = spec
        .persistence
        .unwrap_or_else(|| match rng.random_range(0..3) {
            0 => Persistence::CorrectsAfterL1,
            1 => Persistence::CorrectsAfterL2,
            _ => Persistence::NeedsL3,
        });
    let surface_style = spec.surface_style.unwrap_or_else(|| {
        if rng.random_bool(0.5) {
            SurfaceStyle::Terse
        } else {
            SurfaceStyle::Verbose
        }
    });
    // Style noise is deliberately rare (5%) and always flagged in the artifact.
    let notation_noise = if rng.random_bool(0.05) {
        vec![NotationNoise::DroppedMultiplicationDot]
    } else {
        Vec::new()
    };
    let mut profile = StudentProfile {
        level,
        surface_style,
        notation_noise: notation_noise.clone(),
    };

    let mut transformation = None;
    let mut misconception = None;
    if !correct {
        let id = select_misconception(problem, spec.misconception)?;
        transformation = Some(plant(problem, id)?);
        misconception = Some(id);
    }

    let mut student_turns = if correct {
        vec![opening_turn(
            problem,
            &correct_work(problem, surface_style, level),
            &notation_noise,
        )]
    } else {
        let planted = transformation.as_ref().expect("checked above");
        let wrong_work = wrong_work(problem, planted, surface_style, level);
        let correct_final = correct_work(problem, surface_style, level);
        let mut turns = vec![opening_turn(problem, &wrong_work, &notation_noise)];
        // A correction dialogue always has a tutor diagnosis between the
        // opening attempt and L1. Since student/tutor turns strictly alternate,
        // the student answers that diagnosis before the first hint; persistence
        // then controls how many hint levels are needed before the correct retry.
        turns.push(retry_turn(&diagnosis_response(planted), &notation_noise));
        match persistence {
            Persistence::CorrectsAfterL1 => {
                turns.push(retry_turn(
                    &correct_response(&correct_final),
                    &notation_noise,
                ));
            }
            Persistence::CorrectsAfterL2 => {
                turns.push(retry_turn(&repeated_response(planted), &notation_noise));
                turns.push(retry_turn(
                    &correct_response(&correct_final),
                    &notation_noise,
                ));
            }
            Persistence::NeedsL3 => {
                turns.push(retry_turn(&repeated_response(planted), &notation_noise));
                turns.push(retry_turn(
                    &uncertain_repeat_response(planted),
                    &notation_noise,
                ));
                turns.push(retry_turn(
                    &correct_response(&correct_final),
                    &notation_noise,
                ));
            }
        }
        turns
    };

    // Apply notation noise after composing the mathematical claim. It only
    // drops a multiplication dot; it cannot alter the claim's meaning.
    if notation_noise.contains(&NotationNoise::DroppedMultiplicationDot)
        && !drop_one_multiplication_dot(&mut student_turns)
    {
        profile.notation_noise.clear();
    }

    let attempt = StudentAttempt {
        schema_version: STUDENT_ATTEMPT_SCHEMA_VERSION,
        attempt_id: format!(
            "{}/{}/v{}/seed-{}",
            problem.problem_id, SIMULATOR_NAME, SIMULATOR_VERSION, spec.seed
        ),
        problem_id: problem.problem_id.clone(),
        topic: problem.topic.clone(),
        correct,
        misconception,
        profile,
        persistence,
        transformation,
        student_turns,
        teacher_variety: TeacherVariety {
            requested: false,
            status: TeacherVarietyStatus::Disabled,
            teacher: None,
        },
        provenance: StudentAttemptProvenance {
            generator: NamedVersion {
                name: SIMULATOR_NAME.to_owned(),
                version: SIMULATOR_VERSION,
            },
            seed: spec.seed,
            taxonomy_version: TAXONOMY_VERSION,
        },
    };
    attempt.validate()?;
    Ok(attempt)
}

fn select_misconception(
    problem: &Problem,
    requested: Option<MisconceptionId>,
) -> Result<MisconceptionId, StudentError> {
    if let Some(id) = requested {
        if !problem.misconception_ids.contains(&id) {
            return Err(StudentError::UndeclaredMisconception {
                problem_id: problem.problem_id.clone(),
                misconception: id,
            });
        }
        return Ok(id);
    }
    // Prefer exact derivative-rule transformations; algebra.sign_error is the
    // fallback for simple polynomial evaluation because the taxonomy entry also
    // covers mishandled signs while distributing/evaluating a negative term.
    for id in [
        MisconceptionId::DerivativesProductRule,
        MisconceptionId::DerivativesChainRule,
    ] {
        if problem.misconception_ids.contains(&id) {
            return Ok(id);
        }
    }
    let id = problem
        .misconception_ids
        .first()
        .copied()
        .unwrap_or(MisconceptionId::AlgebraSignError);
    Ok(id)
}

fn plant(problem: &Problem, id: MisconceptionId) -> Result<PlantedTransformation, StudentError> {
    match id {
        MisconceptionId::DerivativesProductRule => plant_product_rule(problem),
        MisconceptionId::DerivativesChainRule => plant_chain_rule(problem),
        other => Err(StudentError::IncompatibleMisconception {
            problem_id: problem.problem_id.clone(),
            misconception: other,
            reason: match other {
                MisconceptionId::DerivativesPowerRuleOvergeneralised => {
                    "the verified problem is an ordinary constant-exponent polynomial, where the power rule is correct; the taxonomy definition needs an e^x-type or nonconstant-exponent expression".to_owned()
                }
                MisconceptionId::DerivativesQuotientRule => {
                    "no verified quotient/rational-function problem kind exists in the merged problem schema".to_owned()
                }
                MisconceptionId::DerivativesImplicitDifferentiation => {
                    "no verified implicit-equation problem kind exists in the merged problem schema".to_owned()
                }
                _ => "no procedural transformation is implemented for this taxonomy ID".to_owned(),
            },
        }),
    }
}

fn plant_product_rule(problem: &Problem) -> Result<PlantedTransformation, StudentError> {
    let Parameters::ProductRuleAtPoint { p, q, point } = &problem.parameters else {
        return incompatible(problem, MisconceptionId::DerivativesProductRule);
    };
    let point = *point;
    let p_d = tutor_verify::polynomial_derivative(p).ok_or_else(|| overflow(problem))?;
    let q_d = tutor_verify::polynomial_derivative(q).ok_or_else(|| overflow(problem))?;
    let p_da = tutor_verify::polynomial_value_at(&p_d, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let q_da = tutor_verify::polynomial_value_at(&q_d, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let pa = tutor_verify::polynomial_value_at(p, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let qa = tutor_verify::polynomial_value_at(q, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let corrupted = p_da.checked_mul(q_da).ok_or_else(|| overflow(problem))?;
    let correct = p_da
        .checked_mul(qa)
        .and_then(|first| {
            pa.checked_mul(q_da)
                .and_then(|second| first.checked_add(second))
        })
        .ok_or_else(|| overflow(problem))?;
    Ok(PlantedTransformation {
        misconception: MisconceptionId::DerivativesProductRule,
        step_index: problem.steps.len().saturating_sub(1),
        step_kind: "add".to_owned(),
        correct_value: correct,
        corrupted_value: corrupted,
        propagates_to_answer: true,
        description: "replace the product-rule sum p'(a)q(a)+p(a)q'(a) with the product of derivatives p'(a)q'(a)".to_owned(),
    })
}

fn plant_chain_rule(problem: &Problem) -> Result<PlantedTransformation, StudentError> {
    let Parameters::ChainRuleAtPoint {
        outer,
        inner,
        point,
    } = &problem.parameters
    else {
        return incompatible(problem, MisconceptionId::DerivativesChainRule);
    };
    let point = *point;
    let u = tutor_verify::polynomial_value_at(inner, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let outer_d = tutor_verify::polynomial_derivative(outer).ok_or_else(|| overflow(problem))?;
    let corrupted = tutor_verify::polynomial_value_at(&outer_d, tutor_verify::Scalar(u))
        .ok_or_else(|| overflow(problem))?;
    let inner_d = tutor_verify::polynomial_derivative(inner).ok_or_else(|| overflow(problem))?;
    let inner_da = tutor_verify::polynomial_value_at(&inner_d, tutor_verify::Scalar(point))
        .ok_or_else(|| overflow(problem))?;
    let correct = corrupted
        .checked_mul(inner_da)
        .ok_or_else(|| overflow(problem))?;
    Ok(PlantedTransformation {
        misconception: MisconceptionId::DerivativesChainRule,
        step_index: problem.steps.len().saturating_sub(1),
        step_kind: "multiply".to_owned(),
        correct_value: correct,
        corrupted_value: corrupted,
        propagates_to_answer: true,
        description: "omit the inner-derivative factor q'(a), leaving only p'(q(a))".to_owned(),
    })
}

fn incompatible<T>(problem: &Problem, misconception: MisconceptionId) -> Result<T, StudentError> {
    Err(StudentError::IncompatibleMisconception {
        problem_id: problem.problem_id.clone(),
        misconception,
        reason:
            "the problem's parameter shape does not contain the step where this misconception bites"
                .to_owned(),
    })
}

fn overflow(problem: &Problem) -> StudentError {
    StudentError::Overflow(problem.problem_id.clone())
}

fn answer_value(problem: &Problem) -> i64 {
    match problem.answer {
        tutor_schema::Answer::Integer { value } => value,
    }
}

fn drop_one_multiplication_dot(student_turns: &mut [Turn]) -> bool {
    for turn in student_turns {
        if let Some(index) = turn.content.find('·') {
            turn.content.remove(index);
            return true;
        }
    }
    false
}

fn opening_turn(problem: &Problem, work: &str, _noise: &[NotationNoise]) -> Turn {
    let mut content = format!("{}\n\nMy attempt: {work}", problem.statement);
    Turn {
        role: Role::Student,
        kind: Some(TurnKind::Attempt),
        content: std::mem::take(&mut content),
        hint_level: None,
        reveals_final_answer: false,
    }
}

fn retry_turn(content: &str, _noise: &[NotationNoise]) -> Turn {
    Turn {
        role: Role::Student,
        kind: Some(TurnKind::Attempt),
        content: content.to_owned(),
        hint_level: None,
        reveals_final_answer: false,
    }
}

fn diagnosis_response(transformation: &PlantedTransformation) -> String {
    format!(
        "I thought that was the right rule, so I still get {}.",
        transformation.corrupted_value
    )
}

fn repeated_response(transformation: &PlantedTransformation) -> String {
    format!(
        "I checked the same step and still get {}.",
        transformation.corrupted_value
    )
}

fn uncertain_repeat_response(transformation: &PlantedTransformation) -> String {
    format!(
        "I see which step you mean, but when I redo it I still get {}.",
        transformation.corrupted_value
    )
}

fn correct_response(correct_work: &str) -> String {
    format!("Let me redo it. {correct_work}")
}

fn wrong_work(
    problem: &Problem,
    transformation: &PlantedTransformation,
    style: SurfaceStyle,
    level: StudentLevel,
) -> String {
    match transformation.misconception {
        MisconceptionId::DerivativesProductRule => {
            product_rule_wrong_work(problem, transformation, style, level)
        }
        MisconceptionId::DerivativesChainRule => {
            chain_rule_wrong_work(problem, transformation, style, level)
        }
        _ => unreachable!("unsupported transformations fail before rendering"),
    }
}

fn correct_work(problem: &Problem, style: SurfaceStyle, level: StudentLevel) -> String {
    match &problem.parameters {
        Parameters::PolynomialDerivativeAtPoint { .. }
        | Parameters::BinomialDerivativeAtPoint { .. } => {
            simple_correct_work(problem, style, level)
        }
        Parameters::ProductRuleAtPoint { .. } => product_rule_correct_work(problem, style, level),
        Parameters::ChainRuleAtPoint { .. } => chain_rule_correct_work(problem, style, level),
    }
}

fn verbose(style: SurfaceStyle, level: StudentLevel) -> bool {
    // Level controls completeness independently of voice: strong students
    // explain even terse work, while struggling students usually stop at the
    // equation chain even when their connective style is verbose.
    level == StudentLevel::Strong
        || (style == SurfaceStyle::Verbose && level != StudentLevel::Struggling)
}

fn simple_correct_work(problem: &Problem, style: SurfaceStyle, level: StudentLevel) -> String {
    let Some(Step::DifferentiatePolynomial { output, .. }) = problem.steps.first() else {
        return String::new();
    };
    let point = *problem.parameters.point();
    let answer = answer_value(problem);
    let derivative = format_polynomial(output);
    if verbose(style, level) {
        format!(
            "Differentiate term by term with the power rule: f′(x) = {derivative}. Then substitute x = {point}, so f′({point}) = {answer}."
        )
    } else {
        format!("f′(x) = {derivative}, so f′({point}) = {answer}.")
    }
}

struct ProductParts {
    p_d: Vec<i64>,
    q_d: Vec<i64>,
    point: i64,
    pa: i64,
    qa: i64,
    p_da: i64,
    q_da: i64,
    t1: i64,
    t2: i64,
    sum: i64,
}

fn product_parts(problem: &Problem) -> Option<ProductParts> {
    let Parameters::ProductRuleAtPoint { p, q, point } = &problem.parameters else {
        return None;
    };
    let p_d = tutor_verify::polynomial_derivative(p)?;
    let q_d = tutor_verify::polynomial_derivative(q)?;
    let pa = tutor_verify::polynomial_value_at(p, tutor_verify::Scalar(*point))?;
    let qa = tutor_verify::polynomial_value_at(q, tutor_verify::Scalar(*point))?;
    let p_da = tutor_verify::polynomial_value_at(&p_d, tutor_verify::Scalar(*point))?;
    let q_da = tutor_verify::polynomial_value_at(&q_d, tutor_verify::Scalar(*point))?;
    let t1 = p_da.checked_mul(qa)?;
    let t2 = pa.checked_mul(q_da)?;
    let sum = t1.checked_add(t2)?;
    Some(ProductParts {
        p_d,
        q_d,
        point: *point,
        pa,
        qa,
        p_da,
        q_da,
        t1,
        t2,
        sum,
    })
}

fn product_rule_correct_work(
    problem: &Problem,
    style: SurfaceStyle,
    level: StudentLevel,
) -> String {
    let Some(parts) = product_parts(problem) else {
        return simple_correct_work(problem, style, level);
    };
    let ProductParts {
        p_d,
        q_d,
        point,
        pa,
        qa,
        p_da,
        q_da,
        t1,
        t2,
        sum,
    } = parts;
    let core = format!(
        "h′({point}) = p′({point})q({point}) + p({point})q′({point}) = {p_da}·{qa} + {pa}·{q_da} = {t1} + {t2} = {sum}."
    );
    if verbose(style, level) {
        format!(
            "Use the product rule. p′(x) = {} and q′(x) = {}. {core}",
            format_polynomial(&p_d),
            format_polynomial(&q_d)
        )
    } else {
        core
    }
}

fn product_rule_wrong_work(
    problem: &Problem,
    transformation: &PlantedTransformation,
    style: SurfaceStyle,
    level: StudentLevel,
) -> String {
    let Some(parts) = product_parts(problem) else {
        return format!("h′(x) = {}.", transformation.corrupted_value);
    };
    let ProductParts {
        p_d,
        q_d,
        point,
        p_da,
        q_da,
        ..
    } = parts;
    let core = format!(
        "p′({point}) = {p_da} and q′({point}) = {q_da}, so h′({point}) = {p_da}·{q_da} = {}.",
        transformation.corrupted_value
    );
    if verbose(style, level) {
        format!(
            "I differentiate each factor first: p′(x) = {} and q′(x) = {}. {core}",
            format_polynomial(&p_d),
            format_polynomial(&q_d)
        )
    } else {
        core
    }
}

struct ChainParts {
    outer_d: Vec<i64>,
    inner_d: Vec<i64>,
    point: i64,
    u: i64,
    outer_d_u: i64,
    inner_d_a: i64,
}

fn chain_parts(problem: &Problem) -> Option<ChainParts> {
    let Parameters::ChainRuleAtPoint {
        outer,
        inner,
        point,
    } = &problem.parameters
    else {
        return None;
    };
    let u = tutor_verify::polynomial_value_at(inner, tutor_verify::Scalar(*point))?;
    let outer_d = tutor_verify::polynomial_derivative(outer)?;
    let inner_d = tutor_verify::polynomial_derivative(inner)?;
    let outer_d_u = tutor_verify::polynomial_value_at(&outer_d, tutor_verify::Scalar(u))?;
    let inner_d_a = tutor_verify::polynomial_value_at(&inner_d, tutor_verify::Scalar(*point))?;
    Some(ChainParts {
        outer_d,
        inner_d,
        point: *point,
        u,
        outer_d_u,
        inner_d_a,
    })
}

fn chain_rule_correct_work(problem: &Problem, style: SurfaceStyle, level: StudentLevel) -> String {
    let Some(ChainParts {
        outer_d,
        inner_d,
        point,
        u,
        outer_d_u,
        inner_d_a,
    }) = chain_parts(problem)
    else {
        return simple_correct_work(problem, style, level);
    };
    let answer = answer_value(problem);
    let core = format!(
        "u = q({point}) = {u}; h′({point}) = p′(u)·q′({point}) = {outer_d_u}·{inner_d_a} = {answer}."
    );
    if verbose(style, level) {
        format!(
            "Use the chain rule. p′(x) = {} and q′(x) = {}. {core}",
            format_polynomial(&outer_d),
            format_polynomial(&inner_d)
        )
    } else {
        core
    }
}

fn chain_rule_wrong_work(
    problem: &Problem,
    transformation: &PlantedTransformation,
    style: SurfaceStyle,
    level: StudentLevel,
) -> String {
    let Some(ChainParts {
        outer_d, point, u, ..
    }) = chain_parts(problem)
    else {
        return format!("h′(x) = {}.", transformation.corrupted_value);
    };
    let core = format!(
        "u = q({point}) = {u}, so h′({point}) = p′(u) = {}.",
        transformation.corrupted_value
    );
    if verbose(style, level) {
        format!(
            "I differentiate the outside function: p′(x) = {}. {core}",
            format_polynomial(&outer_d)
        )
    } else {
        core
    }
}

/// Render ascending-order integer coefficients in descending powers.
pub fn format_polynomial(coefficients: &[i64]) -> String {
    if coefficients.iter().all(|c| *c == 0) {
        return "0".to_owned();
    }
    let mut parts = Vec::new();
    for (power, coefficient) in coefficients.iter().copied().enumerate().rev() {
        if coefficient == 0 {
            continue;
        }
        let variable = match power {
            0 => String::new(),
            1 => "x".to_owned(),
            _ => format!("x^{power}"),
        };
        let magnitude = coefficient.unsigned_abs();
        let body = if power > 0 && magnitude == 1 {
            variable
        } else {
            format!("{magnitude}{variable}")
        };
        if parts.is_empty() {
            parts.push(if coefficient < 0 {
                format!("-{body}")
            } else {
                body
            });
        } else {
            parts.push(if coefficient < 0 {
                format!("- {body}")
            } else {
                format!("+ {body}")
            });
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tutor_schema::{
        Answer, DIALOGUE_SCHEMA_VERSION, DialogueProvenance, DialogueRow, Difficulty,
        GeneratorLanguage, GeneratorProvenance, NamedVersion, RowSource, RowType,
        TUTOR_POLICY_VERSION, Verification,
    };

    fn verification() -> Option<Verification> {
        Some(Verification {
            method: "test".to_owned(),
            verifier: "test".to_owned(),
            verifier_version: "1".to_owned(),
        })
    }

    fn shell(
        name: &str,
        topic: &str,
        parameters: Parameters,
        statement: String,
        answer: i64,
        steps: Vec<Step>,
        misconceptions: Vec<MisconceptionId>,
    ) -> Problem {
        Problem {
            schema_version: 1,
            problem_id: format!("{name}/v1/seed-1"),
            topic: topic.to_owned(),
            generator: GeneratorProvenance {
                name: name.to_owned(),
                version: 1,
                language: GeneratorLanguage::Rust,
                runtime_version: "test".to_owned(),
                seed: 1,
            },
            statement,
            parameters,
            difficulty: Difficulty {
                max_degree: 2,
                coefficient_bound: 9,
                point_bound: 5,
            },
            answer: Answer::Integer { value: answer },
            steps,
            misconception_ids: misconceptions,
            verification: verification(),
        }
    }

    fn chain_problem() -> Problem {
        let outer = vec![0, 0, 1]; // x^2
        let inner = vec![1, 2]; // 1 + 2x
        let point = 1;
        let u = 3;
        let outer_d = vec![0, 2];
        let inner_d = vec![2];
        shell(
            "chain_rule_at_point",
            "calculus.derivatives.chain_rule",
            Parameters::ChainRuleAtPoint {
                outer: outer.clone(),
                inner: inner.clone(),
                point,
            },
            "Let p(x) = x^2 and q(x) = 1 + 2x. If h(x) = p(q(x)), compute h'(1).".to_owned(),
            12,
            vec![
                Step::EvaluatePolynomial {
                    coefficients: inner,
                    point,
                    value: u,
                },
                Step::DifferentiatePolynomial {
                    input: outer,
                    output: outer_d.clone(),
                },
                Step::DifferentiatePolynomial {
                    input: vec![1, 2],
                    output: inner_d.clone(),
                },
                Step::EvaluatePolynomial {
                    coefficients: outer_d,
                    point: u,
                    value: 6,
                },
                Step::EvaluatePolynomial {
                    coefficients: inner_d,
                    point,
                    value: 2,
                },
                Step::Multiply {
                    factors: vec![6, 2],
                    value: 12,
                },
            ],
            vec![MisconceptionId::DerivativesChainRule],
        )
    }

    fn product_problem() -> Problem {
        let p = vec![1, 1];
        let q = vec![2, 1];
        let point = 1;
        shell(
            "product_rule_at_point",
            "calculus.derivatives.product_rule",
            Parameters::ProductRuleAtPoint {
                p: p.clone(),
                q: q.clone(),
                point,
            },
            "Let p(x) = 1 + x and q(x) = 2 + x. If h(x) = p(x)·q(x), compute h'(1).".to_owned(),
            5,
            vec![
                Step::DifferentiatePolynomial {
                    input: p,
                    output: vec![1],
                },
                Step::DifferentiatePolynomial {
                    input: q,
                    output: vec![1],
                },
                Step::EvaluatePolynomial {
                    coefficients: vec![1, 1],
                    point,
                    value: 2,
                },
                Step::EvaluatePolynomial {
                    coefficients: vec![2, 1],
                    point,
                    value: 3,
                },
                Step::EvaluatePolynomial {
                    coefficients: vec![1],
                    point,
                    value: 1,
                },
                Step::EvaluatePolynomial {
                    coefficients: vec![1],
                    point,
                    value: 1,
                },
                Step::Multiply {
                    factors: vec![1, 3],
                    value: 3,
                },
                Step::Multiply {
                    factors: vec![2, 1],
                    value: 2,
                },
                Step::Add {
                    addends: vec![3, 2],
                    value: 5,
                },
            ],
            vec![MisconceptionId::DerivativesProductRule],
        )
    }

    #[test]
    fn chain_rule_plants_missing_inner_factor() {
        let problem = chain_problem();
        let spec = AttemptSpec {
            correct: Some(false),
            misconception: Some(MisconceptionId::DerivativesChainRule),
            level: Some(StudentLevel::Typical),
            persistence: Some(Persistence::CorrectsAfterL2),
            surface_style: Some(SurfaceStyle::Verbose),
            ..AttemptSpec::seeded(7)
        };
        let attempt = simulate(&problem, &spec).unwrap();
        let transformation = attempt.transformation.as_ref().unwrap();
        assert_eq!(transformation.corrupted_value, 6);
        assert_eq!(transformation.correct_value, 12);
        assert_eq!(attempt.student_turns.len(), 4);
        assert!(attempt.student_turns[0].content.contains("p′(u) = 6"));
        assert!(!attempt.student_turns[0].content.contains("q′(1)"));
    }

    #[test]
    fn product_rule_plants_product_of_derivatives() {
        let problem = product_problem();
        let spec = AttemptSpec {
            correct: Some(false),
            misconception: Some(MisconceptionId::DerivativesProductRule),
            persistence: Some(Persistence::CorrectsAfterL1),
            ..AttemptSpec::seeded(8)
        };
        let attempt = simulate(&problem, &spec).unwrap();
        let transformation = attempt.transformation.as_ref().unwrap();
        assert_eq!(transformation.corrupted_value, 1);
        assert_eq!(transformation.correct_value, 5);
        assert!(attempt.student_turns[0].content.contains("1·1 = 1"));
    }

    #[test]
    fn correct_mode_is_dialogue_schema_compatible() {
        let mut problem = chain_problem();
        problem
            .misconception_ids
            .push(MisconceptionId::AlgebraSignError);
        let spec = AttemptSpec {
            correct: Some(true),
            surface_style: Some(SurfaceStyle::Terse),
            ..AttemptSpec::seeded(9)
        };
        let attempt = simulate(&problem, &spec).unwrap();
        assert!(attempt.correct);
        assert_eq!(attempt.student_turns.len(), 1);
        assert!(attempt.student_turns[0].content.contains("= 12"));

        let row = DialogueRow {
            schema_version: DIALOGUE_SCHEMA_VERSION,
            row_id: "test-row".to_owned(),
            row_type: RowType::Confirmation,
            problem_id: problem.problem_id.clone(),
            topic: problem.topic.clone(),
            system_prompt_version: TUTOR_POLICY_VERSION,
            misconceptions: vec![],
            hint_levels_reached: vec![],
            turns: vec![
                Turn {
                    role: Role::System,
                    content: "system".to_owned(),
                    kind: None,
                    hint_level: None,
                    reveals_final_answer: false,
                },
                attempt.student_turns[0].clone(),
                Turn {
                    role: Role::Tutor,
                    content: "Confirmed: h′(1) = 12.".to_owned(),
                    kind: Some(TurnKind::Closing),
                    hint_level: None,
                    reveals_final_answer: true,
                },
            ],
            provenance: DialogueProvenance {
                source: RowSource::Generated,
                taxonomy_version: TAXONOMY_VERSION,
                generator: Some(NamedVersion {
                    name: "test".to_owned(),
                    version: 1,
                }),
            },
            gold: false,
        };
        row.validate().unwrap();
    }

    #[test]
    fn unsupported_taxonomy_ids_fail_closed() {
        let problem = chain_problem();
        for id in [
            MisconceptionId::DerivativesPowerRuleOvergeneralised,
            MisconceptionId::DerivativesQuotientRule,
            MisconceptionId::DerivativesImplicitDifferentiation,
        ] {
            let spec = AttemptSpec {
                correct: Some(false),
                misconception: Some(id),
                ..AttemptSpec::seeded(10)
            };
            assert!(matches!(
                simulate(&problem, &spec),
                Err(StudentError::IncompatibleMisconception { .. })
                    | Err(StudentError::UndeclaredMisconception { .. })
            ));
        }
    }

    #[test]
    fn seeded_output_replays() {
        let problem = chain_problem();
        let spec = AttemptSpec::seeded(42);
        assert_eq!(
            simulate(&problem, &spec).unwrap(),
            simulate(&problem, &spec).unwrap()
        );
    }
}
