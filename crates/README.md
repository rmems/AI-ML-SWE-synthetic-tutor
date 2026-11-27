# crates/

The Rust workspace that runs the pipeline.

- `tutor-schema` — problem and dialogue row types and validation; mirrors `schemas/problem.schema.json` and generates `dialogue.schema.json`.
- `tutor-validate` — the dialogue-row validator CLI (clap + jsonschema).
- `tutor-ci` — PR-time data-validation wiring: taxonomy lint, rights gate, and verification smoke tests used by the data-validation workflow.
- `tutor-wire` — the ZMQ REQ client speaking `tutor-math/1` to the Julia worker (`generate_problem`, `verify_answer`, `ping`).
- `tutor-generate` — the generator registry (per-family Rust/Julia split in `GeneratorSpec`), the Rust-native `binomial_derivative_at_point` generator, the `tutor-generate` CLI, and the generate→verify→emit pipeline (rayon, `verified/` + `rejected/` outputs + manifest).
- `tutor-verify` — independent answer verification: Rust recompute + step replay for Julia-generated problems, the `verify_answer` op for Rust-generated ones (the check always runs on the other side of the boundary), rejection reason codes, and the bridge proof test.
- `tutor-teachers`, `tutor-build` — placeholders for teacher model clients and assembly/splits/export.
