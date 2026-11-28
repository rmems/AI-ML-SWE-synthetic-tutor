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
    // fast-forward on the current branch only touches that branch's ref,
    // packed-refs, or the reftable index. Ask git for the paths so worktrees,
    // whose HEAD lives in
    // the common git dir, are covered too; only watch files that exist.
    let manifest_dir = std::path::PathBuf::from(manifest_dir());
    let git_path = |name: &str| {
        git_output(&["rev-parse", "--git-path", name]).map(|path| manifest_dir.join(path))
    };
    for path in [
        git_path("HEAD"),
        git_path("packed-refs"),
        // Reftable-backed checkouts record ref updates here rather than in
        // the loose ref or packed-refs watched for the files backend.
        git_path("reftable/tables.list"),
    ]
    .into_iter()
    .flatten()
    {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // A branch that is only in packed-refs has no loose ref file yet; the
    // next commit creates one. Watch its nearest existing directory under
    // `refs/` instead (cargo rescans a watched directory), so that first
    // commit also triggers a rebuild. Never climb above `refs/`: watching
    // the git dir itself would rebuild on every index update.
    if let Some(watch) = branch_ref_watch(&git_path) {
        println!("cargo:rerun-if-changed={}", watch.display());
    }
}

/// The current branch's loose ref, or its nearest existing directory under
/// `refs/` when the ref is only packed; `None` on a detached HEAD.
fn branch_ref_watch(
    git_path: &impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let branch = git_output(&["symbolic-ref", "-q", "HEAD"])?;
    let path = git_path(&branch)?;
    path.ancestors()
        .take_while(|candidate| candidate.components().any(|c| c.as_os_str() == "refs"))
        .find(|candidate| candidate.exists())
        .map(std::path::Path::to_path_buf)
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
