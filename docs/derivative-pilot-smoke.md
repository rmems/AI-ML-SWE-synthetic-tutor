# Derivative pilot: local smoke

This is a fixture for [the derivative-misconceptions pilot (#20)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20) and its [generation spec (#41)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/41). It is not a release candidate or a generation batch.

Run from the repository root with the pinned Rust toolchain and Julia 1.13:

```sh
julia --project=math-julia -e 'using Pkg; Pkg.instantiate()'
julia --project=math-julia math-julia/bin/serve.jl
```

In a second terminal:

```sh
cargo test --locked -p tutor-generate --test pilot_smoke -- --ignored
```

Use the default loopback worker endpoint, or set `TUTOR_MATH_ENDPOINT` to the
same numeric TCP loopback address or absolute Unix socket endpoint in both
terminals. The test runs `tutor-generate run` and
`tutor-generate simulate` with fixed seeds, requires zero rejected fixtures
and an independent verification record on every problem, and assembles and
validates the dialogue fixtures in memory. Two product-rule and two chain-rule
corrections plus one power-rule confirmation exercise the pilot's 80/20 mix.
The test replays the same five cases and requires identical dialogue rows.
It makes no teacher calls and keeps artifacts in a temporary directory.

The fixture checks every row against the committed dialogue JSON Schema and
the same policy validator used by `tutor-validate`. Its early-answer check
covers tutor turns: the diagnosis and hints cannot contain a numeric answer,
and only the tutor's closing turn confirms it. The student can state an
answer in a confirmation or final retry, as required by tutor policy v1.

The real pilot still needs release-pipeline wiring, deduplication and held-out
decontamination, a split/run manifest, and at least 30 recorded human QA
verdicts before export. The fixture does not plant power-rule overgeneralisation
or algebra sign errors; the current simulator does not implement those two
transformations. Derivative-definition and implicit-differentiation problems
are also outside this fixture.
