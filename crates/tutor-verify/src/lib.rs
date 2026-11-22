//! Independent verification of generated problems.
//!
//! A generator's answer is never trusted. [`verify`] recomputes it in Rust with
//! checked integer arithmetic and replays every worked step, separately from the
//! generator that produced it. [`verify_remote`] asks the Julia math worker to do
//! the same over ZMQ, for problems generated on the Rust side of the boundary —
//! the check never runs on the side that made the claim. Both fail closed: any
//! mismatch, overflow, unsupported problem kind, replay failure, or
//! unreachable verifier is a [`Rejection`] with a stable [`ReasonCode`]. Only
//! problems returned by a verify call may feed dataset rows.
//!
//! The end-to-end Rust↔Julia proof is `tests/julia_bridge.rs`.

use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;
use tutor_schema::{Answer, Parameters, Problem, SchemaError, Step, Verification};

/// Name recorded in [`Verification::verifier`].
pub const VERIFIER: &str = "tutor-verify";

/// Method recorded when the check ran in this crate (Rust recompute).
pub const RUST_RECOMPUTE: &str = "rust_recompute_steps_replay";

/// Method recorded when the check ran in the Julia worker (`verify_answer` op).
pub const JULIA_RECOMPUTE: &str = "julia_verify_answer";

/// Stable reason codes for rejected problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    /// The problem is not schema-valid.
    SchemaInvalid,
    /// The problem already claims a verification; refuse to trust or overwrite it.
    AlreadyVerified,
    /// Recomputing an answer or step overflowed.
    Overflow,
    /// The recomputed answer differs from the stated one.
    AnswerMismatch,
    /// A worked step's inputs do not trace to parameters/earlier outputs, or its
    /// recorded output disagrees with the recomputation.
    StepReplay,
    /// The parameter kind is not one the verifier can check.
    UnknownParameterKind,
    /// The independent (remote) verifier could not be reached or could not run.
    VerifierUnavailable,
}

impl ReasonCode {
    /// The code as written to rejection logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SchemaInvalid => "verify_schema_invalid",
            Self::AlreadyVerified => "verify_already_verified",
            Self::Overflow => "verify_overflow",
            Self::AnswerMismatch => "verify_answer_mismatch",
            Self::StepReplay => "verify_step_replay",
            Self::UnknownParameterKind => "verify_unknown_parameter_kind",
            Self::VerifierUnavailable => "verify_verifier_unavailable",
        }
    }
}

/// Why a problem was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{problem_id}: {} ({detail})", .code.as_str())]
pub struct Rejection {
    /// ID of the rejected problem.
    pub problem_id: String,
    /// Stable reason code.
    pub code: ReasonCode,
    /// Human-readable detail.
    pub detail: String,
}

impl Rejection {
    fn new(problem: &Problem, code: ReasonCode, detail: impl Into<String>) -> Self {
        Self {
            problem_id: problem.problem_id.clone(),
            code,
            detail: detail.into(),
        }
    }
}

/// `p'(x)` for `p = Σ c_k x^k`, or `None` on overflow.
pub fn polynomial_derivative_at(coefficients: &[i64], x: i64) -> Option<i64> {
    // Horner on p'(x) = Σ_{k≥1} k c_k x^(k-1), highest power first.
    let mut acc: i64 = 0;
    for (k, &c) in coefficients.iter().enumerate().skip(1).rev() {
        let term = i64::try_from(k).ok()?.checked_mul(c)?;
        acc = acc.checked_mul(x)?.checked_add(term)?;
    }
    Some(acc)
}

/// `p(x)` for `p = Σ c_k x^k` (Horner), or `None` on overflow.
pub fn polynomial_value_at(coefficients: &[i64], x: i64) -> Option<i64> {
    let mut acc: i64 = 0;
    for &c in coefficients.iter().rev() {
        acc = acc.checked_mul(x)?.checked_add(c)?;
    }
    Some(acc)
}

/// Coefficients of `p'` for `p = Σ c_k x^k`, or `None` on overflow.
/// Returns `[0]` for a constant polynomial.
pub fn polynomial_derivative(coefficients: &[i64]) -> Option<Vec<i64>> {
    if coefficients.len() == 1 {
        return Some(vec![0]);
    }
    let mut out = Vec::with_capacity(coefficients.len() - 1);
    for (k, &c) in coefficients.iter().enumerate().skip(1) {
        out.push(i64::try_from(k).ok()?.checked_mul(c)?);
    }
    Some(out)
}

