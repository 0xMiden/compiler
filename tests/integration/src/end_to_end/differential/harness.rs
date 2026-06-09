//! Differential fuzzing harness for the Miden compiler.
//!
//! Each test case under `cases/` is just the body of a
//! `#[unsafe(no_mangle)] pub extern "C" fn entrypoint(u32, u32) -> u32`
//! plus any helpers it needs. [`run_case`] prepends a fixed header
//! (`#![no_std]` + `#[panic_handler]`) before writing the case as `src/lib.rs`
//! of a generated cargo project, builds it twice — natively as a host `cdylib`
//! and via `cargo-miden` to a MASM package — and compares outputs across
//! random `(u32, u32)` inputs. [`run_case_with_inputs`] does the same but
//! against an explicit list of inputs, for pinning a known divergence.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, TestRunner},
};

use crate::{CompilerTest, project, testing::executor_with_std};

/// How [`run_case_inner`] supplies the `(input1, input2)` pairs to compare.
enum Inputs<'a> {
    /// 16 random pairs via proptest — the default fuzzing mode.
    Random16,
    /// A fixed list of pairs — deterministic regression inputs, e.g. for
    /// pinning a known divergence independently of the fuzzer.
    Explicit(&'a [(u32, u32)]),
}

/// Compiles `source` for the host and for MASM, then compares the
/// `entrypoint(u32, u32) -> u32` outputs across 16 random input pairs.
///
/// `name` must be unique per case; it is used as the generated package name.
pub(super) fn run_case(name: &str, source: &str) {
    run_case_inner(name, source, Inputs::Random16);
}

/// Like [`run_case`], but compares against an explicit, deterministic list of
/// `(input1, input2)` pairs instead of random fuzzing.
///
/// Use this to pin a specific divergence (e.g. an input that a fuzzed case
/// flagged) as its own reproducer, so it fails reliably on exactly that input
/// rather than only when proptest happens to draw it.
pub(super) fn run_case_with_inputs(name: &str, source: &str, inputs: &[(u32, u32)]) {
    assert!(!inputs.is_empty(), "run_case_with_inputs requires at least one input pair");
    run_case_inner(name, source, Inputs::Explicit(inputs));
}

/// Shared body of [`run_case`] / [`run_case_with_inputs`]: build the case both
/// natively and to MASM, then compare `entrypoint` outputs for the requested
/// inputs.
fn run_case_inner(name: &str, source: &str, inputs: Inputs<'_>) {
    let pkg_name = format!("differential_{name}");
    let manifest = cargo_toml(&pkg_name);
    let miden_project_manifest = miden_project_toml(&pkg_name);
    let full_source = format!("{CASE_HEADER}{source}");

    let masm_proj = project(&format!("{pkg_name}_masm"))
        .file("miden-project.toml", &miden_project_manifest)
        .file("Cargo.toml", &manifest)
        .file("src/lib.rs", &full_source)
        .build();
    let mut test = CompilerTest::rust_source_cargo_miden(
        masm_proj.root(),
        WasmTranslationConfig::default(),
        [],
    );
    let package = test.compile_package();

    let native_proj = project(&format!("{pkg_name}_native"))
        .file("Cargo.toml", &manifest)
        .file("src/lib.rs", &full_source)
        .build();
    let dylib_path = build_host_cdylib(&native_proj.root(), &pkg_name);

    let lib = unsafe { libloading::Library::new(&dylib_path) }
        .unwrap_or_else(|e| panic!("failed to load {}: {e}", dylib_path.display()));
    type EntryFn = unsafe extern "C" fn(u32, u32) -> u32;
    let entry: libloading::Symbol<EntryFn> = unsafe { lib.get(b"entrypoint\0") }
        .unwrap_or_else(|e| panic!("missing `entrypoint` in {}: {e}", dylib_path.display()));

    // Run the case for one input pair and return `(native_out, masm_out)`.
    let eval = |a: u32, b: u32| -> (u32, u32) {
        let native_out = unsafe { entry(a, b) };
        let exec = executor_with_std(
            vec![Felt::new_unchecked(a as u64), Felt::new_unchecked(b as u64)],
            Some(&package),
        );
        let masm_out: u32 =
            exec.execute_into(&package.unwrap_program(), test.session.source_manager.clone());
        (native_out, masm_out)
    };

    match inputs {
        // Proptest: 16 cases, shrinking disabled — the whole case file IS the
        // reduced reproducer, so shrinking individual inputs adds no value.
        // The shrinking generates a lot of noise that messes up the feedback for the agent. We
        // want to capture the exact inputs that triggered the miscompilation. Shrunk inputs might
        // trigger another code path (another miscompilation?).
        Inputs::Random16 => {
            let cfg = Config {
                cases: 16,
                max_shrink_iters: 0,
                failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
                ..Config::default()
            };
            TestRunner::new(cfg)
                .run(&(any::<u32>(), any::<u32>()), |(a, b)| {
                    let (native_out, masm_out) = eval(a, b);
                    prop_assert_eq!(
                        native_out,
                        masm_out,
                        "native vs masm mismatch for inputs ({}, {})",
                        a,
                        b
                    );
                    Ok(())
                })
                .unwrap_or_else(|err| panic!("{name}: {err}"));
        }
        Inputs::Explicit(pairs) => {
            for &(a, b) in pairs {
                let (native_out, masm_out) = eval(a, b);
                assert_eq!(
                    native_out, masm_out,
                    "{name}: native vs masm mismatch for inputs ({a}, {b})"
                );
            }
        }
    }
}

