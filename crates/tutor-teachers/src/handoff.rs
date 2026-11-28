//! A bounded file handoff for Hermes, separate from the math worker protocol.
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tutor_build::{checks, input::InputLine};
use tutor_schema::{DialogueRow, Problem};

use crate::{Error, Provider, TeacherConfig, json_sha256, sha256, smoke};

const VERSION: u32 = 2;
const LIMIT: u64 = 64 * 1024;
const INSTRUCTION: &str = "Rewrite only the diagnosis and hint wording at turn indices 2 and 4. Preserve their meaning and the hint level. Do not reveal the answer, add math claims, or change the problem, student, system or closing turns. Return JSON with version=2, request_id copied exactly, teacher copied exactly, origin=recorded, and replacements=[{turn_index:2,content:...},{turn_index:4,content:...}]. No other fields. This file is an unsent preparation artifact, not authorization to contact a provider.";

// Intended OpenRouter request-body provider options. These are bound into the
// handoff; this offline runner cannot attest that Hermes sends them unchanged.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenRouterProvider {
    only: Vec<String>,
    allow_fallbacks: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u32,
    teacher: TeacherConfig,
    openrouter_provider: Option<OpenRouterProvider>,
    send_authorized: bool,
    instruction: String,
    problem: Problem,
    dialogue: DialogueRow,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    payload: Payload,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Origin {
    SyntheticMock,
    Recorded,
}

