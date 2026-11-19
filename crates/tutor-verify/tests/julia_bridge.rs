//! Rust↔Julia bridge proof: talks to a running `math-julia` worker over ZMQ REQ/REP,
//! checks every reply against the schema, and verifies every answer independently.
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

const GENERATOR: &str = "polynomial_derivative_at_point";

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
    assert!(info.generators.iter().any(|g| g == GENERATOR));

    for seed in 0..200 {
        let problem = client
            .generate_problem(GENERATOR, seed)
            .expect("schema-valid problem");
        assert_eq!(problem.generator.language, GeneratorLanguage::Julia);
        assert_eq!(problem.generator.runtime_version, info.julia_version);
        let verified = verify(problem.clone()).expect("julia answer verifies in rust");
        assert!(verified.verification.is_some());

        let again = client.generate_problem(GENERATOR, seed).unwrap();
        assert_eq!(again, problem, "seed {seed} must replay exactly");

        let mut tampered = problem;
        let Answer::Integer { value } = &mut tampered.answer;
        *value += 1;
        assert_eq!(
            verify(tampered).unwrap_err().code,
            ReasonCode::AnswerMismatch
        );
    }
}

#[test]
#[ignore = "needs a running math-julia worker"]
fn julia_worker_rejects_bad_requests() {
    let mut client = client();
    let err = client.generate_problem("no_such_generator", 1).unwrap_err();
    assert_eq!(err.code(), "unknown_generator");
    // The client must still be usable after an error reply.
    assert!(client.generate_problem(GENERATOR, 1).is_ok());
}
