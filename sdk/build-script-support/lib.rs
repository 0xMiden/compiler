//! Build-script support for Rust-based Miden projects.
//!
//! Rust-based Miden projects call [`prepare_package_cache`] from their `build.rs` so plain Cargo
//! builds and IDE analysis can resolve the compiled Miden packages consumed by SDK procedural
//! macros. A project that depends on a Miden package of its own, such as a Miden Assembly path
//! dependency, generates that package's Rust bindings with `generate_bindings`, behind the
//! `bindgen` feature. Crates that declare bindings to Miden procedures compile their linker stubs
//! with [`stubs::compile_stub_archive`].
//!
//! # Dependencies
//!
//! Without the `bindgen` feature this crate carries no dependencies, and that is deliberate: it is
//! compiled inside every Rust-based Miden project's build script, so everything it pulls in is
//! paid for by every `cargo check` of every such project. The generator needs the package reader
//! and with it most of the compiler's dependency graph — some two hundred crates — which is why
//! it is opt-in. The one cost of staying dependency-free is that the spellings of the package
//! cache contract (`MIDENC_PACKAGE_CACHE`, `miden-deps/build-inputs`) are carried here inline
//! rather than taken from `midenc_frontend_wasm_metadata::package_cache`; a test under the feature
//! checks they agree.

// This crate carries no crate-level lint attribute, and that is deliberate in both directions.
//
// No `deny(warnings)`: this crate is compiled inside every generated user project, on whatever
// nightly they happen to have. A lint that rustc adds or widens tomorrow must not turn into a
// hard build failure in a project nobody can patch.
//
// No `allow(warnings)` either: a crate-root lint attribute overrides command-line lint levels,
// so it would also silence this repository's own `-D warnings` and leave the crate unlinted in
// CI. Warnings are denied where they belong, on the CI command line, and this crate is expected
// to stay clean under it.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "bindgen")]
mod bindgen;
#[cfg(feature = "bindgen")]
pub use bindgen::{Bindings, generate_bindings};

// The package cache contract, spelled inline: see the crate docs on dependencies.
/// The environment variable naming the Miden package cache.
const PACKAGE_CACHE_ENV: &str = "MIDENC_PACKAGE_CACHE";
/// The directory, inside the cache, of the compiler's dependency resolution records.
const DEPENDENCY_MANIFEST_DIR: &str = "miden-deps";
/// The compiler's build-inputs record, inside [`DEPENDENCY_MANIFEST_DIR`].
const BUILD_INPUTS_FILE: &str = "build-inputs";

const BUILD_INPUTS_HEADER: &str = "miden-build-inputs\t1";