/// Prepended to every case source before compilation — supplies the
/// crate-level `#![no_std]` attribute and a minimal `#[panic_handler]` so each
/// case file only has to contain the entrypoint function and its helpers.
///
/// The `rust_eh_personality` stub is required for the native `cdylib`: even
/// though the case is built with `panic = "abort"`, the precompiled `core`
/// library is built with `panic = "unwind"`, so any case that references
/// `core`'s panic machinery (an impossible trap, a guarded index, …) links in
/// unwind tables that reference `rust_eh_personality`. Without `std` nothing
/// defines that symbol, leaving the `cdylib` with an undefined symbol that
/// `dlopen` rejects on Linux (macOS tolerates it). The no-op definition makes
/// the library self-contained; it is never invoked, because panics abort. It is
/// gated to non-wasm so the `cargo-miden` (wasm → MASM) build is unchanged.
const CASE_HEADER: &str = r#"#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(target_family = "wasm"))]
#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

"#;

fn cargo_toml(pkg_name: &str) -> String {
    format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = 3
panic = "abort"

[profile.dev]
panic = "abort"
"#
    )
}

fn miden_project_toml(pkg_name: &str) -> String {
    format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"

[[bin]]
name = "{pkg_name}"
path = "<virtual>"

[dependencies]
miden-core = "*"
"#
    )
}

/// Build `project_root` as a host-target release cdylib and return the produced library path.
///
/// The artifact path is read directly from cargo's JSON build output rather than guessed at,
/// which keeps this robust to platform-specific naming, inherited target-dir overrides
/// (e.g. `CARGO_TARGET_DIR` set by `cargo llvm-cov` or `cargo make`), and any future cargo
/// changes to where cdylibs end up.
fn build_host_cdylib(project_root: &std::path::Path, pkg_name: &str) -> PathBuf {
    // A `no_std` cdylib normally drops the platform runtime libraries, which on
    // macOS leaves `dyld_stub_binder` unresolved at link time. Force rustc to
    // link the default platform libs (libSystem/libc) so the resulting dylib is
    // loadable via `libloading`.
    //
    // Clear `CARGO_TARGET_DIR` so the case project uses its own `target/` rather
    // than the parent's redirected one.
    let mut child = Command::new("cargo")
        .current_dir(project_root)
        .args(["build", "--release", "--lib", "--message-format=json-render-diagnostics"])
        .env("RUSTFLAGS", "-C default-linker-libraries=yes")
        .env_remove("CARGO_TARGET_DIR")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn cargo for native build");

    let stdout = child.stdout.take().expect("piped stdout");
    let reader = std::io::BufReader::new(stdout);
    let mut artifact: Option<PathBuf> = None;
    for msg in cargo_metadata::Message::parse_stream(reader) {
        if let cargo_metadata::Message::CompilerArtifact(a) =
            msg.expect("malformed cargo JSON message")
            && a.target.name == *pkg_name
            && a.target.kind.iter().any(|k| matches!(k, cargo_metadata::TargetKind::CDyLib))
        {
            artifact = a
                .filenames
                .into_iter()
                .find(|p| matches!(p.extension(), Some("dylib" | "so" | "dll")))
                .map(Into::into);
        }
    }

    let status = child.wait().expect("failed to wait on cargo");
    assert!(status.success(), "native cargo build failed for `{pkg_name}`");

    artifact.unwrap_or_else(|| {
        panic!(
            "cargo emitted no cdylib artifact for `{pkg_name}` under {}",
            project_root.display()
        )
    })
}
