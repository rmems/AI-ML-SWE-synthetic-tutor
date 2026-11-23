//! Records the rustc version so `generator.runtime_version` on Rust-generated
//! problems carries the compiler that built the generator.

fn main() {
    let rustc = std::env::var("RUSTC").expect("cargo sets RUSTC");
    let out = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .expect("rustc --version runs");
    let version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    println!("cargo:rustc-env=TUTOR_RUSTC_VERSION={version}");
    println!("cargo:rerun-if-env-changed=RUSTC");
}