/// The answer the parameters imply, recomputed from scratch (no steps used).
fn expected_answer(parameters: &Parameters) -> Option<Answer> {
    let value = match parameters {
        Parameters::PolynomialDerivativeAtPoint {
            coefficients,
            point,
        } => polynomial_derivative_at(coefficients, *point)?,
        Parameters::BinomialDerivativeAtPoint { a, m, b, n, point } => {
            // f'(x) = a·m·x^(m-1) + b·n·x^(n-1); each term by checked pow/mul/add.
            let term = |coeff: i64, e: u32| -> Option<i64> {
                if e == 0 {
                    return Some(0);
                }
                let base = coeff.checked_mul(i64::from(e))?;
                base.checked_mul(point.checked_pow(e - 1)?)
            };
            term(*a, *m)?.checked_add(term(*b, *n)?)?
        }
        Parameters::ProductRuleAtPoint { p, q, point } => {
            // f'(a) = p'(a)·q(a) + p(a)·q'(a).
            let p_d = polynomial_derivative(p)?;
            let q_d = polynomial_derivative(q)?;
            let t1 =
                polynomial_value_at(&p_d, *point)?.checked_mul(polynomial_value_at(q, *point)?)?;
            let t2 =
                polynomial_value_at(p, *point)?.checked_mul(polynomial_value_at(&q_d, *point)?)?;
            t1.checked_add(t2)?
        }
        Parameters::ChainRuleAtPoint {
            outer,
            inner,
            point,
        } => {
            // f'(a) = outer'(inner(a)) · inner'(a).
            let u = polynomial_value_at(inner, *point)?;
            let outer_d = polynomial_derivative(outer)?;
            let inner_d = polynomial_derivative(inner)?;
            polynomial_value_at(&outer_d, u)?.checked_mul(polynomial_value_at(&inner_d, *point)?)?
        }
    };
    Some(Answer::Integer { value })
}

/// Polynomial inputs a step may legitimately consume, by parameter kind.
fn parameter_polynomials(parameters: &Parameters) -> Vec<Vec<i64>> {
    match parameters {
        Parameters::PolynomialDerivativeAtPoint { coefficients, .. } => {
            vec![coefficients.clone()]
        }
        Parameters::BinomialDerivativeAtPoint { a, m, b, n, .. } => {
            vec![dense_binomial(*a, *m, *b, *n)]
        }
        Parameters::ProductRuleAtPoint { p, q, .. } => vec![p.clone(), q.clone()],
        Parameters::ChainRuleAtPoint { outer, inner, .. } => {
            vec![outer.clone(), inner.clone()]
        }
    }
}

/// Dense ascending-order coefficients of `a·x^m + b·x^n` (`m > n`).
pub fn dense_binomial(a: i64, m: u32, b: i64, n: u32) -> Vec<i64> {
    let mut coefficients = vec![0i64; m as usize + 1];
    coefficients[m as usize] = a;
    coefficients[n as usize] = coefficients[n as usize].saturating_add(b);
    coefficients
}

/// Tracks what a step's inputs may legitimately reference: declared parameter
/// values plus outputs produced by earlier steps.
struct Replay {
    /// Polynomials usable as step inputs: declared parameters plus outputs of
    /// `differentiate_polynomial` steps.
    polynomials: Vec<Vec<i64>>,
    /// Integers declared by the parameters; usable as operands without limit.
    declared: HashSet<i64>,
    /// Counts of integers produced by `evaluate_polynomial`/`multiply`/`add`
    /// steps; each production may be consumed as an operand once.
    produced: HashMap<i64, u64>,
    /// The last produced integer (the running result chain head).
    last: Option<i64>,
    /// Index of the step being replayed, for error messages.
    step_index: usize,
}

impl Replay {
    fn new(problem: &Problem) -> Self {
        let mut declared = HashSet::new();
        match &problem.parameters {
            Parameters::PolynomialDerivativeAtPoint { point, .. }
            | Parameters::BinomialDerivativeAtPoint { point, .. }
            | Parameters::ProductRuleAtPoint { point, .. }
            | Parameters::ChainRuleAtPoint { point, .. } => {
                declared.insert(*point);
            }
        }
        if let Parameters::BinomialDerivativeAtPoint { a, m, b, n, .. } = &problem.parameters {
            declared.extend([*a, i64::from(*m), *b, i64::from(*n)]);
        }
        Self {
            polynomials: parameter_polynomials(&problem.parameters),
            declared,
            produced: HashMap::new(),
            last: None,
            step_index: 0,
        }
    }

