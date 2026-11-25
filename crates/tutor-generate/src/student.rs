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
    /// The input problem's claimed answer/steps do not replay exactly.
    #[error("problem {problem_id} does not replay as verified: {detail}")]
    VerificationInvalid {
        /// Problem ID.
        problem_id: String,
        /// Verifier rejection detail.
        detail: String,
    },
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
    /// The problem declares no taxonomy IDs from which a correction can be selected.
    #[error("problem {0} declares no misconception IDs")]
    NoDeclaredMisconceptions(String),
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

/// Profile fields drawn from the attempt seed unless explicitly requested.
#[derive(Debug, Clone, Copy)]
struct DrawnProfile {
    correct: bool,
    level: StudentLevel,
    persistence: Persistence,
    surface_style: SurfaceStyle,
    has_notation_noise: bool,
}

/// Draw profile fields and simulate one seeded student attempt.
pub fn simulate(problem: &Problem, spec: &AttemptSpec) -> Result<StudentAttempt, StudentError> {
    if spec.teacher_variety {
        return Err(StudentError::TeacherVarietyUnavailable);
    }
    if problem.verification.is_none() {
        return Err(StudentError::UnverifiedProblem(problem.problem_id.clone()));
    }
    reverify_problem(problem)?;

    let mut rng = Pcg64::seed_from_u64(spec.seed);
    let drawn = draw_profile(spec, &mut rng);
    let correction = plant_correction(problem, spec.misconception, drawn.correct)?;
    let notation_noise = notation_noise_flags(drawn.has_notation_noise);
    let mut profile = StudentProfile {
        level: drawn.level,
        surface_style: drawn.surface_style,
        notation_noise: notation_noise.clone(),
    };
    let mut student_turns = student_turns(problem, drawn, correction.as_ref(), &notation_noise);
    reconcile_notation_noise(&notation_noise, &mut student_turns, &mut profile);

    let attempt = build_attempt(AttemptParts {
        problem,
        seed: spec.seed,
        drawn,
        correction,
        profile,
        student_turns,
    });
    attempt.validate()?;
    Ok(attempt)
}

fn reverify_problem(problem: &Problem) -> Result<(), StudentError> {
    let mut unverified = problem.clone();
    unverified.verification = None;
    tutor_verify::verify(&unverified)
        .map(|_| ())
        .map_err(|rejection| StudentError::VerificationInvalid {
            problem_id: problem.problem_id.clone(),
            detail: rejection.to_string(),
        })
}

struct PlantedCorrection {
    misconception: MisconceptionId,
    transformation: PlantedTransformation,
}

struct AttemptParts<'a> {
    problem: &'a Problem,
    seed: u64,
    drawn: DrawnProfile,
    correction: Option<PlantedCorrection>,
    profile: StudentProfile,
    student_turns: Vec<Turn>,
}

fn plant_correction(
    problem: &Problem,
    requested: Option<MisconceptionId>,
    correct: bool,
) -> Result<Option<PlantedCorrection>, StudentError> {
    if correct {
        return Ok(None);
    }
    let misconception = select_misconception(problem, requested)?;
    let transformation = plant(problem, misconception)?;
    Ok(Some(PlantedCorrection {
        misconception,
        transformation,
    }))
}

fn notation_noise_flags(has_notation_noise: bool) -> Vec<NotationNoise> {
    if has_notation_noise {
        vec![NotationNoise::DroppedMultiplicationDot]
    } else {
        Vec::new()
    }
}

fn student_turns(
    problem: &Problem,
    drawn: DrawnProfile,
    correction: Option<&PlantedCorrection>,
    notation_noise: &[NotationNoise],
) -> Vec<Turn> {
    if drawn.correct {
        return vec![opening_turn(
            problem,
            &correct_work(problem, drawn.surface_style, drawn.level),
            notation_noise,
        )];
    }
    correction_turns(
        problem,
        &correction
            .expect("incorrect attempts are planted")
            .transformation,
        drawn,
        notation_noise,
    )
}

