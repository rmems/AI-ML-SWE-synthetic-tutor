//! Offline teacher readiness over a static, benign derivative fixture.
//!
//! No HTTP client or credential lookup exists here. Configuration checks are
//! not rights approval. Mock candidates cannot enter the release assembler.

mod config;
mod handoff;
mod smoke;

pub use config::{Provider, TeacherConfig};
pub use handoff::{prepare_request, replay_response, synthetic_response};
pub use smoke::mock_smoke;

/// Errors from configuration, fixture verification, validation, or local I/O.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Content digest used for deterministic local provenance.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

// Hash the compact JSON Value encoding used inside emitted envelopes: recursively
// sorted object keys, UTF-8 and no whitespace. Not an RFC 8785 canonicalizer.
pub(crate) fn json_sha256(value: &impl serde::Serialize) -> Result<String, Error> {
    Ok(sha256(&serde_json::to_vec(&serde_json::to_value(value)?)?))
}
