//! Records the rustc and cargo versions so every run-log entry can name the
//! toolchain that produced it.

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
}

fn tool_version(tool: &str) -> String {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned())
}
