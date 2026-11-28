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

/// Optional additions to the default fixture build.
#[derive(Default)]
struct BuildExtras<'a> {
    /// Extra `--rows` inputs after the valid fixture.
    rows: &'a [&'a Path],
    /// `--previous-manifest`, when the build reruns a release.
    previous_manifest: Option<&'a Path>,
}

fn run_build(out: &Path, extras: &BuildExtras<'_>) -> Output {
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
    args.extend(
        extras
            .rows
            .iter()
            .map(|path| path.to_str().unwrap().to_owned()),
    );
    args.extend([
        "--problems".to_owned(),
        problems.to_str().unwrap().to_owned(),
        "--heldout".to_owned(),
        heldout.to_str().unwrap().to_owned(),
        "--out".to_owned(),
        out.to_str().unwrap().to_owned(),
    ]);
    if let Some(previous) = extras.previous_manifest {
        args.extend([
            "--previous-manifest".to_owned(),
            previous.to_str().unwrap().to_owned(),
        ]);
    }
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

/// Assert the split exports exist, contain only the expected row ids, and
/// carry the export shape (chat messages + provenance).
fn assert_split_exports(out: &Path, expected_ids: &[&str]) {
    let mut accepted_ids = Vec::new();
    for split in ["train", "validation", "test"] {
        let rows = read_jsonl(&out.join(format!("{split}.jsonl")));
        for row in rows {
            assert!(row["messages"].as_array().unwrap().len() >= 3);
            assert_eq!(row["messages"][0]["role"], "system");
            assert!(row["provenance"]["problem_id"].as_str().is_some());
            assert!(row["provenance"]["verification"]["result"] == "pass");
            accepted_ids.push(row["row_id"].as_str().unwrap().to_owned());
        }
    }
    accepted_ids.sort();
    assert_eq!(accepted_ids, expected_ids);
}

/// Assert the rejection log carries stable reason codes per stage.
fn assert_rejections(out: &Path, expected_codes: &[&str]) {
    let rejected = read_jsonl(&out.join("rejected/rejected.jsonl"));
    let mut codes: Vec<String> = rejected
        .iter()
        .map(|r| r["code"].as_str().unwrap().to_owned())
        .collect();
    codes.sort();
    assert_eq!(codes, expected_codes);
    assert!(rejected.iter().all(|r| r["stage"].as_str().is_some()));
}

/// Assert the manifest is self-describing: identity, counts, input and
/// artifact digests, assignments, and a matching `.sha256` sidecar the
/// generation run log (#7) can pick up.
fn assert_manifest(out: &Path, stdout: &str) -> serde_json::Value {
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

    let digest = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&manifest_path).unwrap())
    );
    assert!(
        stdout.contains(&format!("manifest sha256: {digest}")),
        "{stdout}"
    );
    let sidecar = std::fs::read_to_string(out.join("manifest.json.sha256")).unwrap();
    assert!(sidecar.starts_with(&digest));
    manifest
}

