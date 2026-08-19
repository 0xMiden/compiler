//! Tests for the contract `build.rs` package-cache population (#1298).
//!
//! The script under test is the file the templates and examples ship, included byte-for-byte
//! from the canonical copy (the account template); [`template_build_scripts_are_identical`]
//! pins every other copy to those bytes, so these tests cover exactly what users get. The script
//! makes plain `cargo check`/`cargo build` and IDE analysis resolve compiled dependency
//! packages: outside a midenc-driven build it stages a package cache under its `OUT_DIR`, fills
//! it with a nested `cargo miden build --release` that adopts the staged directory through
//! `MIDENC_PACKAGE_CACHE`, and exports the same variable to macro expansion.
//! Complete compiler-produced input records let Cargo skip unchanged staging; opaque records
//! force staging on every relevant invocation. Successful results are published into immutable,
//! content-addressed generations so older IDE readers remain coherent while byte-identical
//! re-staging reuses the existing generation.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use super::basic_wallet_swapp_note_project;
use crate::cargo_proj::project;

/// The canonical contract build script; every template ships these exact bytes.
const TEMPLATE_BUILD_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../extra/templates/rust/account/template/build.rs"
));

/// Returns the repository root of this workspace.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the integration tests live under tests/integration")
}

/// Every template and every Miden example must ship the canonical build script byte-for-byte.
///
/// This is what entitles the tests in this module to speak for all of them while executing
/// one included copy.
#[test]
fn template_build_scripts_are_identical() {
    let templates = workspace_root().join("extra").join("templates");
    let mut copies = Vec::new();

    // Every cargo-generate contract template must carry the script; discovery instead of a
    // hardcoded list, so a future template cannot escape the pin.
    for entry in fs::read_dir(templates.join("rust")).expect("failed to list the rust templates") {
        let template =
            entry.expect("failed to read a rust templates entry").path().join("template");
        if template.is_dir() {
            copies.push(template.join("build.rs"));
        }
    }
    // Every scaffold contract must carry it too.
    for entry in fs::read_dir(templates.join("project").join("contracts"))
        .expect("failed to list the scaffold contracts")
    {
        let contract = entry.expect("failed to read a scaffold contracts entry").path();
        if contract.join("miden-project.toml").is_file() {
            copies.push(contract.join("build.rs"));
        }
    }
    // And every example that is a Miden project, so IDE analysis of the examples works the
    // same way it does for generated projects.
    let examples = workspace_root().join("examples");
    for entry in fs::read_dir(&examples).expect("failed to list the examples directory") {
        let example = entry.expect("failed to read an examples entry").path();
        if example.join("miden-project.toml").is_file() {
            copies.push(example.join("build.rs"));
        }
    }
    assert!(
        copies.len() > 7,
        "the discovery walks must find the contract templates and example projects"
    );

    for copy_path in copies {
        let bytes = fs::read(&copy_path)
            .unwrap_or_else(|err| panic!("missing build.rs copy '{}': {err}", copy_path.display()));
        assert_eq!(
            bytes,
            TEMPLATE_BUILD_SCRIPT.as_bytes(),
            "'{}' differs from the canonical rust/account/template/build.rs; keep every build.rs \
             copy byte-identical",
            copy_path.display()
        );
    }
}

/// The canonical script carries focused tests for its dependency-free publication protocol.
/// Compile it as a test target explicitly: Cargo normally treats `build.rs` only as a build
/// script, so `#[cfg(test)]` tests in that file are otherwise invisible to `cargo test`.
#[test]
fn template_build_script_publication_protocol_tests_pass() {
    let scratch = std::env::temp_dir().join(format!(
        "miden-build-script-tests-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let test_binary = scratch.join(format!("build-script-tests{}", std::env::consts::EXE_SUFFIX));
    let source = workspace_root().join("extra/templates/rust/account/template/build.rs");
    let compile = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition", "2024", "--test"])
        .arg(&source)
        .arg("-o")
        .arg(&test_binary)
        .output()
        .expect("failed to compile the canonical build script as a test target");
    assert!(
        compile.status.success(),
        "failed to compile '{}':\n{}",
        source.display(),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&test_binary)
        .arg("--nocapture")
        .output()
        .expect("failed to run the canonical build-script tests");
    assert!(
        run.status.success(),
        "canonical build-script tests failed:\n{}",
        String::from_utf8_lossy(&run.stdout)
    );
    fs::remove_dir_all(scratch).unwrap();
}

/// Builds the workspace's `cargo-miden` binary once and returns its path.
fn cargo_miden_binary() -> &'static Path {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace_root = workspace_root();
        let output = std::process::Command::new("cargo")
            .args(["build", "-p", "cargo-miden", "--bin", "cargo-miden"])
            .current_dir(workspace_root)
            .output()
            .expect("failed to spawn cargo to build cargo-miden");
        assert!(
            output.status.success(),
            "failed to build cargo-miden:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target"));
        // A relative target dir resolves against the build's working directory above.
        let target_dir = if target_dir.is_absolute() {
            target_dir
        } else {
            workspace_root.join(target_dir)
        };
        target_dir.join("debug").join("cargo-miden")
    })
}

