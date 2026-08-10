//! Build-input fingerprints for the filesystem package cache.
//!
//! The fingerprint models inputs that change the *set and identity* of packages visible to a
//! build. Source files, lockfiles, `rust-toolchain.toml`, Cargo configuration files, and compiler
//! wrappers are deliberately excluded: they are content-only inputs, every resolved package is
//! rewritten into the current cache before its consumers expand, and the generated
//! `include_bytes!` reference makes Cargo re-expand when that package's contents change.
//! Same-path invalidation therefore inherits Cargo's file-freshness semantics: mtime-based unless
//! checksum freshness is enabled.
//! Expansions also record `MIDENC_PACKAGE_CACHE`, so rotating the fingerprinted path re-expands
//! consumers even if best-effort stale-directory pruning does not complete.
//! Concurrent builds with the same fingerprint share one directory. Package publication uses a
//! temporary file followed by atomic rename, preventing consumers from reading a torn package.
//! A macro read and rustc's later `include_bytes!` evaluation can still straddle a concurrent
//! rewrite; that narrow same-fingerprint window is accepted and bounded by the every-run rewrite.
//! Registry and git dependencies contribute declaration text only; in particular, a git branch
//! moving without a manifest edit is outside this fingerprint by design, as are a git package's
//! transitive dependencies. Pinning a revision or deleting the cache directory recovers from a
//! moved unpinned revision. The current fingerprint directory is never emptied before packages
//! are rewritten, so names dropped from the dependency set can linger until another fingerprinted
//! input rotates the directory.
//!
//! A workspace-root locator does not select a package, so `miden_project::Project::load` rejects
//! it. Such a locator contributes its raw manifests plus a load-failure marker and does not walk
//! workspace members; normal workspace compilation is expected to create per-member sessions.
//! Similarly, a Cargo-only project with no sibling `miden-project.toml` contributes both root
//! manifest slots and a load-failure marker, but its dependencies cannot be discovered and are
//! not recursed. That degraded case is reported at debug level while fingerprinting.
//!
//! This manifest walk cannot reuse `miden_project::ProjectDependencyGraphBuilder`: constructing
//! that resolver requires the `PackageRegistry` whose cache path is being derived, which is
//! circular, and its `build` operation may perform network git checkouts. Cache-path derivation
//! must remain local and available before the registry exists.
//!
//! The intended end state is content-addressed package storage, or immutable generation
//! directories derived from the fully resolved dependency graph. That belongs with the #1290
//! package redesign and #1300 macro-side package pins; this fingerprint remains the conservative
//! build-input generation key until then.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
};

use miden_core::crypto::hash::Blake3_256;
use miden_debug_types::{DefaultSourceManager, SourceManager};
use miden_mast_package::Package;
use miden_project::{Dependency, DependencyVersionScheme, Project};

use crate::{DebugInfo, LinkLibrary, OptLevel, Options};

/// The number of lowercase hexadecimal characters in a package-cache fingerprint.
const FINGERPRINT_LEN: usize = 16;

/// Returns true when `name` satisfies the package-cache fingerprint format.
fn is_fingerprint(name: &str) -> bool {
    name.len() == FINGERPRINT_LEN
        && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The extension of a permanent sibling lock file for a fingerprint directory.
const BUILD_LOCK_EXTENSION: &str = "lock";

/// Prepares a filesystem package cache and returns its lifetime shared lock when available.
///
/// Deletion is defense in depth. The primary invalidation is in the FPI expansion itself: it
/// records `option_env!("MIDENC_PACKAGE_CACHE")`, whose value carries the fingerprinted cache
/// path, so Cargo re-expands consumers whenever the fingerprint rotates. Pruning still removes
/// the `include_bytes!` targets of pre-fingerprint expansions, bounds stale package directories,
/// and takes dead caches out of circulation promptly.
///
/// A build holds a shared `packages/<fingerprint>.lock` lock for its registry's lifetime. Pruning
/// takes the corresponding exclusive lock while deleting the sibling directory. The permanent
/// lock lives outside that directory and is acquired before the directory is created, closing the
/// create-before-lock, unlock-before-delete, and unlink/recreate inode-ABA windows.
///
/// `Some` means the shared liveness lock is held, even when a later preparation step degraded.
/// `None` means locking was skipped: the path is unowned, or opening/locking the lock file
/// failed. Every leg keeps the cache configured and the cache directory created when possible,
/// so package publication proceeds against the expected path and reports any concrete
/// filesystem failure itself.
pub(crate) fn prepare_and_lock_filesystem_cache(filesystem_cache: &Path) -> Option<File> {
    if !is_owned_filesystem_cache_path(filesystem_cache) {
        prepare_unowned_filesystem_cache(filesystem_cache);
        return None;
    }

    let parent = filesystem_cache
        .parent()
        .expect("an owned filesystem cache path always has a packages parent");
    if !create_filesystem_cache_parent(parent) {
        return None;
    }
    let Some(filesystem_cache_lock) = acquire_filesystem_cache_lock(filesystem_cache) else {
        // No lock could be held, but the build still runs against this path: create the
        // directory now rather than leaving it to the first publication, so the failure mode
        // is only "unprotected and unswept", not "missing".
        create_current_filesystem_cache(filesystem_cache);
        return None;
    };
    if !create_current_filesystem_cache(filesystem_cache) {
        // The shared lock is already held — keep it. A later publication may still recreate
        // the directory (its writer creates parent directories), and the lock is what stops a
        // concurrent pruner from classifying that recreated cache as dead. Only the sweep is
        // skipped in this degraded mode.
        return Some(filesystem_cache_lock);
    }
    sweep_stale_filesystem_cache_entries(filesystem_cache, parent);
    Some(filesystem_cache_lock)
}

/// Creates an unowned cache path without locking it or sweeping its parent.
fn prepare_unowned_filesystem_cache(filesystem_cache: &Path) {
    if let Err(err) = fs::create_dir_all(filesystem_cache) {
        log::warn!(
            target: "package-registry",
            "failed to create filesystem package cache '{}': {err}; keeping the cache configured so package publication reports the failure",
            filesystem_cache.display()
        );
        return;
    }
    log::debug!(
        target: "package-registry",
        "filesystem package cache '{}' is outside the owned miden/packages/<fingerprint> layout; skipping locking and parent pruning",
        filesystem_cache.display()
    );
}

/// Creates the owned cache parent before its permanent lock file is opened.
fn create_filesystem_cache_parent(parent: &Path) -> bool {
    if let Err(err) = fs::create_dir_all(parent) {
        log::warn!(
            target: "package-registry",
            "failed to create filesystem package cache parent '{}': {err}; keeping the cache configured so package publication reports the failure",
            parent.display()
        );
        return false;
    }
    true
}

/// Creates or recreates the current cache directory after its shared lock is held.
fn create_current_filesystem_cache(filesystem_cache: &Path) -> bool {
    if let Err(err) = fs::create_dir_all(filesystem_cache) {
        log::warn!(
            target: "package-registry",
            "failed to create filesystem package cache '{}': {err}; keeping the cache configured so package publication reports the failure",
            filesystem_cache.display()
        );
        return false;
    }
    true
}

/// Opens the current fingerprint's sibling lock file and holds a shared builder lock.
///
/// Waiting is deadlock-free: pruners only try exclusive locks and never wait while holding one,
/// while a builder waits only for its own lock and holds no other lock. The wait is therefore
/// bounded by one in-progress stale-directory removal.
fn acquire_filesystem_cache_lock(filesystem_cache: &Path) -> Option<File> {
    let lock_path = filesystem_cache_lock_path(filesystem_cache);
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "failed to open filesystem package cache liveness lock '{}': {err}; continuing without a liveness lock",
                lock_path.display()
            );
            return None;
        }
    };

    if let Err(err) = lock.lock_shared() {
        log::warn!(
            target: "package-registry",
            "failed to lock filesystem package cache '{}': {err}; continuing without a liveness lock",
            filesystem_cache.display()
        );
        return None;
    }
    Some(lock)
}

