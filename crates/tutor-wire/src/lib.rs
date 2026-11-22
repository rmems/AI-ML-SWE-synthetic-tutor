//! Rust side of the Rust↔Julia bridge.
//!
//! [`MathClient`] is a ZMQ REQ client for the `math-julia` worker (`TutorMath.serve`).
//! Every request and reply is one JSON message in the `tutor-math/1` envelope:
//!
//! ```text
//! -> {"protocol": "tutor-math/1", "request_id": "req-1", "op": "generate_problem",
//!     "generator": "polynomial_derivative_at_point", "seed": 7}
//! <- {"protocol": "tutor-math/1", "request_id": "req-1", "status": "ok", "result": {...}}
//! <- {"protocol": "tutor-math/1", "request_id": "req-1", "status": "error",
//!     "error": {"code": "invalid_seed", "message": "..."}}
//! ```
//!
//! Replies are checked strictly: protocol, request ID echo, envelope shape, and (for
//! problems) the `problem.schema.json` constraints. A REQ socket that misses a reply is
//! stuck, so after any send/receive failure the client closes and reopens its socket.
//! Answers from the worker are still untrusted; `tutor-verify` checks them.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;
use tutor_schema::{MAX_SEED, Problem, SchemaError};

/// Wire protocol identifier; must match `TutorMath.PROTOCOL`.
pub const PROTOCOL: &str = "tutor-math/1";

/// Default worker endpoint; must match `TutorMath.DEFAULT_ENDPOINT`.
pub const DEFAULT_ENDPOINT: &str = "tcp://127.0.0.1:5557";

/// Environment variable that overrides [`DEFAULT_ENDPOINT`] on both sides.
pub const ENDPOINT_ENV: &str = "TUTOR_MATH_ENDPOINT";

/// Largest reply accepted, in bytes; ZMQ drops larger messages.
pub const MAX_REPLY_BYTES: i64 = 1 << 20;

/// Default per-message send/receive timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Operations the worker supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Health check; returns [`WorkerInfo`].
    Ping,
    /// Generate one problem from a seeded generator; returns a [`Problem`].
    GenerateProblem {
        /// Generator name, e.g. `polynomial_derivative_at_point`.
        generator: String,
        /// Seed in `0..=MAX_SEED`.
        seed: u64,
    },
    /// Independently check a problem's answer and replay its worked steps;
    /// returns a [`Verdict`]. Used for Rust-generated problems so the check
    /// runs on the other side of the Rust/Julia boundary from the generator.
    VerifyAnswer {
        /// The full candidate problem (in `problem.schema.json` shape).
        problem: Box<Problem>,
    },
}

/// The worker's verdict on a `verify_answer` request.
///
/// `verified` is true only when the worker recomputed the answer from the
/// parameters, replayed every worked step, and found no disagreement. On
/// failure `reason` carries a stable code (`answer_mismatch`, `step_replay`,
/// `overflow`, `unknown_parameter_kind`) and `expected` the recomputed answer
/// when one could be computed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    /// Whether the answer and steps survived independent checking.
    pub verified: bool,
    /// Failure reason code when `verified` is false; null otherwise.
    pub reason: Option<String>,
    /// Human-readable detail (empty when verified).
    pub detail: String,
    /// The answer the worker computed independently, when computable.
    pub expected: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct RequestEnvelope<'a> {
    protocol: &'static str,
    request_id: &'a str,
    #[serde(flatten)]
    op: &'a Op,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Error,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyEnvelope {
    protocol: String,
    request_id: Option<String>,
    status: Status,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RemoteError>,
}

/// An error reply from the worker.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteError {
    /// Worker reason code, e.g. `invalid_seed` or `unknown_generator`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

/// Reply to [`Op::Ping`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInfo {
    /// Worker package name (`TutorMath`).
    pub worker: String,
    /// Worker package version.
    pub worker_version: String,
    /// Julia `VERSION` of the worker.
    pub julia_version: String,
    /// Protocol the worker speaks.
    pub protocol: String,
    /// Generator names the worker offers, sorted.
    pub generators: Vec<String>,
}