/// A tiny cross-platform launcher which counts cargo-miden invocations before forwarding them.
fn counting_cargo_miden_binary() -> &'static Path {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        let directory =
            std::env::temp_dir().join(format!("miden-counting-cargo-miden-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("main.rs");
        let binary = directory.join(format!("cargo-miden-counter{}", std::env::consts::EXE_SUFFIX));
        fs::write(
            &source,
            r#"
use std::{env, fs::OpenOptions, io::Write, process::Command};

fn main() {
    let counter = env::var_os("MIDENC_TEST_CARGO_MIDEN_COUNT").unwrap();
    let mut counter = OpenOptions::new().create(true).append(true).open(counter).unwrap();
    writeln!(counter, "invoked").unwrap();
    let real = env::var_os("MIDENC_TEST_REAL_CARGO_MIDEN").unwrap();
    let status = Command::new(real).args(env::args_os().skip(1)).status().unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
"#,
        )
        .unwrap();
        let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .args(["--edition", "2024"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("failed to compile the counting cargo-miden launcher");
        assert!(
            output.status.success(),
            "failed to compile the counting cargo-miden launcher:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}

/// Runs a plain (non-midenc) `cargo check` of `consumer`, the way an IDE does.
fn plain_cargo_check(consumer: &Path) -> Output {
    Command::new("cargo")
        .arg("check")
        .env("CARGO_MIDEN", cargo_miden_binary())
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(consumer)
        .output()
        .expect("failed to spawn cargo check")
}

fn counted_plain_cargo_check(consumer: &Path, counter: &Path) -> Output {
    Command::new("cargo")
        .arg("check")
        .env("CARGO_MIDEN", counting_cargo_miden_binary())
        .env("MIDENC_TEST_REAL_CARGO_MIDEN", cargo_miden_binary())
        .env("MIDENC_TEST_CARGO_MIDEN_COUNT", counter)
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(consumer)
        .output()
        .expect("failed to spawn counted cargo check")
}

fn cargo_miden_invocations(counter: &Path) -> usize {
    fs::read_to_string(counter).map_or(0, |contents| contents.lines().count())
}

fn force_manifest_change(path: &Path) {
    let mut contents = fs::read_to_string(path).unwrap();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    contents.push_str(&format!("\n# integration-test-run-{}-{nonce}\n", std::process::id()));
    fs::write(path, contents).unwrap();
}

/// Asserts one check succeeded, with its stderr in the failure message.
#[track_caller]
fn assert_check_succeeded(phase: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{phase}: plain cargo check must succeed with the template build script:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Finds every immutable package generation under `target_root`'s cargo target directory.
///
/// The script stages generations in its `OUT_DIR`, whose path embeds a cargo-chosen hash:
/// `<target>[/<triple>]/debug/build/<crate>-<hash>/out/miden-packages/gen-<unique-id>`. The
/// fixtures pin a wasm build target in `.cargo/config.toml`, which puts the script's run
/// directory under the triple subtree, so the scan covers the host layout and every per-triple
/// layout.
/// `target_root` is the directory that owns the check's `target/` — the workspace root for
/// the generated pair, the example itself for the standalone p2id check — and `crate_name`
/// is the cargo package name of the crate whose script staged the cache. Published generations
/// are retained until Cargo removes `OUT_DIR`, so callers compare snapshots rather than relying
/// on mtimes or an advisory current pointer.
fn published_generations(target_root: &Path, crate_name: &str) -> Vec<PathBuf> {
    let target = target_root.join("target");
    let mut build_roots = vec![target.join("debug").join("build")];
    if let Ok(entries) = fs::read_dir(&target) {
        for entry in entries.filter_map(Result::ok) {
            build_roots.push(entry.path().join("debug").join("build"));
        }
    }
    let mut generations = build_roots
        .into_iter()
        .filter_map(|build_root| fs::read_dir(build_root).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(crate_name))
        })
        .flat_map(|build_dir| {
            fs::read_dir(build_dir.join("out").join("miden-packages"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_dir()
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("gen-"))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    generations.sort();
    generations
}

fn private_staging_generations(target_root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![target_root.join("target")];
    let mut staging = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".staging-"))
            {
                staging.push(path);
            } else {
                pending.push(path);
            }
        }
    }
    staging
}

/// Returns Cargo's persisted stdout for the build script that owns `generation`.
fn build_script_output(generation: &Path) -> PathBuf {
    generation
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("a generation must live below <build-script>/out/miden-packages")
        .join("output")
}

/// Returns the immutable generation exported by the most recently executed matching build
/// script. Content-addressed publication may reuse a generation from an earlier test process, so
/// the persisted `rustc-env` directive is authoritative; directory creation is not.
fn exported_generation(target_root: &Path, crate_name: &str) -> PathBuf {
    let mut exported = published_generations(target_root, crate_name)
        .into_iter()
        .filter_map(|generation| {
            let output = build_script_output(&generation);
            let contents = fs::read_to_string(&output).ok()?;
            let selected = contents.lines().find_map(|line| {
                line.strip_prefix("cargo:rustc-env=MIDENC_PACKAGE_CACHE=").map(PathBuf::from)
            })?;
            if selected != generation {
                return None;
            }
            let modified = fs::metadata(&output).ok()?.modified().ok()?;
            Some((modified, output, generation))
        })
        .collect::<Vec<_>>();
    exported.sort();
    exported.pop().map(|(_, _, generation)| generation).unwrap_or_else(|| {
        panic!("no persisted MIDENC_PACKAGE_CACHE directive found for cargo package {crate_name}")
    })
}

/// A plain `cargo check` (the LSP flow) must resolve dependency packages through the template
/// build script and re-stage them when a dependency changes.
///
/// Three phases against one generated basic-wallet/swapp-note pair:
/// 1. the first check stages the packages under the script's `OUT_DIR` with a nested
///    `cargo miden build --release` and exports `MIDENC_PACKAGE_CACHE` to macro expansion;
/// 2. because the Rust dependency's Cargo provenance is explicitly opaque, an unchanged check
///    still runs dependency staging, but byte-identical output reuses the immutable generation;
/// 3. editing the dependency's source alone re-stages a package with different contents.
#[test]
fn rust_sdk_build_script_populates_package_cache_for_plain_cargo_check() {
    let swapp_note_source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/components/swapp-note/src/lib.rs"
    ));
    let project = basic_wallet_swapp_note_project(
        "build_script_package_cache",
        swapp_note_source,
        Some(TEMPLATE_BUILD_SCRIPT),
    );
    let consumer = project.root().join("swapp-note");
    let dependency_source = project.root().join("basic-wallet").join("src").join("lib.rs");
    let consumer_manifest = consumer.join("miden-project.toml");

    // Phase 1: the check stages the dependency package built from the original sources.
    force_manifest_change(&consumer_manifest);
    assert_check_succeeded("initial check", &plain_cargo_check(&consumer));
    let after_initial = published_generations(project.root(), "swapp-note");
    let initial = exported_generation(project.root(), "swapp-note");
    let cached = initial.join("basic-wallet.masp");
    let original_package = fs::read(&cached).expect("failed to read the cached package");
    let script_output = build_script_output(&initial);
    let initial_script_output = fs::read_to_string(&script_output)
        .expect("Cargo must persist the build script's output directives");
    assert!(
        initial_script_output.contains("miden-packages.opaque-"),
        "the opaque build-input record must emit an always-missing sentinel"
    );

    // Phase 2: Rust/Cargo provenance is opaque in schema v1, so even an unchanged check must
    // delegate freshness to nested Cargo. The unique sentinel recorded by this execution proves
    // the build script ran again, while identical staged bytes reuse the immutable generation.
    assert_check_succeeded("unchanged opaque check", &plain_cargo_check(&consumer));
    let after_opaque = published_generations(project.root(), "swapp-note");
    assert_eq!(
        after_opaque, after_initial,
        "byte-identical opaque staging must reuse its content-addressed generation"
    );
    let opaque_script_output = fs::read_to_string(&script_output)
        .expect("Cargo must update the build script's output directives");
    assert_ne!(
        opaque_script_output, initial_script_output,
        "an opaque input record must execute the build script on an unchanged check"
    );
    assert!(
        opaque_script_output.contains("miden-packages.opaque-"),
        "the repeated opaque run must retain its always-missing sentinel"
    );
    assert_eq!(
        fs::read(&cached).expect("the old immutable generation must remain readable"),
        original_package,
        "re-staging must not mutate the immutable generation"
    );

    // Phase 3: the dependency source edit alone re-stages with changed package bytes.
    let original_source = fs::read_to_string(&dependency_source).unwrap();
    let mutation_anchor = "        self.add_asset(asset);";
    assert_eq!(
        original_source.matches(mutation_anchor).count(),
        1,
        "the basic-wallet mutation anchor must match exactly once"
    );
    let changed_source = original_source.replacen(
        mutation_anchor,
        "        self.add_asset(asset);\n        self.remove_asset(asset);\n        \
         self.add_asset(asset);",
        1,
    );
    fs::write(&dependency_source, changed_source).unwrap();
    // No manifest touch: the opaque sentinel makes Cargo execute the staging script.

    assert_check_succeeded("check after dependency edit", &plain_cargo_check(&consumer));
    let refreshed_generation = exported_generation(project.root(), "swapp-note");
    assert_ne!(
        refreshed_generation, initial,
        "the dependency edit must select a different immutable generation"
    );
    let refreshed = refreshed_generation.join("basic-wallet.masp");
    let refreshed_package = fs::read(&refreshed).expect("failed to read the refreshed package");
    assert_ne!(
        refreshed_package, original_package,
        "the re-run script must re-stage the edited basic-wallet package"
    );
    assert_eq!(
        fs::read(cached).expect("the old immutable generation must remain readable"),
        original_package,
        "publishing the changed dependency must not mutate its previous generation"
    );
    assert!(
        private_staging_generations(project.root()).is_empty(),
        "successful staging must not leave private package generations behind"
    );
}

/// The p2id-note example must pass an IDE-style plain `cargo check` in place, through its
/// shipped build script, with the basic-wallet dependency package resolved from the staged
/// package cache.
///
/// The check passing is the load-bearing assertion: the macros can only expand when the
/// staged packages are readable. Concurrent driven builds of this example exchange their
/// packages through their own per-build leases and never touch the staging, so no
/// cross-test locking is needed. The staging may survive from an earlier run, so the
/// package-existence assertion does not attribute the file to this run.
#[test]
fn rust_sdk_build_script_p2id_note_plain_cargo_check() {
    let consumer = workspace_root().join("examples").join("p2id-note");

    assert_check_succeeded("p2id-note check", &plain_cargo_check(&consumer));
    // The example's cargo package is named `p2id`, and that name keys its build directory.
    assert!(
        published_generations(&consumer, "p2id")
            .iter()
            .any(|generation| generation.join("basic-wallet.masp").is_file()),
        "the check must stage basic-wallet.masp under p2id-note's build OUT_DIR"
    );
}

/// A complete input record preserves Cargo's no-op behavior.
///
/// This project has only a local MASM dependency: the compiler can describe that source tree and
/// its fixed driver inputs completely, so an unchanged second check must reuse Cargo's persisted
/// build-script output, while an edit inside the tree must publish a new generation.
#[test]
fn complete_build_inputs_keep_an_unchanged_check_fresh() {
    let project = project("build_script_selective_inputs")
        .file(
            "Cargo.toml",
            r#"
[workspace]
members = ["member"]
resolver = "3"
"#,
        )
        .file(
            "miden-project.toml",
            r#"
[workspace]
members = ["member", "masm-dep", "unrelated"]
"#,
        )
        .file(
            "member/Cargo.toml",
            r#"
[package]
name = "selective-inputs"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["rlib"]
"#,
        )
        .file(
            "member/miden-project.toml",
            r#"
[package]
name = "selective-inputs"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:selective-inputs/selective-inputs@0.1.0"
path = "src/lib.rs"

[dependencies]
masm-dep = { path = "../masm-dep" }
"#,
        )
        .file("member/build.rs", TEMPLATE_BUILD_SCRIPT)
        .file("member/src/lib.rs", "")
        .file(
            "masm-dep/miden-project.toml",
            r#"
[package]
name = "masm-dep"
version = "0.1.0"

[lib]
path = "lib/mod.masm"
"#,
        )
        .file("masm-dep/lib/mod.masm", "pub proc entry() -> u32\n    push.1\nend\n")
        .file(
            "unrelated/miden-project.toml",
            r#"
[package]
name = "unrelated"
version = "0.1.0"

[lib]
path = "lib/mod.masm"
"#,
        )
        .file("unrelated/lib/mod.masm", "pub proc unrelated\nend\n")
        .build();
    let consumer = project.root().join("member");

    // Force phase 1 even if this deterministic fixture retained target state from an earlier
    // test process.
    let manifest = consumer.join("miden-project.toml");
    force_manifest_change(&manifest);
    let counter = project.root().join("cargo-miden-invocations");
    let _ = fs::remove_file(&counter);

    assert_check_succeeded(
        "initial selective check",
        &counted_plain_cargo_check(&consumer, &counter),
    );
    assert_eq!(cargo_miden_invocations(&counter), 1);
    let initial = exported_generation(project.root(), "selective-inputs");
    let input_record = fs::read_to_string(initial.join("miden-deps").join("build-inputs"))
        .expect("the compiler must publish the build-input contract");
    assert!(
        !input_record.lines().any(|line| line.starts_with("opaque\t")),
        "the MASM-only dependency graph must have a complete input record:\n{input_record}"
    );
    let workspace_manifest = project.root().join("miden-project.toml");
    assert!(
        input_record
            .lines()
            .any(|line| line == format!("file\t{}", workspace_manifest.display())),
        "the complete record must include the root Miden workspace manifest:\n{input_record}"
    );
    let unrelated_manifest = project.root().join("unrelated/miden-project.toml");
    assert!(
        input_record
            .lines()
            .any(|line| line == format!("file\t{}", unrelated_manifest.display())),
        "the complete record must include every eagerly loaded workspace member:\n{input_record}"
    );

    assert_check_succeeded(
        "unchanged selective check",
        &counted_plain_cargo_check(&consumer, &counter),
    );
    assert_eq!(
        cargo_miden_invocations(&counter),
        1,
        "a complete unchanged input set must leave the build script fresh"
    );
    assert_eq!(
        exported_generation(project.root(), "selective-inputs"),
        initial,
        "an unchanged selective check must keep exporting the same generation"
    );

    let initial_package = fs::read(initial.join("masm-dep.masp"))
        .expect("the selective generation must contain the MASM dependency");
    fs::write(
        project.root().join("masm-dep/lib/mod.masm"),
        "pub proc entry() -> u32\n    push.2\nend\n",
    )
    .unwrap();
    assert_check_succeeded(
        "selective check after MASM edit",
        &counted_plain_cargo_check(&consumer, &counter),
    );
    assert_eq!(
        cargo_miden_invocations(&counter),
        2,
        "editing a selectively watched source tree must re-run staging exactly once"
    );
    let edited = exported_generation(project.root(), "selective-inputs");
    assert_ne!(edited, initial, "a selected tree edit must select a new generation");
    assert_ne!(
        fs::read(edited.join("masm-dep.masp"))
            .expect("the edited generation must contain the MASM dependency"),
        initial_package,
        "the selected tree edit must refresh the compiled dependency"
    );
    assert!(
        private_staging_generations(project.root()).is_empty(),
        "successful selective staging must not leave private generations behind"
    );
}

/// A missing `cargo-miden` is a hard build-script error with an actionable message.
#[test]
fn rust_sdk_build_script_fails_without_cargo_miden() {
    let project = project("build_script_missing_tool")
        .file(
            "Cargo.toml",
            r#"
[package]
name = "missing-tool"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["rlib"]
"#,
        )
        .file(
            "miden-project.toml",
            r#"
[package]
name = "missing-tool"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:missing-tool/missing-tool@0.1.0"
path = "src/lib.rs"
"#,
        )
        .file("build.rs", TEMPLATE_BUILD_SCRIPT)
        .file("src/lib.rs", "")
        .build();

    let output = std::process::Command::new("cargo")
        .arg("check")
        .env("CARGO_MIDEN", project.root().join("definitely-missing-cargo-miden"))
        .env_remove("MIDENC_PACKAGE_CACHE")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(project.root())
        .output()
        .expect("failed to spawn cargo check");
    assert!(!output.status.success(), "cargo check must fail without cargo-miden");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to run `cargo miden build`"),
        "the build script must name the missing tool, got:\n{stderr}"
    );
    assert!(
        private_staging_generations(project.root()).is_empty(),
        "a failed staging command must not leak a private package generation"
    );
}
