//! The project template bundle.
//!
//! Templates are the one release unit that publishes no crates. What it
//! publishes is an archive, and the archive has to satisfy two constraints that
//! pull in opposite directions: it must contain everything `cargo miden new`
//! renders from, and nothing else. Repository furniture that happens to sit
//! beside the templates — READMEs, licences, the CI inherited from the imported
//! repository, the test harness that depends on `cargo-miden` by path — would
//! either confuse a generated project or fail to resolve for anyone outside
//! this repository.
//!
//! The archive is deterministic in the same way the binary archives are, so a
//! bundle's digest identifies its content rather than the moment it was built.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;

use crate::config::UnitConfig;

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Bundle {
    pub schema_version: u32,
    pub version: Version,
    pub templates: BTreeMap<String, TemplateEntry>,
    /// The manifest's own file name, e.g. `"bundle.toml"` or `"templates.toml"`.
    ///
    /// `ArtifactSource::manifest` is an arbitrary path; nothing constrains its
    /// name. This is set from the path [`Bundle::load`] was given, not
    /// deserialized, so the archive seeds the manifest under the name it was
    /// actually loaded from rather than a hardcoded literal.
    #[serde(skip)]
    manifest_name: String,
    /// Requirement declarations, keyed by the manifest key that holds them --
    /// `sdk-requirement` in this repository. Which key a unit uses comes from
    /// its `tracks` entry, so the bundle format itself stays agnostic.
    ///
    /// This is `flatten`ed, so `Bundle` must never gain `deny_unknown_fields`:
    /// the two are incompatible.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateEntry {
    pub path: PathBuf,
}

impl Bundle {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read bundle manifest at {}", path.display()))?;
        let mut bundle: Self =
            toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))?;

        if bundle.schema_version != SUPPORTED_SCHEMA_VERSION {
            bail!(
                "bundle schema version {} is not supported (expected {})",
                bundle.schema_version,
                SUPPORTED_SCHEMA_VERSION
            );
        }
        if bundle.templates.is_empty() {
            bail!("the bundle declares no templates");
        }
        bundle.manifest_name = path
            .file_name()
            .with_context(|| format!("{} has no file name", path.display()))?
            .to_string_lossy()
            .into_owned();
        Ok(bundle)
    }

    /// Every file the archive should contain, relative to the source root, in
    /// a stable order.
    pub fn files(&self, root: &Path, include: &[PathBuf]) -> Result<Vec<PathBuf>> {
        Ok(self.entries(root, include)?.into_iter().map(|(path, _)| path).collect())
    }

    /// Every file the archive should contain, paired with whether it is
    /// executable, in a stable order.
    ///
    /// The executable flag comes from git's mode rather than the filesystem's:
    /// git records exactly one bit (`100644` or `100755`), whereas an on-disk
    /// mode varies with umask and platform and would put the archive's digest
    /// back at the mercy of whoever built it.
    pub fn entries(&self, root: &Path, include: &[PathBuf]) -> Result<Vec<(PathBuf, bool)>> {
        // The manifest itself is always in the archive: a `Bundle` only exists
        // where the unit has one, and a consumer reads it to find the sources.
        // Its name comes from the path it was loaded from, not a literal --
        // `ArtifactSource::manifest` lets a unit name it anything.
        let mut files = vec![(PathBuf::from(&self.manifest_name), false)];

        for relative in include {
            let path = root.join(relative);
            if !path.exists() {
                bail!("the include list names {}, which does not exist", relative.display());
            }
            collect(&path, root, &mut files)
                .with_context(|| format!("collecting '{}'", relative.display()))?;
        }

        files.sort();
        files.dedup();
        Ok(files)
    }

    /// The requirement this bundle declares under `key`.
    pub fn requirement(&self, key: &str) -> Result<&str> {
        self.extra
            .get(key)
            .and_then(toml::Value::as_str)
            .with_context(|| format!("the bundle manifest declares no string key '{key}'"))
    }
}

