use std::{env, str};

fn main() {
    println!("cargo::rerun-if-env-changed=MIDENC_BUILD_VERSION");
    println!("cargo::rerun-if-env-changed=MIDENC_BUILD_REV");
    // Do not declare `rerun-if-env-changed` for Cargo-controlled variables such as
    // `CARGO_PKG_VERSION` or `PROFILE`. Cargo compares them against its own ambient
    // environment, where they are normally unset — but a test or a build script that
    // spawns `cargo` against this target directory has them set, which flips the stored
    // value and forces a full rebuild of every dependent crate on each alternation.

    if let Some(sha) = git_describe() {
        println!("cargo::rustc-env=MIDENC_BUILD_REV={sha}");
    } else {
        println!("cargo::rustc-env=MIDENC_BUILD_REV=unknown");
    }

    if let Ok(version) = env::var("MIDENC_BUILD_VERSION") {
        println!("cargo::rustc-env=MIDENC_BUILD_VERSION={}", version);
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let profile = env::var("PROFILE").unwrap();
    if profile == "debug" {
        println!("cargo::rustc-env=MIDENC_BUILD_VERSION=nightly-{version}");
    } else {
        println!("cargo::rustc-env=MIDENC_BUILD_VERSION={version}");
    }
}

fn git_describe() -> Option<String> {
    use std::process::Command;

    Command::new("git")
        .args(["describe", "--tags", "--always"])
        .output()
        .ok()
        .and_then(|out| str::from_utf8(&out.stdout[..]).map(str::trim).map(str::to_owned).ok())
}
