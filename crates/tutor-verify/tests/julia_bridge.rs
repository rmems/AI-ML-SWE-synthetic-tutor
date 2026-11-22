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
use tutor_schema::{Answer, GeneratorLanguage};
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