/// List a template directory's files as the *repository* defines them.
///
/// The enumeration comes from git rather than from a filesystem walk so that the
/// archive is a function of the commit and nothing else. A walk asks "what is on
/// this machine", and the answer varies: an ignored directory, a stray editor
/// file, or -- the case that caught this -- a `.git/info/exclude` entry private
/// to one clone silently changes the bundle's contents and therefore its digest.
/// The archive built here has to be reproducible by anyone checking out the same
/// commit, because `lint` compares it against the copy committed for
/// `cargo-miden` to embed.
///
/// Files present on disk but unknown to git are reported by [`untracked`] rather
/// than included.
fn collect(directory: &Path, root: &Path, files: &mut Vec<(PathBuf, bool)>) -> Result<()> {
    // `-s` adds the staged mode, which is where the executable bit comes from.
    // The record is `<mode> <object> <stage>\t<path>`, NUL-terminated.
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-s", "-z", "--"])
        .arg(directory)
        .output()
        .context("failed to run `git ls-files`; the bundle is built from a checkout")?;

    if !output.status.success() {
        bail!(
            "`git ls-files` failed for {}: {}",
            directory.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let record = String::from_utf8_lossy(record);
        let Some((metadata, path)) = record.split_once('\t') else {
            bail!("unexpected `git ls-files -s` record: {record}");
        };
        let mode = metadata.split_whitespace().next().unwrap_or_default();

        // Symlinks (120000) and gitlinks (160000) are not file content and have
        // no meaning in an extracted template.
        if !matches!(mode, "100644" | "100755") {
            continue;
        }

        let path = PathBuf::from(path);
        // A tracked path that is gone from the working tree would otherwise
        // fail later, when the archive tries to read it.
        if !root.join(&path).is_file() {
            continue;
        }
        files.push((path, mode == "100755"));
    }
    Ok(())
}

/// Files sitting under an included path that git does not track.
///
/// These are invisible to [`Bundle::files`] by design, but silently dropping
/// them is how an archive comes to differ between two checkouts of the same
/// commit. Callers surface them so the omission is a decision rather than an
/// accident.
pub fn untracked(_bundle: &Bundle, root: &Path, include: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();

    for relative in include {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z", "--others", "--"])
            .arg(root.join(relative))
            .output()
            .context("failed to run `git ls-files --others`")?;
        if !output.status.success() {
            continue;
        }
        for path in output.stdout.split(|byte| *byte == 0) {
            if path.is_empty() {
                continue;
            }
            let path = PathBuf::from(String::from_utf8_lossy(path).into_owned());
            if matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some(".DS_Store") | Some("Cargo.lock")
            ) {
                continue;
            }
            if path.components().any(|c| c.as_os_str() == "target") {
                continue;
            }
            found.push(path);
        }
    }

    found.sort();
    found.dedup();
    Ok(found)
}

/// Build the release archive for this bundle.
///
/// Returns the archive bytes and their digest. The digest is what a compiler
/// release checks its embedded copy against, so it must depend on the template
/// contents and nothing else -- which is why the archive is built through the
/// deterministic writer rather than the system `tar`.
pub fn archive(root: &Path, bundle: &Bundle, include: &[PathBuf]) -> Result<(Vec<u8>, String)> {
    let mut entries = Vec::new();
    for (relative, executable) in bundle.entries(root, include)? {
        let path = root.join(&relative);
        let bytes =
            std::fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        entries.push(crate::archive::Entry {
            path: relative.to_string_lossy().replace('\\', "/"),
            bytes,
            // A template can ship a script -- the project scaffold has a hook
            // that has to be runnable once generated -- so the bit is carried
            // through rather than flattened.
            executable,
        });
    }

    let bytes = crate::archive::tar_gz(entries)?;
    let digest = crate::registry::sha256_hex(&bytes);
    Ok((bytes, digest))
}

