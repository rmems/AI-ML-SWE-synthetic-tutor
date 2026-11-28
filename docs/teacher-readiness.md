# Offline teacher readiness for Hermes

Hermes is the command runner; DeepSeek would be the teacher. This slice proves
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

Outputs under the chosen directory:

- `candidates.jsonl`: two **mock-only** envelopes containing the problem, draft,
  provider/model/route/vendor, fixture/config/dialogue SHA-256 hashes and versions.
- `report.json`: validation result, count, whole-file digest, and explicit blocked
  live-generation/training/export states. Identical inputs produce identical bytes.

These are disposable fixtures, not a pilot release or an 80/20 batch. The release
assembler rejects the envelopes. Extracted dialogues also fail with
`mock_teacher_not_exportable`; there is no flag to approve mock data for training.
Do not strip or relabel provenance to feed them into training or export.

## Select a future teacher without enabling it

Copy `configs/teacher.deepseek.example.json` to a local configuration file.
Set `provider` to `deepseek`, `model` to the exact intended model pin, `endpoint`
to the intended HTTPS API base URL, and record its `route` and `serving_vendor`.
The example's `.invalid` endpoint and pin values are deliberately placeholders,
not claims about current provider APIs, terms or available models.

`api_key_env` is the environment variable **name** `TUTOR_TEACHER_API_KEY` only.
Do not put a key in JSON. No environment value is read in this implementation.
Unknown fields, embedded URL credentials, URL query strings and fragments fail.

```sh
cd /path/to/AI-ML-SWE-synthetic-tutor
cargo run --offline --locked -p tutor-teachers -- check-config \
  --config configs/teacher.deepseek.example.json
```

Success means only that the configuration shape is valid. Passing that same file
to `mock-smoke` fails before output creation; it never silently falls back to mock.

Before any future live call/run: resolve RM-2500 with reproducible terms
snapshots and exact model/route/vendor pins, approve the generation spec and the
run. Training/export additionally need a release schema that carries and enforces
teacher rights provenance; the current schema/assembler cannot admit teacher rows.
The repo's documented route policy still applies; this config does not override it.
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

`request.json` contains a SHA-256 `request_id` bound to its versioned `payload`:
teacher selection, instruction, verified static problem, and fixed correction
dialogue. `send_authorized` is always false: producing a file grants no permission
to send it. Replaying recomputes the fixture and checks the whole request against
this implementation; editing/re-hashing its problem, policy or approval fails.

After rights and explicit run approval are resolved outside this rehearsal,
Hermes can give the request file to the selected teacher with its existing
provider tooling and save the returned JSON text as `response.json`. No repo
HTTP client or additional adapter code is needed for this **bounded wording
handoff**. Do not change the unsent artifact's `send_authorized` field. This
implementation offers no provider invocation or authorization mechanism.

The response is the JSON object requested by `payload.instruction`:

- `version`: 1; `request_id`: copy exactly from the request.
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