fn reconcile_notation_noise(
    notation_noise: &[NotationNoise],
    student_turns: &mut [Turn],
    profile: &mut StudentProfile,
) {
    // Apply notation noise after composing the mathematical claim. It only
    // drops a multiplication dot; it cannot alter the claim's meaning.
    if notation_noise.contains(&NotationNoise::DroppedMultiplicationDot)
        && !drop_one_multiplication_dot(student_turns)
    {
        profile.notation_noise.clear();
    }
}

fn attempt_config_fingerprint(
    parts: &AttemptParts<'_>,
    misconception: Option<MisconceptionId>,
) -> u64 {
    let config = serde_json::json!({
        "correct": parts.drawn.correct,
        "misconception": misconception.map(MisconceptionId::as_str),
        "level": parts.drawn.level,
        "persistence": parts.drawn.persistence,
        "surface_style": parts.drawn.surface_style,
        "notation_noise": parts.profile.notation_noise,
    });
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in config.to_string().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn build_attempt(parts: AttemptParts<'_>) -> StudentAttempt {
    let fingerprint = attempt_config_fingerprint(
        &parts,
        parts
            .correction
            .as_ref()
            .map(|correction| correction.misconception),
    );
    let (misconception, transformation) = parts
        .correction
        .map(|correction| {
            (
                Some(correction.misconception),
                Some(correction.transformation),
            )
        })
        .unwrap_or((None, None));
    StudentAttempt {
        schema_version: STUDENT_ATTEMPT_SCHEMA_VERSION,
        attempt_id: format!(
            "{}/{}/v{}/seed-{}-{:016x}",
            parts.problem.problem_id, SIMULATOR_NAME, SIMULATOR_VERSION, parts.seed, fingerprint,
        ),
        problem_id: parts.problem.problem_id.clone(),
        topic: parts.problem.topic.clone(),
        correct: parts.drawn.correct,
        misconception,
        profile: parts.profile,
        persistence: parts.drawn.persistence,
        transformation,
        student_turns: parts.student_turns,
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
            seed: parts.seed,
            taxonomy_version: TAXONOMY_VERSION,
        },
    }
}

fn draw_profile(spec: &AttemptSpec, rng: &mut Pcg64) -> DrawnProfile {
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
    let has_notation_noise = rng.random_bool(0.05);
    DrawnProfile {
        correct,
        level,
        persistence,
        surface_style,
        has_notation_noise,
    }
}

fn correction_turns(
    problem: &Problem,
    planted: &PlantedTransformation,
    drawn: DrawnProfile,
    notation_noise: &[NotationNoise],
) -> Vec<Turn> {
    let wrong_work = wrong_work(problem, planted, drawn.surface_style, drawn.level);
    let correct_final = correct_work(problem, drawn.surface_style, drawn.level);
    let mut turns = vec![opening_turn(problem, &wrong_work, notation_noise)];
    // A correction dialogue always has a tutor diagnosis between the opening
    // attempt and L1. Since student/tutor turns strictly alternate, the student
    // answers that diagnosis before the first hint; persistence then controls
    // how many hint levels are needed before the correct retry.
    turns.push(retry_turn(&diagnosis_response(planted), notation_noise));
    match drawn.persistence {
        Persistence::CorrectsAfterL1 => {
            turns.push(retry_turn(
                &correct_response(&correct_final),
                notation_noise,
            ));
        }
        Persistence::CorrectsAfterL2 => {
            turns.push(retry_turn(&repeated_response(planted), notation_noise));
            turns.push(retry_turn(
                &correct_response(&correct_final),
                notation_noise,
            ));
        }
        Persistence::NeedsL3 => {
            turns.push(retry_turn(&repeated_response(planted), notation_noise));
            turns.push(retry_turn(
                &uncertain_repeat_response(planted),
                notation_noise,
            ));
            turns.push(retry_turn(
                &correct_response(&correct_final),
                notation_noise,
            ));
        }
    }
    turns
}