/// Removes dead fingerprint directories and legacy flat package files from `parent`.
fn sweep_stale_filesystem_cache_entries(filesystem_cache: &Path, parent: &Path) {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                target: "package-registry",
                "failed to inspect filesystem package cache '{}': {err}",
                parent.display()
            );
            return;
        }
    };
    let current_lock_path = filesystem_cache_lock_path(filesystem_cache);

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                log::debug!(
                    target: "package-registry",
                    "failed to inspect an entry in filesystem package cache '{}': {err}",
                    parent.display()
                );
                continue;
            }
        };
        let path = entry.path();
        if path == filesystem_cache || path == current_lock_path {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                log::debug!(
                    target: "package-registry",
                    "failed to inspect filesystem package cache entry '{}': {err}",
                    path.display()
                );
                continue;
            }
        };

        let is_stale_fingerprint =
            file_type.is_dir() && is_package_cache_fingerprint(&entry.file_name());
        let is_legacy_package = file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION));
        if is_stale_fingerprint {
            prune_stale_fingerprint(&path, parent);
        } else if is_legacy_package && let Err(err) = fs::remove_file(&path) {
            warn_prune_failure(&path, parent, &err);
        }
    }
}

/// Deletes a stale fingerprint directory while holding its exclusive permanent sibling lock.
fn prune_stale_fingerprint(fingerprint_dir: &Path, parent: &Path) {
    let lock_path = filesystem_cache_lock_path(fingerprint_dir);
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            );
            return;
        }
    };

    match lock.try_lock() {
        Ok(()) => {
            if let Err(err) = fs::remove_dir_all(fingerprint_dir) {
                warn_prune_failure(fingerprint_dir, parent, &err);
            }
        }
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "skipping live filesystem package cache '{}' during stale-cache pruning",
                fingerprint_dir.display()
            )
        }
        Err(TryLockError::Error(err)) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            )
        }
    }
}

/// Logs a best-effort cleanup failure with the exact directory a user can remove.
fn warn_prune_failure(path: &Path, parent: &Path, err: &std::io::Error) {
    log::warn!(
        target: "package-registry",
        "failed to prune stale filesystem package cache entry '{}': {err}; stale cache entries may survive; delete '{}' manually",
        path.display(),
        parent.display()
    );
}

/// Returns the permanent sibling lock path associated with a fingerprint directory.
fn filesystem_cache_lock_path(filesystem_cache: &Path) -> PathBuf {
    filesystem_cache.with_extension(BUILD_LOCK_EXTENSION)
}

/// Returns true when a path is lexically owned by the `miden/packages/<fingerprint>` layout.
/// Returns the package-cache parent directory for a project directory.
///
/// This is the producer half of the owned-layout contract: the path it builds must satisfy
/// [`is_owned_filesystem_cache_path`] once a fingerprint component is appended, or the locking
/// and pruning protocol silently degrades to a debug log. `Session::filesystem_package_cache_dir`
/// derives through here, and its unit test asserts the coupling.
pub(crate) fn package_cache_parent(project_dir: &Path) -> PathBuf {
    project_dir.join("target").join("miden").join("packages")
}

pub(crate) fn is_owned_filesystem_cache_path(filesystem_cache: &Path) -> bool {
    filesystem_cache.file_name().is_some_and(is_package_cache_fingerprint)
        && filesystem_cache
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == OsStr::new("packages"))
        && filesystem_cache
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_some_and(|name| name == OsStr::new("miden"))
}

/// Returns true when `name` has the cache fingerprint format owned by `midenc`.
fn is_package_cache_fingerprint(name: &OsStr) -> bool {
    name.to_str().is_some_and(is_fingerprint)
}

/// Computes the filesystem package cache fingerprint for a project build.
///
/// Failures while reading or loading manifests are recorded as markers instead of being
/// returned. The normal project-loading path will diagnose those failures later with its full
/// context. A private source manager keeps fingerprinting from interning manifests in the
/// compilation session's source manager as a side effect.
pub(crate) fn fingerprint(
    options: &Options,
    project_dir: &Path,
    inherited_rustflags: Option<&OsStr>,
    inherited_rustup_toolchain: Option<&OsStr>,
    compiler_version: &str,
    compiler_revision: &str,
) -> String {
    let mut transcript = Transcript::new();
    transcript.field("compiler.version", compiler_version.as_bytes());
    transcript.field("compiler.revision", compiler_revision.as_bytes());
    record_options(&mut transcript, options, inherited_rustflags, inherited_rustup_toolchain);

    let source_manager = DefaultSourceManager::default();
    let mut manifests = ManifestClosure::new(&mut transcript, &source_manager, None);
    manifests.visit_project(project_dir, None);

    let digest = Blake3_256::hash(transcript.as_bytes());
    let fingerprint = miden_core::utils::to_hex(&digest.as_bytes()[..FINGERPRINT_LEN / 2]);
    log::debug!(
        target: "package-cache",
        "filesystem package cache fingerprint for '{}': {fingerprint}",
        project_dir.display()
    );
    fingerprint
}

