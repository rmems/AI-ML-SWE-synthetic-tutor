//! Exercises `MathClient` against an in-process stand-in for the Julia worker: happy
//! path, worker errors, wrong replies, and timeout recovery. The stand-in is a ROUTER
//! socket so it can drop a request without replying, which a REP socket cannot.

use serde_json::{Value, json};
use std::thread;
use std::time::Duration;
use tutor_wire::{MathClient, PROTOCOL};

const PROBLEM: &str = include_str!("../../tutor-schema/tests/fixtures/problem.json");

/// Handle `requests` requests with `respond`; `None` drops the request unanswered.
fn spawn_worker(
    requests: usize,
    respond: impl Fn(&Value) -> Option<Value> + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let context = zmq::Context::new();
    let socket = context.socket(zmq::ROUTER).unwrap();
    socket.set_linger(0).unwrap();
    socket.set_rcvtimeo(10_000).unwrap();
    socket.bind("tcp://127.0.0.1:*").unwrap();
    let endpoint = socket.get_last_endpoint().unwrap().unwrap();
    let handle = thread::spawn(move || {
        let _context = context;
        for _ in 0..requests {
            let frames = socket.recv_multipart(0).unwrap();
            let [identity, empty, body] = frames.as_slice() else {
                panic!("unexpected REQ framing: {frames:?}");
            };
            assert!(empty.is_empty());
            let request: Value = serde_json::from_slice(body).unwrap();
            if let Some(reply) = respond(&request) {
                let reply = reply.to_string().into_bytes();
                socket
                    .send_multipart([identity.clone(), Vec::new(), reply], 0)
                    .unwrap();
            }
        }
    });
    (endpoint, handle)
}

fn ok(request: &Value, result: Value) -> Value {
    json!({"protocol": PROTOCOL, "request_id": request["request_id"], "status": "ok",
           "result": result})
}

fn problem() -> Value {
    serde_json::from_str(PROBLEM).unwrap()
}

fn info() -> Value {
    json!({"worker": "TutorMath", "worker_version": "0.1.0", "julia_version": "1.13.4",
           "protocol": PROTOCOL, "generators": ["polynomial_derivative_at_point"]})
}

#[test]
fn ping_and_generate() {
    let (endpoint, worker) = spawn_worker(2, |request| match request["op"].as_str() {
        Some("ping") => Some(ok(request, info())),
        Some("generate_problem") => {
            assert_eq!(request["seed"], json!(7));
            Some(ok(request, problem()))
        }
        other => panic!("unexpected op {other:?}"),
    });
    let mut client = MathClient::connect(&endpoint, Duration::from_secs(5)).unwrap();
    assert_eq!(client.ping().unwrap().worker, "TutorMath");
    let generated = client
        .generate_problem("polynomial_derivative_at_point", 7)
        .unwrap();
    assert_eq!(generated.generator.seed, 7);
    worker.join().unwrap();
}

#[test]
fn surfaces_worker_errors_and_bad_results() {
    let (endpoint, worker) = spawn_worker(4, |request| {
        Some(match request["seed"].as_u64() {
            Some(1) => json!({"protocol": PROTOCOL, "request_id": request["request_id"],
                              "status": "error",
                              "error": {"code": "unknown_generator", "message": "nope"}}),
            Some(2) => {
                let mut bad = problem();
                bad["answer"]["extra"] = json!(true);
                ok(request, bad)
            }
            Some(3) => ok(request, problem()), // seed 7, not 3
            _ => {
                let mut verified = problem();
                verified["verification"] =
                    json!({"method": "m", "verifier": "julia", "verifier_version": "1"});
                ok(request, verified)
            }
        })
    });
    let mut client = MathClient::connect(&endpoint, Duration::from_secs(5)).unwrap();
    let name = "polynomial_derivative_at_point";
    let code = |client: &mut MathClient, seed| {
        client
            .generate_problem(name, seed)
            .unwrap_err()
            .code()
            .to_owned()
    };
    assert_eq!(code(&mut client, 1), "unknown_generator");
    assert_eq!(code(&mut client, 2), "schema_shape");
    assert_eq!(code(&mut client, 3), "wire_malformed_reply");
    assert_eq!(code(&mut client, 7), "wire_malformed_reply");
    worker.join().unwrap();
}

#[test]
fn recovers_after_timeout() {
    let (endpoint, worker) = spawn_worker(2, |request| {
        (request["request_id"] != json!("req-1")).then(|| ok(request, info()))
    });
    let mut client = MathClient::connect(&endpoint, Duration::from_millis(300)).unwrap();
    assert_eq!(client.ping().unwrap_err().code(), "wire_timeout");
    assert_eq!(client.ping().unwrap().protocol, PROTOCOL);
    worker.join().unwrap();
}

#[test]
fn rejects_sub_millisecond_timeouts() {
    for timeout in [Duration::ZERO, Duration::from_micros(500)] {
        let err = MathClient::connect("tcp://127.0.0.1:1", timeout)
            .err()
            .unwrap();
        assert_eq!(err.code(), "wire_invalid_request");
    }
}