/// Why a request failed.
#[derive(Debug, Error)]
pub enum WireError {
    /// A ZMQ call failed.
    #[error("zmq error: {0}")]
    Zmq(#[from] zmq::Error),
    /// No reply (or send) within the timeout; the socket has been reset.
    #[error("no reply from the math worker within {0:?}")]
    Timeout(Duration),
    /// The request was refused before sending.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The reply is not a valid `tutor-math/1` envelope.
    #[error("malformed reply: {0}")]
    MalformedReply(String),
    /// The reply names a different protocol.
    #[error("protocol mismatch: expected {PROTOCOL}, got {0:?}")]
    ProtocolMismatch(String),
    /// The reply does not echo our request ID.
    #[error("request_id mismatch: expected {expected:?}, got {got:?}")]
    RequestIdMismatch {
        /// ID we sent.
        expected: String,
        /// ID in the reply.
        got: Option<String>,
    },
    /// The worker answered with an error.
    #[error("worker error {}: {}", .0.code, .0.message)]
    Remote(RemoteError),
    /// The `result` is not schema-valid.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl WireError {
    /// Stable reason code for rejection logs. Worker errors keep the worker's code.
    pub fn code(&self) -> &str {
        match self {
            Self::Zmq(_) => "wire_zmq",
            Self::Timeout(_) => "wire_timeout",
            Self::InvalidRequest(_) => "wire_invalid_request",
            Self::MalformedReply(_) => "wire_malformed_reply",
            Self::ProtocolMismatch(_) => "wire_protocol_mismatch",
            Self::RequestIdMismatch { .. } => "wire_request_id_mismatch",
            Self::Remote(remote) => &remote.code,
            Self::Schema(err) => err.code(),
        }
    }
}

/// Encode one request envelope.
pub fn encode_request(request_id: &str, op: &Op) -> Result<String, WireError> {
    if request_id.is_empty() {
        return Err(WireError::InvalidRequest(
            "request_id must be non-empty".into(),
        ));
    }
    match op {
        Op::GenerateProblem { generator, seed } => {
            if generator.is_empty() {
                return Err(WireError::InvalidRequest(
                    "generator must be non-empty".into(),
                ));
            }
            if *seed > MAX_SEED {
                return Err(WireError::InvalidRequest(format!(
                    "seed {seed} exceeds {MAX_SEED}"
                )));
            }
        }
        Op::VerifyAnswer { problem } => {
            if let Err(err) = problem.validate() {
                return Err(WireError::InvalidRequest(err.to_string()));
            }
        }
        Op::Ping => {}
    }
    serde_json::to_string(&RequestEnvelope {
        protocol: PROTOCOL,
        request_id,
        op,
    })
    .map_err(|err| WireError::InvalidRequest(err.to_string()))
}

/// Decode a reply to the request `request_id` and return its `result`.
pub fn decode_reply(request_id: &str, raw: &[u8]) -> Result<Value, WireError> {
    let reply: ReplyEnvelope =
        serde_json::from_slice(raw).map_err(|err| WireError::MalformedReply(err.to_string()))?;
    if reply.protocol != PROTOCOL {
        return Err(WireError::ProtocolMismatch(reply.protocol));
    }
    match (reply.status, reply.result, reply.error) {
        (Status::Error, None, Some(error)) => {
            // A worker that could not parse the request cannot echo its ID.
            if reply
                .request_id
                .as_deref()
                .is_some_and(|id| id != request_id)
            {
                return Err(WireError::RequestIdMismatch {
                    expected: request_id.to_owned(),
                    got: reply.request_id,
                });
            }
            Err(WireError::Remote(error))
        }
        (Status::Ok, Some(result), None) => {
            if reply.request_id.as_deref() != Some(request_id) {
                return Err(WireError::RequestIdMismatch {
                    expected: request_id.to_owned(),
                    got: reply.request_id,
                });
            }
            Ok(result)
        }
        (status, _, _) => Err(WireError::MalformedReply(format!(
            "status {status:?} needs exactly one of `result` (ok) or `error` (error)"
        ))),
    }
}

/// ZMQ REQ client for the `math-julia` worker.
pub struct MathClient {
    context: zmq::Context,
    socket: zmq::Socket,
    endpoint: String,
    timeout: Duration,
    next_request: u64,
}

impl MathClient {
    /// Connect to `endpoint` with a per-message `timeout`. ZMQ connects lazily, so a
    /// missing worker shows up as [`WireError::Timeout`] on the first request.
    pub fn connect(endpoint: &str, timeout: Duration) -> Result<Self, WireError> {
        if timeout.as_millis() == 0 || timeout.as_millis() > i32::MAX as u128 {
            return Err(WireError::InvalidRequest(format!(
                "timeout {timeout:?} must be between 1ms and {}ms",
                i32::MAX
            )));
        }
        let context = zmq::Context::new();
        let socket = open_socket(&context, endpoint, timeout)?;
        Ok(Self {
            context,
            socket,
            endpoint: endpoint.to_owned(),
            timeout,
            next_request: 1,
        })
    }

