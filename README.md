# AI-ML-SWE-synthetic-tutor

Synthetic tutoring datasets for post-training. This repository generates
multi-turn tutoring dialogues — verified problems, synthetic student attempts
with labelled misconceptions, and hint-ladder tutor turns — so a model learns
to **tutor** (diagnose a mistake, ask a targeted question, give a graded hint)
instead of only producing final answers. Sources are procedural generation
plus open-weight / distillable teachers; datasets ship as small per-family
Hugging Face datasets.

Topics: synthetic data · post-training · distillation · tutor

> **Status: pivot in progress.** The Rust workspace, schemas, and generators
> described below are planned under the
> [pivot epic (#4)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/4)
> and do not exist yet. Nothing in this README describes code that is not
> already in the repo unless it says *planned*. The pre-pivot Julia
> playground is still here; see [Legacy](#legacy-neuromathplaygroundjl).

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

A full gold example row will be shown here once the dialogue schema issue
(#13) lands its gold examples.

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
([#7](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/7)).

---

## Stack

| Role | Choice |
|------|--------|
| Pipeline (generators, verification harness, student simulator, assembly, export) | **Rust**, in a Cargo workspace — *planned* ([#5](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/5)) |
| Math-heavy generation and checking (symbolic / numeric work simpler in Julia) | **Julia** math workers, driven by the Rust pipeline over **ZMQ** (REQ/REP, JSON payloads) — the same Rust `zmq` ↔ Julia `ZMQ.jl` bridge used in [Limen-Capital](https://github.com/rmems/Limen-Capital) and [DendriteTrader.jl](https://github.com/rmems/DendriteTrader.jl). Julia is a math engine, not a second pipeline. |
| Teachers | **DeepSeek V4.1 Flash** and **Nemotron 3** (exact variant pinned per use). They vary dialogue surface and critique drafts; they never decide a correct answer — verification does. |
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

No unverified answer ships. Problem generators emit a problem and its answer
together with a replay seed; a separate verification pass then checks that
answer by an independent method (across the Rust / Julia ZMQ boundary for
math-heavy problems, and by compiling and running code for code-bearing
families) and records the method and pass/fail result on the row. A row that
fails or skips verification is never assembled into a dialogue. Details:
[problem generators + verification pipeline (#18)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18).

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

---

## Out of scope

- Training or fine-tuning models (that lives in the training-infra repos)
- A tutoring app, UI, or live tutor service
- Personal study logging (logging survives only as the generation run log)

---

## Legacy: NeuroMathPlayground.jl

Before the pivot this repo was **NeuroMathPlayground.jl**, a personal Julia
playground for math and neuromorphic computing. Its code is kept, not
deleted, and parts of it seed the Julia math workers; the restructure issue
([#5](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/5)) moves it
into `math-julia/` or `legacy/`. Until then it lives where it always has:

| Path | Contents |
|------|----------|
| `src/` | `NeuroMathPlayground` module: LIF / STDP, spike functions, stochastic processes, signals, Kelly, math helpers, GPU detection |
| `examples/`, `sandbox/`, `notebooks/` | Demo scripts, worked examples, Pluto notebook |
| `test/runtests.jl` | Playground test suite (`julia --project=. test/runtests.jl`) |

The full pre-pivot README is preserved at
[`6198dea`](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/blob/6198dea/README.md).