/// Build-script inputs of a project's package cache.
///
/// Contract build scripts consume this through `cargo miden package-cache`: the watch list
/// drives their `cargo:rerun-if-changed` directives, and the dependency count decides whether
/// a nested `cargo miden build` is required at all.
#[derive(Debug, Default)]
pub struct PackageCacheBuildInputs {
    /// Manifest, source, and package paths whose changes require a new nested build.
    ///
    /// Only paths that exist are listed: cargo re-runs a build script unconditionally while a
    /// watched path is missing, which would turn every check into a nested build.
    pub watch_paths: Vec<PathBuf>,
    /// The number of direct dependencies whose packages a build compiles into the cache.
    ///
    /// Registry dependencies and explicit `.masp` file paths are excluded: the assembler
    /// resolves the former, and macros read the latter straight from the manifest's path.
    pub source_dependency_count: usize,
}

/// Collects the build-script inputs of the project at `project_dir`.
///
/// The watch list covers the manifest closure the fingerprint walks: every project's
/// manifests, each dependency project's declared target source directories and `wit`
/// directory, each `wit` manifest-key override file, and each preassembled package file. The
/// root project's own sources are deliberately excluded — they do not change dependency
/// packages, and watching them would re-run the nested build on every edit. The
/// source-dependency count is taken from the same walk, so classification lives in one place.
pub(crate) fn build_script_inputs(project_dir: &Path) -> PackageCacheBuildInputs {
    let source_manager = DefaultSourceManager::default();
    let mut transcript = Transcript::new();
    let mut watch_paths = BTreeSet::new();
    let mut manifests =
        ManifestClosure::new(&mut transcript, &source_manager, Some(&mut watch_paths));
    manifests.visit_project(project_dir, None);
    let source_dependency_count = manifests.root_source_dependency_count;

    PackageCacheBuildInputs {
        watch_paths: watch_paths.into_iter().collect(),
        source_dependency_count,
    }
}

/// Returns true when a path dependency's URI names a preassembled `.masp` package file.
///
/// Extension-classified like the fingerprint walk, before any canonicalization.
fn is_package_file_uri(uri: &miden_project::Uri) -> bool {
    Path::new(uri.path())
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION))
}

/// A length-prefixed, domain-separated byte transcript.
struct Transcript {
    bytes: Vec<u8>,
}

impl Transcript {
    /// Creates an empty package-cache fingerprint transcript.
    fn new() -> Self {
        let mut transcript = Self { bytes: Vec::new() };
        transcript.field("domain", b"midenc-package-cache-v1");
        transcript
    }

    /// Appends one named field to this transcript.
    fn field(&mut self, name: &str, value: &[u8]) {
        self.bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(value);
    }

    /// Appends an optional named field to this transcript.
    fn optional_field(&mut self, name: &str, value: Option<&str>) {
        self.optional_bytes_field(name, value.map(str::as_bytes));
    }

    /// Appends an optional named byte field to this transcript.
    fn optional_bytes_field(&mut self, name: &str, value: Option<&[u8]>) {
        match value {
            Some(value) => {
                self.field(&format!("{name}.state"), b"present");
                self.field(name, value);
            }
            None => self.field(&format!("{name}.state"), b"missing"),
        }
    }

    /// Returns the encoded transcript.
    fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Records the build configuration which can affect package identity or selection.
fn record_options(
    transcript: &mut Transcript,
    options: &Options,
    inherited_rustflags: Option<&OsStr>,
    inherited_rustup_toolchain: Option<&OsStr>,
) {
    let Options {
        manifest_path: _,
        name: _,
        entrypoint: _,
        profile,
        workspace,
        packages,
        target,
        target_type,
        optimize,
        debug,
        output_types: _,
        search_paths: _,
        link_libraries,
        link_modules: _,
        sysroot,
        midenup_home: _,
        toolchain,
        color: _,
        diagnostics: _,
        current_dir: _,
        // The cache root is intentionally tied to the project directory rather than
        // `--target-dir`, so every nested build participant derives the same location.
        target_dir: _,
        output_dir: _,
        output_file: _,
        remap_path_prefixes: _,
        print_hir_source_locations: _,
        stop_after: _,
        parse_only: _,
        analyze_only: _,
        link_only: _,
        no_link: _,
        lint: _,
        print_cfg_after_all: _,
        print_cfg_after_pass: _,
        print_ir_before_stage: _,
        print_ir_after_all: _,
        print_ir_after_pass: _,
        print_ir_after_modified: _,
        print_ir_filters: _,
        save_temps: _,
        rustflags,
        cargo_frontmatter: _,
        flags: _,
    } = options;

    // Deliberate exclusions are classified here so adding an `Options` field forces a choice.
    // Output paths, naming, diagnostics, printing, and stop flags do not select dependency
    // packages. Search paths, link modules, remapped paths, custom flags, and similar
    // content-affecting controls self-heal through the in-run package rewrite; the package-name
    // set itself is driven by the manifest closure recorded below.
    transcript.field("options.profile", profile.as_bytes());
    transcript.field("options.optimize", opt_level_name(*optimize).as_bytes());
    transcript.field("options.debug", debug_info_name(*debug).as_bytes());
    transcript.optional_field("options.target", target.as_deref());

    let target_type = target_type.map(|target_type| target_type.to_string());
    transcript.optional_field("options.target_type", target_type.as_deref());

    let mut packages = packages.clone();
    packages.sort();
    transcript.field("options.packages.count", &(packages.len() as u64).to_le_bytes());
    for package in packages {
        transcript.field("options.package", package.as_bytes());
    }

    transcript.field("options.workspace", &[u8::from(*workspace)]);
    transcript.optional_field("options.rustflags", rustflags.as_deref());
    transcript.optional_bytes_field(
        "options.inherited_rustflags",
        inherited_rustflags.map(OsStr::as_encoded_bytes),
    );
    // Inherited `CARGO_ENCODED_RUSTFLAGS` is deliberately NOT fingerprinted: `cargo_env` sets
    // the variable authoritatively for every nested build, so the inherited value has no effect
    // on what gets built.
    transcript.optional_bytes_field(
        "options.inherited_rustup_toolchain",
        inherited_rustup_toolchain.map(OsStr::as_encoded_bytes),
    );
    transcript.optional_field("options.toolchain", toolchain.as_deref());

    let mut link_libraries = link_libraries.iter().map(link_library_input).collect::<Vec<_>>();
    link_libraries.sort();
    transcript.field("options.link_libraries.count", &(link_libraries.len() as u64).to_le_bytes());
    for (name, path, linkage) in link_libraries {
        transcript.field("options.link_library.name", name.as_bytes());
        transcript.optional_bytes_field("options.link_library.path", path.as_deref());
        transcript.field("options.link_library.linkage", linkage.as_bytes());
    }

    transcript.optional_bytes_field(
        "options.sysroot",
        sysroot.as_deref().map(|path| path.as_os_str().as_encoded_bytes()),
    );
}

/// Returns the stable transcript name for an optimization level.
fn opt_level_name(level: OptLevel) -> &'static str {
    match level {
        OptLevel::None => "none",
        OptLevel::Basic => "basic",
        OptLevel::Balanced => "balanced",
        OptLevel::Max => "max",
        OptLevel::Size => "size",
        OptLevel::SizeMin => "size-min",
    }
}

