# Offline teacher readiness for Hermes

Hermes is the command runner; DeepSeek would be the teacher. This slice proves
local candidate drafting and validation only. It does **not** implement a live
teacher client or approve provider rights. RM-2500 remains an explicit gate.

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

Before any future live implementation/run: resolve RM-2500 with reproducible terms
snapshots and exact model/route/vendor pins, approve the generation spec and the
run, and implement a gated teacher adapter with rights-bearing row provenance.
The repo's documented route policy still applies; this config does not override it.
This slice makes no claim that current terms permit a provider or route. It does
not configure Hermes authentication or provide a live-generation switch.

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
config rejection, existing-output protection and export rejection. Separate
checks reject forged math, early tutor answers and wrong closing answers.
