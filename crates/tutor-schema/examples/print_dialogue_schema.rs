//! Print the generated `schemas/dialogue.schema.json` to stdout.
//!
//! Run `cargo run -p tutor-schema --example print_dialogue_schema` and write
//! the output over `schemas/dialogue.schema.json` whenever the dialogue row
//! types change; `tests/dialogue.rs` fails while the two drift apart.

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&tutor_schema::dialogue_json_schema())
            .expect("generated schema serializes")
    );
}