/// Returns the stable transcript name for a debug-information level.
fn debug_info_name(level: DebugInfo) -> &'static str {
    match level {
        DebugInfo::None => "none",
        DebugInfo::Line => "line",
        DebugInfo::Full => "full",
    }
}

/// Returns the I/O-free identity of a requested link library.
///
/// Built-in library versions are already pinned by the compiler build version.
fn link_library_input(library: &LinkLibrary) -> (String, Option<Vec<u8>>, &'static str) {
    (
        library.name.to_string(),
        library.path.as_deref().map(|path| path.as_os_str().as_encoded_bytes().to_vec()),
        library.linkage.as_str(),
    )
}

/// Walks and records the manifest closure of one project.
struct ManifestClosure<'a> {
    transcript: &'a mut Transcript,
    source_manager: &'a dyn SourceManager,
    visited_projects: BTreeSet<PathBuf>,
    visited_packages: BTreeSet<PathBuf>,
    visited_workspace_roots: BTreeSet<PathBuf>,
    /// Existing filesystem inputs collected for build scripts, when a collector is attached.
    watch_paths: Option<&'a mut BTreeSet<PathBuf>>,
    /// The root project's direct dependencies that a build compiles into the package cache.
    ///
    /// Counted where the walk classifies each dependency, so the build-script trigger and the
    /// fingerprint walk cannot drift apart. Registry dependencies and preassembled `.masp`
    /// files are excluded: the assembler resolves the former, and macros read the latter
    /// straight from the manifest's path.
    root_source_dependency_count: usize,
}

impl<'a> ManifestClosure<'a> {
    /// Creates an empty manifest-closure walk.
    fn new(
        transcript: &'a mut Transcript,
        source_manager: &'a dyn SourceManager,
        watch_paths: Option<&'a mut BTreeSet<PathBuf>>,
    ) -> Self {
        Self {
            transcript,
            source_manager,
            visited_projects: BTreeSet::new(),
            visited_packages: BTreeSet::new(),
            visited_workspace_roots: BTreeSet::new(),
            watch_paths,
            root_source_dependency_count: 0,
        }
    }

    /// Records a path for build-script watching when collection is active and the path exists.
    fn watch(&mut self, path: &Path) {
        if let Some(watch_paths) = self.watch_paths.as_deref_mut()
            && path.exists()
        {
            watch_paths.insert(path.to_path_buf());
        }
    }

    /// Counts one direct root dependency whose package a build compiles into the cache.
    fn count_root_source_dependency(&mut self, of_root: bool) {
        if of_root {
            self.root_source_dependency_count += 1;
        }
    }

    /// Records a dependency project's declared target sources and `wit` directory for watching.
    fn watch_project_sources(&mut self, project_dir: &Path, package: &miden_project::Package) {
        let targets =
            package.library_target().into_iter().chain(package.executable_targets().iter());
        for target in targets {
            let source_path = Path::new(target.inner().path.inner().path());
            let source_path = if source_path.is_absolute() {
                source_path.to_path_buf()
            } else {
                project_dir.join(source_path)
            };
            // Sibling modules live next to the target's root source, so its directory is the
            // watch unit — unless that directory is the project root itself, which would sweep
            // in `target/` churn and re-run the nested build after every build.
            match source_path.parent() {
                Some(parent) if parent != project_dir => self.watch(parent),
                _ => self.watch(&source_path),
            }
        }
        self.watch(&project_dir.join("wit"));
    }

    /// Records the `wit` manifest-key override files of a project's dependencies for watching.
    ///
    /// In the override flow the named `.wit` file (or directory) is the only source of a
    /// dependency's interface, so an edit must re-run the nested build and the consumer's
    /// expansion. Malformed metadata shapes are skipped here; macro expansion diagnoses them.
    fn watch_wit_overrides(&mut self, project_dir: &Path, package: &miden_project::Package) {
        let Some(dependencies) = package
            .metadata()
            .get("miden")
            .and_then(|meta| meta.get("dependencies"))
            .and_then(|value| value.as_table())
        else {
            return;
        };
        for config in dependencies.values() {
            let Some(wit_path) = config
                .as_table()
                .and_then(|config| config.get("wit"))
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            let wit_path = Path::new(wit_path);
            let wit_path = if wit_path.is_absolute() {
                wit_path.to_path_buf()
            } else {
                project_dir.join(wit_path)
            };
            self.watch(&wit_path);
        }
    }