impl Origin {
    fn quarantine(&self) -> (&'static str, &'static str) {
        match self {
            Self::SyntheticMock => ("mock_teacher_surface", "mock_teacher_not_exportable"),
            Self::Recorded => ("deferred_teacher_surface", "teacher_admission_blocked"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    turn_index: usize,
    content: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u32,
    request_id: String,
    teacher: TeacherConfig,
    origin: Origin,
    replacements: Vec<Replacement>,
}

fn request(config: &TeacherConfig) -> Result<Request, Error> {
    config.validate()?;
    let problem = tutor_verify::verify(&Problem::from_json(smoke::PROBLEM)?)?;
    let dialogue = smoke::draft_fixture(&problem, true)?;
    let payload = Payload {
        version: VERSION,
        teacher: config.clone(),
        openrouter_provider: match config.provider {
            Provider::Mock => None,
            Provider::Deepseek => Some(OpenRouterProvider {
                only: vec!["deepseek".into()],
                allow_fallbacks: false,
            }),
        },
        send_authorized: false,
        instruction: INSTRUCTION.into(),
        problem,
        dialogue,
    };
    let request_id = json_sha256(&payload)?;
    Ok(Request {
        request_id,
        payload,
    })
}

fn read(path: &Path) -> Result<String, Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err("handoff input exceeds 64 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "handoff input must be UTF-8".into())
}

fn load_request(path: &Path) -> Result<(Request, String), Error> {
    let raw = read(path)?;
    let parsed: Request = serde_json::from_str(&raw).map_err(|_| "invalid request shape")?;
    // Recompute from the immutable fixture and config, not a supplied verification
    // claim. Even a rehashed modified problem/prompt/approval cannot broaden scope.
    if parsed != request(&parsed.payload.teacher)? {
        return Err("request does not match this version's verified fixture and policy".into());
    }
    Ok((parsed, raw))
}

fn write_new(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

/// Emit a versioned, content-bound, unsent request. No credentials are included.
pub fn prepare_request(config: &TeacherConfig, out: &Path) -> Result<(), Error> {
    write_new(out, &request(config)?)
}

/// Make a clearly synthetic response for offline testing, including future configs.
pub fn synthetic_response(request_path: &Path, out: &Path) -> Result<(), Error> {
    let (request, _) = load_request(request_path)?;
    let response = Response {
        version: VERSION, request_id: request.request_id,
        teacher: request.payload.teacher, origin: Origin::SyntheticMock,
        replacements: vec![
            Replacement { turn_index: 2, content: "What is the base in the simple power rule: x, or the entire expression you used?".into() },
            Replacement { turn_index: 4, content: "Differentiate the original polynomial term by term using powers of x, then substitute the given point.".into() },
        ],
    };
    write_new(out, &response)
}

fn apply_response(request: &Request, response: &Response) -> Result<DialogueRow, Error> {
    let binding = (response.version, &response.request_id, &response.teacher);
    let expected = (VERSION, &request.request_id, &request.payload.teacher);
    if binding != expected {
        return Err("response version, request_id or teacher binding mismatch".into());
    }
    let mut row = request.payload.dialogue.clone();
    let mut seen = HashSet::new();
    for replacement in &response.replacements {
        if ![2, 4].contains(&replacement.turn_index) || !seen.insert(replacement.turn_index) {
            return Err("response must replace each permitted tutor turn exactly once".into());
        }
        if replacement.content.trim().is_empty() || replacement.content.len() > 4096 {
            return Err("replacement wording must contain 1..4096 bytes".into());
        }
        row.turns[replacement.turn_index]
            .content
            .clone_from(&replacement.content);
    }
    if seen.len() != 2 {
        return Err("response must include both permitted tutor turns".into());
    }
    Ok(row)
}

/// Replay wording through verification, validation and the actual admission gate.
/// Valid candidates are quarantined with a blocked report; never release rows.
pub fn replay_response(
    request_path: &Path,
    response_path: &Path,
    out: &Path,
) -> Result<Value, Error> {
    let (request, request_raw) = load_request(request_path)?;
    let response_raw = read(response_path)?;
    let response: Response =
        serde_json::from_str(&response_raw).map_err(|_| "invalid response shape")?;
    let mut row = apply_response(&request, &response)?;
    let problem = &request.payload.problem;
    let problems = BTreeMap::from([(problem.problem_id.clone(), problem.clone())]);
    let validator = jsonschema::validator_for(&tutor_schema::dialogue_json_schema())?;
    smoke::check_draft(&row, &problems, &validator)?;
    let (generator, admission_code) = response.origin.quarantine();
    row.provenance
        .generator
        .as_mut()
        .ok_or("missing draft generator")?
        .name = generator.into();
    let input = InputLine {
        file: "replay-candidate.json".into(),
        line: None,
        text: serde_json::to_string(&row)?,
    };
    let rejection = checks::check(&input, Some(&problems), &validator, &mut HashSet::new())
        .err()
        .ok_or("teacher admission unexpectedly allowed; refusing to write")?;
    if rejection.code != admission_code {
        return Err("unexpected admission rejection".into());
    }
    let candidate = json!({
        "kind": "quarantined_teacher_candidate_v1", "dialogue": row, "problem": problem,
        "training_approved": false, "export_approved": false,
        "provenance": {
            "request_id": request.request_id, "request_sha256": sha256(request_raw.as_bytes()),
            "response_sha256": sha256(response_raw.as_bytes()), "response_origin": response.origin,
            "teacher": response.teacher, "dialogue_sha256": json_sha256(&row)?,
            "intended_openrouter_provider": request.payload.openrouter_provider,
            "provider_execution_verified": false,
            "routing_execution_verified": false,
            "fixture_sha256": sha256(smoke::PROBLEM.as_bytes()),
            "verification_scope": "rust_recompute_of_static_fixture_not_fresh_julia_run"
        }
    });
    let report = json!({
        "validation": "schema_policy_grounding_passed", "admitted": false,
        "admission_code": rejection.code, "request_id": request.request_id,
        "candidate_sha256": sha256(&serde_json::to_vec_pretty(&candidate)?),
        "blockers": ["RM-2500 exact model/route/vendor rights evidence", "explicit run approval", "release schema lacks teacher rights provenance"],
        "live_generation": "blocked", "training": "blocked", "export": "blocked"
    });
    crate::publication::publish_artifacts(out, |staged| {
        write_new(&staged.join("candidate.json"), &candidate)?;
        write_new(&staged.join("report.json"), &report)
    })?;
    Ok(report)
}
