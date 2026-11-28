//! Print `schemas/run-log.schema.json`:
//! `cargo run -p tutor-runlog --example print_run_log_schema > schemas/run-log.schema.json`

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&tutor_runlog::run_log_json_schema())
            .expect("schema serializes")
    );
}