    /// Records a project and recursively visits its local dependencies.
    fn visit_project(&mut self, locator: &Path, expected_name: Option<&str>) {
        let loaded = match expected_name {
            Some(name) => Project::load_project_reference(name, locator, self.source_manager),
            None => Project::load(locator, self.source_manager),
        };
        let project_dir = loaded
            .as_ref()
            .ok()
            .and_then(|project| project.package().manifest_path().map(Path::to_path_buf))
            .and_then(|manifest| manifest.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| locator_project_dir(locator));
        let project_key = canonical_or_original(&project_dir);
        if !self.visited_projects.insert(project_key) {
            return;
        }

        self.transcript.field("project", b"begin");
        let miden_manifest = project_dir.join("miden-project.toml");
        let cargo_manifest = project_dir.join("Cargo.toml");
        self.record_manifest(&miden_manifest);
        self.record_manifest(&cargo_manifest);
        if cargo_manifest.is_file() && !miden_manifest.is_file() {
            log::debug!(
                target: "package-cache",
                "Cargo-only project '{}' has no sibling miden-project.toml; fingerprinting records its root manifests but cannot recurse dependencies",
                project_dir.display()
            );
        }

        let project = match loaded {
            Ok(project) => project,
            Err(err) => {
                log::debug!(
                    target: "package-cache",
                    "failed to load project '{}' while fingerprinting its manifest closure: {err}",
                    locator.display()
                );
                self.transcript.field("project.load", b"failed");
                self.transcript.field("project", b"end");
                return;
            }
        };
        self.transcript.field("project.load", b"succeeded");

        let package = project.package();

        // Dependency sources feed dependency packages, so build scripts watch them. The root
        // project's sources do not: its package is not read back by its own macro expansion,
        // and watching them would re-run the nested build on every edit. The root is the one
        // project visited without an expected dependency name. A `wit` override file is
        // watched for every project including the root, because it feeds the *consumer's*
        // expansion directly.
        if expected_name.is_some() {
            self.watch_project_sources(&project_dir, &package);
        }
        self.watch_wit_overrides(&project_dir, &package);
        let workspace = match &project {
            Project::WorkspacePackage { workspace, .. } => Some(workspace.as_ref()),
            Project::Package(_) => None,
        };
        let workspace_root = workspace.and_then(miden_project::Workspace::workspace_root);
        if let Some(workspace_root) = workspace_root {
            let workspace_key = canonical_or_original(workspace_root);
            if self.visited_workspace_roots.insert(workspace_key) {
                self.record_manifest(&workspace_root.join("miden-project.toml"));
                self.record_manifest(&workspace_root.join("Cargo.toml"));
            }
        }

        let mut dependencies = package.dependencies().iter().collect::<Vec<_>>();
        dependencies.sort_by_cached_key(|dependency| dependency_sort_key(dependency));
        self.transcript
            .field("project.dependencies.count", &(dependencies.len() as u64).to_le_bytes());
        for dependency in dependencies {
            self.visit_dependency(
                dependency,
                project_dir.as_path(),
                workspace,
                expected_name.is_none(),
            );
        }
        self.transcript.field("project", b"end");
    }

    /// Records one project manifest, including an explicit marker when it cannot be read.
    fn record_manifest(&mut self, path: &Path) {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("manifest");
        self.transcript.field("manifest.name", name.as_bytes());
        match std::fs::read(path) {
            Ok(bytes) => {
                self.watch(path);
                self.transcript.field("manifest.state", b"present");
                self.transcript.field("manifest.bytes", &bytes);
            }
            Err(err) => {
                log::debug!(
                    target: "package-cache",
                    "unable to read manifest '{}' while fingerprinting: {err}; recording a missing marker",
                    path.display()
                );
                self.transcript.field("manifest.state", b"missing");
            }
        }
    }

    /// Records one dependency and follows it when it names a local project or package file.
    ///
    /// `of_root` marks a direct dependency of the root project, the granularity the
    /// source-dependency count reports.
    fn visit_dependency(
        &mut self,
        dependency: &Dependency,
        manifest_dir: &Path,
        workspace: Option<&miden_project::Workspace>,
        of_root: bool,
    ) {
        // Keep scheme handling aligned with
        // `frontend/masm/src/project.rs::collect_dependency_metadata_for_scheme`. The fingerprint
        // walk intentionally differs from resolution in only two ways: path dependencies are
        // extension-classified before canonicalization, so a symlink to a `.masp` is treated as
        // source; and git declarations are recorded but their checkouts are never recursed.
        self.transcript.field("dependency", b"begin");
        self.transcript.field("dependency.name", dependency.name().as_bytes());
        self.transcript
            .field("dependency.scheme", dependency_scheme_key(dependency).as_bytes());

        match dependency.scheme() {
            DependencyVersionScheme::Registry(_) => {}
            DependencyVersionScheme::Path { path, .. } => {
                self.visit_path_dependency(dependency, manifest_dir, path.inner(), of_root);
            }
            DependencyVersionScheme::WorkspacePath { path, .. } => {
                if let Some(workspace_root) =
                    workspace.and_then(miden_project::Workspace::workspace_root)
                {
                    self.visit_path_dependency(dependency, workspace_root, path.inner(), of_root);
                } else {
                    log::debug!(
                        target: "package-cache",
                        "cannot resolve workspace path dependency '{}' while fingerprinting outside a workspace",
                        dependency.name()
                    );
                    self.transcript.field("dependency.path", b"unresolved-workspace");
                    // An unresolved declaration still selects a source dependency; the nested
                    // build resolves and compiles it with the full workspace context.
                    if !is_package_file_uri(path.inner()) {
                        self.count_root_source_dependency(of_root);
                    }
                }
            }
            DependencyVersionScheme::Workspace { member, .. } => {
                self.count_root_source_dependency(of_root);
                if let Some(manifest_path) = workspace
                    .and_then(|workspace| {
                        workspace.get_member_by_relative_path(member.inner().path())
                    })
                    .and_then(|package| package.manifest_path().map(Path::to_path_buf))
                {
                    self.visit_project(&manifest_path, Some(dependency.name().as_ref()));
                } else {
                    log::debug!(
                        target: "package-cache",
                        "cannot resolve workspace member dependency '{}' at '{}' while fingerprinting",
                        dependency.name(),
                        member.inner().path()
                    );
                    self.transcript.field("dependency.path", b"unresolved-workspace");
                }
            }
            DependencyVersionScheme::Git { repo, revision, .. } => {
                self.count_root_source_dependency(of_root);
                self.transcript.field("dependency.git.repo", repo.inner().as_bytes());
                self.transcript
                    .field("dependency.git.revision", revision.inner().to_string().as_bytes());
            }
        }
        self.transcript.field("dependency", b"end");
    }

    /// Resolves and records a filesystem dependency using the manifest scheme's base directory.
    fn visit_path_dependency(
        &mut self,
        dependency: &Dependency,
        base_dir: &Path,
        uri: &miden_project::Uri,
        of_root: bool,
    ) {
        if uri.scheme().is_some_and(|scheme| scheme != "file") {
            log::debug!(
                target: "package-cache",
                "unsupported URI '{}' for path dependency '{}' while fingerprinting",
                uri.as_str(),
                dependency.name()
            );
            self.transcript.field("dependency.path", b"unsupported-uri");
            return;
        }

        let relative = Path::new(uri.path());
        let path = if relative.is_absolute() {
            relative.to_path_buf()
        } else {
            base_dir.join(relative)
        };
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION))
        {
            self.record_package_file(&path);
        } else {
            self.count_root_source_dependency(of_root);
            self.visit_project(&path, Some(dependency.name().as_ref()));
        }
    }

    /// Records the content hash of a preassembled package dependency.
    fn record_package_file(&mut self, path: &Path) {
        let key = canonical_or_original(path);
        if !self.visited_packages.insert(key) {
            return;
        }

        self.transcript.field("package.file", b"begin");
        match std::fs::read(path) {
            Ok(bytes) => {
                self.watch(path);
                self.transcript.field("package.file.state", b"present");
                let digest = Blake3_256::hash(&bytes);
                self.transcript.field("package.file.digest", digest.as_bytes());
            }
            Err(err) => {
                log::debug!(
                    target: "package-cache",
                    "unable to read preassembled package '{}' while fingerprinting: {err}; recording a missing marker",
                    path.display()
                );
                self.transcript.field("package.file.state", b"missing");
            }
        }
        self.transcript.field("package.file", b"end");
    }
}

