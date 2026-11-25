//! Print the generated JSON Schema for `StudentAttempt`.

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&tutor_schema::student_attempt_json_schema())
            .expect("schema serializes")
    );
}
