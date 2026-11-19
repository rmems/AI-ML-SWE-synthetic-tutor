# crates/

The Rust workspace that runs the pipeline. `tutor-schema` holds row types and validation, `tutor-wire` is the ZMQ client for the Julia worker, and `tutor-verify` independently checks answers (and holds the bridge proof test). `tutor-generate`, `tutor-teachers`, and `tutor-build` are placeholders for generators and the student simulator, teacher model clients, and assembly/splits/export.
