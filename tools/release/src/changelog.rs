//! Changelog prompts.
//!
//! This emits a prompt and nothing else. It never writes changelog entries, and
//! that restraint is the point: a changelog is a claim about what changed and
//! why it matters to someone outside this repository, which is a judgement no
//! commit-message digest can make. Generating prose here would produce something
//! that looks reviewed without having been.
//!
//! What it *can* do reliably is assemble the material: which unit is being
//! described, what its previous release was, and which commits since then
//! touched the packages that unit publishes. Gathering that by hand is where
//! entries get missed.

use std::{collections::BTreeSet, path::Path, process::Command};

use anyhow::{Context, Result, bail};

use crate::{
    config::{Config, Unit},
    workspace::Workspace,
};

/// One commit in the range being described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug)]
pub struct Prompt {
    pub unit: String,
    pub changelog: String,
    /// The revision range, as passed to git.
    pub range: String,
    /// The tag the range starts from, when there was one.
    pub baseline: Option<String>,
    pub changes: Vec<Change>,
    /// Paths whose history was consulted.
    pub paths: Vec<String>,
}

/// The user-facing headings for a unit's changelog.
///
/// The compiler's changelog is grouped by what a reader is looking for -- a
/// tool, or an API they depend on -- rather than by crate, because the crate
/// split is an implementation detail nobody outside this repository tracks.
fn headings(unit: &str) -> &'static [&'static str] {
    match unit {
        "compiler" => &[
            "Compiler and `midenc`",
            "`cargo-miden`",
            "`miden-objtool`",
            "Libraries and public APIs",
            "Migration and breaking changes",
        ],
        "sdk" => &["Added", "Changed", "Fixed", "Migration and breaking changes"],
        "templates" => &["Templates", "SDK compatibility"],
        _ => &["Added", "Changed", "Fixed"],
    }
}

/// Build the prompt material for a unit.
///
/// `range` overrides the default, which runs from the unit's most recent
/// release tag to `HEAD`. Before a unit's first release there is no baseline,
/// and the whole history is in scope.
pub fn prepare(
    ws: &Workspace,
    config: &Config,
    unit: &str,
    range: Option<String>,
) -> Result<Prompt> {
    let unit_config = config
        .units
        .get(unit)
        .with_context(|| format!("'{unit}' is not a release unit; see .release/config.toml"))?;

    let paths = unit_paths(ws, config, unit)?;
    if paths.is_empty() {
        bail!("unit '{unit}' owns no paths, so there is nothing to describe");
    }

    let (range, baseline) = match range {
        Some(explicit) => (explicit, None),
        None => match latest_tag(&ws.root, &unit_config.tag)? {
            Some(tag) => (format!("{tag}..HEAD"), Some(tag)),
            // No previous release: everything is new.
            None => ("HEAD".to_string(), None),
        },
    };

    let changes = commits(&ws.root, &range, &paths)?;

    Ok(Prompt {
        unit: unit.to_string(),
        changelog: unit_config.changelog.clone(),
        range,
        baseline,
        changes,
        paths,
    })
}

/// Directories whose history describes a unit.
///
/// Derived from the unit's packages rather than hardcoded, so a package moving
/// between units changes what its commits describe without anyone remembering
/// to update a list here.
fn unit_paths(ws: &Workspace, config: &Config, unit: &str) -> Result<Vec<String>> {
    // Templates publish an archive rather than crates, so no package is ever
    // assigned to them and their history lives in the template sources.
    if unit == "templates" {
        return Ok(vec!["extra/templates".to_string()]);
    }

    let wanted: Unit = unit
        .parse()
        .with_context(|| format!("'{unit}' is not a unit this tool can resolve packages for"))?;

    let mut paths = BTreeSet::new();
    for package in config.packages_in(wanted).filter(|package| package.publish) {
        let Some(actual) = ws.packages.get(&package.name) else {
            continue;
        };
        let Some(directory) = actual.manifest_path.parent() else {
            continue;
        };
        let relative = directory.strip_prefix(&ws.root).unwrap_or(directory);
        paths.insert(relative.to_string_lossy().replace('\\', "/"));
    }

    Ok(paths.into_iter().collect())
}

/// The most recent tag matching a unit's tag pattern.
///
/// Sorted by version rather than by date: tags are created in whatever order
/// releases happened, and a patch on an older line would otherwise look like
/// the latest release.
fn latest_tag(root: &Path, pattern: &str) -> Result<Option<String>> {
    let glob = pattern.replace("{version}", "*");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["tag", "--list", &glob, "--sort=-v:refname"])
        .output()
        .context("failed to run `git tag`")?;

    if !output.status.success() {
        bail!("`git tag --list {glob}` failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string))
}

fn commits(root: &Path, range: &str, paths: &[String]) -> Result<Vec<Change>> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        // --no-merges: a merge commit's subject names the branch, not the
        // change, and its content is already present in the commits it brings.
        .args(["log", "--no-merges", "--format=%H%x00%s", range, "--"]);
    for path in paths {
        command.arg(path);
    }

    let output = command.output().context("failed to run `git log`")?;
    if !output.status.success() {
        bail!("`git log {range}` failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .map(|(sha, subject)| Change {
            sha: sha.to_string(),
            subject: subject.to_string(),
        })
        .collect())
}

