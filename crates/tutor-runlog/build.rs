//! Records the rustc and cargo versions so every run-log entry can name the
//! toolchain that produced it, plus the source checkout's commit so the
//! entry names the code that produced the batch rather than whatever
//! repository the CLI happened to run from.

fn main() {
    let rustc = std::env::var("RUSTC").expect("cargo sets RUSTC");
    println!(
        "cargo:rustc-env=TUTOR_RUNLOG_RUSTC_VERSION={}",
        tool_version(&rustc)
    );
    let cargo = std::env::var("CARGO").expect("cargo sets CARGO");
    println!(
        "cargo:rustc-env=TUTOR_RUNLOG_CARGO_VERSION={}",
        tool_version(&cargo)
    );
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=CARGO");

    if let Some(commit) = git_head() {
        println!("cargo:rustc-env=TUTOR_RUNLOG_GIT_COMMIT={commit}");
    }
    // Rebuild when the checkout's HEAD moves (e.g. a branch switch) so the
    // recorded commit stays accurate. Not all checkouts have .git/HEAD —
    // worktrees keep a .git file instead — so only watch what exists.
    if std::path::Path::new("../../.git/HEAD").exists() {
        println!("cargo:rerun-if-changed=../../.git/HEAD");
    }
}

fn tool_version(tool: &str) -> String {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned())
}

/// `git rev-parse HEAD` of the pipeline source checkout at build time;
/// `None` when the crate is built outside a git worktree or git is
/// unavailable (the run entry then records `git_commit: null`).
fn git_head() -> Option<String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!head.is_empty()).then_some(head)
}
