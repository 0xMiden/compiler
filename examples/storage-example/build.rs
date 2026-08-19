//! Populates the Miden package cache for builds that `midenc` does not drive.
//!
//! Plain `cargo check`, `cargo build`, and IDE analysis expand the Miden SDK macros without a
//! surrounding `cargo miden build`. Those macros read compiled dependency packages from the
//! directory named by `MIDENC_PACKAGE_CACHE`. The compiler's own package cache is deleted when
//! each build ends, so this script stages generations of its own under `OUT_DIR`, fills a new
//! generation with a nested `cargo miden build` that adopts it through the same variable, and
//! exports the variable to the compilation of this crate.
//!
//! Published generations are immutable and retained until Cargo removes `OUT_DIR`, so an IDE or
//! in-flight macro expansion that still names an older generation always sees one consistent
//! build. Byte-identical results share one content-addressed generation. The nested build fills a
//! private generation and only a fully successful build publishes and exports it; a staging
//! failure fails the outer build rather than exporting stale packages.
//!
//! The compiler records a versioned invalidation contract next to the staged packages. When every
//! frontend can enumerate its inputs completely, this script translates that record into Cargo
//! change directives. If any frontend or driver input marks its provenance opaque — Rust/Cargo
//! source builds, PATH-resolved launchers, and undiscovered workspace boundaries do so today —
//! the script deliberately re-runs dependency-only staging on every Cargo invocation.
//!
//! One sharing caveat: cargo keys build-script output by crate name and version, not by
//! project path. Two different projects with the same package name and version that share one
//! `CARGO_TARGET_DIR` reuse each other's script output, including this staged cache. Use
//! per-checkout target directories for such layouts.
//!
//! See <https://github.com/0xMiden/compiler/issues/1298>.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const BUILD_INPUTS_HEADER: &str = "miden-build-inputs\t1";

fn main() {
    // The manifests declare the dependency set this script stages packages for. Naming any
    // watch disables cargo's watch-everything default, which would otherwise re-run the
    // nested build after every source edit of this crate.
    println!("cargo:rerun-if-changed=miden-project.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // Re-evaluate when the build mode or the tool selection changes.
    println!("cargo:rerun-if-env-changed=MIDENC_PACKAGE_CACHE");
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
    if env::var_os("MIDENC_PACKAGE_CACHE").is_some_and(|value| !value.is_empty()) {
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

    // Validate the compiler's invalidation contract before publishing the generation. A missing
    // or unknown record is an incompatible compiler/script pair, not permission to cache the
    // build with incomplete change detection.
    let build_inputs = read_build_inputs(staging.path());

    // The child has exited and no reader has ever received the final path, so publication is the
    // immutable generation boundary. Reuse an existing byte-identical generation: opaque input
    // records may re-stage on every Cargo invocation, but unchanged outputs should not consume
    // unbounded disk or churn the cache path seen by rustc.
    let published = staging.publish(&generations);

    emit_build_input_directives(&build_inputs, &out_dir, &staging_id);
    println!("cargo:rustc-env=MIDENC_PACKAGE_CACHE={}", published.display());
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

/// Reads the dependency-free, versioned build-input protocol emitted by the compiler.
fn read_build_inputs(generation: &Path) -> BuildInputs {
    let path = generation.join("miden-deps").join("build-inputs");
    let contents = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("failed to read the build-input record '{}': {err}", path.display())
    });
    let mut lines = contents.lines();
    let header = lines.next().unwrap_or_default();
    assert_eq!(
        header,
        BUILD_INPUTS_HEADER,
        "unsupported build-input record in '{}': expected '{BUILD_INPUTS_HEADER}', got '{header}'",
        path.display()
    );

    let mut inputs = BuildInputs {
        paths: Vec::new(),
        environment: Vec::new(),
        opaque: false,
    };
    for (index, line) in lines.enumerate() {
        let (kind, value) = line.split_once('\t').unwrap_or_else(|| {
            panic!(
                "malformed build-input record '{}' at line {}",
                path.display(),
                index + 2
            )
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
/// midenc target directories live under `nested_target` (inside this script's `OUT_DIR`),
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
        .env("MIDENC_PACKAGE_CACHE", cache_dir)
        .env("CARGO_TARGET_DIR", nested_target.join("cargo"))
        .env("MIDENC_TARGET_DIR", nested_target.join("miden"));
    command.output().unwrap_or_else(|err| {
        panic!(
            "failed to run `cargo miden build`: {err}.\nInstall cargo-miden (`cargo install \
             cargo-miden`) or point the CARGO_MIDEN environment variable at a cargo-miden \
             binary."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
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
        assert_eq!(fs::read(&first.join("package.masp")).unwrap(), b"first");
        assert_eq!(fs::read(&second.join("package.masp")).unwrap(), b"second");
        fs::remove_dir_all(root).unwrap();
    }
}