    /// Record a polynomial produced by a step.
    fn produce_polynomial(&mut self, p: Vec<i64>) {
        self.polynomials.push(p);
    }

    /// Record an integer produced by a step.
    fn produce_integer(&mut self, v: i64) {
        *self.produced.entry(v).or_insert(0) += 1;
        self.last = Some(v);
    }

    /// Whether `coefficients` is a declared parameter or an earlier output.
    fn polynomial_known(&self, coefficients: &[i64]) -> bool {
        self.polynomials.iter().any(|p| p == coefficients)
    }

    /// Consume one use of `v` as an operand; declared parameters are always
    /// usable, produced values only as many times as they were produced.
    fn take_integer(&mut self, v: i64) -> bool {
        if self.declared.contains(&v) {
            return true;
        }
        match self.produced.get_mut(&v) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        }
    }

    /// `steps[i]` in error messages.
    fn step_prefix(&self) -> String {
        format!("steps[{}]", self.step_index)
    }

    fn replay_differentiate(&mut self, input: &[i64], output: &[i64]) -> Result<(), String> {
        let at = self.step_prefix();
        if !self.polynomial_known(input) {
            return Err(format!(
                "{at}: differentiate input {input:?} is not a declared parameter or earlier output"
            ));
        }
        let recomputed =
            polynomial_derivative(input).ok_or(format!("{at}: derivative overflowed"))?;
        if recomputed != output {
            return Err(format!(
                "{at}: derivative of {input:?} is {recomputed:?}, not {output:?}"
            ));
        }
        self.produce_polynomial(output.to_vec());
        Ok(())
    }

    fn replay_evaluate(
        &mut self,
        coefficients: &[i64],
        point: i64,
        value: i64,
    ) -> Result<(), String> {
        let at = self.step_prefix();
        if !self.polynomial_known(coefficients) {
            return Err(format!(
                "{at}: evaluate coefficients {coefficients:?} are not a declared parameter or earlier output"
            ));
        }
        // `point` must be a declared parameter or an earlier integer output.
        if !self.take_integer(point) {
            return Err(format!(
                "{at}: evaluation point {point} is not a declared parameter or earlier output"
            ));
        }
        let recomputed = polynomial_value_at(coefficients, point)
            .ok_or(format!("{at}: evaluation overflowed"))?;
        if recomputed != value {
            return Err(format!(
                "{at}: {coefficients:?} at {point} is {recomputed}, not {value}"
            ));
        }
        self.produce_integer(value);
        Ok(())
    }

    /// Replay a `multiply`/`add` step: consume each operand, recompute with
    /// checked `op` semantics, compare and produce `value`.
    fn replay_combine(
        &mut self,
        op: CombineOp,
        operands: &[i64],
        value: i64,
    ) -> Result<(), String> {
        let at = self.step_prefix();
        for operand in operands {
            if !self.take_integer(*operand) {
                return Err(format!(
                    "{at}: {} {operand} is not a declared parameter or earlier output",
                    op.operand_name()
                ));
            }
        }
        let recomputed = operands
            .iter()
            .try_fold(op.identity(), |acc, o| op.combine(acc, *o))
            .ok_or(format!("{at}: recomputation overflowed"))?;
        if recomputed != value {
            return Err(format!(
                "{at}: {}s {operands:?} combine to {recomputed}, not {value}",
                op.operand_name()
            ));
        }
        self.produce_integer(value);
        Ok(())
    }
}

/// Which integer-combining step a [`Step::Multiply`] or [`Step::Add`] is.
#[derive(Clone, Copy)]
enum CombineOp {
    Multiply,
    Add,
}

impl CombineOp {
    fn operand_name(self) -> &'static str {
        match self {
            CombineOp::Multiply => "factor",
            CombineOp::Add => "addend",
        }
    }

    fn identity(self) -> i64 {
        match self {
            CombineOp::Multiply => 1,
            CombineOp::Add => 0,
        }
    }

    fn combine(self, a: i64, b: i64) -> Option<i64> {
        match self {
            CombineOp::Multiply => a.checked_mul(b),
            CombineOp::Add => a.checked_add(b),
        }
    }
}

