# Offline teacher readiness for Hermes

Hermes is the command runner; DeepSeek through OpenRouter is the intended teacher
route. This slice proves
local candidate drafting, a file request/response handoff and validation. Hermes
can own a later provider call; this repo requires no direct network adapter for
that handoff. It does **not** approve provider rights. RM-2500 remains a gate.

## Run from Hermes

Set Hermes's working directory to the root of this repository. Rust 1.99.1 and
the workspace's locked dependencies are required (see `rust-toolchain.toml`).
The existing workspace uses libzmq through the verifier; follow the repository
build setup. Once dependencies are cached, this command is fully offline:

```sh
cd /path/to/AI-ML-SWE-synthetic-tutor
cargo run --offline --locked -p tutor-teachers -- mock-smoke --out /tmp/tutor-teacher-smoke
```

Use a new output path each time; its parent must exist. Existing directories
are refused. No environment variables or API keys are needed. The command does
not inspect credentials, start Julia, or contact any teacher endpoint.

Inputs are built into the binary: the existing static
`crates/tutor-schema/tests/fixtures/problem.json` (power-rule derivative) and
`prompts/tutor-system-v1.txt`. This is **not** a fresh Julia generation run.
Rust's existing verifier recomputes the answer and replays the steps before
the mock drafts one correction and one confirmation. Production schema, policy
and answer-grounding checks then validate the drafts separately.
The correction wrongly applies the simple power rule to the entire polynomial
as a base, matching `derivatives.power_rule_overgeneralised`; the confirmation
extends the quadratic to a cubic. No taxonomy or topic mapping is changed.

Outputs under the chosen directory:

- `candidates.jsonl`: two **mock-only** envelopes containing the problem, draft,
  provider/model/route/vendor, fixture/config/dialogue SHA-256 hashes and versions.
- `report.json`: validation result, count, whole-file digest, and explicit blocked
  live-generation/training/export states. Identical inputs produce identical bytes.

These are disposable fixtures, not a pilot release or an 80/20 batch. The release
assembler rejects the envelopes. Extracted dialogues also fail with
`mock_teacher_not_exportable`; there is no flag to approve mock data for training.
Do not strip or relabel provenance to feed them into training or export.

## Pinned OpenRouter selection, without enabling it

`configs/teacher.deepseek.example.json` pins this pilot's intended selection:

| Field | Value |
| --- | --- |
| `provider` | `deepseek` |
| `model` | `deepseek/deepseek-v4.1-flash-1017` |
| `endpoint` | `https://openrouter.ai/api/v1` |
| `route` | `openrouter` |
| `serving_vendor` | `deepseek` |
| `api_key_env` | `OPENROUTER_API_KEY` |