/// The package cache [`prepare_package_cache`] selected in this build-script process.
///
/// The `cargo:rustc-env` directive that exports the cache reaches the compilation of the crate,
/// not this process, so `generate_bindings` finds it here.
static SELECTED_PACKAGE_CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Populates the Miden package cache for builds that `midenc` does not drive.
///
/// Plain `cargo check`, `cargo build`, and IDE analysis expand the Miden SDK macros without a
/// surrounding `cargo miden build`. Those macros read compiled dependency packages from the
/// directory named by `MIDENC_PACKAGE_CACHE`. The compiler's own package cache is deleted when
/// each build ends, so this script stages generations of its own under `OUT_DIR`, fills a new
/// generation with a nested `cargo miden build` that adopts it through the same variable, and
/// exports the variable to the compilation of the consumer crate.
///
/// Published generations are immutable and retained until Cargo removes `OUT_DIR`, so an IDE or
/// in-flight macro expansion that still names an older generation always sees one consistent
/// build. Byte-identical results share one content-addressed generation. The nested build fills a
/// private generation and only a fully successful build publishes and exports it; a staging
/// failure fails the outer build rather than exporting stale packages.
///
/// The compiler records a versioned invalidation contract next to the staged packages. When every
/// frontend can enumerate its inputs completely, this script translates that record into Cargo
/// change directives. If any frontend or driver input marks its provenance opaque — Rust/Cargo
/// source builds, PATH-resolved launchers, and undiscovered workspace boundaries do so today —
/// the script deliberately re-runs dependency-only staging on every Cargo invocation.
///
/// One sharing caveat: cargo keys build-script output by crate name and version, not by
/// project path. Two different projects with the same package name and version that share one
/// `CARGO_TARGET_DIR` reuse each other's script output, including this staged cache. Use
/// per-checkout target directories for such layouts.
///
/// See <https://github.com/0xMiden/compiler/issues/1298>.
pub fn prepare_package_cache() {
    // The manifests declare the dependency set this script stages packages for. Naming any
    // watch disables cargo's watch-everything default, which would otherwise re-run the
    // nested build after every source edit of the consumer crate.
    println!("cargo:rerun-if-changed=miden-project.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // Re-evaluate when the build mode or the tool selection changes.
    println!("cargo:rerun-if-env-changed={PACKAGE_CACHE_ENV}");
    println!("cargo:rerun-if-env-changed=CARGO_MIDEN");
    // These inputs shape the compiled packages. Cargo prefers the encoded rustflags variable
    // over the plain one, so both spellings are watched.
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");
    println!("cargo:rerun-if-env-changed=MIDENUP_HOME");
    println!("cargo:rerun-if-env-changed=MIDENUP_TOOLCHAIN");
    println!("cargo:rerun-if-env-changed=MIDEN_SYSROOT");
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=CARGO");

    // Inside a midenc-driven build the compiler owns the package cache, macro expansion
    // already sees the variable, and a nested build would recurse into this script forever.
    // An empty value counts as unset, matching the compiler and the SDK macros.
    if let Some(adopted) = configured_package_cache() {
        select_package_cache(&adopted);
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Generations live under this script's OUT_DIR, so they belong to this crate and build
    // configuration and are removed by `cargo clean`.
    let generations = out_dir.join("miden-packages");
    fs::create_dir_all(&generations).expect("failed to create the Miden package cache directory");
    let staging = StagingGeneration::create(&generations);
    let staging_id = staging.id().to_string();

    // Stage the dependency packages: the nested compiler adopts the generation through
    // MIDENC_PACKAGE_CACHE, publishes every dependency package into it before the root
    // target compiles, and leaves the directory in place for the outer build to read.
    let build =
        run_cargo_miden_build(&manifest_dir, staging.path(), &out_dir.join("nested-target"));
    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        // A missing `cargo miden` plugin is a setup error, not a broken build; surface it
        // with installation instructions.
        if stderr.contains("no such command") || stderr.contains("no such subcommand") {
            panic!(
                "`cargo miden build` failed ({}): the `cargo miden` plugin was not \
                 found.\nInstall cargo-miden (`cargo install cargo-miden`) or point the \
                 CARGO_MIDEN environment variable at a cargo-miden binary.",
                build.status,
            );
        }
        panic!(
            "`cargo miden build --release --stop-after=dependencies` failed ({}):\n{}",
            build.status,
            String::from_utf8_lossy(&build.stderr),
        );
    }

    // Read the compiler's invalidation contract before publishing the generation. A record this
    // script cannot read is never permission to cache the build with incomplete change
    // detection: it degrades to opaque, which re-stages on every Cargo invocation.
    let build_inputs = read_build_inputs(staging.path());

    // The child has exited and no reader has ever received the final path, so publication is the
    // immutable generation boundary. Reuse an existing byte-identical generation: opaque input
    // records may re-stage on every Cargo invocation, but unchanged outputs should not consume
    // unbounded disk or churn the cache path seen by rustc.
    let published = staging.publish(&generations);

    emit_build_input_directives(&build_inputs, &out_dir, &staging_id);
    println!("cargo:rustc-env={PACKAGE_CACHE_ENV}={}", published.display());
    select_package_cache(&published);
}

/// The package cache the environment configures: `MIDENC_PACKAGE_CACHE`, absolutized as the
/// compiler and the SDK macros do. An empty value counts as unset.
fn configured_package_cache() -> Option<PathBuf> {
    env::var_os(PACKAGE_CACHE_ENV).filter(|value| !value.is_empty()).map(|value| {
        let path = PathBuf::from(value);
        std::path::absolute(&path).unwrap_or(path)
    })
}

/// Records `cache` as the package cache `generate_bindings` reads in this process.
fn select_package_cache(cache: &Path) {
    let mut selected =
        SELECTED_PACKAGE_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *selected = Some(cache.to_path_buf());
}

/// The package cache [`prepare_package_cache`] selected in this process, if it has run.
#[cfg(feature = "bindgen")]
fn selected_package_cache() -> Option<PathBuf> {
    SELECTED_PACKAGE_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// A private generation which removes itself unless publication succeeds.
struct StagingGeneration {
    id: String,
    path: PathBuf,
    armed: bool,
}

impl StagingGeneration {
    fn create(generations: &Path) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock must be after the Unix epoch")
            .as_nanos();
        for attempt in 0u32..1024 {
            let id = format!("{timestamp:032x}-{:08x}-{attempt:03x}", std::process::id());
            let path = generations.join(format!(".staging-{id}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Self {
                        id,
                        path,
                        armed: true,
                    };
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!(
                    "failed to create the Miden package cache staging directory '{}': {err}",
                    path.display()
                ),
            }
        }
        panic!("failed to allocate a unique Miden package cache generation after 1024 attempts");
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(self, generations: &Path) -> PathBuf {
        let fingerprint = generation_fingerprint(&self.path).unwrap_or_else(|err| {
            panic!(
                "failed to fingerprint staged Miden packages in '{}': {err}",
                self.path.display()
            )
        });
        self.publish_with_fingerprint(generations, fingerprint, |_| {})
    }

    fn publish_with_fingerprint(
        mut self,
        generations: &Path,
        fingerprint: u64,
        mut before_rename: impl FnMut(&Path),
    ) -> PathBuf {
        'candidate: for collision in 0u32..1024 {
            let suffix = if collision == 0 {
                String::new()
            } else {
                format!("-{collision:03x}")
            };
            let published = generations.join(format!("gen-{fingerprint:016x}{suffix}"));
            loop {
                match fs::symlink_metadata(&published) {
                    Ok(_) => {
                        if generations_equal(&self.path, &published).unwrap_or_else(|err| {
                            panic!(
                                "failed to compare staged Miden packages '{}' with '{}': {err}",
                                self.path.display(),
                                published.display()
                            )
                        }) {
                            fs::remove_dir_all(&self.path).unwrap_or_else(|err| {
                                panic!(
                                    "failed to discard duplicate Miden package generation '{}': \
                                     {err}",
                                    self.path.display()
                                )
                            });
                            self.armed = false;
                            return published;
                        }
                        continue 'candidate;
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => panic!(
                        "failed to inspect Miden package generation '{}': {err}",
                        published.display()
                    ),
                }

                before_rename(&published);
                match fs::rename(&self.path, &published) {
                    Ok(()) => {
                        self.armed = false;
                        return published;
                    }
                    // Another publisher may have won the same content-addressed name between the
                    // metadata check and rename. Recheck that candidate and reuse it if identical.
                    Err(_) if published.exists() => continue,
                    Err(err) => panic!(
                        "failed to publish the Miden package cache generation '{}' as '{}': {err}",
                        self.path.display(),
                        published.display()
                    ),
                }
            }
        }
        panic!("failed to publish a unique Miden package generation after 1024 collisions");
    }
}

impl Drop for StagingGeneration {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenerationEntryKind {
    Directory,
    File,
}

fn generation_entries(root: &Path) -> io::Result<Vec<(PathBuf, GenerationEntryKind)>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("generation entries must remain beneath their root")
                .to_path_buf();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                entries.push((relative, GenerationEntryKind::Directory));
                pending.push(path);
            } else if file_type.is_file() {
                entries.push((relative, GenerationEntryKind::File));
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported package-cache entry '{}'", path.display()),
                ));
            }
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

