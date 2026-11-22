# crates/

The Rust workspace that runs the pipeline. `tutor-schema` holds problem and dialogue row types, validation, and the generated `dialogue.schema.json`; `tutor-validate` is the dialogue-row validator CLI (clap + jsonschema), `tutor-wire` is the ZMQ client for the Julia worker, and `tutor-verify` independently checks answers (and holds the bridge proof test). `tutor-generate`, `tutor-teachers`, and `tutor-build` are placeholders for generators and the student simulator, teacher model clients, and assembly/splits/export.
