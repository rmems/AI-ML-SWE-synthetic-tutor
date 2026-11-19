//! Independent verification of generated problems.
//!
//! A generator's answer is never trusted. [`verify`] recomputes it in Rust with checked
//! integer arithmetic, separately from the Julia generator that produced it, and fails
//! closed: any mismatch, overflow, or unsupported problem kind is a [`Rejection`] with a
//! stable [`ReasonCode`]. Only problems returned by [`verify`] may feed dataset rows.
//!
//! The end-to-end Rust↔Julia proof is `tests/julia_bridge.rs`.

use thiserror::Error;
use tutor_schema::{Answer, Parameters, Problem, SchemaError, Verification};

/// Name recorded in [`Verification::verifier`].
pub const VERIFIER: &str = "tutor-verify";

/// Method recorded for exact recomputation of integer answers.
pub const EXACT_INTEGER_RECOMPUTE: &str = "exact_integer_recompute";

/// Stable reason codes for rejected problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    /// The problem is not schema-valid.
    SchemaInvalid,
    /// The problem already claims a verification; refuse to trust or overwrite it.
    AlreadyVerified,
    /// Recomputing the answer overflowed.
    Overflow,
    /// The recomputed answer differs from the stated one.
    AnswerMismatch,
}

impl ReasonCode {
    /// The code as written to rejection logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SchemaInvalid => "verify_schema_invalid",
            Self::AlreadyVerified => "verify_already_verified",
            Self::Overflow => "verify_overflow",
            Self::AnswerMismatch => "verify_answer_mismatch",
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

/// `f'(x)` for `f = Σ c_k x^k`, or `None` on overflow.
pub fn polynomial_derivative_at(coefficients: &[i64], x: i64) -> Option<i64> {
    // Horner on f'(x) = Σ_{k≥1} k c_k x^(k-1), highest power first.
    let mut acc: i64 = 0;
    for (k, &c) in coefficients.iter().enumerate().skip(1).rev() {
        let term = i64::try_from(k).ok()?.checked_mul(c)?;
        acc = acc.checked_mul(x)?.checked_add(term)?;
    }
    Some(acc)
}

/// Check `problem` independently and return it with [`Problem::verification`] set.
pub fn verify(mut problem: Problem) -> Result<Problem, Rejection> {
    if let Err(err) = problem.validate() {
        return Err(Rejection::new(
            &problem,
            ReasonCode::SchemaInvalid,
            schema_detail(&err),
        ));
    }
    if problem.verification.is_some() {
        return Err(Rejection::new(
            &problem,
            ReasonCode::AlreadyVerified,
            "input already carries a verification block",
        ));
    }
    let (expected, method) = match &problem.parameters {
        Parameters::PolynomialDerivativeAtPoint {
            coefficients,
            point,
        } => {
            let Some(value) = polynomial_derivative_at(coefficients, *point) else {
                return Err(Rejection::new(
                    &problem,
                    ReasonCode::Overflow,
                    "derivative overflows i64",
                ));
            };
            (Answer::Integer { value }, EXACT_INTEGER_RECOMPUTE)
        }
    };
    if problem.answer != expected {
        return Err(Rejection::new(
            &problem,
            ReasonCode::AnswerMismatch,
            format!("stated {:?}, recomputed {expected:?}", problem.answer),
        ));
    }
    problem.verification = Some(Verification {
        method: method.to_owned(),
        verifier: VERIFIER.to_owned(),
        verifier_version: env!("CARGO_PKG_VERSION").to_owned(),
    });
    Ok(problem)
}

fn schema_detail(err: &SchemaError) -> String {
    format!("{}: {err}", err.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tutor_schema::{GeneratorLanguage, GeneratorProvenance};

    fn problem(coefficients: Vec<i64>, point: i64, answer: i64) -> Problem {
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
                coefficients,
                point,
            },
            answer: Answer::Integer { value: answer },
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
    }

    #[test]
    fn accepts_correct_answers() {
        let verified = verify(problem(vec![5, -2, 3], 2, 10)).unwrap();
        let record = verified.verification.unwrap();
        assert_eq!(record.method, EXACT_INTEGER_RECOMPUTE);
        assert_eq!(record.verifier, VERIFIER);
    }

    #[test]
    fn rejects_deliberately_wrong_answers() {
        // Typical planted errors: off by one, sign flip, forgetting the chain factor k,
        // and returning f(x) instead of f'(x).
        for wrong in [9, 11, -10, 4, 13] {
            let rejection = verify(problem(vec![5, -2, 3], 2, wrong)).unwrap_err();
            assert_eq!(rejection.code, ReasonCode::AnswerMismatch, "answer {wrong}");
        }
    }

    #[test]
    fn fails_closed() {
        let overflow = verify(problem(vec![0, 0, i64::MAX], 2, 0)).unwrap_err();
        assert_eq!(overflow.code, ReasonCode::Overflow);
        let invalid = verify(problem(vec![1, 0], 2, 0)).unwrap_err();
        assert_eq!(invalid.code, ReasonCode::SchemaInvalid);
        let verified = verify(problem(vec![5, -2, 3], 2, 10)).unwrap();
        let twice = verify(verified).unwrap_err();
        assert_eq!(twice.code, ReasonCode::AlreadyVerified);
        assert_eq!(twice.code.as_str(), "verify_already_verified");
    }
}
