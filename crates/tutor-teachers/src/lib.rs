//! Offline teacher readiness over a static, benign derivative fixture.
//!
//! No HTTP client or credential lookup exists here. Configuration checks are
//! not rights approval. Mock candidates cannot enter the release assembler.

mod config;
mod handoff;
mod publication;
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

#[cfg(test)]
mod tests {
    #[test]
    fn digest_uses_sorted_keys_at_every_depth() {
        // Struct declaration order deliberately differs from lexical key order.
        #[derive(serde::Serialize)]
        struct Unsorted {
            z: u32,
            a: Vec<Nested>,
        }
        #[derive(serde::Serialize)]
        struct Nested {
            y: bool,
            b: &'static str,
        }
        let fixture = Unsorted {
            z: 7,
            a: vec![Nested { y: true, b: "math" }],
        };
        // Handwritten expected bytes, independent of serde's map implementation.
        let expected = br#"{"a":[{"b":"math","y":true}],"z":7}"#;
        assert_eq!(
            super::json_sha256(&fixture).unwrap(),
            super::sha256(expected)
        );
    }
}