/// Replay `problem`'s worked steps: recompute each step's output from its inputs
/// and check every input traces to a declared parameter or an earlier output.
/// Returns the final step's value on success.
fn replay_steps(problem: &Problem) -> Result<i64, String> {
    let mut replay = Replay::new(problem);
    let mut saw_differentiation = false;
    for (i, step) in problem.steps.iter().enumerate() {
        replay_step(&mut replay, i, step)?;
        saw_differentiation |= matches!(step, Step::DifferentiatePolynomial { .. });
    }
    // Every topic is a derivative problem: a solution that never
    // differentiates cannot have shown the work for the answer.
    if !saw_differentiation {
        return Err("steps never differentiate a polynomial".to_owned());
    }
    // The worked solution must end on the answer: a trailing non-integer step
    // (e.g. a spare differentiation) leaves a result chain that never lands.
    if !matches!(
        problem.steps.last(),
        Some(Step::EvaluatePolynomial { .. } | Step::Multiply { .. } | Step::Add { .. })
    ) {
        return Err("steps must end on an integer-producing step".to_owned());
    }
    replay
        .last
        .ok_or_else(|| "no step produced an integer result".to_owned())
}

fn replay_step(replay: &mut Replay, i: usize, step: &Step) -> Result<(), String> {
    replay.step_index = i;
    match step {
        Step::DifferentiatePolynomial { input, output } => {
            replay.replay_differentiate(input, output)
        }
        Step::EvaluatePolynomial {
            coefficients,
            point,
            value,
        } => replay.replay_evaluate(coefficients, *point, *value),
        Step::Multiply { factors, value } => {
            replay.replay_combine(CombineOp::Multiply, factors, *value)
        }
        Step::Add { addends, value } => replay.replay_combine(CombineOp::Add, addends, *value),
    }
}

/// Check `problem` independently (schema, step replay, answer recompute) and
/// return it with [`Problem::verification`] set.
///
/// Use this for problems produced on the Julia side of the boundary; for
/// Rust-generated problems use [`verify_remote`] so the check runs in Julia.
pub fn verify(problem: &Problem) -> Result<Problem, Rejection> {
    check_unverified(problem)?;
    let mut problem = problem.clone();
    let expected = expected_answer(&problem.parameters).ok_or_else(|| {
        Rejection::new(
            &problem,
            ReasonCode::Overflow,
            "recomputing the answer overflows i64",
        )
    })?;
    if problem.answer != expected {
        return Err(Rejection::new(
            &problem,
            ReasonCode::AnswerMismatch,
            format!("stated {:?}, recomputed {expected:?}", problem.answer),
        ));
    }
    let last = replay_steps(&problem)
        .map_err(|detail| Rejection::new(&problem, ReasonCode::StepReplay, detail))?;
    let Answer::Integer { value } = expected;
    if last != value {
        return Err(Rejection::new(
            &problem,
            ReasonCode::StepReplay,
            format!("steps end at {last} but the answer is {value}"),
        ));
    }
    problem.verification = Some(Verification {
        method: RUST_RECOMPUTE.to_owned(),
        verifier: VERIFIER.to_owned(),
        verifier_version: env!("CARGO_PKG_VERSION").to_owned(),
    });
    Ok(problem)
}