    /// Connect to `$TUTOR_MATH_ENDPOINT`, or [`DEFAULT_ENDPOINT`] when unset.
    pub fn connect_from_env(timeout: Duration) -> Result<Self, WireError> {
        let endpoint = std::env::var(ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
        Self::connect(&endpoint, timeout)
    }

    /// Endpoint this client talks to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Ask the worker who it is.
    pub fn ping(&mut self) -> Result<WorkerInfo, WireError> {
        let result = self.call(&Op::Ping)?;
        let info: WorkerInfo = serde_json::from_value(result)
            .map_err(|err| WireError::MalformedReply(err.to_string()))?;
        if info.protocol != PROTOCOL {
            return Err(WireError::ProtocolMismatch(info.protocol));
        }
        Ok(info)
    }

    /// Generate one schema-valid (but not yet verified) problem.
    pub fn generate_problem(&mut self, generator: &str, seed: u64) -> Result<Problem, WireError> {
        let result = self.call(&Op::GenerateProblem {
            generator: generator.to_owned(),
            seed,
        })?;
        let problem = Problem::from_value(result)?;
        if problem.generator.name != generator || problem.generator.seed != seed {
            return Err(WireError::MalformedReply(format!(
                "asked for {generator}/seed {seed}, got {}/seed {}",
                problem.generator.name, problem.generator.seed
            )));
        }
        if problem.verification.is_some() {
            return Err(WireError::MalformedReply(
                "worker must not mark problems as verified".into(),
            ));
        }
        Ok(problem)
    }

    /// Ask the worker to independently verify `problem`'s answer and steps.
    ///
    /// The problem must already be schema-valid and must not carry a
    /// verification block — the worker checks unverified candidates only.
    pub fn verify_answer(&mut self, problem: &Problem) -> Result<Verdict, WireError> {
        let result = self.call(&Op::VerifyAnswer {
            problem: Box::new(problem.clone()),
        })?;
        serde_json::from_value(result).map_err(|err| WireError::MalformedReply(err.to_string()))
    }

    /// Send one request and wait for its reply.
    pub fn call(&mut self, op: &Op) -> Result<Value, WireError> {
        let request_id = format!("req-{}", self.next_request);
        self.next_request += 1;
        let request = encode_request(&request_id, op)?;
        let raw = match self.exchange(request.as_bytes()) {
            Ok(raw) => raw,
            Err(err) => {
                self.reset()?;
                return Err(err);
            }
        };
        decode_reply(&request_id, &raw)
    }

    fn exchange(&self, request: &[u8]) -> Result<Vec<u8>, WireError> {
        self.socket
            .send(request, 0)
            .map_err(|err| self.map_err(err))?;
        self.socket.recv_bytes(0).map_err(|err| self.map_err(err))
    }

    fn map_err(&self, err: zmq::Error) -> WireError {
        match err {
            zmq::Error::EAGAIN => WireError::Timeout(self.timeout),
            other => WireError::Zmq(other),
        }
    }

    fn reset(&mut self) -> Result<(), WireError> {
        self.socket = open_socket(&self.context, &self.endpoint, self.timeout)?;
        Ok(())
    }
}

fn open_socket(
    context: &zmq::Context,
    endpoint: &str,
    timeout: Duration,
) -> Result<zmq::Socket, WireError> {
    let millis = i32::try_from(timeout.as_millis())
        .map_err(|_| WireError::InvalidRequest(format!("timeout {timeout:?} too large")))?;
    let socket = context.socket(zmq::REQ)?;
    socket.set_linger(0)?;
    socket.set_rcvtimeo(millis)?;
    socket.set_sndtimeo(millis)?;
    socket.set_maxmsgsize(MAX_REPLY_BYTES)?;
    socket.connect(endpoint)?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_envelopes() {
        let ping: Value = serde_json::from_str(&encode_request("r1", &Op::Ping).unwrap()).unwrap();
        assert_eq!(
            ping,
            json!({"protocol": PROTOCOL, "request_id": "r1", "op": "ping"})
        );
        let op = Op::GenerateProblem {
            generator: "g".into(),
            seed: 7,
        };
        let generate: Value = serde_json::from_str(&encode_request("r2", &op).unwrap()).unwrap();
        assert_eq!(
            generate,
            json!({"protocol": PROTOCOL, "request_id": "r2", "op": "generate_problem",
                   "generator": "g", "seed": 7})
        );
    }

    #[test]
    fn refuses_bad_requests() {
        let too_big = Op::GenerateProblem {
            generator: "g".into(),
            seed: MAX_SEED + 1,
        };
        assert_eq!(
            encode_request("r", &too_big).unwrap_err().code(),
            "wire_invalid_request"
        );
        assert_eq!(
            encode_request("", &Op::Ping).unwrap_err().code(),
            "wire_invalid_request"
        );
    }

    fn decode(reply: Value) -> Result<Value, WireError> {
        decode_reply("r1", reply.to_string().as_bytes())
    }

    #[test]
    fn decodes_ok_and_error_replies() {
        let ok =
            json!({"protocol": PROTOCOL, "request_id": "r1", "status": "ok", "result": {"x": 1}});
        assert_eq!(decode(ok).unwrap(), json!({"x": 1}));
        let err = json!({"protocol": PROTOCOL, "request_id": "r1", "status": "error",
                         "error": {"code": "invalid_seed", "message": "m"}});
        assert_eq!(decode(err).unwrap_err().code(), "invalid_seed");
        let unparsed = json!({"protocol": PROTOCOL, "request_id": null, "status": "error",
                              "error": {"code": "malformed_json", "message": "m"}});
        assert_eq!(decode(unparsed).unwrap_err().code(), "malformed_json");
    }

    #[test]
    fn rejects_bad_replies() {
        let cases = [
            (
                json!({"protocol": "tutor-math/0", "request_id": "r1", "status": "ok", "result": 1}),
                "wire_protocol_mismatch",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r2", "status": "ok", "result": 1}),
                "wire_request_id_mismatch",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": null, "status": "ok", "result": 1}),
                "wire_request_id_mismatch",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r2", "status": "error",
                    "error": {"code": "c", "message": "m"}}),
                "wire_request_id_mismatch",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r1", "status": "ok"}),
                "wire_malformed_reply",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r1", "status": "ok", "result": 1,
                    "error": {"code": "c", "message": "m"}}),
                "wire_malformed_reply",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r1", "status": "maybe", "result": 1}),
                "wire_malformed_reply",
            ),
            (
                json!({"protocol": PROTOCOL, "request_id": "r1", "status": "ok", "result": 1,
                    "extra": true}),
                "wire_malformed_reply",
            ),
        ];
        for (reply, code) in cases {
            assert_eq!(decode(reply.clone()).unwrap_err().code(), code, "{reply}");
        }
        assert_eq!(
            decode_reply("r1", b"not json").unwrap_err().code(),
            "wire_malformed_reply"
        );
    }
}