fn select_misconception(
    problem: &Problem,
    requested: Option<MisconceptionId>,
) -> Result<MisconceptionId, StudentError> {
    if let Some(id) = requested {
        return problem
            .misconception_ids
            .contains(&id)
            .then_some(id)
            .ok_or_else(|| StudentError::UndeclaredMisconception {
                problem_id: problem.problem_id.clone(),
                misconception: id,
            });
    }
    // Prefer the exact derivative-rule transformations. Any other declared ID
    // is attempted next so unsupported shapes still fail closed in `plant`.
    for id in [
        MisconceptionId::DerivativesProductRule,
        MisconceptionId::DerivativesChainRule,
    ] {
        if problem.misconception_ids.contains(&id) {
            return Ok(id);
        }
    }
    problem
        .misconception_ids
        .first()
        .copied()
        .ok_or_else(|| StudentError::NoDeclaredMisconceptions(problem.problem_id.clone()))
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
    nondegenerate(
        problem,
        PlantedTransformation {
            misconception: MisconceptionId::DerivativesProductRule,
            step_index: biting_step(problem, "add")?,
            step_kind: "add".to_owned(),
            correct_value: correct,
            corrupted_value: corrupted,
            propagates_to_answer: true,
            description: "replace the product-rule sum p'(a)q(a)+p(a)q'(a) with the product of derivatives p'(a)q'(a)".to_owned(),
        },
    )
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
    nondegenerate(
        problem,
        PlantedTransformation {
            misconception: MisconceptionId::DerivativesChainRule,
            step_index: biting_step(problem, "multiply")?,
            step_kind: "multiply".to_owned(),
            correct_value: correct,
            corrupted_value: corrupted,
            propagates_to_answer: true,
            description: "omit the inner-derivative factor q'(a), leaving only p'(q(a))".to_owned(),
        },
    )
}

fn nondegenerate(
    problem: &Problem,
    transformation: PlantedTransformation,
) -> Result<PlantedTransformation, StudentError> {
    if transformation.correct_value == transformation.corrupted_value {
        return Err(StudentError::IncompatibleMisconception {
            problem_id: problem.problem_id.clone(),
            misconception: transformation.misconception,
            reason: "the transformation is degenerate at this point and does not change the value"
                .to_owned(),
        });
    }
    Ok(transformation)
}

fn biting_step(problem: &Problem, expected_kind: &str) -> Result<usize, StudentError> {
    let answer = answer_value(problem);
    problem
        .steps
        .iter()
        .enumerate()
        .rev()
        .find(|(_, step)| step_kind(step) == expected_kind && step_value(step) == Some(answer))
        .map(|(index, _)| index)
        .ok_or_else(|| StudentError::IncompatibleMisconception {
            problem_id: problem.problem_id.clone(),
            misconception: match expected_kind {
                "add" => MisconceptionId::DerivativesProductRule,
                _ => MisconceptionId::DerivativesChainRule,
            },
            reason: format!(
                "the verified steps contain no final-answer {expected_kind} step where this transformation bites"
            ),
        })
}

fn step_kind(step: &Step) -> &'static str {
    match step {
        Step::DifferentiatePolynomial { .. } => "differentiate_polynomial",
        Step::EvaluatePolynomial { .. } => "evaluate_polynomial",
        Step::Multiply { .. } => "multiply",
        Step::Add { .. } => "add",
    }
}

fn step_value(step: &Step) -> Option<i64> {
    match step {
        Step::EvaluatePolynomial { value, .. }
        | Step::Multiply { value, .. }
        | Step::Add { value, .. } => Some(*value),
        Step::DifferentiatePolynomial { .. } => None,
    }
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
        if let Some(index) = student_work_dot_index(&turn.content) {
            turn.content.remove(index);
            return true;
        }
    }
    false
}