/// Produces a stable FNV-1a candidate key; exact comparison below makes collisions harmless.
fn generation_fingerprint(root: &Path) -> io::Result<u64> {
    let entries = generation_entries(root)?;
    let mut fingerprint = 0xcbf29ce484222325u64;
    for (relative, kind) in entries {
        update_fingerprint(&mut fingerprint, relative.to_string_lossy().as_bytes());
        update_fingerprint(
            &mut fingerprint,
            &[match kind {
                GenerationEntryKind::Directory => 0,
                GenerationEntryKind::File => 1,
            }],
        );
        if kind == GenerationEntryKind::File {
            update_fingerprint(&mut fingerprint, &fs::read(root.join(relative))?);
        }
    }
    Ok(fingerprint)
}

fn update_fingerprint(fingerprint: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *fingerprint ^= u64::from(*byte);
        *fingerprint = (*fingerprint).wrapping_mul(0x00000100000001b3);
    }
}

fn generations_equal(left: &Path, right: &Path) -> io::Result<bool> {
    let left_entries = generation_entries(left)?;
    let right_entries = generation_entries(right)?;
    if left_entries != right_entries {
        return Ok(false);
    }
    for (relative, kind) in left_entries {
        if kind == GenerationEntryKind::File
            && fs::read(left.join(&relative))? != fs::read(right.join(relative))?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug)]