/// Returns a deterministic key for dependency traversal order.
fn dependency_sort_key(dependency: &Dependency) -> (String, String) {
    (dependency.name().to_string(), dependency_scheme_key(dependency))
}

/// Returns a stable textual projection of a dependency's resolved scheme.
fn dependency_scheme_key(dependency: &Dependency) -> String {
    match dependency.scheme() {
        DependencyVersionScheme::Registry(requirement) => format!("registry:{requirement}"),
        DependencyVersionScheme::Path { path, version } => {
            format!("path:{}:{}", path.inner().as_str(), optional_display(version.as_ref()))
        }
        DependencyVersionScheme::WorkspacePath { path, version } => format!(
            "workspace-path:{}:{}",
            path.inner().as_str(),
            optional_display(version.as_ref())
        ),
        DependencyVersionScheme::Workspace { member, version } => {
            format!("workspace:{}:{}", member.inner().as_str(), optional_display(version.as_ref()))
        }
        DependencyVersionScheme::Git {
            repo,
            revision,
            version,
        } => format!(
            "git:{}:{}:{}",
            repo.inner().as_str(),
            revision.inner(),
            optional_display(version.as_ref().map(|version| version.inner()))
        ),
    }
}

/// Formats an optional display value without conflating absence with an empty value.
fn optional_display(value: Option<&impl core::fmt::Display>) -> String {
    value.map(ToString::to_string).unwrap_or_else(|| "<missing>".into())
}

/// Returns the directory whose sibling project manifests describe a locator.
fn locator_project_dir(locator: &Path) -> PathBuf {
    if locator.file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case("miden-project.toml") || name.eq_ignore_ascii_case("Cargo.toml")
    }) {
        locator.parent().map(Path::to_path_buf).unwrap_or_else(|| locator.to_path_buf())
    } else {
        locator.to_path_buf()
    }
}