/// Verify `problem` by asking the Julia math worker to recompute the answer and
/// replay the steps (the `verify_answer` op). This is the check for
/// Rust-generated problems: it never runs on the generator's own side of the
/// boundary. Fails closed — a worker error or unreachable worker rejects the
/// problem with [`ReasonCode::VerifierUnavailable`].
pub fn verify_remote(
    problem: &Problem,
    client: &mut tutor_wire::MathClient,
) -> Result<Problem, Rejection> {
    use tutor_wire::Verdict;

    check_unverified(problem)?;
    let mut problem = problem.clone();
    let verdict = client.verify_answer(&problem).map_err(|err| {
        Rejection::new(&problem, ReasonCode::VerifierUnavailable, err.to_string())
    })?;
    let Verdict {
        verified,
        reason,
        detail,
        expected,
    } = verdict;
    if !verified {
        let code = match reason.as_deref() {
            Some("verify_answer_mismatch") => ReasonCode::AnswerMismatch,
            Some("verify_step_replay") => ReasonCode::StepReplay,
            Some("verify_overflow") => ReasonCode::Overflow,
            Some("verify_unknown_parameter_kind") => ReasonCode::UnknownParameterKind,
            Some("verify_schema_invalid") | Some("malformed_problem") => ReasonCode::SchemaInvalid,
            Some("verify_already_verified") => ReasonCode::AlreadyVerified,
            // A verifier-side failure (internal_error or an unrecognized
            // code) is not the problem's fault.
            Some(_) | None => ReasonCode::VerifierUnavailable,
        };
        return Err(Rejection::new(&problem, code, detail));
    }
    // A passing verdict must carry the worker's independently computed
    // answer; `verified: true` without it proves nothing.
    let Some(expected) = expected else {
        return Err(Rejection::new(
            &problem,
            ReasonCode::VerifierUnavailable,
            "worker verdict omitted the recomputed answer",
        ));
    };
    let expected: i64 = serde_json::from_value(expected).map_err(|err| {
        Rejection::new(&problem, ReasonCode::VerifierUnavailable, err.to_string())
    })?;
    let Answer::Integer { value } = problem.answer;
    if expected != value {
        return Err(Rejection::new(
            &problem,
            ReasonCode::AnswerMismatch,
            format!("stated {value}, worker computed {expected}"),
        ));
    }
    problem.verification = Some(Verification {
        method: JULIA_RECOMPUTE.to_owned(),
        verifier: VERIFIER.to_owned(),
        verifier_version: env!("CARGO_PKG_VERSION").to_owned(),
    });
    Ok(problem)
}

fn check_unverified(problem: &Problem) -> Result<(), Rejection> {
    if let Err(err) = problem.validate() {
        return Err(Rejection::new(
            problem,
            ReasonCode::SchemaInvalid,
            schema_detail(&err),
        ));
    }
    if problem.verification.is_some() {
        return Err(Rejection::new(
            problem,
            ReasonCode::AlreadyVerified,
            "input already carries a verification block",
        ));
    }
    Ok(())
}

/// Pick the verification side opposite the generator: Julia-made problems are
/// checked here in Rust, Rust-made problems go to the Julia worker.
pub fn verify_auto(
    problem: &Problem,
    client: &mut tutor_wire::MathClient,
) -> Result<Problem, Rejection> {
    match problem.generator.language {
        tutor_schema::GeneratorLanguage::Julia => verify(problem),
        tutor_schema::GeneratorLanguage::Rust => verify_remote(problem, client),
    }
}

fn schema_detail(err: &SchemaError) -> String {
    format!("{}: {err}", err.code())
}

/// Aggregate of [`verify`] over a batch: verified problems and rejections,
/// preserving input order. Used by the pipeline.
pub fn verify_all(problems: Vec<Problem>) -> (Vec<Problem>, Vec<Rejection>) {
    let mut verified = Vec::new();
    let mut rejected = Vec::new();
    for problem in problems {
        match verify(&problem) {
            Ok(ok) => verified.push(ok),
            Err(err) => rejected.push(err),
        }
    }
    (verified, rejected)
}