fn student_work_dot_index(content: &str) -> Option<usize> {
    const ATTEMPT_MARKER: &str = "My attempt: ";
    let work_start = content
        .find(ATTEMPT_MARKER)
        .map(|marker| marker + ATTEMPT_MARKER.len())
        .unwrap_or(0);
    implicit_multiplication_dot(&content[work_start..]).map(|index| work_start + index)
}

fn implicit_multiplication_dot(work: &str) -> Option<usize> {
    for (index, character) in work.char_indices() {
        if character != '·' {
            continue;
        }
        let before = work[..index].chars().next_back();
        let after = work[index + character.len_utf8()..].chars().next();
        if before.is_some_and(|c| !c.is_ascii_digit()) && after.is_some_and(|c| !c.is_ascii_digit())
        {
            return Some(index);
        }
    }
    None
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

fn verbose(style: SurfaceStyle, _level: StudentLevel) -> bool {
    // Surface style controls connective detail. Level already affects the
    // correctness mix in `draw_profile`; it must not override the style
    // metadata recorded on the attempt.
    style == SurfaceStyle::Verbose
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
        if coefficient != 0 {
            push_polynomial_term(&mut parts, power, coefficient);
        }
    }
    parts.join(" ")
}

fn push_polynomial_term(parts: &mut Vec<String>, power: usize, coefficient: i64) {
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
    let separator = match (parts.is_empty(), coefficient < 0) {
        (true, true) => "-",
        (true, false) => "",
        (false, true) => "- ",
        (false, false) => "+ ",
    };
    parts.push(format!("{separator}{body}"));
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

    struct ShellFixture {
        name: &'static str,
        topic: &'static str,
        parameters: Parameters,
        statement: String,
        answer: i64,
        steps: Vec<Step>,
        misconceptions: Vec<MisconceptionId>,
    }

    fn shell(fixture: ShellFixture) -> Problem {
        let ShellFixture {
            name,
            topic,
            parameters,
            statement,
            answer,
            steps,
            misconceptions,
        } = fixture;
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
        shell(ShellFixture {
            name: "chain_rule_at_point",
            topic: "calculus.derivatives.chain_rule",
            parameters: Parameters::ChainRuleAtPoint {
                outer: outer.clone(),
                inner: inner.clone(),
                point,
            },
            statement: "Let p(x) = x^2 and q(x) = 1 + 2x. If h(x) = p(q(x)), compute h'(1)."
                .to_owned(),
            answer: 12,
            steps: vec![
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
            misconceptions: vec![MisconceptionId::DerivativesChainRule],
        })
    }

    fn product_problem() -> Problem {
        let p = vec![1, 1];
        let q = vec![2, 1];
        let point = 1;
        shell(ShellFixture {
            name: "product_rule_at_point",
            topic: "calculus.derivatives.product_rule",
            parameters: Parameters::ProductRuleAtPoint {
                p: p.clone(),
                q: q.clone(),
                point,
            },
            statement: "Let p(x) = 1 + x and q(x) = 2 + x. If h(x) = p(x)·q(x), compute h'(1)."
                .to_owned(),
            answer: 5,
            steps: vec![
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
            misconceptions: vec![MisconceptionId::DerivativesProductRule],
        })
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
    fn fabricated_verification_records_do_not_trust_stale_answers() {
        let mut problem = chain_problem();
        problem.answer = Answer::Integer { value: 13 };
        let spec = AttemptSpec::seeded(12);
        assert!(matches!(
            simulate(&problem, &spec),
            Err(StudentError::VerificationInvalid { .. })
        ));
    }

    #[test]
    fn unsupported_taxonomy_ids_fail_closed() {
        let mut problem = chain_problem();
        problem.misconception_ids.extend([
            MisconceptionId::DerivativesPowerRuleOvergeneralised,
            MisconceptionId::DerivativesQuotientRule,
            MisconceptionId::DerivativesImplicitDifferentiation,
        ]);
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
            ));
        }
    }

    #[test]
    fn degenerate_transformation_is_rejected_instead_of_mislabeled() {
        let mut problem = chain_problem();
        let inner = vec![0, 1]; // q(x) = x, so q'(1) = 1 and omitting it changes nothing.
        problem.parameters = Parameters::ChainRuleAtPoint {
            outer: vec![0, 0, 1],
            inner: inner.clone(),
            point: 1,
        };
        problem.answer = Answer::Integer { value: 2 };
        problem.steps = vec![
            Step::EvaluatePolynomial {
                coefficients: inner,
                point: 1,
                value: 1,
            },
            Step::DifferentiatePolynomial {
                input: vec![0, 0, 1],
                output: vec![0, 2],
            },
            Step::DifferentiatePolynomial {
                input: vec![0, 1],
                output: vec![1],
            },
            Step::EvaluatePolynomial {
                coefficients: vec![0, 2],
                point: 1,
                value: 2,
            },
            Step::EvaluatePolynomial {
                coefficients: vec![1],
                point: 1,
                value: 1,
            },
            Step::Multiply {
                factors: vec![2, 1],
                value: 2,
            },
        ];
        let spec = AttemptSpec {
            correct: Some(false),
            misconception: Some(MisconceptionId::DerivativesChainRule),
            ..AttemptSpec::seeded(11)
        };
        assert!(matches!(
            simulate(&problem, &spec),
            Err(StudentError::IncompatibleMisconception { .. })
        ));
    }

    #[test]
    fn notation_noise_drops_only_implicit_multiplication_in_student_work() {
        let problem = product_problem();
        let mut turns = vec![opening_turn(&problem, "p′(u)·q′(1) = 12; 2·3 = 6.", &[])];
        assert!(drop_one_multiplication_dot(&mut turns));
        let content = &turns[0].content;
        assert!(content.contains("p(x)·q(x)"));
        assert!(content.contains("p′(u)q′(1) = 12"));
        assert!(content.contains("2·3 = 6"));
    }

    #[test]
    fn numeric_multiplication_is_not_claimed_as_notation_noise() {
        let problem = product_problem();
        let mut turns = vec![opening_turn(&problem, "1·1 = 1.", &[])];
        assert!(!drop_one_multiplication_dot(&mut turns));
        assert!(turns[0].content.contains("1·1 = 1."));
    }

    #[test]
    fn surface_style_controls_rendered_detail_at_every_level() {
        let problem = chain_problem();
        let base = AttemptSpec {
            correct: Some(true),
            level: Some(StudentLevel::Strong),
            ..AttemptSpec::seeded(43)
        };
        let terse = simulate(
            &problem,
            &AttemptSpec {
                surface_style: Some(SurfaceStyle::Terse),
                ..base.clone()
            },
        )
        .unwrap();
        let verbose = simulate(
            &problem,
            &AttemptSpec {
                surface_style: Some(SurfaceStyle::Verbose),
                ..base
            },
        )
        .unwrap();
        assert!(
            !terse.student_turns[0]
                .content
                .contains("Use the chain rule")
        );
        assert!(
            verbose.student_turns[0]
                .content
                .contains("Use the chain rule")
        );
    }

    #[test]
    fn attempt_id_distinguishes_forced_correctness_modes() {
        let problem = chain_problem();
        let base = AttemptSpec {
            seed: 42,
            level: Some(StudentLevel::Typical),
            persistence: Some(Persistence::CorrectsAfterL1),
            surface_style: Some(SurfaceStyle::Terse),
            ..AttemptSpec::seeded(42)
        };
        let correct = simulate(
            &problem,
            &AttemptSpec {
                correct: Some(true),
                ..base.clone()
            },
        )
        .unwrap();
        let correction = simulate(
            &problem,
            &AttemptSpec {
                correct: Some(false),
                misconception: Some(MisconceptionId::DerivativesChainRule),
                ..base
            },
        )
        .unwrap();
        assert_ne!(correct.attempt_id, correction.attempt_id);
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