The non-mock configuration must match this pin; other selections fail closed.
Routing references checked 2026-11-28: [OpenRouter API documentation](https://openrouter.ai/docs/api_reference/overview),
[model catalog entry](https://openrouter.ai/deepseek/deepseek-v4.1-flash-1017),
and [provider routing](https://openrouter.ai/docs/guides/routing/provider-selection).
Catalog presence and API configuration are not rights clearance or a runtime test.

`api_key_env` is the environment variable **name** `OPENROUTER_API_KEY` only,
owned by Hermes for a future authorized call. Do not put a key in JSON. No
environment value is read, stored or logged by this implementation.
Unknown fields, embedded URL credentials, URL query strings and fragments fail.

```sh
cd /path/to/AI-ML-SWE-synthetic-tutor
cargo run --offline --locked -p tutor-teachers -- check-config \
  --config configs/teacher.deepseek.example.json
```

Success means only that the configuration and pilot pin are valid. Passing that same file
to `mock-smoke` fails before output creation; it never silently falls back to mock.

Before any future live call/run: resolve RM-2500 with reproducible terms
snapshots and exact model/route/vendor pins, approve the generation spec and the
run. Training/export additionally need a release schema that carries and enforces
teacher rights provenance; the current schema/assembler cannot admit teacher rows.
This pilot selects OpenRouter explicitly; it does not change `docs/rights.md` or
grant the selected route permission to generate training data.
This slice makes no claim that current terms permit a provider or route. It does
not configure Hermes authentication or provide a live-generation switch.

## Hermes request/response handoff (offline rehearsal)

The only existing transport, `tutor-wire`, serves Julia math operations. Teacher
wording uses local JSON files instead, keeping verification separate from drafting.
Run the following from the repository root; every command stays offline even
with the future DeepSeek config. `mock-response` always labels its output synthetic.

```sh
cd /path/to/AI-ML-SWE-synthetic-tutor
tutor_handoff_dir=$(mktemp -d /tmp/tutor-handoff.XXXXXX)
cargo run --offline --locked -p tutor-teachers -- prepare-request \
  --config configs/teacher.deepseek.example.json --out "$tutor_handoff_dir/request.json"
cargo run --offline --locked -p tutor-teachers -- mock-response \
  --request "$tutor_handoff_dir/request.json" --out "$tutor_handoff_dir/response.json"
cargo run --offline --locked -p tutor-teachers -- replay-response \
  --request "$tutor_handoff_dir/request.json" --response "$tutor_handoff_dir/response.json" \
  --out "$tutor_handoff_dir/replay"
```

**The last command deliberately exits 2:** valid replay, admission blocked.
Exit 1 means invalid input, failed verification/validation, or I/O failure.
Prepare/mock-response return 0 on success. These commands refuse existing output
files/directories. Omitting `--config` on `prepare-request` uses the offline mock.

`request.json` contains a SHA-256 `request_id` bound to its version-2 `payload`:
teacher selection, instruction, verified static problem, and fixed correction
dialogue. Regenerate older handoff files after changes to routing, fixture or
digest encoding; replay accepts only this implementation's exact payload.
`send_authorized` is always false: producing a file grants no permission
to send it. Replaying recomputes the fixture and checks the whole request against
this implementation; editing/re-hashing its problem, policy or approval fails.
The embedded draft is already marked `mock_teacher_surface`; extracting it with
its matching verified problem still fails release admission. Quality checks use
an in-memory copy and never write or return a draft with an admissible marker.

After rights and explicit run approval are resolved outside this rehearsal,
Hermes can give the request file to the selected teacher with its existing
provider tooling and save the returned JSON text as `response.json`. No repo
HTTP client or additional adapter code is needed for this **bounded wording
handoff**. Do not change the unsent artifact's `send_authorized` field. This
implementation offers no provider invocation or authorization mechanism.

For the OpenRouter config, `payload.openrouter_provider` contains the intended
OpenRouter request-body `provider` options:

```json
{"only": ["deepseek"], "allow_fallbacks": false}
```

These options are immutable and covered by the request hash. When a future call
is authorized, Hermes must preserve them as the API body's `provider` field,
along with the pinned model and base URL. **This offline runner does not enforce
or verify what Hermes actually sends.** Check that Hermes's selected provider
tool can pass these options unchanged before approving a live run; if it cannot,
that is a runner-capability blocker, not permission to silently use defaults or
fallbacks. No automatic route/model fallback is implemented here. Mock requests
carry `openrouter_provider: null`.

The response is the JSON object requested by `payload.instruction`:

- `version`: 2; `request_id`: copy exactly from the request.
- `teacher`: copy `payload.teacher` exactly, including model/endpoint/route/vendor.
- `origin`: `recorded` for saved teacher output; `synthetic_mock` for the offline
  fixture. This is caller-declared provenance, not proof of provider execution.
- `replacements`: exactly two objects, each containing `turn_index` and `content`,
  covering indices 2 and 4 once each (diagnosis and L1 hint). Wording is limited to
  4096 bytes per turn. Unknown/duplicate fields and inputs over 64 KiB fail.

Save the assistant's JSON content, not an entire provider-specific API envelope.
The original problem, verification, system/student turns, closing answer, labels,
roles and hint levels are immutable. Production checks reject structural errors
and detected early answers; they do not establish pedagogical equivalence or
replace independent review of future teacher wording.

`replay/candidate.json` is quarantined. Its provenance binds the request ID,
request/response file hashes, teacher, response origin, fixture and dialogue.
`dialogue_sha256` hashes the nested dialogue object encoded as compact UTF-8 JSON
with recursively sorted object keys and no whitespace (`serde_json::Value`
serialization). The request ID uses the same encoding for `payload`. Hashes of
request, response and candidate files instead cover their exact on-disk bytes.
This is the repository's JSON encoding, not a general RFC 8785 canonicalizer;
copy the emitted request ID into responses rather than recomputing it in Hermes.
It also records the intended provider options and keeps both
`provider_execution_verified` and `routing_execution_verified` false. Echoed
metadata does not prove the serving provider or routing actually used.
`replay/report.json` binds the candidate file hash and reports the actual release
admission rejection `teacher_admission_blocked`. Both wrapped and extracted
teacher-marked dialogues remain ineligible for release assembly. Missing rights
evidence or run approval cannot be supplied as a response flag; such fields are
rejected, and there is no admission override. Even a valid recorded response
remains blocked until the release provenance support lands in its own scope.

The full RM-2503 pilot still needs complete topic coverage, native Rust/Julia
generation/verification, release assembly, held-out checks, review evidence and
export. Existing stack #46 → #47 → #48 owns the local procedural/secant proof;
this independent prerequisite neither closes those issues nor changes that stack.

## Verify

```sh
cargo test --offline --locked -p tutor-teachers -p tutor-build
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
```

The CLI tests cover repeatability, live-provider refusal, invalid/secret-shaped
config rejection, existing-output protection and export rejection. They also
exercise synthetic and recorded-shaped replay, request/teacher/version mismatches,
forbidden/duplicate/missing turn edits, early answers, size limits, forged requests
and attempted approval fields. Recorded-shaped tests use invented wording only.
Separate checks reject forged math and wrong closing answers.
