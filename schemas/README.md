# schemas/

Machine-readable contracts for everything the pipeline emits. `problem.schema.json` is the generated-problem contract shared by the Julia worker and `crates/tutor-schema` (a Rust test keeps them in sync); `dialogue.schema.json` is a stub for [#13](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/13); `taxonomy.yaml` is the misconception taxonomy for [#6](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/6) — see [docs/taxonomy.md](../docs/taxonomy.md); its IDs are mirrored as `tutor_schema::MisconceptionId` and kept in sync by a test in `crates/tutor-schema`.