impl Prompt {
    /// Render the prompt.
    pub fn render(&self, version: Option<&str>) -> String {
        let mut out = String::new();

        let heading = match version {
            Some(version) => format!("## [{version}]"),
            None => "## [Unreleased]".to_string(),
        };

        out.push_str(&format!(
            "Write the `{heading}` section of `{}` for the {} release unit.\n\n",
            self.changelog, self.unit
        ));

        out.push_str("Rules:\n");
        out.push_str(
            "- Describe what changed for someone using this software, not what changed in the \
             code. If a commit has no user-visible effect, leave it out.\n",
        );
        out.push_str("- Group entries under these headings, omitting any that would be empty:\n");
        for heading in headings(&self.unit) {
            out.push_str(&format!("    - {heading}\n"));
        }
        out.push_str(
            "- Anything that breaks a build, changes behaviour silently, or needs a code change \
             from the reader goes under migration, with the change spelled out.\n",
        );
        out.push_str(
            "- Do not invent entries, and do not describe commits you cannot explain in \
             user-facing terms; list those separately as needing review.\n",
        );
        out.push_str("- Keep the existing file's style and Keep a Changelog structure.\n\n");

        match &self.baseline {
            Some(tag) => out.push_str(&format!(
                "Range: {} (since the last {} release, {tag})\n",
                self.range, self.unit
            )),
            // The SDK and template tag namespaces are new, so their first
            // release under this tooling has no baseline and the range is the
            // entire history. That is the truth, but it is rarely the useful
            // question, so say how to narrow it.
            None => out.push_str(&format!(
                "Range: {} — this unit has no previous release tag, so this is its whole history. \
                 If that is not the range you mean, pass one explicitly: `release-tool \
                 changelog-prompt {} <range>`.\n",
                self.range, self.unit
            )),
        }
        out.push_str(&format!("Paths: {}\n\n", self.paths.join(", ")));

        if self.changes.is_empty() {
            out.push_str(
                "No commits touched this unit's paths in this range. Confirm that is expected \
                 before releasing it.\n",
            );
            return out;
        }

        out.push_str(&format!("{} commit(s):\n\n", self.changes.len()));
        for change in &self.changes {
            out.push_str(&format!(
                "  {} {}\n",
                &change.sha[..12.min(change.sha.len())],
                change.subject
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;
    use crate::{
        config::{PackageConfig, UnitConfig},
        workspace::Package,
    };

    fn config() -> Config {
        Config {
            schema_version: 1,
            units: BTreeMap::from([
                (
                    "compiler".to_string(),
                    UnitConfig {
                        tag: "v{version}".into(),
                        changelog: "CHANGELOG.md".into(),
                    },
                ),
                (
                    "templates".to_string(),
                    UnitConfig {
                        tag: "templates/v{version}".into(),
                        changelog: "extra/templates/CHANGELOG.md".into(),
                    },
                ),
            ]),
            packages: vec![PackageConfig {
                name: "midenc".into(),
                unit: Unit::Compiler,
                publish: true,
                version_source: None,
            }],
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/repo"),
            packages: BTreeMap::from([(
                "midenc".to_string(),
                Package {
                    version: "0.9.2".into(),
                    manifest_path: PathBuf::from("/repo/midenc/Cargo.toml"),
                    local_deps: vec![],
                    publishable: true,
                },
            )]),
        }
    }

    #[test]
    fn a_units_paths_come_from_its_packages() {
        let paths = unit_paths(&workspace(), &config(), "compiler").unwrap();
        assert_eq!(paths, ["midenc"]);
    }

    #[test]
    fn templates_are_described_by_their_sources() {
        let paths = unit_paths(&workspace(), &config(), "templates").unwrap();
        assert_eq!(paths, ["extra/templates"], "templates publish no crates");
    }

    #[test]
    fn an_unknown_unit_is_rejected() {
        let err = prepare(&workspace(), &config(), "nonsense", None).unwrap_err().to_string();
        assert!(err.contains("not a release unit"), "{err}");
    }

    fn prompt(changes: Vec<Change>, baseline: Option<&str>) -> Prompt {
        Prompt {
            unit: "compiler".into(),
            changelog: "CHANGELOG.md".into(),
            range: baseline.map(|t| format!("{t}..HEAD")).unwrap_or("HEAD".into()),
            baseline: baseline.map(str::to_string),
            changes,
            paths: vec!["midenc".into()],
        }
    }

    #[test]
    fn the_prompt_carries_the_commits_and_the_headings() {
        let rendered = prompt(
            vec![Change {
                sha: "0123456789abcdef".into(),
                subject: "fix: stop miscompiling loops".into(),
            }],
            Some("v0.9.2"),
        )
        .render(Some("0.10.0"));

        assert!(rendered.contains("## [0.10.0]"), "{rendered}");
        assert!(rendered.contains("CHANGELOG.md"), "{rendered}");
        assert!(rendered.contains("v0.9.2..HEAD"), "{rendered}");
        assert!(rendered.contains("0123456789ab"), "shas are abbreviated to 12: {rendered}");
        assert!(rendered.contains("stop miscompiling loops"), "{rendered}");
        assert!(rendered.contains("`cargo-miden`"), "the compiler headings: {rendered}");
    }

    #[test]
    fn it_asks_for_prose_and_never_writes_it() {
        let rendered = prompt(vec![], Some("v0.9.2")).render(None);
        assert!(rendered.starts_with("Write the"), "the output is a prompt: {rendered}");
        assert!(rendered.contains("## [Unreleased]"), "{rendered}");
    }

    /// An empty range is a fact worth stating: it usually means the unit should
    /// not be in the release at all.
    #[test]
    fn an_empty_range_says_so() {
        let rendered = prompt(vec![], Some("v0.9.2")).render(Some("0.10.0"));
        assert!(rendered.contains("No commits touched"), "{rendered}");
        assert!(rendered.contains("Confirm that is expected"), "{rendered}");
    }

    #[test]
    fn a_unit_with_no_previous_release_says_everything_is_new() {
        let rendered = prompt(vec![], None).render(Some("0.1.0"));
        assert!(rendered.contains("no previous release tag"), "{rendered}");
    }
}
