//! Build-input fingerprints for the filesystem package cache.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use miden_core::crypto::hash::Blake3_256;
use miden_project::{Dependency, DependencyVersionScheme, Project};

use crate::{DebugInfo, LinkLibrary, OptLevel, Options, SourceManager};

/// Computes the filesystem package cache fingerprint for a project build.
///
/// Failures while reading or loading manifests are recorded as markers instead of being
/// returned. The normal project-loading path will diagnose those failures later with its full
/// context.
pub(crate) fn fingerprint(
    options: &Options,
    project_dir: &Path,
    source_manager: &dyn SourceManager,
    compiler_version: &str,
    compiler_revision: &str,
) -> String {
    let mut transcript = Transcript::new();
    transcript.field("compiler.version", compiler_version.as_bytes());
    transcript.field("compiler.revision", compiler_revision.as_bytes());
    record_options(&mut transcript, options);

    let mut manifests = ManifestClosure::new(&mut transcript, source_manager);
    manifests.visit_project(project_dir, None);

    let digest = Blake3_256::hash(transcript.as_bytes());
    let mut fingerprint = String::with_capacity(16);
    for byte in &digest.as_bytes()[..8] {
        use core::fmt::Write;
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a string cannot fail");
    }
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
        match value {
            Some(value) => {
                self.field(&format!("{name}.state"), b"present");
                self.field(name, value.as_bytes());
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
fn record_options(transcript: &mut Transcript, options: &Options) {
    transcript.field("options.profile", options.profile.as_bytes());
    transcript.field("options.optimize", opt_level_name(options.optimize).as_bytes());
    transcript.field("options.debug", debug_info_name(options.debug).as_bytes());
    transcript.optional_field("options.target", options.target.as_deref());

    let target_type = options.target_type.map(|target_type| target_type.to_string());
    transcript.optional_field("options.target_type", target_type.as_deref());

    let mut packages = options.packages.clone();
    packages.sort();
    transcript.field("options.packages.count", &(packages.len() as u64).to_le_bytes());
    for package in packages {
        transcript.field("options.package", package.as_bytes());
    }

    transcript.field("options.workspace", &[u8::from(options.workspace)]);
    transcript.optional_field("options.rustflags", options.rustflags.as_deref());
    transcript.optional_field("options.toolchain", options.toolchain.as_deref());

    let mut link_libraries = options
        .link_libraries
        .iter()
        .map(|library| link_library_input(library, options))
        .collect::<Vec<_>>();
    link_libraries.sort();
    transcript.field("options.link_libraries.count", &(link_libraries.len() as u64).to_le_bytes());
    for (name, version) in link_libraries {
        transcript.field("options.link_library.name", name.as_bytes());
        transcript.optional_field("options.link_library.version", version.as_deref());
    }

    let sysroot = options.sysroot.as_deref().map(path_string);
    transcript.optional_field("options.sysroot", sysroot.as_deref());
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

/// Resolves the name and package version of a requested link library.
fn link_library_input(library: &LinkLibrary, options: &Options) -> (String, Option<String>) {
    let version = library.load(options).ok().map(|package| package.version.to_string());
    (library.name.to_string(), version)
}

/// Walks and records the manifest closure of one project.
struct ManifestClosure<'a> {
    transcript: &'a mut Transcript,
    source_manager: &'a dyn SourceManager,
    visited_projects: BTreeSet<PathBuf>,
    visited_packages: BTreeSet<PathBuf>,
}

impl<'a> ManifestClosure<'a> {
    /// Creates an empty manifest-closure walk.
    fn new(transcript: &'a mut Transcript, source_manager: &'a dyn SourceManager) -> Self {
        Self {
            transcript,
            source_manager,
            visited_projects: BTreeSet::new(),
            visited_packages: BTreeSet::new(),
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
            .unwrap_or_else(|| project_dir(locator));
        let project_key = canonical_or_original(&project_dir);
        if !self.visited_projects.insert(project_key) {
            return;
        }

        self.transcript.field("project", b"begin");
        self.record_manifest(&project_dir.join("miden-project.toml"));
        self.record_manifest(&project_dir.join("Cargo.toml"));

        let Ok(project) = loaded else {
            self.transcript.field("project.load", b"failed");
            self.transcript.field("project", b"end");
            return;
        };
        self.transcript.field("project.load", b"succeeded");

        let package = project.package();
        let manifest_dir =
            package.manifest_path().and_then(Path::parent).unwrap_or(project_dir.as_path());
        let workspace_root = match &project {
            Project::WorkspacePackage { workspace, .. } => workspace.workspace_root(),
            Project::Package(_) => None,
        };

        let mut dependencies = package.dependencies().iter().collect::<Vec<_>>();
        dependencies.sort_by_cached_key(|dependency| dependency_sort_key(dependency));
        self.transcript
            .field("project.dependencies.count", &(dependencies.len() as u64).to_le_bytes());
        for dependency in dependencies {
            self.visit_dependency(dependency, manifest_dir, workspace_root);
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
            Err(_) => self.transcript.field("manifest.state", b"missing"),
        }
    }

    /// Records one dependency and follows it when it names a local project or package file.
    fn visit_dependency(
        &mut self,
        dependency: &Dependency,
        manifest_dir: &Path,
        workspace_root: Option<&Path>,
    ) {
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
                if let Some(workspace_root) = workspace_root {
                    self.visit_path_dependency(dependency, workspace_root, path.inner());
                } else {
                    self.transcript.field("dependency.path", b"unresolved-workspace");
                }
            }
            DependencyVersionScheme::Workspace { member, .. } => {
                if let Some(workspace_root) = workspace_root {
                    self.visit_path_dependency(dependency, workspace_root, member.inner());
                } else {
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
            self.transcript.field("dependency.path", b"unsupported-uri");
            return;
        }

        let relative = Path::new(uri.path());
        let path = if relative.is_absolute() {
            relative.to_path_buf()
        } else {
            base_dir.join(relative)
        };
        if path.extension().is_some_and(|extension| extension == "masp") {
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
            Err(_) => self.transcript.field("package.file.state", b"missing"),
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
fn project_dir(locator: &Path) -> PathBuf {
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

/// Converts a path into the stable string representation used in the transcript.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use miden_debug_types::DefaultSourceManager;
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
        fingerprint(options, project_dir, &DefaultSourceManager::default(), version, rev)
    }

    #[test]
    fn fingerprint_is_stable_for_unchanged_inputs() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();

        let first = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");
        let second = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        assert_eq!(first, second);
        assert_eq!(first.len(), 16);
        assert!(first.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
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
    fn fingerprint_changes_with_compiler_identity() {
        let temp = TempDir::new().unwrap();
        write_project(temp.path(), "root", "");
        let options = Options::default();
        let baseline = test_fingerprint(&options, temp.path(), "1.2.3", "abc123");

        assert_ne!(baseline, test_fingerprint(&options, temp.path(), "1.2.4", "abc123"));
        assert_ne!(baseline, test_fingerprint(&options, temp.path(), "1.2.3", "def456"));
    }
}
