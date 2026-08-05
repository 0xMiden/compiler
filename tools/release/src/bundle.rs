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

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Bundle {
    pub schema_version: u32,
    pub version: Version,
    /// The `miden` requirement the templates carry.
    pub sdk_requirement: String,
    pub templates: BTreeMap<String, TemplateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateEntry {
    pub path: PathBuf,
}

impl Bundle {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read bundle manifest at {}", path.display()))?;
        let bundle: Self =
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
        Ok(bundle)
    }

    /// Every file the archive should contain, relative to the templates root,
    /// in a stable order.
    pub fn files(&self, root: &Path) -> Result<Vec<PathBuf>> {
        Ok(self.entries(root)?.into_iter().map(|(path, _)| path).collect())
    }

    /// Every file the archive should contain, paired with whether it is
    /// executable, in a stable order.
    ///
    /// The executable flag comes from git's mode rather than the filesystem's:
    /// git records exactly one bit (`100644` or `100755`), whereas an on-disk
    /// mode varies with umask and platform and would put the archive's digest
    /// back at the mercy of whoever built it.
    pub fn entries(&self, root: &Path) -> Result<Vec<(PathBuf, bool)>> {
        let mut files = vec![(PathBuf::from("bundle.toml"), false)];

        for (name, entry) in &self.templates {
            let directory = root.join(&entry.path);
            if !directory.is_dir() {
                bail!(
                    "template '{name}' points at {}, which is not a directory",
                    entry.path.display()
                );
            }
            collect(&directory, root, &mut files)
                .with_context(|| format!("collecting template '{name}'"))?;
        }

        files.sort();
        files.dedup();
        Ok(files)
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

/// Files sitting in a template directory that git does not track.
///
/// These are invisible to [`Bundle::files`] by design, but silently dropping
/// them is how an archive comes to differ between two checkouts of the same
/// commit. Callers surface them so the omission is a decision rather than an
/// accident.
pub fn untracked(bundle: &Bundle, root: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();

    for entry in bundle.templates.values() {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z", "--others", "--"])
            .arg(root.join(&entry.path))
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
pub fn archive(root: &Path, bundle: &Bundle) -> Result<(Vec<u8>, String)> {
    let mut entries = Vec::new();
    for (relative, executable) in bundle.entries(root)? {
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

/// The `miden` requirement templates should carry for a given SDK version.
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

/// Rewrite the bundle's declared requirement and every template manifest to
/// match a new SDK version.
pub fn set_sdk_requirement(root: &Path, requirement: &str) -> Result<Vec<PathBuf>> {
    let bundle_path = root.join("bundle.toml");
    let bundle = Bundle::load(&bundle_path)?;
    let mut changed = Vec::new();

    let text = std::fs::read_to_string(&bundle_path)?;
    let mut document: toml_edit::DocumentMut = text.parse()?;
    document["sdk-requirement"] = toml_edit::value(requirement);
    std::fs::write(&bundle_path, document.to_string())?;
    changed.push(bundle_path);

    let old = format!("\"{}\"", bundle.sdk_requirement);
    let new = format!("\"{requirement}\"");

    for entry in bundle.templates.values() {
        let mut manifests = Vec::new();
        find_manifests(&root.join(&entry.path), &mut manifests)?;
        for manifest in manifests {
            let text = std::fs::read_to_string(&manifest)?;
            let updated: String = text
                .lines()
                .map(|line| {
                    let trimmed = line.trim();
                    let is_miden_version = (trimmed.starts_with("miden ")
                        || trimmed.starts_with("miden="))
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

/// Move the bundle's own version.
pub fn set_version(root: &Path, version: &Version) -> Result<()> {
    let path = root.join("bundle.toml");
    let text = std::fs::read_to_string(&path)?;
    let mut document: toml_edit::DocumentMut = text.parse()?;
    document["version"] = toml_edit::value(version.to_string());
    std::fs::write(&path, document.to_string())?;
    Ok(())
}

/// Check that every template's SDK requirement matches what the bundle declares.
///
/// This is the drift that would otherwise be silent: after an SDK minor bump,
/// a template left at the old requirement still renders, still builds, and
/// quietly pins generated projects to the previous SDK.
pub fn check_sdk_requirements(root: &Path, bundle: &Bundle) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let expected = format!("\"{}\"", bundle.sdk_requirement);

    for (name, entry) in &bundle.templates {
        let directory = root.join(&entry.path);
        let mut manifests = Vec::new();
        find_manifests(&directory, &mut manifests)?;

        for manifest in manifests {
            let text = std::fs::read_to_string(&manifest)?;
            for (number, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                // `miden = "0.13"` or `miden = { version = "0.13" }`. Lines
                // selecting a path or git source are development escape hatches
                // and carry no version.
                if !trimmed.starts_with("miden ") && !trimmed.starts_with("miden=") {
                    continue;
                }
                if !trimmed.contains("version") && !trimmed.contains('"') {
                    continue;
                }
                if trimmed.contains("path") || trimmed.contains("git") {
                    continue;
                }
                if !trimmed.contains(&expected) {
                    problems.push(format!(
                        "{}:{}: template '{name}' requires `{trimmed}` but the bundle declares \
                         sdk-requirement = \"{}\"",
                        manifest.display(),
                        number + 1,
                        bundle.sdk_requirement
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
        let dir = std::env::temp_dir().join(format!("bundle-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        write(
            &dir.join("bundle.toml"),
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
             }\n",
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
        let files = bundle.files(&dir).unwrap();

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
        let (before, digest_before) = archive(&dir, &bundle).unwrap();

        write(&dir.join("rust/account/template/.claude/settings.json"), "{}");
        let (after, digest_after) = archive(&dir, &bundle).unwrap();

        assert_eq!(digest_before, digest_after, "an untracked file changed the bundle digest");
        assert_eq!(before, after);

        // ... but it is reported, so the omission is visible rather than silent.
        let strays = untracked(&bundle, &dir).unwrap();
        assert_eq!(
            strays,
            [PathBuf::from("rust/account/template/.claude/settings.json")],
            "an untracked template file must be reported"
        );

        // Once committed it ships, which is the only way to add template content.
        git(&dir, &["add", "-A"]);
        let (_, digest_tracked) = archive(&dir, &bundle).unwrap();
        assert_ne!(digest_before, digest_tracked);
        assert!(untracked(&bundle, &dir).unwrap().is_empty());
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
        let entries = bundle.entries(&dir).unwrap();

        let executable: Vec<&PathBuf> =
            entries.iter().filter(|(_, exec)| *exec).map(|(path, _)| path).collect();
        assert_eq!(executable, [&PathBuf::from("rust/account/template/hook.sh")], "{entries:?}");
    }

    #[test]
    fn the_file_list_is_stable() {
        let dir = fixture("stable");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        let first = bundle.files(&dir).unwrap();
        for _ in 0..8 {
            assert_eq!(bundle.files(&dir).unwrap(), first);
        }
    }

    #[test]
    fn a_matching_sdk_requirement_is_accepted() {
        let dir = fixture("match");
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        assert!(check_sdk_requirements(&dir, &bundle).unwrap().is_empty());
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

        let problems = check_sdk_requirements(&dir, &bundle).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("0.14"), "{}", problems[0]);
        assert!(problems[0].contains("account"), "{}", problems[0]);
    }

    #[test]
    fn path_and_git_escape_hatches_are_ignored() {
        let dir = fixture("escape-hatch");
        write(
            &dir.join("rust/account/template/Cargo.toml"),
            "[dependencies]\n{% if compiler_path %}\nmiden = { path = \"{{ compiler_path \
             }}/sdk/sdk\" }\n{% else %}\nmiden = { version = \"0.13\" }\n{% endif %}\n",
        );
        let bundle = Bundle::load(&dir.join("bundle.toml")).unwrap();
        assert!(
            check_sdk_requirements(&dir, &bundle).unwrap().is_empty(),
            "development source selections carry no version and must not be flagged"
        );
    }
}