/// Whether a manifest line declares a requirement on one of `tracked`.
fn is_tracked_dependency(line: &str, tracked: &[String]) -> bool {
    tracked.iter().any(|name| {
        line.strip_prefix(name.as_str())
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// The SDK requirement templates should carry for a given SDK version.
///
/// Stable releases get a minor-level requirement, so a later SDK patch needs no
/// template change. Prereleases get the exact version: a caret requirement never
/// matches a prerelease, so `"0.14"` would leave a generated project unable to
/// resolve the very SDK it was released alongside.
pub fn requirement_for(version: &Version) -> String {
    if version.pre.is_empty() {
        format!("{}.{}", version.major, version.minor)
    } else {
        version.to_string()
    }
}

/// Rewrite the requirement the artifact manifest declares under `key`, and
/// every source manifest under `include` that requires one of `tracked`.
///
/// `manifest` is the artifact manifest's path relative to `root`; `include` is
/// the unit's include list, also relative to `root`.
pub fn set_requirement(
    root: &Path,
    manifest: &Path,
    key: &str,
    tracked: &[String],
    include: &[PathBuf],
    requirement: &str,
) -> Result<Vec<PathBuf>> {
    let bundle_path = root.join(manifest);
    let bundle = Bundle::load(&bundle_path)?;
    let old = format!("\"{}\"", bundle.requirement(key)?);
    let new = format!("\"{requirement}\"");
    let mut changed = Vec::new();

    let text = std::fs::read_to_string(&bundle_path)?;
    let mut document: toml_edit::DocumentMut = text.parse()?;
    document[key] = toml_edit::value(requirement);
    std::fs::write(&bundle_path, document.to_string())?;
    changed.push(bundle_path);

    for relative in include {
        let mut manifests = Vec::new();
        find_manifests(&root.join(relative), &mut manifests)?;
        for manifest in manifests {
            let text = std::fs::read_to_string(&manifest)?;
            let updated: String = text
                .lines()
                .map(|line| {
                    let trimmed = line.trim();
                    let is_miden_version = is_tracked_dependency(trimmed, tracked)
                        && !trimmed.contains("path")
                        && !trimmed.contains("git")
                        && trimmed.contains(&old);
                    if is_miden_version {
                        line.replace(&old, &new)
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let updated = if text.ends_with('\n') {
                format!("{updated}\n")
            } else {
                updated
            };

            if updated != text {
                std::fs::write(&manifest, updated)?;
                changed.push(manifest);
            }
        }
    }

    changed.sort();
    Ok(changed)
}

/// The directory an artifact unit archives, resolved against the workspace root.
pub fn source_root(root: &Path, unit: &UnitConfig) -> Result<PathBuf> {
    let source = unit.source.as_ref().context("the unit declares no artifact source")?;
    let directory = source
        .directory
        .as_ref()
        .context("the unit's source is a file, not a directory to archive")?;
    Ok(root.join(directory))
}

/// The include list: paths relative to the unit's directory, each enumerated
/// with `git ls-files`.
///
/// This is deliberately not a whole-subtree walk. The archive must contain
/// what a consumer renders from and nothing else; repository furniture sitting
/// beside the sources — READMEs, licences, inherited CI, a test harness
/// depending on a local crate by path — would either confuse a consumer or fail
/// to resolve outside this repository.
///
/// A manifest is a named include list: its entries supply the same paths, plus
/// per-entry metadata the inline form has no room for.
pub fn include_paths(root: &Path, unit: &UnitConfig) -> Result<Vec<PathBuf>> {
    let source = unit.source.as_ref().context("the unit declares no artifact source")?;

    if !source.include.is_empty() {
        return Ok(source.include.clone());
    }

    let directory = source.directory.as_ref().context("the unit archives no directory")?;
    let manifest = source
        .manifest
        .as_ref()
        .context("the unit declares neither 'include' nor 'manifest'")?;
    let bundle = Bundle::load(&root.join(directory).join(manifest))?;
    Ok(bundle.templates.values().map(|entry| entry.path.clone()).collect())
}

/// Where an artifact unit's version is recorded, and under which key.
///
/// An explicit `version-file` wins; otherwise the manifest's `version` key. A
/// unit with neither has no version of its own, and its version lives only in
/// the candidate declaration.
fn version_location(root: &Path, unit: &UnitConfig) -> Option<(PathBuf, String)> {
    let source = unit.source.as_ref()?;
    if let Some(file) = &source.version_file {
        return Some((root.join(&file.path), file.key.clone()));
    }
    let directory = source.directory.as_ref()?;
    let manifest = source.manifest.as_ref()?;
    Some((root.join(directory).join(manifest), "version".to_string()))
}

pub fn read_version(root: &Path, unit: &UnitConfig) -> Result<Version> {
    let (path, key) = version_location(root, unit).context(
        "the unit records no version: it declares neither a 'version-file' nor a 'source.manifest'",
    )?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let document: toml::Value =
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))?;
    let raw = document
        .get(&key)
        .and_then(toml::Value::as_str)
        .with_context(|| format!("{} has no string key '{key}'", path.display()))?;
    Version::parse(raw)
        .with_context(|| format!("{} key '{key}' is not valid SemVer: {raw}", path.display()))
}

/// Write a new version, leaving the rest of the file byte-identical.
///
/// Refuses to create the key if it is not already present, rather than
/// silently inserting a new top-level key nothing reads -- which is what a
/// misconfigured `version-file.key` (a dotted one, say, since `toml_edit`'s
/// string index treats it as one literal key rather than a path) would
/// otherwise do.
pub fn write_version(root: &Path, unit: &UnitConfig, version: &Version) -> Result<PathBuf> {
    let (path, key) = version_location(root, unit).context(
        "the unit records no version: it declares neither a 'version-file' nor a 'source.manifest'",
    )?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut document: toml_edit::DocumentMut = text.parse()?;
    if !document.contains_key(key.as_str()) {
        bail!(
            "{} has no key '{key}' to write the version to; refusing to create one, since a \
             dotted key like 'package.version' is a single literal key here rather than a path",
            path.display()
        );
    }
    document[key.as_str()] = toml_edit::value(version.to_string());
    std::fs::write(&path, document.to_string())?;
    Ok(path)
}

/// Check that every source requirement on `tracked` matches `expected`.
///
/// This is the drift that would otherwise be silent: after a tracked unit's
/// minor bump, a source left at the old requirement still renders, still
/// builds, and quietly pins consumers to the previous version.
///
/// `expected` is the bare requirement -- `"0.14"` -- rather than a bundle, so
/// this stays independent of the artifact manifest's format.
pub fn check_requirements(
    root: &Path,
    include: &[PathBuf],
    tracked: &[String],
    expected: &str,
) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let quoted = format!("\"{expected}\"");

    for relative in include {
        let directory = root.join(relative);
        let mut manifests = Vec::new();
        find_manifests(&directory, &mut manifests)?;

        for manifest in manifests {
            let text = std::fs::read_to_string(&manifest)?;
            for (number, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                // `miden = "0.13"`, `miden-sdk-build-script-support = "0.13"`, or their
                // inline-table forms. Lines selecting a path or git source are development
                // escape hatches and carry no version.
                if !is_tracked_dependency(trimmed, tracked) {
                    continue;
                }
                if !trimmed.contains("version") && !trimmed.contains('"') {
                    continue;
                }
                if trimmed.contains("path") || trimmed.contains("git") {
                    continue;
                }
                if !trimmed.contains(&quoted) {
                    problems.push(format!(
                        "{}:{}: requires `{trimmed}` but the declared requirement is \
                         \"{expected}\"",
                        manifest.display(),
                        number + 1,
                    ));
                }
            }
        }
    }

    problems.sort();
    Ok(problems)
}

fn find_manifests(directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            find_manifests(&path, found)?;
        } else if entry.file_name() == "Cargo.toml" {
            found.push(path);
        }
    }
    found.sort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn fixture(label: &str) -> PathBuf {
        fixture_named(label, "bundle.toml")
    }

    /// Like [`fixture`], but the manifest is written under an arbitrary name
    /// rather than the conventional `bundle.toml`.
    fn fixture_named(label: &str, manifest_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bundle-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        write(
            &dir.join(manifest_name),
            r#"schema-version = 1
version = "0.1.0"
sdk-requirement = "0.13"

[templates]
account = { path = "rust/account" }
"#,
        );
        write(
            &dir.join("rust/account/template/Cargo.toml"),
            "[package]\nname = \"{{crate_name}}\"\n\n[dependencies]\nmiden = { version = \"0.13\" \
             }\n\n[build-dependencies]\nmiden-sdk-build-script-support = { version = \"0.13\" }\n",
        );
        write(&dir.join("rust/account/template/src/lib.rs"), "");
        // Repository furniture that must not ship.
        write(&dir.join("README.md"), "not a template");
        write(&dir.join("rust/.github/workflows/ci.yml"), "name: CI");
        write(&dir.join("rust/tests/Cargo.toml"), "[package]\nname = \"tests\"\n");

        // The file list comes from git, so the fixture has to be a repository.
        git(&dir, &["init", "-q"]);
        git(&dir, &["add", "-A"]);
        dir
    }

    /// The fixture's include list, resolved the way production resolves it:
    /// through the unit's manifest.
    fn include(dir: &Path) -> Vec<PathBuf> {
        include_paths(dir, &artifact_unit(".", Some("bundle.toml"), &[])).unwrap()
    }

    /// The packages the fixture's templates track, as `[units.templates.tracks.sdk]`
    /// names them in this repository.
    fn tracked() -> Vec<String> {
        vec!["miden".to_string(), "miden-sdk-build-script-support".to_string()]
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn the_archive_contains_only_declared_templates() {
        let dir = fixture("contents");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let files = bundle.files(&dir, &include(&dir)).unwrap();

        assert!(files.contains(&PathBuf::from("bundle.toml")));
        assert!(files.contains(&PathBuf::from("rust/account/template/Cargo.toml")));
        assert!(files.contains(&PathBuf::from("rust/account/template/src/lib.rs")));

        // The furniture is not template content.
        assert!(!files.iter().any(|f| f.starts_with("rust/tests")), "{files:?}");
        assert!(!files.iter().any(|f| f.starts_with("rust/.github")), "{files:?}");
        assert!(!files.contains(&PathBuf::from("README.md")), "{files:?}");
    }

    /// The bug this guards against shipped a bundle nobody else could
    /// reproduce: a `.git/info/exclude` entry private to one clone hid nine
    /// files from git, so the archive built there contained them and the
    /// archive built in CI did not. The bundle must depend on the commit, not
    /// on whose machine it was built.
    #[test]
    fn a_file_git_does_not_track_stays_out_of_the_archive() {
        let dir = fixture("untracked");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let include = include(&dir);
        let (before, digest_before) = archive(&dir, &bundle, &include).unwrap();

        write(&dir.join("rust/account/template/.claude/settings.json"), "{}");
        let (after, digest_after) = archive(&dir, &bundle, &include).unwrap();

        assert_eq!(digest_before, digest_after, "an untracked file changed the bundle digest");
        assert_eq!(before, after);

        // ... but it is reported, so the omission is visible rather than silent.
        let strays = untracked(&bundle, &dir, &include).unwrap();
        assert_eq!(
            strays,
            [PathBuf::from("rust/account/template/.claude/settings.json")],
            "an untracked template file must be reported"
        );

        // Once committed it ships, which is the only way to add template content.
        git(&dir, &["add", "-A"]);
        let (_, digest_tracked) = archive(&dir, &bundle, &include).unwrap();
        assert_ne!(digest_before, digest_tracked);
        assert!(untracked(&bundle, &dir, &include).unwrap().is_empty());
    }

    /// The project scaffold ships a hook that has to be runnable in a generated
    /// project. The bit comes from git's mode, not the filesystem's, so it does
    /// not vary with umask.
    #[test]
    fn an_executable_template_file_stays_executable() {
        let dir = fixture("exec");
        let script = dir.join("rust/account/template/hook.sh");
        write(&script, "#!/bin/sh\necho hi\n");
        git(&dir, &["add", "-A"]);
        git(&dir, &["update-index", "--chmod=+x", "rust/account/template/hook.sh"]);

        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let entries = bundle.entries(&dir, &include(&dir)).unwrap();

        let executable: Vec<&PathBuf> =
            entries.iter().filter(|(_, exec)| *exec).map(|(path, _)| path).collect();
        assert_eq!(executable, [&PathBuf::from("rust/account/template/hook.sh")], "{entries:?}");
    }

    /// `ArtifactSource::manifest` is an arbitrary path; nothing constrains its
    /// name. An archive seeded with a hardcoded `bundle.toml` would die trying
    /// to read a file that does not exist whenever a unit names its manifest
    /// something else.
    #[test]
    fn the_archive_seeds_the_manifest_under_its_own_name() {
        let dir = fixture_named("manifest-name", "templates.toml");
        let bundle = Bundle::load(&dir.join("templates.toml")).unwrap();
        let unit = artifact_unit(".", Some("templates.toml"), &[]);
        let include = include_paths(&dir, &unit).unwrap();

        let files = bundle.files(&dir, &include).unwrap();
        assert!(files.contains(&PathBuf::from("templates.toml")), "{files:?}");
        assert!(!files.contains(&PathBuf::from("bundle.toml")), "{files:?}");

        // `archive` reads every entry; it must not go looking for a
        // "bundle.toml" that was never written.
        let (_, digest) = archive(&dir, &bundle, &include).unwrap();
        assert!(!digest.is_empty());
    }

    #[test]
    fn the_file_list_is_stable() {
        let dir = fixture("stable");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let include = include(&dir);
        let first = bundle.files(&dir, &include).unwrap();
        for _ in 0..8 {
            assert_eq!(bundle.files(&dir, &include).unwrap(), first);
        }
    }

    #[test]
    fn a_matching_sdk_requirement_is_accepted() {
        let dir = fixture("match");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let expected = bundle.requirement("sdk-requirement").unwrap();
        assert!(
            check_requirements(&dir, &include(&dir), &tracked(), expected)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_sdk_bump_updates_runtime_and_build_support_requirements() {
        let dir = fixture("sdk-bump");
        set_requirement(
            &dir,
            Path::new("bundle.toml"),
            "sdk-requirement",
            &tracked(),
            &include(&dir),
            "0.14",
        )
        .unwrap();

        let manifest =
            std::fs::read_to_string(dir.join("rust/account/template/Cargo.toml")).unwrap();
        assert!(manifest.contains("miden = { version = \"0.14\" }"), "{manifest}");
        assert!(
            manifest.contains("miden-sdk-build-script-support = { version = \"0.14\" }"),
            "{manifest}"
        );
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let expected = bundle.requirement("sdk-requirement").unwrap();
        assert_eq!(expected, "0.14");
        assert!(
            check_requirements(&dir, &include(&dir), &tracked(), expected)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_template_left_behind_by_an_sdk_bump_is_reported() {
        let dir = fixture("drift");
        // The bundle moved to 0.14; the template did not.
        write(
            &dir.join("bundle.toml"),
            r#"schema-version = 1
version = "0.2.0"
sdk-requirement = "0.14"

[templates]
account = { path = "rust/account" }
"#,
        );
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();

        let problems = check_requirements(
            &dir,
            &include(&dir),
            &tracked(),
            bundle.requirement("sdk-requirement").unwrap(),
        )
        .unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().all(|problem| problem.contains("0.14")), "{problems:?}");
        assert!(problems.iter().all(|problem| problem.contains("account")), "{problems:?}");
        assert!(problems.iter().any(|problem| problem.contains("miden =")), "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("miden-sdk-build-script-support")),
            "{problems:?}"
        );
    }

    #[test]
    fn path_and_git_escape_hatches_are_ignored() {
        let dir = fixture("escape-hatch");
        write(
            &dir.join("rust/account/template/Cargo.toml"),
            "[dependencies]\n{% if compiler_path %}\nmiden = { path = \"{{ compiler_path \
             }}/sdk/sdk\" }\n{% else %}\nmiden = { version = \"0.13\" }\n{% endif \
             %}\n\n[build-dependencies]\n{% if compiler_path %}\nmiden-sdk-build-script-support = \
             { path = \"{{ compiler_path }}/sdk/build-script-support\" }\n{% else \
             %}\nmiden-sdk-build-script-support = { version = \"0.13\" }\n{% endif %}\n",
        );
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        assert!(
            check_requirements(
                &dir,
                &include(&dir),
                &tracked(),
                bundle.requirement("sdk-requirement").unwrap()
            )
            .unwrap()
            .is_empty(),
            "development source selections carry no version and must not be flagged"
        );
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "midenc-release-bundle-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn artifact_unit(directory: &str, manifest: Option<&str>, include: &[&str]) -> UnitConfig {
        crate::config::testing::config(&format!(
            r#"
schema-version = 2

[units.thing]
kind = "artifact"
tag = "thing/v{{version}}"
changelog = "CHANGELOG.md"

[units.thing.source]
directory = "{directory}"
{}
{}
"#,
            manifest.map(|m| format!("manifest = \"{m}\"")).unwrap_or_default(),
            if include.is_empty() {
                String::new()
            } else {
                format!(
                    "include = [{}]",
                    include.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(", ")
                )
            },
        ))
        .units
        .remove("thing")
        .unwrap()
    }

    #[test]
    fn an_artifact_units_version_comes_from_its_manifest() {
        let dir = temp_dir("version-manifest");
        std::fs::create_dir_all(dir.join("extra/templates")).unwrap();
        std::fs::write(
            dir.join("extra/templates/bundle.toml"),
            "schema-version = 1\nversion = \"3.1.0\"\nsdk-requirement = \
             \"0.1\"\n[templates.demo]\npath = \"demo\"\n",
        )
        .unwrap();

        let unit = artifact_unit("extra/templates", Some("bundle.toml"), &[]);
        assert_eq!(read_version(&dir, &unit).unwrap(), Version::parse("3.1.0").unwrap());
    }

    #[test]
    fn writing_a_version_preserves_the_rest_of_the_manifest() {
        let dir = temp_dir("version-write");
        std::fs::create_dir_all(dir.join("extra/templates")).unwrap();
        let before = "schema-version = 1\nversion = \"3.1.0\"\nsdk-requirement = \
                      \"0.1\"\n[templates.demo]\npath = \"demo\"\n";
        std::fs::write(dir.join("extra/templates/bundle.toml"), before).unwrap();

        let unit = artifact_unit("extra/templates", Some("bundle.toml"), &[]);
        write_version(&dir, &unit, &Version::parse("3.2.0").unwrap()).unwrap();

        let after = std::fs::read_to_string(dir.join("extra/templates/bundle.toml")).unwrap();
        // Only the version line may differ: the doc comment's claim of
        // byte-identical otherwise is enforced by substituting just that line
        // in the original and comparing the whole file, not by `contains`
        // checks that would also pass a reformatted file.
        let expected = before.replace("version = \"3.1.0\"", "version = \"3.2.0\"");
        assert_eq!(after, expected);
    }

    #[test]
    fn writing_a_version_to_a_key_the_file_lacks_is_refused() {
        let dir = temp_dir("version-write-missing-key");
        std::fs::create_dir_all(dir.join("extra/templates")).unwrap();
        std::fs::write(
            dir.join("extra/templates/bundle.toml"),
            "schema-version = 1\nversion = \"3.1.0\"\nsdk-requirement = \
             \"0.1\"\n[templates.demo]\npath = \"demo\"\n",
        )
        .unwrap();

        // `package.version` is a single literal key here, not a path, and
        // this manifest has no such top-level key.
        let mut unit = artifact_unit("extra/templates", Some("bundle.toml"), &[]);
        unit.source.as_mut().unwrap().version_file = Some(crate::config::VersionFile {
            path: PathBuf::from("extra/templates/bundle.toml"),
            key: "package.version".to_string(),
        });

        let error = format!(
            "{:#}",
            write_version(&dir, &unit, &Version::parse("3.2.0").unwrap()).unwrap_err()
        );
        assert!(error.contains("package.version"), "{error}");

        let after = std::fs::read_to_string(dir.join("extra/templates/bundle.toml")).unwrap();
        assert!(
            after.contains("version = \"3.1.0\""),
            "the file must be left untouched: {after}"
        );
    }

    #[test]
    fn an_inline_include_list_needs_no_manifest() {
        let dir = temp_dir("include-inline");
        let unit = artifact_unit("site", None, &["index.html", "assets"]);
        assert_eq!(
            include_paths(&dir, &unit).unwrap(),
            [PathBuf::from("index.html"), PathBuf::from("assets")]
        );
    }

    #[test]
    fn a_manifest_supplies_the_include_list() {
        let dir = temp_dir("include-manifest");
        std::fs::create_dir_all(dir.join("extra/templates")).unwrap();
        std::fs::write(
            dir.join("extra/templates/bundle.toml"),
            "schema-version = 1\nversion = \"1.0.0\"\nsdk-requirement = \
             \"0.1\"\n[templates.demo]\npath = \"demo\"\n[templates.counter]\npath = \"counter\"\n",
        )
        .unwrap();

        let unit = artifact_unit("extra/templates", Some("bundle.toml"), &[]);
        let mut paths = include_paths(&dir, &unit).unwrap();
        paths.sort();
        assert_eq!(paths, [PathBuf::from("counter"), PathBuf::from("demo")]);
    }

    #[test]
    fn a_unit_that_records_no_version_says_so() {
        let dir = temp_dir("version-none");
        let unit = artifact_unit("site", None, &["index.html"]);
        let error = format!("{:#}", read_version(&dir, &unit).unwrap_err());
        assert!(error.contains("no version"), "{error}");
    }

    /// An explicit `version-file` wins over the manifest's `version` key, and
    /// -- per `VersionFile::path`'s doc, "relative to the workspace root" --
    /// resolves against the workspace root rather than the unit's own source
    /// directory. The file here sits outside "site" entirely, so a resolution
    /// against the unit directory would miss it.
    #[test]
    fn an_explicit_version_file_wins_and_resolves_against_the_workspace_root() {
        let dir = temp_dir("version-file-explicit");
        std::fs::create_dir_all(dir.join("site")).unwrap();
        std::fs::write(dir.join("site/index.html"), "<html></html>").unwrap();
        std::fs::write(
            dir.join("VERSION.toml"),
            "release-version = \"9.9.9\"\nother = \"untouched\"\n",
        )
        .unwrap();

        let mut unit = artifact_unit("site", None, &["index.html"]);
        unit.source.as_mut().unwrap().version_file = Some(crate::config::VersionFile {
            path: PathBuf::from("VERSION.toml"),
            key: "release-version".to_string(),
        });

        assert_eq!(read_version(&dir, &unit).unwrap(), Version::parse("9.9.9").unwrap());

        let path = write_version(&dir, &unit, &Version::parse("9.9.10").unwrap()).unwrap();
        assert_eq!(path, dir.join("VERSION.toml"));

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("release-version = \"9.9.10\""), "{after}");
        assert!(after.contains("other = \"untouched\""), "{after}");
    }

    #[test]
    fn requirements_are_rewritten_only_for_tracked_packages() {
        let dir = temp_dir("tracks-rewrite");
        let root = dir.join("t");
        std::fs::create_dir_all(root.join("demo")).unwrap();
        std::fs::write(
            root.join("bundle.toml"),
            "schema-version = 1\nversion = \"1.0.0\"\nlib-requirement = \
             \"0.1\"\n[templates.demo]\npath = \"demo\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("demo/Cargo.toml"),
            "[dependencies]\nthelib = \"0.1\"\nsomething-else = \"0.1\"\n",
        )
        .unwrap();

        let changed = set_requirement(
            &root,
            Path::new("bundle.toml"),
            "lib-requirement",
            &["thelib".to_string()],
            &[PathBuf::from("demo")],
            "0.2",
        )
        .unwrap();

        assert_eq!(changed.len(), 2, "the manifest and one template: {changed:?}");
        let demo = std::fs::read_to_string(root.join("demo/Cargo.toml")).unwrap();
        assert!(demo.contains("thelib = \"0.2\""), "{demo}");
        assert!(
            demo.contains("something-else = \"0.1\""),
            "an untracked package must not be rewritten: {demo}"
        );
    }

    #[test]
    fn a_stale_requirement_is_reported() {
        let dir = temp_dir("tracks-check");
        let root = dir.join("t");
        std::fs::create_dir_all(root.join("demo")).unwrap();
        std::fs::write(root.join("demo/Cargo.toml"), "[dependencies]\nthelib = \"0.1\"\n").unwrap();

        let problems =
            check_requirements(&root, &[PathBuf::from("demo")], &["thelib".to_string()], "0.2")
                .unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("thelib"), "{problems:?}");
    }

    /// `#[serde(flatten)]` collects every key the struct does not name, and
    /// `#[serde(skip)]` removes `manifest_name` from deserialization entirely.
    /// The two must not interact: the private invariant stays set from the
    /// loaded path, and never leaks into `extra`.
    #[test]
    fn the_flattened_map_holds_only_undeclared_keys() {
        let dir = temp_dir("flatten-invariant");
        let root = dir.join("t");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("templates.toml"),
            "schema-version = 1\nversion = \"1.0.0\"\nsdk-requirement = \
             \"0.13\"\n[templates.demo]\npath = \"demo\"\n",
        )
        .unwrap();

        let bundle = Bundle::load(&root.join("templates.toml")).unwrap();
        assert_eq!(bundle.manifest_name, "templates.toml");
        assert_eq!(bundle.extra.keys().collect::<Vec<_>>(), ["sdk-requirement"]);
        assert!(!bundle.extra.contains_key("manifest_name"), "{:?}", bundle.extra);
        assert!(!bundle.extra.contains_key("manifest-name"), "{:?}", bundle.extra);
        assert!(!bundle.extra.contains_key("schema-version"), "{:?}", bundle.extra);
        assert!(!bundle.extra.contains_key("version"), "{:?}", bundle.extra);
        assert!(!bundle.extra.contains_key("templates"), "{:?}", bundle.extra);
        assert_eq!(bundle.requirement("sdk-requirement").unwrap(), "0.13");
    }

    #[test]
    fn a_requirement_key_the_manifest_lacks_is_an_error_not_a_panic() {
        let dir = temp_dir("tracks-missing");
        let root = dir.join("t");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("bundle.toml"),
            "schema-version = 1\nversion = \"1.0.0\"\n[templates.demo]\npath = \"demo\"\n",
        )
        .unwrap();

        let bundle = Bundle::load(&root.join("bundle.toml")).unwrap();
        let error = format!("{:#}", bundle.requirement("lib-requirement").unwrap_err());
        assert!(error.contains("lib-requirement"), "{error}");
    }
}
