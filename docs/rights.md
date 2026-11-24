# Rights registry

Which generation sources may produce training rows, and on what evidence.
Spec: [issue #17](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/17)
(Linear twin RM-2500). Same gate as
[synthetic-factory#186](https://github.com/rmems/synthetic-factory/issues/186),
applied to this repo's teachers.

## Rules

- **No snapshot, no generation.** A third-party source may not produce rows
  unless it has an `allowed` entry below carrying a dated terms snapshot and
  the exact clause relied on. Missing or unproducible evidence leaves the
  entry pending — never mark `allowed` on a family name alone.
- **First-party exception.** Procedural generation is our own code; it needs
  no third-party terms snapshot. Its evidence is the repo itself (generator +
  independent verifier), recorded in its entry.
- **Fail-closed covers everything.** Any source not listed here has no entry
  and cannot be used; a generation spec naming an unlisted source cannot
  proceed. Lanes in this repo's scope that are blocked are listed as blocked,
  not omitted.
- Per the roster scope update of 2026-11-23, this registry covers only the
  sources this repo uses. Consumer research-only lanes (Claude/Fable, Grok)
  are registered in synthetic-factory's registry, not here — and fail-closed
  bars them in this repo regardless, since they have no entry.
- Every generated record's **rights envelope names teacher + route + serving
  vendor**, and stays compatible with the downstream ingest gate
  ([rmems/peira#63](https://github.com/rmems/peira/issues/63)), which consumes
  rights evidence rather than trusting paths.
- **Route (roster-wide, decided 2026-11-21):** Merge Gateway primary,
  OpenRouter fallback. All roster pins are confirmed in the Gateway catalog.

## Entries

### 1. DeepSeek V4.1 Flash — `allowed`

- **Channel/route:** DeepSeek Open Platform API (API-only; too large to run on
  the local RTX 5080). Under the roster-wide route rule, gateway-routed calls
  (Merge Gateway primary / OpenRouter fallback) additionally require the
  route's model-specific terms to permit training use — see "Route terms"
  below; until that snapshot is pinned, `allowed` covers the Open Platform API
  channel only.
- **Terms:** DeepSeek Open Platform Terms of Service —
  <https://cdn.deepseek.com/policies/en-US/deepseek-open-platform-terms-of-service.html>
  (release 2026-04-22, effective 2026-04-29). Snapshot pinned 2026-11-24.
- **Clause relied on:** §4.2(3) — "You may apply the Inputs and Outputs of the
  Services to a wide range of use cases, including personal use, academic
  research, derivative product development, training other models (such as
  model distillation), etc."
- **Verified:** output-use evidence verified 2026-11-20 (Raul, via KAI); terms
  snapshot re-verified and pinned 2026-11-24 (Devin, issue #17).
- **Notes:** decision record
  [synthetic-factory#186](https://github.com/rmems/synthetic-factory/issues/186).
- **Route terms (gateway routes, pending model-specific snapshot):**
  - Merge Gateway Terms — <https://www.merge.dev/legal/gateway-terms>
    (snapshot 2026-11-24). §4.2 requires compliance with each AI Provider's
    terms.
  - OpenRouter Terms of Service — <https://openrouter.ai/terms>
    (last updated 2026-08-31; snapshot 2026-11-24). §5.1/§6.1: output use is
    governed by the applicable Model Terms for the model served.
  - **Pending:** the DeepSeek model entry's Model Terms on each gateway must
    be snapshotted into this entry before gateway-routed DeepSeek calls are
    covered by `allowed`.

### 2. MiMo V2.6 Flash (Xiaomi) — `allowed` (checkpoint pinned)

- **Exact checkpoint:** `XiaomiMiMo/MiMo-V2.6-Flash-RL` (HF) at immutable
  revision `f657ba2f7d24c9adc5730675baaa2d34de878a49` — generation and
  provenance must pin this revision, not the mutable repo name. The
  `MiMo-V2.6-Flash-MOPD` checkpoint named in the roster amendment is **not**
  covered by this entry — it needs its own model-card snapshot and revision
  before use.
- **Channel/route:** open weights (self-host) or API via Merge Gateway primary /
  OpenRouter fallback. **Never the MiMo Token Plan subscription** — it covers
  interactive coding tools, not scripted dataset generation.
- **Licence evidence:** MIT, per the HF model-card metadata (`License: mit`
  tag on `XiaomiMiMo/MiMo-V2.6-Flash-RL`). The weight repos carry no LICENSE
  file, so the card metadata is the licence evidence; card snapshot pinned
  2026-11-24. Xiaomi itself ships a distilled model from MiMo outputs
  (MiMo-V2.6-Distill-Qwen-9B), so distillation is the vendor's own practice.
- **Clause relied on:** the MIT licence's permission grant — "Permission is
  hereby granted, free of charge, to any person obtaining a copy of this
  software and associated documentation files … to use, copy, modify, merge,
  publish, distribute, sublicense, and/or sell copies of the Software" — as
  named by the card's `License: mit` tag, plus the card's **Use
  Restrictions** section (snapshot 2026-11-24), which asks users not to use
  the model or derivatives for unlawful/malicious purposes, IP/privacy
  violations, military purposes, malicious cyber activity, unauthorized
  personal-data processing, or high-risk autonomous actions — tutoring dataset
  generation is none of these, and the card states the notice "does not amend
  the applicable model licence."
- **Route terms snapshots:**
  - OpenRouter Terms of Service — <https://openrouter.ai/terms>
    (last updated 2026-08-31; snapshot 2026-11-24). §5.1 makes the applicable
    Model Terms govern whether Inputs/Outputs may be used as intended; §6.1
    sets output ownership per those Model Terms. For this checkpoint the Model
    Terms are the MIT weights licence above.
  - Merge Gateway Terms — <https://www.merge.dev/legal/gateway-terms>
    (snapshot 2026-11-24). §4.2 requires compliance with each AI Provider's
    terms; §5.1 states Merge retains only request/response metadata unless
    payload logging is enabled.
- **Verified:** licence evidence and route snapshots produced and pinned
  2026-11-24 (Devin, issue #17), per the fail-closed rule — allowed only now
  that the snapshot exists.
- **Conditions:** the rights envelope for every MiMo row must name the exact
  checkpoint, route, and serving vendor. Serving via any route whose terms are
  not snapshotted here is not covered by this entry.

### 3. Procedural generation — `allowed` (no teacher)

- **Channel/route:** first-party code — parameterised problem generators in
  Rust (`crates/tutor-generate`) with Julia math workers over the ZMQ bridge
  (`math-julia`, protocol `tutor-math/1`).
- **Terms:** none required — this is our own code and computed answers.
- **Clause relied on:** n/a (no third-party terms).
- **Verified:** pipeline exists per
  [#18](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/18);
  registered 2026-11-24 (Devin, issue #17).
- **Notes:** the preferred source wherever it can produce the family. Teachers
  add dialogue surface and variety on top of verified problems; they never
  originate an answer.

### 4. Nemotron 3 Ultra 550B-A55B — `parked`

- **Status:** parked — removed from the active roster 2026-11-23 (Raul). DeepSeek
  + MiMo are sufficient for now. **Not rights-blocked**: the pin analysis
  stands and a dated amendment can reactivate it. Kept listed so the pin work
  isn't lost and the removal is explicit. While parked it has no `allowed`
  entry and cannot be used.
- **Weights:** `nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16` (HF) at revision
  `77df655d5e9f8362164ed14dd8b48f8bce657498`; HF card tags the licence
  `openmdw-1.1` — card snapshot pinned 2026-11-24.
- **Terms:** OpenMDW License Agreement, version 1.1 (OpenMDW-1.1). The licence
  text snapshot and the exact output-use clause are **still owed** — required
  before any reactivation can mark this `allowed`.
- **Route (when reactivated):** was pinned via OpenRouter; under the roster
  route decision it would go Merge Gateway primary / OpenRouter fallback, with
  the route terms snapshotted into this entry as for MiMo.
- **Verified:** parked status per issue #17 roster update 2026-11-23; licence
  tag confirmed on the HF card 2026-11-24 (Devin).

## Wiring

- The generation-spec template ([#9](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/9),
  pending) will reference this registry: a spec naming a source with no
  `allowed` entry cannot proceed. Until #9 lands, this registry plus the
  tutor-teachers refusal contract below are the enforcement points.
- `crates/tutor-teachers` (reqwest + tokio) consumes these pins and **must
  refuse any teacher without an `allowed` entry** — that refusal contract is
  the wiring requirement noted here. The teacher clients themselves are not
  built in this issue.
- The on-row representation of the rights envelope (teacher + route + serving
  vendor) is owned by the dialogue schema and the downstream ingest gate
  (rmems/peira#63), not this issue.
