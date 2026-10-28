//! Shared harness for the scratch consumer crates that compile and run generated bindings.

use std::{env, fs, path::Path, process::Command};

use midenc_integration_test_support::{scrub_nested_cargo_env, workspace_root};

/// Returns the native rustc host target.
fn host_target() -> String {
    let output = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-vV")
        .output()
        .expect("failed to query the rustc host target");
    assert!(output.status.success(), "rustc -vV failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc -vV did not report a host target")
        .to_owned()
}

/// Copies the workspace patch table into an isolated consumer manifest.
///
/// The consumer crate must resolve the same patched dependencies as the workspace. A
/// workspace without a patch table has nothing to copy, and the function returns an empty
/// string.
fn workspace_patch_section(workspace: &Path) -> String {
    let manifest = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    let mut section = String::new();
    let mut copying = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed == "[patch.crates-io]" {
            copying = true;
        } else if copying && trimmed.starts_with('[') {
            break;
        }
        if copying {
            section.push_str(line);
            section.push('\n');
        }
    }
    section
}

/// Writes a binary crate named `name` with `main_source` into `dir`, which depends on
/// `miden-bindings`, and runs it; panics with its output when it fails to build or run.
pub fn run_consumer(dir: &Path, name: &str, main_source: &str) {
    let workspace = workspace_root();
    fs::create_dir_all(dir.join("src")).unwrap();
    // Seed the workspace lockfile so the offline build resolves to the versions that the
    // workspace already downloaded instead of racing the registry index.
    fs::copy(workspace.join("Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    let bindings_dir = workspace.join("sdk/bindings");
    let patches = workspace_patch_section(&workspace);
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[workspace]

# Keep only file/line debug information: this project exists to prove the
# generated bindings compile and run, and full DWARF for the native Miden
# dependency cone costs gigabytes in the shared test build directory.
[profile.dev]
debug = "line-tables-only"

[dependencies]
miden-bindings = {{ path = {bindings_dir:?} }}

{patches}
"#,
            bindings_dir = bindings_dir.to_string_lossy(),
        ),
    )
    .unwrap();
    fs::write(dir.join("src/main.rs"), main_source).unwrap();

    let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cargo
        .args(["run", "--quiet", "--target"])
        .arg(host_target())
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", workspace.join("target/bindings-consumer"))
        // Share the hash-keyed intermediates with the other test builds; only the
        // name-keyed final artifacts stay in the directory above. Convention from
        // `tests/support`.
        .env("CARGO_BUILD_BUILD_DIR", workspace.join("target/miden_build_cache"))
        .env("CARGO_NET_OFFLINE", "true");
    scrub_nested_cargo_env(&mut cargo);
    let output = cargo
        .output()
        .expect("failed to spawn Cargo for the generated bindings consumer");
    assert!(
        output.status.success(),
        "generated bindings consumer `{name}` failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
