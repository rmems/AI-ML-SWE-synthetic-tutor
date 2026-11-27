//! End-to-end check of `tutor-build build`: dialogue rows + verified
//! problems + a held-out directory in, split exports + rejections +
//! manifest out. Runs in a temp directory; no real dataset output is
//! produced.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tutor-build"))
}

fn fixture(parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    for part in parts {
        path.push(part);
    }
    path
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "tutor-build-cli-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_build(out: &Path, extra_rows: &[String], extra_args: &[String]) -> Output {
    let rows = fixture(&["rows", "valid.jsonl"]);
    let problems = fixture(&["problems", "problems.jsonl"]);
    let heldout = fixture(&["heldout"]);
    let mut args = vec![
        "build".to_owned(),
        "--family".to_owned(),
        "fx-test".to_owned(),
        "--release".to_owned(),
        "v1".to_owned(),
        "--rows".to_owned(),
        rows.to_str().unwrap().to_owned(),
    ];
    args.extend(extra_rows.iter().cloned());
    args.extend([
        "--problems".to_owned(),
        problems.to_str().unwrap().to_owned(),
        "--heldout".to_owned(),
        heldout.to_str().unwrap().to_owned(),
        "--out".to_owned(),
        out.to_str().unwrap().to_owned(),
    ]);
    args.extend(extra_args.iter().cloned());
    bin().args(args).output().expect("runs tutor-build")
}

fn read_jsonl(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("artifact is JSONL"))
        .collect()
}

#[test]
fn build_end_to_end() {
    let dir = unique_dir("e2e");
    let out = dir.join("out");
    // A second rows file with an unparseable line (kept out of the committed
    // fixtures so fixture lint never sees malformed JSON).
    let broken = dir.join("broken.jsonl");
    let mut file = std::fs::File::create(&broken).unwrap();
    writeln!(file, "{{not json").unwrap();
    drop(file);

    let output = run_build(&out, &[broken.to_str().unwrap().to_owned()], &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Split exports exist and contain only the three accepted rows.
    let mut accepted_ids = Vec::new();
    let mut total = 0usize;
    for split in ["train", "validation", "test"] {
        let rows = read_jsonl(&out.join(format!("{split}.jsonl")));
        total += rows.len();
        for row in rows {
            assert!(row["messages"].as_array().unwrap().len() >= 3);
            assert_eq!(row["messages"][0]["role"], "system");
            assert!(row["provenance"]["problem_id"].as_str().is_some());
            assert!(row["provenance"]["verification"]["result"] == "pass");
            accepted_ids.push(row["row_id"].as_str().unwrap().to_owned());
        }
    }
    accepted_ids.sort();
    assert_eq!(
        accepted_ids,
        [
            "fx-confirmation-power-0001",
            "fx-correction-power-0001",
            "fx-handwritten-sign-0001"
        ]
    );
    assert_eq!(total, 3);

    // Rejections carry stable reason codes per stage.
    let rejected = read_jsonl(&out.join("rejected/rejected.jsonl"));
    let mut codes: Vec<String> = rejected
        .iter()
        .map(|r| r["code"].as_str().unwrap().to_owned())
        .collect();
    codes.sort();
    assert_eq!(
        codes,
        [
            "exact_duplicate",
            "heldout_exact_match",
            "invalid_json",
            "near_duplicate",
            "schema_constraint"
        ]
    );
    assert!(rejected.iter().all(|r| r["stage"].as_str().is_some()));

    // The manifest is written last and self-describing.
    let manifest_path = out.join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["tool"]["name"], "tutor-build");
    assert_eq!(manifest["family"], "fx-test");
    assert_eq!(manifest["release"], "v1");
    assert_eq!(manifest["counts"]["accepted"], 3);
    assert_eq!(manifest["counts"]["rejected"], 5);
    let split_total: u64 = manifest["counts"]["splits"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(split_total, 3);
    assert!(manifest["counts"]["topics"]["calculus.derivatives.power_rule"].is_object());
    assert!(manifest["counts"]["taxonomy_ids"]["none"].is_object());
    assert!(manifest["artifacts"]["train.jsonl"]["sha256"].is_string());
    assert_eq!(manifest["inputs"]["rows"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["inputs"]["problems"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["inputs"]["heldout"].as_array().unwrap().len(), 1);
    assert_eq!(
        manifest["assignments"]["rows"].as_object().unwrap().len(),
        3
    );

    // The printed hash and the sidecar both record the manifest's SHA-256,
    // so the generation run log (#7) can pick it up.
    let digest = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&manifest_path).unwrap())
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("manifest sha256: {digest}")),
        "{stdout}"
    );
    let sidecar = std::fs::read_to_string(out.join("manifest.json.sha256")).unwrap();
    assert!(sidecar.starts_with(&digest));

    // A second release frozen against this manifest keeps every assignment.
    let out2 = dir.join("out2");
    let previous = manifest_path.to_str().unwrap().to_owned();
    let output2 = run_build(&out2, &[], &["--previous-manifest".to_owned(), previous]);
    assert!(
        output2.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output2.stderr)
    );
    let manifest2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out2.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["assignments"]["rows"], manifest2["assignments"]["rows"],
        "frozen rows must not move between releases"
    );
    assert_eq!(
        manifest["assignments"]["problems"], manifest2["assignments"]["problems"],
        "frozen problems must not move between releases"
    );
}

#[test]
fn build_fails_on_missing_heldout_dir() {
    let dir = unique_dir("heldout-missing");
    let out = dir.join("out");
    let rows = fixture(&["rows", "valid.jsonl"]);
    let output = bin()
        .args([
            "build",
            "--family",
            "fx-test",
            "--release",
            "v1",
            "--rows",
            rows.to_str().unwrap(),
            "--heldout",
            dir.join("missing").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn generated_rows_require_problems_for_full_provenance() {
    let dir = unique_dir("no-problems");
    let out = dir.join("out");
    let rows = fixture(&["rows", "valid.jsonl"]);
    let output = bin()
        .args([
            "build",
            "--family",
            "fx-test",
            "--release",
            "v1",
            "--rows",
            rows.to_str().unwrap(),
            "--heldout",
            fixture(&["heldout"]).to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    // The build still succeeds: generated rows are rejected with a reason
    // code, not fatal; the hand-written row ships.
    assert!(output.status.success());
    let rejected = read_jsonl(&out.join("rejected/rejected.jsonl"));
    assert!(
        rejected
            .iter()
            .any(|r| r["code"] == "provenance_missing_problem"),
        "expected a provenance_missing_problem rejection: {rejected:?}"
    );
}