/// Assert a second release frozen against `manifest_path` keeps every
/// assignment and records the previous manifest as an input.
fn assert_frozen_rerun(dir: &Path, manifest: &serde_json::Value, manifest_path: &Path) {
    let out2 = dir.join("out2");
    let previous = manifest_path.to_str().unwrap().to_owned();
    let output2 = run_build(
        &out2,
        &BuildExtras {
            previous_manifest: Some(manifest_path),
            ..BuildExtras::default()
        },
    );
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
    assert_eq!(
        manifest2["inputs"]["previous_manifest"][0]["path"]
            .as_str()
            .unwrap(),
        previous,
        "the freeze source must be recorded in release provenance"
    );
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

    let output = run_build(
        &out,
        &BuildExtras {
            rows: &[&broken],
            ..BuildExtras::default()
        },
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_split_exports(
        &out,
        &[
            "fx-confirmation-power-0001",
            "fx-correction-power-0001",
            "fx-handwritten-sign-0001",
        ],
    );
    assert_rejections(
        &out,
        &[
            "exact_duplicate",
            "heldout_exact_match",
            "invalid_json",
            "near_duplicate",
            "schema_constraint",
        ],
    );
    let manifest = assert_manifest(&out, &String::from_utf8_lossy(&output.stdout));
    assert_frozen_rerun(&dir, &manifest, &out.join("manifest.json"));
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

#[test]
fn build_refuses_to_overwrite_a_foreign_manifest() {
    // `--out` pointed at a directory another producer owns (e.g. a
    // generation run): the build must fail instead of replacing its
    // manifest.
    let dir = unique_dir("foreign-manifest");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let manifest_path = out.join("manifest.json");
    let foreign = r#"{"schema":"problem.schema.json","seeds":"0..10"}"#;
    std::fs::write(&manifest_path, foreign).unwrap();

    let output = run_build(&out, &BuildExtras::default());
    assert!(
        !output.status.success(),
        "expected failure, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(&manifest_path).unwrap(),
        foreign,
        "the foreign manifest must be left untouched"
    );
}

/// Two distinct rows (`row-a`, `row-b`) grounding the same fixture problem.
fn two_rows_for_one_problem() -> (serde_json::Value, serde_json::Value) {
    let fixture_rows = std::fs::read_to_string(fixture(&["rows", "valid.jsonl"])).unwrap();
    let mut lines = fixture_rows.lines();
    let mut row_a: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let mut row_b = row_a.clone();
    row_a["row_id"] = serde_json::json!("row-a");
    row_b["row_id"] = serde_json::json!("row-b");
    // Give row-b a different dialogue so it is not a dedup duplicate.
    row_b["turns"][1]["content"] = serde_json::json!(
        "Let f(x) = 2x^3 + x. Compute f'(1).\n\nMy attempt: f'(x) = 2x^3, so f'(1) = 2."
    );
    row_b["turns"][2]["content"] = serde_json::json!(
        "You brought the exponent down on the cubic term but dropped the +x term entirely. What does the power rule give for the x term?"
    );
    row_b["turns"][3]["content"] =
        serde_json::json!("The x term differentiates to 1, so f'(x) = 6x^2 + 1 and f'(1) = 7.");
    row_b["turns"][4]["content"] =
        serde_json::json!("Each term differentiates separately, including a lone x.");
    row_b["turns"][5]["content"] = serde_json::json!("Right, f'(1) = 6 + 1 = 7.");
    row_b["turns"][6]["content"] = serde_json::json!("Confirmed: f'(1) = 7.");
    (row_a, row_b)
}

/// Build `rows` (in order) as release v2 against a previous manifest that
/// froze `frozen_rows`.
fn build_against_frozen(
    dir: &Path,
    rows: &[&serde_json::Value],
    frozen_rows: serde_json::Value,
) -> Output {
    let rows_path = dir.join("rows.jsonl");
    let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
    std::fs::write(&rows_path, text).unwrap();
    let previous = dir.join("previous.json");
    std::fs::write(
        &previous,
        serde_json::json!({
            "tool": {"name": "tutor-build", "version": "0.1.0"},
            "family": "fx-test",
            "assignments": {"rows": frozen_rows, "problems": {}}
        })
        .to_string(),
    )
    .unwrap();
    bin()
        .args([
            "build",
            "--family",
            "fx-test",
            "--release",
            "v2",
            "--rows",
            rows_path.to_str().unwrap(),
            "--problems",
            fixture(&["problems", "problems.jsonl"]).to_str().unwrap(),
            "--heldout",
            fixture(&["heldout"]).to_str().unwrap(),
            "--out",
            dir.join("out").to_str().unwrap(),
            "--previous-manifest",
            previous.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

#[test]
fn build_fails_on_conflicting_frozen_splits() {
    // A stale manifest froze row-a to train and row-b to test; both now
    // ground the same problem, so the freezes cannot all hold.
    let dir = unique_dir("frozen-conflict");
    let (row_a, row_b) = two_rows_for_one_problem();
    let output = build_against_frozen(
        &dir,
        &[&row_a, &row_b],
        serde_json::json!({"row-a": "train", "row-b": "test"}),
    );
    assert!(
        !output.status.success(),
        "expected failure, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("conflicting frozen split assignments"),
        "stderr should name the conflict: {stderr}"
    );
}

#[test]
fn a_new_row_before_a_frozen_row_follows_the_frozen_split() {
    // Only row-a is frozen; row-b is new and comes first in the input. For
    // every split row-a could be frozen to (at least one differs from the
    // problem's hash draw), row-b must inherit it instead of conflicting.
    let (row_a, row_b) = two_rows_for_one_problem();
    for split in ["train", "validation", "test"] {
        let dir = unique_dir(&format!("frozen-order-{split}"));
        let output =
            build_against_frozen(&dir, &[&row_b, &row_a], serde_json::json!({"row-a": split}));
        assert!(
            output.status.success(),
            "{split}: stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let ids: Vec<String> = read_jsonl(&dir.join("out").join(format!("{split}.jsonl")))
            .iter()
            .map(|row| row["row_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            ids,
            ["row-a", "row-b"],
            "{split}: both rows share the frozen split"
        );
    }
}

#[test]
fn an_in_place_rerun_cannot_name_its_own_manifest_as_previous() {
    let dir = unique_dir("self-previous");
    let out = dir.join("out");
    let first = run_build(&out, &BuildExtras::default());
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let manifest_path = out.join("manifest.json");
    let before = std::fs::read(&manifest_path).unwrap();
    // Name the destination through a `..` alias as well as directly.
    let alias = out.join("rejected").join("..").join("manifest.json");
    for previous in [&manifest_path, &alias] {
        let rerun = run_build(
            &out,
            &BuildExtras {
                previous_manifest: Some(previous),
                ..BuildExtras::default()
            },
        );
        assert!(!rerun.status.success());
        assert!(
            String::from_utf8_lossy(&rerun.stderr).contains("is also --previous-manifest"),
            "stderr: {}",
            String::from_utf8_lossy(&rerun.stderr)
        );
        assert_eq!(
            std::fs::read(&manifest_path).unwrap(),
            before,
            "release untouched"
        );
    }
}

#[test]
fn a_failed_rebuild_leaves_the_previous_release_intact() {
    let dir = unique_dir("failed-rebuild");
    let out = dir.join("out");
    let first = run_build(&out, &BuildExtras::default());
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let manifest = std::fs::read(out.join("manifest.json")).unwrap();
    let train = std::fs::read(out.join("train.jsonl")).unwrap();
    // Block the first staged write: its staging name is a directory.
    std::fs::create_dir_all(out.join("train.jsonl.staging")).unwrap();
    let rerun = run_build(&out, &BuildExtras::default());
    assert!(!rerun.status.success(), "the staging write must fail");
    let intact = std::fs::read(out.join("manifest.json")).ok() == Some(manifest)
        && std::fs::read(out.join("train.jsonl")).ok() == Some(train);
    assert!(
        intact,
        "the previous manifest and artifacts survive a failed rebuild"
    );
}
