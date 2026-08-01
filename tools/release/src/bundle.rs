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
        let mut files = vec![PathBuf::from("bundle.toml")];

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

/// Walk a template directory, skipping what should never ship.
fn collect(directory: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // Build output and VCS metadata are never part of a template. `Cargo.lock`
        // is excluded from the rendered templates for the same reason Cargo omits
        // it from published libraries: the generated project should resolve fresh.
        if matches!(name.as_ref(), "target" | ".git" | ".DS_Store") {
            continue;
        }

        if path.is_dir() {
            collect(&path, root, files)?;
        } else {
            files.push(
                path.strip_prefix(root).expect("walked paths are under the root").to_path_buf(),
            );
        }
    }
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
        dir
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
