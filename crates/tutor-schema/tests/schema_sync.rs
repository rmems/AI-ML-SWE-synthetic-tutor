//! Keeps `tutor_schema::Problem` and `schemas/problem.schema.json` in line, and checks
//! that invalid problems are rejected.

use serde_json::{Value, json};
use std::collections::BTreeSet;
use tutor_schema::{MAX_SEED, Problem, SchemaError};

const SCHEMA: &str = include_str!("../../../schemas/problem.schema.json");
const FIXTURE: &str = include_str!("fixtures/problem.json");

type Mutation = Box<dyn FnOnce(&mut Value)>;

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("problem.schema.json is valid JSON")
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture is valid JSON")
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

fn strings(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect()
}

fn reject(mutate: impl FnOnce(&mut Value)) -> SchemaError {
    let mut value = fixture();
    mutate(&mut value);
    Problem::from_value(value).expect_err("mutated fixture must be rejected")
}

#[test]
fn fixture_is_valid_and_round_trips() {
    let problem = Problem::from_json(FIXTURE).expect("fixture is valid");
    let encoded = serde_json::to_value(&problem).expect("serializes");
    assert_eq!(encoded, fixture());
}

#[test]
fn top_level_fields_match_schema() {
    let schema = schema();
    assert_eq!(schema["additionalProperties"], json!(false));
    let mut serialized = fixture();
    serialized["verification"] = json!({
        "method": "m", "verifier": "v", "verifier_version": "1"
    });
    Problem::from_value(serialized.clone()).expect("fixture with verification is valid");
    assert_eq!(keys(&schema["properties"]), keys(&serialized));
    let mut required = keys(&serialized);
    required.remove("verification");
    assert_eq!(strings(&schema["required"]), required);
}

#[test]
fn nested_object_fields_match_schema() {
    let schema = schema();
    let defs = &schema["$defs"];
    let value = fixture();
    assert_eq!(
        keys(&defs["generator"]["properties"]),
        keys(&value["generator"])
    );
    assert_eq!(
        strings(&defs["generator"]["required"]),
        keys(&value["generator"])
    );
    assert_eq!(
        strings(&defs["verification"]["required"]),
        keys(&defs["verification"]["properties"])
    );
    assert_eq!(
        keys(&defs["difficulty"]["properties"]),
        keys(&value["difficulty"])
    );
    assert_eq!(
        strings(&defs["difficulty"]["required"]),
        keys(&value["difficulty"])
    );
    assert_eq!(
        keys(&defs["step"]["oneOf"][0]["properties"]),
        keys(&value["steps"][0])
    );
    assert_eq!(
        keys(&defs["step"]["oneOf"][1]["properties"]),
        keys(&value["steps"][1])
    );
    let answer = &defs["answer"]["oneOf"][0];
    assert_eq!(keys(&answer["properties"]), keys(&value["answer"]));
}

#[test]
fn nested_enum_shapes_match_schema() {
    let schema = schema();
    let defs = &schema["$defs"];
    let value = fixture();
    let parameters = &defs["parameters"]["oneOf"][0];
    assert_eq!(keys(&parameters["properties"]), keys(&value["parameters"]));
    assert_eq!(
        parameters["properties"]["kind"]["const"],
        value["parameters"]["kind"]
    );
    assert_eq!(
        parameters["properties"]["coefficients"]["$ref"],
        json!("#/$defs/coefficients")
    );
    assert_eq!(
        defs["coefficients"]["maxItems"],
        json!(tutor_schema::MAX_COEFFICIENTS)
    );
    assert_eq!(defs["parameters"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(defs["step"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(
        schema["properties"]["steps"]["maxItems"],
        json!(tutor_schema::MAX_STEPS)
    );
    assert_eq!(
        schema["properties"]["misconception_ids"]["maxItems"],
        json!(tutor_schema::MAX_MISCONCEPTION_IDS)
    );
    assert_eq!(
        defs["generator"]["properties"]["seed"]["maximum"],
        json!(MAX_SEED)
    );
    assert_eq!(
        strings(&defs["generator"]["properties"]["language"]["enum"]),
        BTreeSet::from(["julia".to_owned(), "rust".to_owned()])
    );
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json!(tutor_schema::PROBLEM_SCHEMA_VERSION)
    );
}

#[test]
fn unknown_and_missing_fields_are_rejected() {
    assert_eq!(reject(|v| v["extra"] = json!(1)).code(), "schema_shape");
    assert_eq!(
        reject(|v| v["generator"]["extra"] = json!(1)).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["parameters"]["extra"] = json!(1)).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["answer"]["extra"] = json!(1)).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| {
            v.as_object_mut().unwrap().remove("answer");
        })
        .code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["parameters"]["kind"] = json!("nope")).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["steps"][0]["kind"] = json!("nope")).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| {
            v.as_object_mut().unwrap().remove("difficulty");
        })
        .code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| {
            v.as_object_mut().unwrap().remove("steps");
        })
        .code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| {
            v.as_object_mut().unwrap().remove("misconception_ids");
        })
        .code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["generator"]["language"] = json!("python")).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["generator"]["seed"] = json!(-1)).code(),
        "schema_shape"
    );
    assert_eq!(
        reject(|v| v["answer"]["value"] = json!(1.5)).code(),
        "schema_shape"
    );
}

#[test]
fn null_verification_is_rejected() {
    let mut value = fixture();
    value["verification"] = Value::Null;
    assert!(Problem::from_json(&value.to_string()).is_err());
}

#[test]
fn constraint_violations_are_rejected() {
    let cases: Vec<Mutation> = vec![
        Box::new(|v| v["schema_version"] = json!(2)),
        Box::new(|v| v["problem_id"] = json!("")),
        Box::new(|v| v["topic"] = json!("Calculus.Derivatives")),
        Box::new(|v| v["topic"] = json!("calculus..derivatives")),
        Box::new(|v| v["generator"]["version"] = json!(0)),
        Box::new(|v| v["generator"]["seed"] = json!(MAX_SEED + 1)),
        Box::new(|v| v["statement"] = json!("")),
        Box::new(|v| v["parameters"]["coefficients"] = json!([])),
        Box::new(|v| v["parameters"]["coefficients"] = json!([1, 2, 0])),
        Box::new(|v| v["parameters"]["coefficients"] = json!([0])),
        Box::new(|v| v["parameters"]["coefficients"] = json!(vec![1; 17])),
        Box::new(|v| {
            v["verification"] = json!({"method": "", "verifier": "v", "verifier_version": "1"})
        }),
        Box::new(|v| v["difficulty"]["max_degree"] = json!(0)),
        Box::new(|v| v["difficulty"]["coefficient_bound"] = json!(0)),
        Box::new(|v| v["difficulty"]["point_bound"] = json!(0)),
        Box::new(|v| v["steps"] = json!([])),
        Box::new(|v| v["steps"][0]["input"] = json!([])),
        Box::new(|v| v["steps"][1]["coefficients"] = json!([1, 2, 0])),
        Box::new(|v| v["misconception_ids"] = json!([])),
        Box::new(|v| v["misconception_ids"] = json!(vec!["derivatives.chain_rule"; 9])),
    ];
    // Valid shape but unknown taxonomy IDs are schema_shape (enum rejects them).
    assert_eq!(
        reject(|v| v["misconception_ids"] = json!(["derivatives.no_such_id"])).code(),
        "schema_shape"
    );
    for mutate in cases {
        assert_eq!(reject(mutate).code(), "schema_constraint");
    }
}