/// Canonicalizes a path for cycle detection, retaining the original spelling on failure.
fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::{fs::OpenOptions, sync::mpsc, time::Duration};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn creating_a_filesystem_cache_prunes_only_stale_owned_entries() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        let unrelated_directory = parent.join("not-a-midenc-cache");
        let uppercase_directory = parent.join("ABCDEF0123456789");
        let legacy_package = parent.join("legacy.masp");
        let uppercase_legacy_package = parent.join("uppercase.MASP");
        let permanent_orphan_lock = parent.join("1111111111111111.lock");
        let live_precreation_lock_path = parent.join("2222222222222222.lock");
        let unrelated_file = parent.join("keep.txt");

        for directory in [&current, &stale, &unrelated_directory, &uppercase_directory] {
            fs::create_dir_all(directory).unwrap();
        }
        let current_marker = current.join("keep");
        fs::write(&current_marker, b"current").unwrap();
        fs::write(stale.join("old.masp"), b"stale").unwrap();
        fs::write(&legacy_package, b"legacy").unwrap();
        fs::write(&uppercase_legacy_package, b"legacy").unwrap();
        fs::write(&permanent_orphan_lock, b"").unwrap();
        let live_precreation_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&live_precreation_lock_path)
            .unwrap();
        live_precreation_lock.try_lock_shared().unwrap();
        fs::write(&unrelated_file, b"unrelated").unwrap();

        let current_lock =
            prepare_and_lock_filesystem_cache(&current).expect("current cache must be locked");

        assert!(current_marker.exists(), "the current cache must remain intact");
        assert!(!stale.exists(), "a stale fingerprint directory must be removed");
        assert!(
            filesystem_cache_lock_path(&stale).exists(),
            "the stale fingerprint's rendezvous lock must remain permanent"
        );
        assert!(!legacy_package.exists(), "a legacy flat package must be removed");
        assert!(
            !uppercase_legacy_package.exists(),
            "legacy package extensions must be matched case-insensitively"
        );
        assert!(
            permanent_orphan_lock.exists(),
            "an orphan fingerprint lock is a permanent rendezvous object"
        );
        assert!(
            live_precreation_lock_path.exists(),
            "a lock held before its directory is created must remain permanent"
        );
        assert!(unrelated_directory.exists(), "unowned directories must be retained");
        assert!(uppercase_directory.exists(), "non-lowercase directories must be retained");
        assert!(unrelated_file.exists(), "unowned files must be retained");

        drop(live_precreation_lock);
        drop(current_lock);
    }

    #[test]
    fn builder_waits_for_an_in_progress_prune_and_recreates_its_cache() {
        let temp = TempDir::new().unwrap();
        let current = temp.path().join("miden").join("packages").join("fedcba9876543210");
        fs::create_dir_all(&current).unwrap();
        let lock_path = filesystem_cache_lock_path(&current);
        let exclusive_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        exclusive_lock.try_lock().unwrap();
        fs::remove_dir_all(&current).unwrap();

        let (started_tx, started_rx) = mpsc::channel();
        let (completed_tx, completed_rx) = mpsc::channel();
        let thread_cache = current.clone();
        let builder = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            completed_tx.send(prepare_and_lock_filesystem_cache(&thread_cache)).unwrap();
        });

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            completed_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "the builder must wait while the pruner holds the exclusive lock"
        );
        drop(exclusive_lock);

        let builder_lock = completed_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("the builder must acquire a shared lock after pruning completes");
        builder.join().unwrap();
        assert!(current.is_dir(), "the builder must recreate the pruned cache directory");
        let exclusive_contender =
            OpenOptions::new().read(true).write(true).open(lock_path).unwrap();
        assert!(matches!(exclusive_contender.try_lock(), Err(TryLockError::WouldBlock)));
        drop(builder_lock);
    }

    #[test]
    fn cache_create_failure_keeps_the_acquired_liveness_lock_and_skips_the_sweep() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        std::fs::create_dir_all(&stale).unwrap();
        // A regular file at the fingerprint path makes `create_dir_all` fail after the shared
        // lock is already held.
        std::fs::write(&current, b"not a directory").unwrap();

        let lock = prepare_and_lock_filesystem_cache(&current);

        assert!(lock.is_some(), "the acquired liveness lock must survive a create failure");
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filesystem_cache_lock_path(&current))
            .unwrap();
        assert!(
            matches!(contender.try_lock(), Err(TryLockError::WouldBlock)),
            "the shared lock must still protect the cache path"
        );
        assert!(stale.exists(), "the sweep must be skipped in the degraded mode");
    }

    #[test]
    fn lock_open_failure_still_creates_the_cache_directory() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        // A directory at the lock path makes the lock file unopenable.
        std::fs::create_dir_all(filesystem_cache_lock_path(&current)).unwrap();

        let lock = prepare_and_lock_filesystem_cache(&current);

        assert!(lock.is_none(), "no lock can be held when its file cannot be opened");
        assert!(
            current.is_dir(),
            "the cache directory must be created so the build runs against the expected path"
        );
    }

    #[test]
    fn live_stale_fingerprint_survives_until_its_lock_is_released() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        fs::create_dir_all(&stale).unwrap();

        let stale_lock_path = filesystem_cache_lock_path(&stale);
        let stale_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&stale_lock_path)
            .unwrap();
        stale_lock.try_lock_shared().unwrap();

        let current_lock =
            prepare_and_lock_filesystem_cache(&current).expect("current cache must be locked");
        assert!(filesystem_cache_lock_path(&current).exists());
        let current_contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filesystem_cache_lock_path(&current))
            .unwrap();
        current_contender.try_lock_shared().unwrap();
        assert!(stale.exists(), "a live sibling cache must not be pruned");

        drop(stale_lock);
        let second_lock = prepare_and_lock_filesystem_cache(&current)
            .expect("same-input builders share the lock");
        assert!(!stale.exists(), "the stale cache must be pruned after its build exits");
        assert!(stale_lock_path.exists(), "the stale sibling lock must remain permanent");

        drop(second_lock);
        drop(current_lock);
    }

    #[test]
    fn same_fingerprint_contender_remains_live_after_first_builder_exits() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let shared = parent.join("fedcba9876543210");
        let different = parent.join("0123456789abcdef");

        let first =
            prepare_and_lock_filesystem_cache(&shared).expect("first builder must lock the cache");
        let contender = prepare_and_lock_filesystem_cache(&shared)
            .expect("same-input contender must share the lock");
        drop(first);

        let different_lock = prepare_and_lock_filesystem_cache(&different)
            .expect("different-input builder must lock its cache");

        assert!(shared.exists(), "the live contender's cache must not be pruned");

        drop(different_lock);
        drop(contender);
    }

    #[test]
    fn arbitrary_cache_path_cannot_sweep_its_parent() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("arbitrary-parent");
        let current = parent.join("cache");
        let fingerprint_sibling = parent.join("0123456789abcdef");
        let package_sibling = parent.join("unrelated.masp");
        fs::create_dir_all(&fingerprint_sibling).unwrap();
        fs::write(&package_sibling, b"unrelated").unwrap();

        let lock = prepare_and_lock_filesystem_cache(&current);

        assert!(lock.is_none());
        assert!(current.is_dir(), "an arbitrary cache path is still created");
        assert!(!filesystem_cache_lock_path(&current).exists());
        assert!(fingerprint_sibling.exists());
        assert!(package_sibling.exists());
    }

    #[test]
    fn fingerprint_name_outside_owned_layout_cannot_sweep_its_parent() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("shared");
        let current = parent.join("fedcba9876543210");
        let fingerprint_sibling = parent.join("0123456789abcdef");
        let package_sibling = parent.join("unrelated.masp");
        fs::create_dir_all(&fingerprint_sibling).unwrap();
        fs::write(&package_sibling, b"unrelated").unwrap();

        let lock = prepare_and_lock_filesystem_cache(&current);

        assert!(lock.is_none());
        assert!(current.is_dir(), "an out-of-layout cache path is still created");
        assert!(!filesystem_cache_lock_path(&current).exists());
        assert!(fingerprint_sibling.exists());
        assert!(package_sibling.exists());
    }

    /// Writes a minimal Miden project and Cargo manifest to `dir`.
    fn write_project(dir: &Path, name: &str, dependencies: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("miden-project.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n\n[lib]\npath = \
                 \"src/lib.rs\"\n{dependencies}"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"),
        )
        .unwrap();
    }

    /// Computes a test fingerprint with a fresh source manager.
    fn test_fingerprint(options: &Options, project_dir: &Path, version: &str, rev: &str) -> String {
        fingerprint(options, project_dir, None, None, version, rev)
    }

    #[test]
    fn build_script_inputs_watch_dependency_sources_but_not_root_sources() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let dependency = temp.path().join("dependency");
        let prebuilt = temp.path().join("prebuilt.masp");
        write_project(
            &root,
            "root",
            "\n[dependencies]\nregistry-dep = \"*\"\ndependency = { path = \"../dependency\" \
             }\nprebuilt = { path = \"../prebuilt.masp\" }\n",
        );
        write_project(&dependency, "dependency", "");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(dependency.join("src")).unwrap();
        fs::create_dir_all(dependency.join("wit")).unwrap();
        fs::write(&prebuilt, b"package bytes").unwrap();

        let inputs = build_script_inputs(&root);

        // Paths may carry `..` components from manifest-relative joins; compare by suffix.
        let watched = |suffix: &str| inputs.watch_paths.iter().any(|path| path.ends_with(suffix));
        assert!(watched("root/miden-project.toml"), "the root manifests must be watched");
        assert!(watched("root/Cargo.toml"), "the root manifests must be watched");
        assert!(watched("dependency/miden-project.toml"));
        assert!(watched("dependency/Cargo.toml"));
        assert!(watched("dependency/src"), "dependency sources must be watched");
        assert!(watched("dependency/wit"), "dependency WIT must be watched");
        assert!(watched("prebuilt.masp"), "preassembled packages must be watched");
        assert!(!watched("root/src"), "root sources must not re-run the nested build");
        assert!(!watched("root/wit"), "a nonexistent path must never be watched");

        assert_eq!(
            inputs.source_dependency_count, 1,
            "only the source-project dependency counts; registry and `.masp` deps do not"
        );
    }

    #[test]
    fn build_script_inputs_watch_declared_target_sources_and_wit_overrides() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let dependency = temp.path().join("dependency");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("miden-project.toml"),
            "[package]\nname = \"root\"\nversion = \"1.0.0\"\n\n[lib]\npath = \
             \"src/lib.rs\"\n\n[dependencies]\ndependency = { path = \"../dependency\" \
             }\n\n[package.metadata.miden.dependencies.dependency]\nwit = \"overrides/dep.wit\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"root\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("overrides")).unwrap();
        std::fs::write(root.join("overrides/dep.wit"), "package d:d@1.0.0;\n").unwrap();
        std::fs::create_dir_all(&dependency).unwrap();
        std::fs::write(
            dependency.join("miden-project.toml"),
            "[package]\nname = \"dependency\"\nversion = \"1.0.0\"\n\n[lib]\npath = \
             \"custom/entry.rs\"\n",
        )
        .unwrap();
        std::fs::write(
            dependency.join("Cargo.toml"),
            "[package]\nname = \"dependency\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dependency.join("custom")).unwrap();

        let inputs = build_script_inputs(&root);

        let watched = |suffix: &str| inputs.watch_paths.iter().any(|path| path.ends_with(suffix));
        assert!(
            watched("dependency/custom"),
            "the declared target source directory must be watched, not a hard-coded `src`"
        );
        assert!(
            watched("root/overrides/dep.wit"),
            "the consumer's `wit` override file must be watched"
        );
        assert_eq!(inputs.source_dependency_count, 1);
    }

    #[test]
    fn fingerprint_is_stable_for_unchanged_inputs() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();

        let first = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");
        let second = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        assert_eq!(first, second);
        assert!(is_fingerprint(&first));
    }

    #[test]
    fn fingerprint_changes_with_manifest_closure() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let dependency = temp.path().join("dependency");
        write_project(
            &root,
            "root",
            "\n[dependencies]\ndependency = { path = \"../dependency\" }\n",
        );
        write_project(&dependency, "dependency", "");
        let options = Options::default();
        let before = test_fingerprint(&options, &root, "1.2.3", "abc123");

        std::fs::write(
            dependency.join("Cargo.toml"),
            "[package]\nname = \"dependency\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        let after = test_fingerprint(&options, &root, "1.2.3", "abc123");

        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_walk_terminates_on_dependency_cycles() {
        let temp = TempDir::new().unwrap();
        let first_project = temp.path().join("first");
        let second_project = temp.path().join("second");
        write_project(
            &first_project,
            "first",
            "\n[dependencies]\nsecond = { path = \"../second\" }\n",
        );
        write_project(
            &second_project,
            "second",
            "\n[dependencies]\nfirst = { path = \"../first\" }\n",
        );
        let options = Options::default();

        let first = test_fingerprint(&options, &first_project, "1.2.3", "abc123");
        let second = test_fingerprint(&options, &first_project, "1.2.3", "abc123");

        assert_eq!(first, second);
    }

    #[test]
    fn fingerprint_changes_with_preassembled_package_content() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let package = temp.path().join("dependency.masp");
        write_project(
            &root,
            "root",
            "\n[dependencies]\ndependency = { path = \"../dependency.masp\" }\n",
        );
        std::fs::write(&package, b"first package").unwrap();
        let options = Options::default();
        let before = test_fingerprint(&options, &root, "1.2.3", "abc123");

        std::fs::write(&package, b"different package").unwrap();
        let after = test_fingerprint(&options, &root, "1.2.3", "abc123");

        assert_ne!(before, after);
    }

    #[test]
    fn project_load_failure_marker_is_stable_and_distinct() {
        let temp = TempDir::new().unwrap();
        let options = Options::default();

        let first = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");
        let second = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");
        assert_eq!(first, second);

        write_project(temp.path(), "root", "");
        let loadable = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        assert_ne!(first, loadable);
    }

    #[test]
    fn fingerprint_changes_with_build_options() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();
        let baseline = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        let mut profile = options.clone();
        profile.profile = "release".into();
        assert_ne!(baseline, test_fingerprint(&profile, temp.path(), "1.2.3", "abc123"));

        let mut optimized = options.clone();
        optimized.optimize = OptLevel::Max;
        assert_ne!(baseline, test_fingerprint(&optimized, temp.path(), "1.2.3", "abc123"));
    }

    #[test]
    fn fingerprint_changes_with_inherited_rustflags() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();

        let missing = fingerprint(&options, temp.path(), None, None, "1.2.3", "abc123");
        let present = fingerprint(
            &options,
            temp.path(),
            Some(OsStr::new("-C target-feature=+bulk-memory")),
            None,
            "1.2.3",
            "abc123",
        );

        assert_ne!(missing, present);
    }

    #[test]
    fn fingerprint_changes_with_inherited_rustup_toolchain() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();

        let missing = fingerprint(&options, temp.path(), None, None, "1.2.3", "abc123");
        let present = fingerprint(
            &options,
            temp.path(),
            None,
            Some(OsStr::new("nightly-2026-08-05")),
            "1.2.3",
            "abc123",
        );

        assert_ne!(missing, present);
    }

    #[test]
    fn fingerprint_resolves_workspace_members_before_classifying_paths() {
        let temp = TempDir::new().unwrap();
        let dependency = temp.path().join("dep.masp");
        let application = temp.path().join("app");
        write_project(&dependency, "dep", "");
        write_project(&application, "app", "\n[dependencies]\ndep.workspace = true\n");
        std::fs::write(
            temp.path().join("miden-project.toml"),
            "[workspace]\nmembers = [\"dep.masp\", \"app\"]\n\n[workspace.dependencies]\ndep = { \
             path = \"dep.masp\" }\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"dep.masp\", \"app\"]\n",
        )
        .unwrap();
        let options = Options::default();
        let before = test_fingerprint(&options, &application, "1.2.3", "abc123");

        std::fs::write(
            dependency.join("Cargo.toml"),
            "[package]\nname = \"dep\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        let after = test_fingerprint(&options, &application, "1.2.3", "abc123");

        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_changes_with_workspace_manifests() {
        let temp = TempDir::new().unwrap();
        let member = temp.path().join("member");
        write_project(&member, "member", "");
        std::fs::write(
            temp.path().join("miden-project.toml"),
            "[workspace]\nmembers = [\"member\"]\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[workspace]\nmembers = [\"member\"]\n")
            .unwrap();
        let options = Options::default();
        let before = test_fingerprint(&options, &member, "1.2.3", "abc123");

        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        let after = test_fingerprint(&options, &member, "1.2.3", "abc123");

        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_changes_with_compiler_identity() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();
        let baseline = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        assert_ne!(baseline, test_fingerprint(&options, temp.path(), "1.2.4", "abc123"));
        assert_ne!(baseline, test_fingerprint(&options, temp.path(), "1.2.3", "def456"));
    }
}
