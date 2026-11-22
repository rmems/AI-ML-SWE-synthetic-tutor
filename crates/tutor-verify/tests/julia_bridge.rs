//! Rust↔Julia bridge proof: talks to a running `math-julia` worker over ZMQ REQ/REP,
//! checks every reply against the schema, and verifies every answer independently.
//! Julia-generated problems are checked here in Rust; a tampered Rust-side problem
//! is sent to the worker's `verify_answer` op so a wrong answer is caught on the
//! other side of the boundary.
//!
//! Ignored by default because it needs the worker. Start it, then run the test:
//!
//! ```sh
//! julia --project=math-julia math-julia/bin/serve.jl &
//! cargo test -p tutor-verify --test julia_bridge -- --ignored
//! ```
//!
//! `TUTOR_MATH_ENDPOINT` overrides the endpoint for both sides.

use std::time::Duration;
use tutor_schema::{
    Answer, Difficulty, GeneratorLanguage, GeneratorProvenance, MisconceptionId, Parameters,
    Problem, Step,
};
use tutor_verify::{ReasonCode, verify};
use tutor_wire::{MathClient, PROTOCOL};

const GENERATORS: [&str; 3] = [
    "polynomial_derivative_at_point",
    "product_rule_at_point",
    "chain_rule_at_point",
];

fn client() -> MathClient {
    MathClient::connect_from_env(Duration::from_secs(30)).expect("valid endpoint")
}

#[test]
#[ignore = "needs a running math-julia worker"]
fn julia_worker_round_trip() {
    let mut client = client();
    let info = client.ping().expect("worker answers ping");
    assert_eq!(info.worker, "TutorMath");
    assert_eq!(info.protocol, PROTOCOL);
    for g in GENERATORS {
        assert!(info.generators.iter().any(|x| x == g), "missing {g}");
    }

    for generator in GENERATORS {
        for seed in 0..50 {
            let problem = client
                .generate_problem(generator, seed)
                .expect("schema-valid problem");
            assert_eq!(problem.generator.language, GeneratorLanguage::Julia);
            assert_eq!(problem.generator.runtime_version, info.julia_version);
            let verified = verify(&problem).expect("julia answer verifies in rust");
            assert!(verified.verification.is_some());

            let again = client.generate_problem(generator, seed).unwrap();
            assert_eq!(again, problem, "seed {seed} must replay exactly");

            let mut tampered = problem;
            let Answer::Integer { value } = &mut tampered.answer;
            *value += 1;
            assert_eq!(
                verify(&tampered).unwrap_err().code,
                ReasonCode::AnswerMismatch
            );
        }
    }
}

#[test]
#[ignore = "needs a running math-julia worker"]
fn julia_verify_answer_round_trip() {
    // A Rust-side problem verified by the worker: the check crosses the boundary.
    let mut client = client();
    let mut problem = client
        .generate_problem("polynomial_derivative_at_point", 11)
        .expect("problem");
    problem.generator.language = GeneratorLanguage::Rust;

    let verified =
        tutor_verify::verify_remote(&problem, &mut client).expect("worker verifies good answer");
    assert_eq!(
        verified.verification.unwrap().method,
        tutor_verify::JULIA_RECOMPUTE
    );

    // Deliberately wrong answer must be caught on the Julia side.
    let mut wrong = problem.clone();
    let Answer::Integer { value } = &mut wrong.answer;
    *value += 1;
    let rejection = tutor_verify::verify_remote(&wrong, &mut client).unwrap_err();
    assert_eq!(rejection.code, ReasonCode::AnswerMismatch);

    // Tampered step is caught too.
    let mut bad_step = problem;
    let tutor_schema::Step::EvaluatePolynomial { value, .. } = &mut bad_step.steps[1] else {
        panic!("unexpected step shape");
    };
    *value += 1;
    let rejection = tutor_verify::verify_remote(&bad_step, &mut client).unwrap_err();
    assert!(
        matches!(
            rejection.code,
            ReasonCode::StepReplay | ReasonCode::AnswerMismatch
        ),
        "{rejection:?}"
    );

    // The Rust-native binomial parameter kind is verified by the worker too:
    // the same shape tutor-generate emits, not a relabeled Julia problem.
    let binomial = rust_binomial_problem(((3, 4), (-2, 2)), 1);
    tutor_verify::verify_remote(&binomial, &mut client)
        .expect("worker verifies rust binomial problem");

    // An out-of-schema exponent must reject, never reach the worker's dense
    // binomial allocation.
    let mut huge = rust_binomial_problem(((1, 2), (1, 1)), 0);
    let Parameters::BinomialDerivativeAtPoint { m, .. } = &mut huge.parameters else {
        unreachable!()
    };
    *m = 1_000_000;
    let rejection = tutor_verify::verify_remote(&huge, &mut client).unwrap_err();
    assert_eq!(rejection.code, ReasonCode::SchemaInvalid);

    // A raw wire call fails closed too: the client validates before sending.
    // The worker's own dense_binomial bound is exercised in math-julia's
    // runtests.jl (only non-tutor-wire clients can reach it).
    let direct = client.verify_answer(&huge).unwrap_err();
    assert!(
        direct.to_string().contains("exponent must be below"),
        "{direct:?}"
    );
}

/// A problem of the same shape `tutor-generate`'s Rust binomial generator
/// emits (kind `binomial_derivative_at_point`, `language = "rust"`).
fn rust_binomial_problem(terms: ((i64, u32), (i64, u32)), point: i64) -> Problem {
    let ((a, m), (b, n)) = terms;
    let dense = tutor_verify::dense_binomial(a, m, b, n);
    let derivative = tutor_verify::polynomial_derivative(&dense).unwrap();
    let value = tutor_verify::polynomial_value_at(&derivative, point).unwrap();
    Problem {
        schema_version: 1,
        problem_id: format!("binomial_derivative_at_point/v1/seed-{a}-{m}-{b}-{n}"),
        topic: "calculus.derivatives.power_rule".into(),
        generator: GeneratorProvenance {
            name: "binomial_derivative_at_point".into(),
            version: 1,
            language: GeneratorLanguage::Rust,
            runtime_version: "test".into(),
            seed: 0,
        },
        statement: format!("Let f(x) = {a}x^{m} + {b}x^{n}. Compute f'({point})."),
        parameters: Parameters::BinomialDerivativeAtPoint { a, m, b, n, point },
        difficulty: Difficulty {
            max_degree: 6,
            coefficient_bound: 9,
            point_bound: 5,
        },
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
        misconception_ids: vec![MisconceptionId::DerivativesPowerRuleOvergeneralised],
        verification: None,
    }
}

#[test]
#[ignore = "needs a running math-julia worker"]
fn julia_worker_rejects_bad_requests() {
    let mut client = client();
    let err = client.generate_problem("no_such_generator", 1).unwrap_err();
    assert_eq!(err.code(), "unknown_generator");
    // The client must still be usable after an error reply.
    assert!(client.generate_problem(GENERATORS[0], 1).is_ok());
}