struct BuildInputs {
    paths: Vec<String>,
    environment: Vec<String>,
    opaque: bool,
}

/// The record every compiler/script pair falls back to when they cannot agree on the protocol.
///
/// Opaque means "re-stage on every relevant Cargo invocation", which is always sound: it
/// delegates freshness back to the nested Cargo and compiler instead of trusting a record this
/// script cannot read.
fn unreadable_build_inputs() -> BuildInputs {
    BuildInputs {
        paths: Vec::new(),
        environment: Vec::new(),
        opaque: true,
    }
}

/// Reads the dependency-free, versioned build-input protocol emitted by the compiler.
///
/// Toolchain skew is tolerated in both directions: a compiler too old to write the record at
/// all, and one that writes a schema this script does not know, both degrade to
/// [`unreadable_build_inputs`] rather than failing the outer build.
fn read_build_inputs(generation: &Path) -> BuildInputs {
    let path = generation.join(DEPENDENCY_MANIFEST_DIR).join(BUILD_INPUTS_FILE);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!(
                "cargo:warning=no build-input record at '{}'; the `cargo miden` that staged these \
                 packages does not write one - treating as opaque",
                path.display()
            );
            return unreadable_build_inputs();
        }
        Err(err) => {
            panic!("failed to read the build-input record '{}': {err}", path.display())
        }
    };
    let mut lines = contents.lines();
    let header = lines.next().unwrap_or_default();
    if header != BUILD_INPUTS_HEADER {
        println!(
            "cargo:warning=unsupported build-input record in '{}': expected \
             '{BUILD_INPUTS_HEADER}', got '{header}' - treating as opaque",
            path.display()
        );
        return unreadable_build_inputs();
    }

    let mut inputs = BuildInputs {
        paths: Vec::new(),
        environment: Vec::new(),
        opaque: false,
    };
    for (index, line) in lines.enumerate() {
        let (kind, value) = line.split_once('\t').unwrap_or_else(|| {
            panic!("malformed build-input record '{}' at line {}", path.display(), index + 2)
        });
        assert!(
            !value.is_empty(),
            "empty {kind} value in build-input record '{}' at line {}",
            path.display(),
            index + 2
        );
        match kind {
            "file" | "tree" => inputs.paths.push(value.to_string()),
            "env" => inputs.environment.push(value.to_string()),
            "opaque" => inputs.opaque = true,
            unknown => panic!(
                "unknown build-input kind '{unknown}' in '{}' at line {}",
                path.display(),
                index + 2
            ),
        }
    }
    inputs
}