/// Per-step-kind count of a problem, for run manifests.
pub fn step_kind_counts(problem: &Problem) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for step in &problem.steps {
        let kind = match step {
            Step::DifferentiatePolynomial { .. } => "differentiate_polynomial",
            Step::EvaluatePolynomial { .. } => "evaluate_polynomial",
            Step::Multiply { .. } => "multiply",
            Step::Add { .. } => "add",
        };
        *counts.entry(kind).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use tutor_schema::{Difficulty, GeneratorLanguage, GeneratorProvenance, MisconceptionId};

    fn problem(coefficients: Vec<i64>, point: i64, answer: i64) -> Problem {
        let derivative = polynomial_derivative(&coefficients).unwrap();
        Problem {
            schema_version: 1,
            problem_id: "p".into(),
            topic: "calculus.derivatives.power_rule".into(),
            generator: GeneratorProvenance {
                name: "polynomial_derivative_at_point".into(),
                version: 1,
                language: GeneratorLanguage::Julia,
                runtime_version: "1.13.4".into(),
                seed: 0,
            },
            statement: "s".into(),
            parameters: Parameters::PolynomialDerivativeAtPoint {
                coefficients: coefficients.clone(),
                point,
            },
            difficulty: Difficulty {
                max_degree: 4,
                coefficient_bound: 9,
                point_bound: 5,
            },
            answer: Answer::Integer { value: answer },
            steps: vec![
                Step::DifferentiatePolynomial {
                    input: coefficients,
                    output: derivative.clone(),
                },
                Step::EvaluatePolynomial {
                    coefficients: derivative,
                    point,
                    value: answer,
                },
            ],
            misconception_ids: vec![MisconceptionId::DerivativesPowerRuleOvergeneralised],
            verification: None,
        }
    }

    #[test]
    fn derivative_matches_hand_computation() {
        assert_eq!(polynomial_derivative_at(&[5, -2, 3], 2), Some(10));
        assert_eq!(polynomial_derivative_at(&[7], 3), Some(0));
        assert_eq!(polynomial_derivative_at(&[0, 0, 0, 1], -2), Some(12));
        assert_eq!(polynomial_derivative_at(&[1, 4, -9, 0, 2], -5), Some(-906));
        assert_eq!(polynomial_derivative_at(&[0, 0, i64::MAX], 2), None);
        assert_eq!(polynomial_value_at(&[5, -2, 3], 2), Some(13));
        assert_eq!(polynomial_derivative(&[5, -2, 3]), Some(vec![-2, 6]));
        assert_eq!(polynomial_derivative(&[7]), Some(vec![0]));
    }

    #[test]
    fn accepts_correct_answers() {
        let verified = verify(&problem(vec![5, -2, 3], 2, 10)).unwrap();
        let record = verified.verification.unwrap();
        assert_eq!(record.method, RUST_RECOMPUTE);
        assert_eq!(record.verifier, VERIFIER);
    }

    #[test]
    fn rejects_deliberately_wrong_answers() {
        // Typical planted errors: off by one, sign flip, forgetting the chain factor k,
        // and returning f(x) instead of f'(x).
        for wrong in [9, 11, -10, 4, 13] {
            let mut p = problem(vec![5, -2, 3], 2, wrong);
            // Keep the steps honest so the answer mismatch is what fires.
            p.steps[1] = Step::EvaluatePolynomial {
                coefficients: vec![-2, 6],
                point: 2,
                value: 10,
            };
            let rejection = verify(&p).unwrap_err();
            assert_eq!(rejection.code, ReasonCode::AnswerMismatch, "answer {wrong}");
        }
    }

    #[test]
    fn rejects_tampered_steps() {
        // Wrong intermediate value: the evaluate step's recorded value disagrees
        // with the recomputation.
        let mut p = problem(vec![5, -2, 3], 2, 10);
        p.steps[0] = Step::DifferentiatePolynomial {
            input: vec![5, -2, 3],
            output: vec![-2, 7],
        };
        assert_eq!(verify(&p).unwrap_err().code, ReasonCode::StepReplay);

        // An operand that never came from a parameter or earlier output.
        let mut p = problem(vec![5, -2, 3], 2, 10);
        p.steps.push(Step::Multiply {
            factors: vec![10, 99],
            value: 990,
        });
        assert_eq!(verify(&p).unwrap_err().code, ReasonCode::StepReplay);

        // Last step's value no longer equals the answer.
        let mut p = problem(vec![5, -2, 3], 2, 10);
        p.steps[1] = Step::EvaluatePolynomial {
            coefficients: vec![-2, 6],
            point: 2,
            value: 10,
        };
        p.answer = Answer::Integer { value: 11 };
        assert_eq!(verify(&p).unwrap_err().code, ReasonCode::AnswerMismatch);
    }

    #[test]
    fn product_rule_recompute() {
        let mut p = problem(vec![5, -2, 3], 2, 10);
        // f = p·q with p = [1, 2] (2x+1), q = [3, 1] (x+3), point 2:
        // p'=2, q'=1 → f'(2) = 2·5 + 5·1 = 15.
        p.parameters = Parameters::ProductRuleAtPoint {
            p: vec![1, 2],
            q: vec![3, 1],
            point: 2,
        };
        p.answer = Answer::Integer { value: 15 };
        p.steps = vec![
            Step::DifferentiatePolynomial {
                input: vec![1, 2],
                output: vec![2],
            },
            Step::DifferentiatePolynomial {
                input: vec![3, 1],
                output: vec![1],
            },
            Step::EvaluatePolynomial {
                coefficients: vec![2],
                point: 2,
                value: 2,
            },
            Step::EvaluatePolynomial {
                coefficients: vec![3, 1],
                point: 2,
                value: 5,
            },
            Step::EvaluatePolynomial {
                coefficients: vec![1, 2],
                point: 2,
                value: 5,
            },
            Step::EvaluatePolynomial {
                coefficients: vec![1],
                point: 2,
                value: 1,
            },
            Step::Multiply {
                factors: vec![2, 5],
                value: 10,
            },
            Step::Multiply {
                factors: vec![5, 1],
                value: 5,
            },
            Step::Add {
                addends: vec![10, 5],
                value: 15,
            },
        ];
        assert!(verify(&p).is_ok());
    }

    #[test]
    fn chain_rule_recompute() {
        let mut p = problem(vec![5, -2, 3], 2, 10);
        // f = outer∘inner, outer = u^2 ([0,0,1]), inner = x+1 ([1,1]), point 2:
        // inner(2)=3, outer'(u)=2u → 6, inner'=1 → f'(2) = 6.
        p.parameters = Parameters::ChainRuleAtPoint {
            outer: vec![0, 0, 1],
            inner: vec![1, 1],
            point: 2,
        };
        p.answer = Answer::Integer { value: 6 };
        p.steps = vec![
            Step::EvaluatePolynomial {
                coefficients: vec![1, 1],
                point: 2,
                value: 3,
            },
            Step::DifferentiatePolynomial {
                input: vec![0, 0, 1],
                output: vec![0, 2],
            },
            Step::EvaluatePolynomial {
                coefficients: vec![0, 2],
                point: 3,
                value: 6,
            },
            Step::DifferentiatePolynomial {
                input: vec![1, 1],
                output: vec![1],
            },
            Step::EvaluatePolynomial {
                coefficients: vec![1],
                point: 2,
                value: 1,
            },
            Step::Multiply {
                factors: vec![6, 1],
                value: 6,
            },
        ];
        assert!(verify(&p).is_ok());

        // Wrong chain-rule answer (the classic f'·g' error shape = 2).
        p.answer = Answer::Integer { value: 2 };
        assert_eq!(verify(&p).unwrap_err().code, ReasonCode::AnswerMismatch);
    }

    #[test]
    fn binomial_recompute() {
        // f(x) = 3x^4 - 2x^2, f'(2) = 3·4·8 - 2·2·2 = 96 - 8 = 88.
        let mut p = problem(vec![5, -2, 3], 2, 10);
        p.parameters = Parameters::BinomialDerivativeAtPoint {
            a: 3,
            m: 4,
            b: -2,
            n: 2,
            point: 2,
        };
        p.answer = Answer::Integer { value: 88 };
        p.steps = vec![
            Step::DifferentiatePolynomial {
                input: dense_binomial(3, 4, -2, 2),
                output: polynomial_derivative(&dense_binomial(3, 4, -2, 2)).unwrap(),
            },
            Step::EvaluatePolynomial {
                coefficients: polynomial_derivative(&dense_binomial(3, 4, -2, 2)).unwrap(),
                point: 2,
                value: 88,
            },
        ];
        assert!(verify(&p).is_ok());
        p.answer = Answer::Integer { value: 87 };
        assert_eq!(verify(&p).unwrap_err().code, ReasonCode::AnswerMismatch);
    }

    #[test]
    fn fails_closed() {
        // Derivative [0, 2·u32::MAX] fits, but evaluating it at x = u32::MAX overflows.
        // Loose declared bounds let the parameters validate; replay still fails closed.
        let mut overflowing = problem(vec![0, 0, i64::from(u32::MAX)], i64::from(u32::MAX), 0);
        overflowing.difficulty = Difficulty {
            max_degree: 4,
            coefficient_bound: u32::MAX,
            point_bound: u32::MAX,
        };
        let overflow = verify(&overflowing).unwrap_err();
        assert_eq!(overflow.code, ReasonCode::Overflow);
        let invalid = verify(&problem(vec![1, 0], 2, 0)).unwrap_err();
        assert_eq!(invalid.code, ReasonCode::SchemaInvalid);
        let verified = verify(&problem(vec![5, -2, 3], 2, 10)).unwrap();
        let twice = verify(&verified).unwrap_err();
        assert_eq!(twice.code, ReasonCode::AlreadyVerified);
        assert_eq!(twice.code.as_str(), "verify_already_verified");
    }
}
