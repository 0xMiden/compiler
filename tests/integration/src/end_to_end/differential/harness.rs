//! Differential fuzzing harness for the Miden compiler.
//!
//! Each test case under `cases/` is just the body of a
//! `#[unsafe(no_mangle)] pub extern "C" fn entrypoint(u32, u32) -> u32`
//! plus any helpers it needs. [`run_case`] prepends a fixed header
//! (`#![no_std]` + `#[panic_handler]`) before writing the case as `src/lib.rs`
//! of a generated cargo project, builds it twice — natively as a host `cdylib`
//! and via `cargo-miden` to a MASM package — and compares outputs across
//! boundary-biased random `(u32, u32)` inputs (uniform draws mixed with a
//! table of width/sign-boundary values and occasional forced-equal pairs).
//! [`run_case_with_inputs`] does the same but
//! against an explicit list of inputs, for pinning a known divergence.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;
use proptest::{
    prelude::*,
    sample,
    test_runner::{Config, FileFailurePersistence, TestRunner},
};

use crate::{CompilerTest, project, testing::executor_with_std};

/// How [`run_case_inner`] supplies the `(input1, input2)` pairs to compare.
enum Inputs<'a> {
    /// 16 boundary-biased random pairs via proptest (see [`fuzz_pair`]) — the
    /// default fuzzing mode.
    Random16,
    /// A fixed list of pairs — deterministic regression inputs, e.g. for
    /// pinning a known divergence independently of the fuzzer.
    Explicit(&'a [(u32, u32)]),
}

/// `u32` values at the semantic boundaries the differential corpus probes:
/// algebraic identities, shift counts at every lane width the cases build
/// from their inputs (8/16/32/64/128, each ±1), sub-word sign/max values, and
/// the i32/u32 sign and max boundaries. Uniform draws essentially never
/// produce any of these.
#[rustfmt::skip]
const INTERESTING_U32: &[u32] = &[
    0, 1, 2, 3,                            // identities, zero-length, zero/one-trip
    7, 8, 9, 15, 16, 17,                   // i8/i16 lane-width shift counts
    31, 32, 33, 63, 64, 65, 127, 128, 129, // u32/u64/u128 width boundaries; i8 sign
    255, 256,                              // u8::MAX boundary
    0x7FFF, 0x8000, 0xFFFF, 0x1_0000,      // i16/u16 sign and max boundaries
    0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, // i32 MAX / MIN / MIN+1
    0xFFFF_FFFE, 0xFFFF_FFFF,              // u32 MAX-1 / MAX (-2 / -1 as i32)
];

/// Boundary-biased `u32`: half uniform, half drawn from [`INTERESTING_U32`],
/// so edge semantics (zeros, MIN/MAX, width-boundary shift counts) are
/// exercised as a matter of course rather than once in 2^32 draws, while bulk
/// random values stay represented.
fn fuzz_u32() -> impl Strategy<Value = u32> {
    prop_oneof![
        1 => any::<u32>(),
        1 => sample::select(INTERESTING_U32),
    ]
}

/// The input-pair distribution for [`Inputs::Random16`]: independent
/// boundary-biased components, plus 1 pair in 8 forced equal — a relation
/// independent draws essentially never hit (`divisor == dividend`, `x ⋄ x`
/// self-application shapes).
fn fuzz_pair() -> impl Strategy<Value = (u32, u32)> {
    prop_oneof![
        7 => (fuzz_u32(), fuzz_u32()),
        1 => fuzz_u32().prop_map(|a| (a, a)),
    ]
}

/// Compiles `source` for the host and for MASM, then compares the
/// `entrypoint(u32, u32) -> u32` outputs across 16 boundary-biased random
/// input pairs (see [`fuzz_pair`]).
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
        let exec =
            executor_with_std(vec![Felt::new_unchecked(a as u64), Felt::new_unchecked(b as u64)]);
        let masm_out: u32 = exec.execute_into(package.clone(), test.session.source_manager.clone());
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
                .run(&fuzz_pair(), |(a, b)| {
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
pub(crate) const CASE_HEADER: &str = r#"#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(target_family = "wasm"))]
#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

"#;

pub(crate) fn cargo_toml(pkg_name: &str) -> String {
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

pub(crate) fn miden_project_toml(pkg_name: &str) -> String {
    format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"

[[bin]]
name = "{pkg_name}"
path = "src/lib.rs"

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
    // `CARGO_TARGET_DIR` is inherited rather than cleared, so this build joins the
    // one build directory the rest of the suite shares (`cargo_proj::shared_build_dir`)
    // instead of giving each of the 90+ cases a private `target/`. Nothing here
    // depends on where the artifact lands: the path is read back out of cargo's JSON
    // output, and these units differ from every other unit in the directory by their
    // `RUSTFLAGS`, which Cargo already folds into the metadata hash.
    let mut child = Command::new("cargo")
        .current_dir(project_root)
        .args(["build", "--release", "--lib", "--message-format=json-render-diagnostics"])
        .env("RUSTFLAGS", "-C default-linker-libraries=yes")
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
