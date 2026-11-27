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
    // Rebuild when the checkout's commit moves so the recorded commit stays
    // accurate: HEAD itself changes on a branch switch, while a commit or
    // fast-forward on the current branch only touches that branch's ref (or
    // packed-refs). Ask git for the paths so worktrees, whose HEAD lives in
    // the common git dir, are covered too; only watch files that exist.
    let mut watched = vec![git_output(&["rev-parse", "--git-path", "HEAD"])];
    if let Some(branch) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(git_output(&["rev-parse", "--git-path", &branch]));
    }
    watched.push(git_output(&["rev-parse", "--git-path", "packed-refs"]));
    let manifest_dir = std::path::PathBuf::from(manifest_dir());
    for path in watched.into_iter().flatten() {
        let path = manifest_dir.join(path);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
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
    git_output(&["rev-parse", "HEAD"])
}

fn manifest_dir() -> String {
    std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")
}

/// Trimmed stdout of `git <args>` run in this crate's directory; `None`
/// when git fails, is unavailable, or prints nothing.
fn git_output(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(manifest_dir())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}
