//! Build-input fingerprints for the filesystem package cache.
//!
//! The fingerprint models inputs that change the *set and identity* of packages visible to a
//! build. Source files and lockfiles are deliberately excluded: every resolved package is
//! rewritten into the current cache before its consumers expand, and the generated
//! `include_bytes!` reference makes Cargo re-expand when that package's contents change.
//! Expansions also record `MIDENC_PACKAGE_CACHE`, so rotating the fingerprinted path re-expands
//! consumers even if best-effort stale-directory pruning does not complete.
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

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use miden_core::crypto::hash::Blake3_256;
use miden_debug_types::{DefaultSourceManager, SourceManager};
use miden_mast_package::Package;
use miden_project::{Dependency, DependencyVersionScheme, Project};

use crate::{DebugInfo, LinkLibrary, OptLevel, Options};

/// The number of lowercase hexadecimal characters in a package-cache fingerprint.
pub(crate) const FINGERPRINT_LEN: usize = 16;

/// Returns true when `name` satisfies the package-cache fingerprint format.
pub(crate) fn is_fingerprint(name: &str) -> bool {
    name.len() == FINGERPRINT_LEN
        && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    let mut manifests = ManifestClosure::new(&mut transcript, &source_manager);
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
}

impl<'a> ManifestClosure<'a> {
    /// Creates an empty manifest-closure walk.
    fn new(transcript: &'a mut Transcript, source_manager: &'a dyn SourceManager) -> Self {
        Self {
            transcript,
            source_manager,
            visited_projects: BTreeSet::new(),
            visited_packages: BTreeSet::new(),
            visited_workspace_roots: BTreeSet::new(),
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
            self.visit_dependency(dependency, project_dir.as_path(), workspace);
        }
        self.transcript.field("project", b"end");
    }

    /// Records one project manifest, including an explicit marker when it cannot be read.
    fn record_manifest(&mut self, path: &Path) {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("manifest");
        self.transcript.field("manifest.name", name.as_bytes());
        match std::fs::read(path) {
            Ok(bytes) => {
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
    fn visit_dependency(
        &mut self,
        dependency: &Dependency,
        manifest_dir: &Path,
        workspace: Option<&miden_project::Workspace>,
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
                self.visit_path_dependency(dependency, manifest_dir, path.inner());
            }
            DependencyVersionScheme::WorkspacePath { path, .. } => {
                if let Some(workspace_root) =
                    workspace.and_then(miden_project::Workspace::workspace_root)
                {
                    self.visit_path_dependency(dependency, workspace_root, path.inner());
                } else {
                    log::debug!(
                        target: "package-cache",
                        "cannot resolve workspace path dependency '{}' while fingerprinting outside a workspace",
                        dependency.name()
                    );
                    self.transcript.field("dependency.path", b"unresolved-workspace");
                }
            }
            DependencyVersionScheme::Workspace { member, .. } => {
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
    use tempfile::TempDir;

    use super::*;

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
