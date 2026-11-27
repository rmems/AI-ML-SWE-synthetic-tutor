# AI-ML-SWE-synthetic-tutor

Synthetic tutoring datasets for post-training. This repository is being
built (see status below) to generate multi-turn tutoring dialogues — verified problems, synthetic student attempts
with labelled misconceptions, and hint-ladder tutor turns — so a model learns
to **tutor** (diagnose a mistake, ask a targeted question, give a graded hint)
instead of only producing final answers. Sources are procedural generation
plus open-weight / distillable teachers; datasets will ship as small
per-family Hugging Face datasets.

Topics: synthetic data · post-training · distillation · tutor

> **Status: pivot in progress** under the
> [pivot epic (#4)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/4).
> The repo layout, Rust workspace, Julia math worker, ZMQ bridge, four seeded
> derivative generators (power/product/chain rule, one Rust-native), and the
> independent verification pipeline exist
> ([#18](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18)).
> Student attempts, tutor dialogues, and QA do not exist yet; splits and
> export exist for already-assembled dialogue rows (`tutor-build`, below);
> anything not yet in the repo says *planned*. The pre-pivot Julia
> playground lives in [`legacy/`](legacy/README.md).

---

## What one row is

Each dataset row is one tutoring dialogue in a standard chat format:

- grounded in a problem whose correct answer has passed independent verification
- a student attempt that is either correct or carries exactly one planted,
  taxonomy-labelled misconception
  ([misconception taxonomy, #6](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/6))
- tutor turns that follow a defined hint ladder — diagnose first, then
  nudge → targeted question → partial worked step → full solution only as a
  last resort ([dialogue schema + tutor policy, #13](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/13))
- full provenance: generator, teacher (if any), verification result,
  curriculum topic, and rights status

Gold example rows live in [`datasets/gold/`](datasets/gold/README.md); the
row contract is [`schemas/dialogue.schema.json`](schemas/dialogue.schema.json)
generated from `tutor_schema::DialogueRow`, and the pedagogy is
[`docs/tutor-policy.md`](docs/tutor-policy.md).

---

## Generation loop

```
curriculum topic
      │
      ▼
problem generator ──► synthetic student attempt ──► tutor dialogue
                                                        │
                                                        ▼
Hugging Face export ◄── split ◄── QA ◄────────── verification
```

1. **Curriculum topic** — a row in the coverage matrix
   ([#16](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/16)).
2. **Problem generator** — parameterised, seeded, produces the problem and its
   answer together ([#18](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18)).
3. **Synthetic student attempt** — correct, or wrong with one planted
   misconception at graded difficulty
   ([#19](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/19)).
4. **Tutor dialogue** — hint-ladder turns per the tutor policy
   ([#13](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/13)).
5. **Verification** — the answer is checked independently of the generator
   ([#18](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18)).
6. **QA** — sampled human review with recorded verdicts
   ([#21](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/21)).
7. **Split** — dedup, decontamination against the held-out eval set, frozen
   train / validation / test splits
   ([#8](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/8)).
8. **Hugging Face export** — one dataset per family
   ([#21](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/21)).

Every batch leaves a run-log manifest
([#7](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/7)): each
`tutor-generate` run or simulate appends one hash-chained entry to
`run-log.jsonl` next to the batch (`schemas/run-log.schema.json`), and
batches run one family at a time on the cadence in
[docs/cadence.md](docs/cadence.md).

---

## Stack

| Role | Choice |
|------|--------|
| Pipeline (generators, verification harness, student simulator, assembly, export) | **Rust**, in a Cargo workspace (`crates/`). Schema types, the ZMQ client, the `tutor-generate` generator registry + pipeline, and `tutor-verify` verification, and the seeded student simulator exist; assembly and export are *planned*. |
| Math-heavy generation and checking (symbolic / numeric work simpler in Julia) | **Julia** math workers, driven by the Rust pipeline over **ZMQ** (REQ/REP, JSON payloads) — the same Rust `zmq` ↔ Julia `ZMQ.jl` bridge used in [Limen-Capital](https://github.com/rmems/Limen-Capital) and [DendriteTrader.jl](https://github.com/rmems/DendriteTrader.jl). Julia is a math engine, not a second pipeline. The worker is `TutorMath` in `math-julia/` (protocol `tutor-math/1`, ops `ping` / `generate_problem` / `verify_answer`); it serves the polynomial, product-rule, and chain-rule generators plus the Julia-side verifier. |
| Teachers | **DeepSeek V4.1 Flash** and **MiMo V2.6 Flash** (exact checkpoints pinned per use in [docs/rights.md](docs/rights.md); Nemotron 3 parked). They vary dialogue surface and critique drafts; they never decide a correct answer — verification does. |
| Python | Not a pipeline language here. It returns later as a *target* language students are tutored in. |

**Rights rule: no terms snapshot, no generation.** No row may be generated
from a source without an `allowed` entry and a pinned terms snapshot in the
rights registry ([#17](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/17)).
Blocked sources are listed as blocked, not omitted.

This repo is a tutoring-only spin-out that reuses the provenance, rights, and
verification patterns of [synthetic-factory](https://github.com/rmems/synthetic-factory)
(see [synthetic-factory#186](https://github.com/rmems/synthetic-factory/issues/186));
it does not rebuild a general-purpose factory.

---

## How a row is verified

This is the policy the pipeline enforces; `crates/tutor-verify` implements it for every registered generator ([#18](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18)). No unverified answer ships. Problem generators emit a problem and its answer
together with a replay seed; a separate verification pass then checks that
answer by an independent method — always on the opposite side of the Rust /
Julia ZMQ boundary from the generator, so the check is never the generator
grading itself — and records the method on the verified row. Problems that
fail are rejected with a reason code into `rejected/` and never ship.
Details:
[problem generators + verification pipeline (#18)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18).

---

## Synthetic student attempts

The simulator is the Rust procedural core required by
[#19](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/19). It starts
from a verified `Problem`, never invents symbolic math in Rust, and replays
the exact-integer problem trace before use so a stale or fabricated verification
record cannot ship. It either:

- renders the verified solution as a correct attempt for confirmation rows, or
- applies one taxonomy misconception as a defined transformation at the worked
  step where it bites, then propagates the corrupted value through the attempt.

Attempt artifacts validate against
[`schemas/student-attempt.schema.json`](schemas/student-attempt.schema.json).
Their `student_turns` reuse the exact `Turn` representation of the dialogue
schema, so dialogue assembly can splice them into rows without a shape
conversion; profile, persistence, planted transformation, surface-noise flags,
and replay provenance are recorded around those turns.

Implemented transformations cover the verified product-rule and chain-rule
problem kinds. Power-rule-overgeneralisation, quotient-rule, and
implicit-differentiation IDs fail closed today because the verified problem
substrate from #18 does not yet contain an expression shape where those
taxonomy definitions can bite faithfully; those requests are written to
`rejections.jsonl` rather than being mislabeled. Teacher surface variety is
flag-gated and currently fails closed because no rights-approved teacher lane
is wired.

```sh
cargo run -p tutor-generate --bin tutor-generate -- simulate \
  --problems out/verified/problems.jsonl \
  --out out/student-attempts \
  --seed-start 0 --attempts-per-problem 4 --mode mixed
```

The command writes `attempts.jsonl` and `rejections.jsonl` with atomic file
replacement, serializes same-directory writers with `.simulate.lock`, then
writes `manifest.json` last. The manifest records each data file's SHA-256 and
byte length; because no portable filesystem primitive commits several files as
one transaction, consumers should validate those digests before treating the
set as a complete bundle. Before replacing pre-existing data files, the
simulator validates their digests against the committed manifest or a
pending-publish record. That record contains the intended artifact digests
plus the verified digests of the partial files it may replace, so interrupted
retries can resume without overwriting unrelated artifacts. If a simulation
is killed while publishing, confirm no writer is still active, remove the
orphaned `.simulate.lock`, and rerun the command. Use a simulation output
directory separate from the problem-run root; the command refuses to replace
a non-simulator `manifest.json` or artifact files without matching simulator
provenance. If validation rejects every candidate, the command fails before
publication and leaves any previous bundle unchanged; those rejections are
reported on stderr rather than committing an empty attempt batch.
`--seed-start` is mixed with the problem ID and per-problem attempt index, so
a problem's attempts stay stable when unrelated input lines change. Seeds are
capped at `2^53 - 1` so JSON consumers can replay them exactly. Use
`--mode correction` or `--mode confirmation` to force the row type, and
`--level`, `--persistence`, `--surface-style`, or `--misconception` to pin
profile fields for debugging; omitted fields are deterministic seed draws.

---

## Dataset assembly and export

`crates/tutor-build` turns already-assembled dialogue rows into a release:
dialogue checks (schema + tutor policy via `tutor-schema`, plus a
cross-check against the verified problems file), decontamination against
`evals/heldout/` (input-only, run before dedup so a contaminated row cannot
displace a clean near-duplicate), dedup (exact + near on normalized problem
and dialogue text), a deterministic frozen split, and chat-JSONL
export ([#8](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/8)).

```sh
cargo run -p tutor-build --bin tutor-build -- build \
  --family derivatives-pilot \
  --release v1 \
  --rows path/to/dialogue-rows.jsonl \
  --problems out/verified/problems.jsonl \
  --out datasets/derivatives-pilot
```

- `--rows` takes `.json` (one row) or `.jsonl` (one row per line) files.
- `--problems` is required whenever `generated` rows are among the inputs —
  their seed/parameters/verification provenance lives on the problem.
- `--heldout` defaults to `evals/heldout/`; `--out` defaults to
  `datasets/<family>/`.
- `--previous-manifest <path>` freezes splits: a row or problem already
  assigned never moves; new rows fill the next release. It must not be the
  manifest the build is about to replace — copy the previous release's
  `manifest.json` to its own path first, so the digest the new manifest
  records stays verifiable.
- A rebuild into an existing `--out` removes the old `manifest.json` (the
  bundle's commit record) before rewriting any split file and publishes the
  new one last, so a reader never sees new files under the old manifest.

Outputs under `datasets/<family>/` are gitignored (only `README.md` and
`CARD.md` drafts are committed):

```
datasets/<family>/
  train.jsonl            chat messages + full provenance per row
  validation.jsonl
  test.jsonl
  rejected/rejected.jsonl  every dropped row with stage + reason code
  manifest.json          split counts (per split/topic/taxonomy), artifact
                         + input SHA-256s, frozen row/problem assignments
  manifest.json.sha256   manifest digest, also printed by the CLI for the
                         generation run log (#7)
```

Two pieces are deliberately not in this slice: assembling `DialogueRow`s
from verified problems + raw student attempts (needs tutor-turn content —
a teacher model or new templates), and the Hugging Face publish step
([#21](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/21)); the
`{split}.jsonl` + manifest layout is what a later HF export consumes.

---

## Dataset families

Math comes first (most mechanically verifiable), then ML math, then tutoring
datasets for other programming languages. Status values come from the
[curriculum coverage matrix (#16)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/16),
not from this README; until the matrix exists, no family has a status.

| Family / slice | Issue | Status |
|----------------|-------|--------|
| Pilot: derivative misconceptions | [#20](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20) | per coverage matrix (pending #16) |
| Calculus I | [#11](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/11) | per coverage matrix (pending #16) |
| ↳ slice: derivative definition — secant to tangent | [#15](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/15) | per coverage matrix (pending #16) |
| Precalculus | [#10](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/10) | per coverage matrix (pending #16) |
| ↳ slice: function transformations | [#14](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/14) | per coverage matrix (pending #16) |
| ML math | [#12](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/12) | per coverage matrix (pending #16) |

The repo name says AI / ML / SWE, but current coverage is math and ML math
only; programming-language families are planned after the math pipeline is
proven.

New families start with a spec, not code:

1. **Make sure the taxonomy and curriculum entries exist** — every targeted
   misconception id must already be in `schemas/taxonomy.yaml` and every
   topic in `curriculum/coverage-matrix.md`, because the spec's pre-flight
   checks them. Land any missing id first via the
   [taxonomy-entry form](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/new?template=taxonomy-entry.yml),
   and any missing topic in the matrix.
2. **Open a generation spec that links them** — the
   [generation-spec issue form](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/new?template=generation-spec.yml)
   is the contract; no generation work starts on a family without it.
3. **Generation issues follow from the spec** — a teacher named in a spec
   needs an `allowed` [rights-registry](docs/rights.md) entry; one without
   it is a blocker, not a TODO. Family releases use the
   [dataset-release form](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/new?template=dataset-release.yml).

---

## Out of scope

- Training or fine-tuning models (that lives in the training-infra repos)
- A tutoring app, UI, or live tutor service
- Personal study logging (logging survives only as the generation run log)

---

## Repository layout

```
Cargo.toml, Cargo.lock, rust-toolchain.toml   Rust workspace (Rust 1.99.1, edition 2024)
crates/
  tutor-schema/     problem, student-attempt, and dialogue row types and validation (mirrors schemas/)
  tutor-validate/   dialogue row validator CLI (JSON schema + policy checks)
  tutor-ci/         PR-time taxonomy, rights, and verification-smoke wiring
  tutor-wire/       ZMQ REQ client for the Julia worker (tutor-math/1 JSON envelope)
  tutor-verify/     independent answer verification + Rust<->Julia bridge proof test
  tutor-generate/   seeded problem generators (#18) and the procedural student simulator (#19)
  tutor-runlog/     append-only, hash-chained generation run log + `tutor-runlog` verify/pause CLI (#7)
  tutor-teachers/   placeholder: teacher model clients (#17)
  tutor-build/      build pipeline: checks, dedup, decontamination, frozen
                    splits, chat-JSONL export + manifest (#8); HF export is #21
math-julia/         TutorMath: seeded problem generators + ZMQ REP worker
schemas/            problem.schema.json, dialogue.schema.json, student-attempt.schema.json, run-log.schema.json (generated), taxonomy.yaml
docs/               mission, tutor policy (v1), rights, batch cadence + run-log rules
prompts/            versioned tutor system prompts
curriculum/         coverage matrix (#16)
datasets/           one folder per dataset family; generated rows are gitignored
evals/              heldout/ items and grading rubric
legacy/             pre-pivot NeuroMathPlayground.jl, unchanged
```

## Development

Linux (or WSL) with `libzmq3-dev` and `pkg-config`, Rust 1.99.1 (pinned by
`rust-toolchain.toml`), and Julia 1.13. The `zmq` crate does not build on
Windows with the MinGW toolchain; use WSL there. CI (`.github/workflows/ci.yml`)
runs everything below.

```sh
# Rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --locked

# Julia worker
julia --project=math-julia -e 'using Pkg; Pkg.instantiate(); Pkg.test()'

# Rust <-> Julia bridge proof (worker binds tcp://127.0.0.1:5557; override with TUTOR_MATH_ENDPOINT)
julia --project=math-julia math-julia/bin/serve.jl &
cargo test --locked -p tutor-verify --test julia_bridge -- --ignored
```

---

## Legacy: NeuroMathPlayground.jl

Before the pivot this repo was **NeuroMathPlayground.jl**, a personal Julia
playground for math and neuromorphic computing. Its code is kept, not
deleted, under [`legacy/NeuroMathPlayground/`](legacy/README.md), moved with
`git mv` so history follows. Its finite-difference math helpers were copied
into `math-julia/` to cross-check generated answers; `legacy/README.md`
records what moved, what was reused, and why.

| Path | Contents |
|------|----------|
| `legacy/NeuroMathPlayground/src/` | `NeuroMathPlayground` module: LIF / STDP, spike functions, stochastic processes, signals, Kelly, math helpers, GPU detection |
| `legacy/NeuroMathPlayground/examples/`, `sandbox/`, `notebooks/` | Demo scripts, worked examples, Pluto notebook |
| `legacy/NeuroMathPlayground/test/runtests.jl` | Playground test suite (`julia --project=legacy/NeuroMathPlayground -e 'using Pkg; Pkg.test()'`) |

The full pre-pivot README is preserved at
[`6198dea`](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/blob/6198dea/README.md).