fn emit_build_input_directives(inputs: &BuildInputs, out_dir: &Path, generation_id: &str) {
    if inputs.opaque {
        // The path is private to this generation and is never created. Cargo therefore runs this
        // script for every relevant invocation, delegating freshness to the nested Cargo/compiler
        // rather than pretending an observed file list is complete.
        println!(
            "cargo:rerun-if-changed={}",
            out_dir.join(format!("miden-packages.opaque-{generation_id}")).display()
        );
        return;
    }
    for path in &inputs.paths {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in &inputs.environment {
        println!("cargo:rerun-if-env-changed={name}");
    }
}

/// Runs `cargo miden build --release --stop-after=dependencies` for the project in
/// `manifest_dir`, staging the
/// dependency packages into `cache_dir`.
///
/// `CARGO_MIDEN` selects a specific `cargo-miden` binary; otherwise the `cargo miden` plugin
/// is resolved through the `cargo` that drives this build. The nested build's cargo and
/// midenc target directories live under `nested_target` (inside the consumer's `OUT_DIR`),
/// so every write lands beneath the outer build's configured target directory and is
/// removed by `cargo clean`. The nested cargo target must stay disjoint from the outer
/// target directory itself: the outer cargo holds a lock on it while build scripts run,
/// and a nested build against the same directory would deadlock.
fn run_cargo_miden_build(manifest_dir: &Path, cache_dir: &Path, nested_target: &Path) -> Output {
    let mut command = match env::var_os("CARGO_MIDEN") {
        Some(cargo_miden) => {
            let cargo_miden = PathBuf::from(cargo_miden);
            // A bare name intentionally uses PATH. Resolve an explicit relative path ourselves,
            // because Command's interaction between a relative program and `current_dir` is
            // platform-specific.
            let cargo_miden = if cargo_miden.is_absolute() || cargo_miden.components().count() == 1
            {
                cargo_miden
            } else {
                manifest_dir.join(cargo_miden)
            };
            Command::new(cargo_miden)
        }
        None => Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
    };
    command
        // `--stop-after=dependencies` stages the dependency packages and the compiler's
        // resolution records, then stops before compiling this crate itself — the macros
        // only ever read the dependencies, and the outer cargo build compiles the crate.
        .args(["miden", "build", "--release", "--stop-after=dependencies"])
        .current_dir(manifest_dir)
        .env(PACKAGE_CACHE_ENV, cache_dir)
        .env("CARGO_TARGET_DIR", nested_target.join("cargo"))
        .env("MIDENC_TARGET_DIR", nested_target.join("miden"));
    command.output().unwrap_or_else(|err| {
        panic!(
            "failed to run `cargo miden build`: {err}.\nInstall cargo-miden (`cargo install \
             cargo-miden`) or point the CARGO_MIDEN environment variable at a cargo-miden binary."
        )
    })
}

pub mod stubs {
    //! Compiles linker-stub crates into archives that dependents link.
    //!
    //! A Rust binding to a Miden procedure is an `extern "C"` declaration, and the Wasm linker
    //! needs a definition for it. A stub is that definition: a function exported under the
    //! procedure's link name with a diverging body, which the Wasm frontend recognizes by name
    //! and lowers to the Miden procedure. A crate that declares bindings compiles its stubs from
    //! its build script with [`compile_stub_archive`].
    //!
    //! The result is a native static library (`.a`) that contains only the stub object files
    //! (no panic handler), to avoid duplicate panic symbols in downstream component builds. The
    //! stub crate is compiled as an rlib and the output is named `.a`, so dependents pick it up
    //! through the native link search path.
    //!
    //! - Why not an rlib? `cargo:rustc-link-lib`/`cargo:rustc-link-search` are for native
    //!   archives; an `.rlib` doesn't fit that model, and attempts to use `rustc-link-arg` don't
    //!   propagate to dependents.
    //! - Why not a staticlib via rustc directly? A `no_std` staticlib usually requires a
    //!   `#[panic_handler]`, which then collides at link time with other crates that also define
    //!   panic symbols. Packaging a single object keeps the archive minimal and free of panic
    //!   symbols.

    use std::{
        env,
        ffi::OsString,
        path::{Path, PathBuf},
        process::Command,
    };

    /// A stub crate to compile into an archive.
    #[derive(Clone, Debug)]
    pub struct StubArchive {
        /// The stub crate's name. The archive is `lib<crate_name>.a` and dependents link it as
        /// `<crate_name>`, so it must be unique among the archives one build links.
        pub crate_name: String,
        /// The stub crate root. It must be `#![no_std]` and define no panic handler.
        pub source: PathBuf,
    }

    /// Compiles `archive.source` to `<OUT_DIR>/lib<crate_name>.a` with the stub recipe and
    /// prints the `cargo:rustc-link-search` and `cargo:rustc-link-lib=static:+whole-archive=`
    /// directives that link it into every dependent.
    ///
    /// Only `archive.source` is watched (`cargo:rerun-if-changed`): a stub crate split into
    /// modules must watch its other files itself. When `TARGET` does not start with `wasm32`
    /// there is nothing to link stubs into, so this prints the watch and returns.
    ///
    /// # Panics
    ///
    /// If `rustc` cannot be spawned or fails to compile the stubs. This runs in a build script,
    /// where a panic fails the build with its message.
    pub fn compile_stub_archive(archive: &StubArchive) {
        println!("cargo:rerun-if-changed={}", archive.source.display());

        let target = env::var("TARGET").unwrap_or_else(|_| "wasm32-wasip1".to_string());
        if !target.starts_with("wasm32") {
            return;
        }

        // Do not declare `rerun-if-env-changed=TARGET`: Cargo controls that variable, and it
        // is unset in Cargo's own environment but set inside build-script environments, so
        // the declaration makes the fingerprint flip between the two. Cargo already runs
        // this script once for each target platform.
        println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");
        println!("cargo:rerun-if-env-changed=RUSTFLAGS");

        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        let out = out_dir.join(format!("lib{}.a", archive.crate_name));
        // The compiler Cargo is driving this build with, so a toolchain override applies to the
        // stub crate too; Cargo sets `RUSTC` for every build script.
        let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = Command::new(rustc)
            .args(rustc_args(&archive.crate_name, &target, &out, &archive.source))
            .status()
            .unwrap_or_else(|err| {
                panic!("failed to spawn rustc for the `{}` stub archive: {err}", archive.crate_name)
            });
        if !status.success() {
            panic!("failed to compile the `{}` stub archive: {status}", archive.crate_name);
        }

        println!("cargo:rustc-link-search=native={}", out_dir.display());
        // The linker adds the `lib` prefix itself when it searches for the file.
        println!("cargo:rustc-link-lib=static:+whole-archive={}", archive.crate_name);
    }

    /// The rustc arguments that compile the stub crate root `source` into the archive `out`.
    fn rustc_args(crate_name: &str, target: &str, out: &Path, source: &Path) -> Vec<OsString> {
        // LLVM MergeFunctions pass https://llvm.org/docs/MergeFunctions.html considers some
        // functions in the stub library identical (e.g. `intrinsics::felt::add` and
        // `intrinsics::felt::mul`) because besides the same sig they have the same body
        // (`unreachable`). The pass merges them which manifests in the compiled Wasm as if both
        // `add` and `mul` are linked to the same (`add` in this case) function.
        // Setting `opt-level=1` seems to be skipping this pass and is enough on its own, but I
        // also put `-Z merge-functions=disabled` in case `opt-level=1` behaviour changes
        // in the future and runs the MergeFunctions pass.
        // `opt-level=0` - introduces import for panic infra leading to WIT encoder error
        // (unsatisfied import).
        let mut args: Vec<OsString> = [
            "--crate-name",
            crate_name,
            "--edition=2024",
            "--crate-type=rlib",
            "--target",
            target,
            "-C",
            "opt-level=1",
            "-C",
            "codegen-units=1",
            "-C",
            "debuginfo=0",
            "-Z",
            "merge-functions=disabled",
            "-C",
            "target-feature=+bulk-memory,+wide-arithmetic",
            "-o",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        args.push(out.into());
        args.push(source.into());
        args
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_recipe_compiles_the_stub_crate_to_the_named_archive() {
            let args = rustc_args(
                "miden_example_stubs",
                "wasm32-wasip1",
                Path::new("/out/libmiden_example_stubs.a"),
                Path::new("/crate/stubs/lib.rs"),
            );
            let expected = [
                "--crate-name",
                "miden_example_stubs",
                "--edition=2024",
                "--crate-type=rlib",
                "--target",
                "wasm32-wasip1",
                "-C",
                "opt-level=1",
                "-C",
                "codegen-units=1",
                "-C",
                "debuginfo=0",
                "-Z",
                "merge-functions=disabled",
                "-C",
                "target-feature=+bulk-memory,+wide-arithmetic",
                "-o",
                "/out/libmiden_example_stubs.a",
                "/crate/stubs/lib.rs",
            ]
            .map(OsString::from);
            assert_eq!(args, expected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temporary directory, named for `label`.
    pub(crate) fn scratch(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "miden-build-script-{label}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn staged(generations: &Path, contents: &[u8]) -> StagingGeneration {
        let staging = StagingGeneration::create(generations);
        fs::write(staging.path().join("package.masp"), contents).unwrap();
        staging
    }

    #[test]
    fn byte_identical_publications_reuse_one_generation() {
        let root = scratch("deduplicate");
        let first = staged(&root, b"same").publish_with_fingerprint(&root, 7, |_| {});
        let second = staged(&root, b"same").publish_with_fingerprint(&root, 7, |_| {});
        assert_eq!(first, second);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rename_loser_rechecks_and_reuses_the_winning_generation() {
        let root = scratch("rename-race");
        let staging = staged(&root, b"same");
        let mut installed_winner = false;
        let published = staging.publish_with_fingerprint(&root, 11, |candidate| {
            if !installed_winner {
                fs::create_dir(candidate).unwrap();
                fs::write(candidate.join("package.masp"), b"same").unwrap();
                installed_winner = true;
            }
        });
        assert_eq!(published, root.join("gen-000000000000000b"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_real_fingerprint_collision_uses_a_distinct_immutable_suffix() {
        let root = scratch("collision");
        let first = staged(&root, b"first").publish_with_fingerprint(&root, 13, |_| {});
        let second = staged(&root, b"second").publish_with_fingerprint(&root, 13, |_| {});
        assert_eq!(first, root.join("gen-000000000000000d"));
        assert_eq!(second, root.join("gen-000000000000000d-001"));
        assert_eq!(fs::read(first.join("package.masp")).unwrap(), b"first");
        assert_eq!(fs::read(second.join("package.masp")).unwrap(), b"second");
        fs::remove_dir_all(root).unwrap();
    }
}
