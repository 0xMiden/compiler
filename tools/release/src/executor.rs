//! Driving publication.
//!
//! Everything up to here decides; this acts. It publishes a sealed plan stage
//! by stage, reconciling against live registry state before each one so that a
//! first attempt and a resume are the same operation.
//!
//! Two properties are load-bearing and easy to lose:
//!
//! The token never reaches a command line. Process arguments are visible to
//! every other process on the machine, and `cargo publish --token` is
//! deprecated for exactly that reason. Production passes credentials through
//! the environment.
//!
//! Subject-controlled Cargo configuration never reaches the credentialed
//! invocation. `--no-verify` keeps package build scripts from running beside a
//! live token, but configuration is the larger hazard: a `.cargo/config.toml`
//! in the checkout can set `http.proxy` and `http.cainfo` and exfiltrate the
//! token without executing a line of package code.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    github::{self, GitHub},
    plan::Plan,
    reconcile::{self, Disposition},
    registry::client::IndexClient,
    workspace::Workspace,
};

/// Where a stage publishes to.
pub enum Target {
    /// Production. Credentials come from `CARGO_REGISTRY_TOKEN` in the
    /// environment, never from an argument.
    CratesIo,
    /// A rehearsal registry. `--index` requires an explicit `--token`, which is
    /// acceptable only because the value is worthless.
    Rehearsal { index_url: String, token: String },
}

impl Target {
    fn is_production(&self) -> bool {
        matches!(self, Self::CratesIo)
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CratesIo => f.write_str("crates.io"),
            Self::Rehearsal { index_url, .. } => write!(f, "{index_url}"),
        }
    }
}

/// One recorded operation. The journal is diagnostic: registry state is
/// authoritative, and every decision is re-derived from it rather than replayed
/// from here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct JournalEntry {
    pub stage: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Journal {
    pub entries: Vec<JournalEntry>,
}

impl Journal {
    fn record(&mut self, stage: &str, action: &str, detail: impl Into<String>) {
        self.entries.push(JournalEntry {
            stage: stage.to_string(),
            action: action.to_string(),
            detail: detail.into(),
        });
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("journal is serializable")
    }
}

pub struct Options {
    /// Print what would happen and stop before anything irreversible.
    pub dry_run: bool,
    /// Cargo's config discovery walks up from the working directory, so an
    /// isolated home is not sufficient on its own; see [`check_no_subject_config`].
    pub cargo_home: PathBuf,
}

/// Publish every stage of a plan, in order.
///
/// Stages are strictly sequential and each is verified before the next begins:
/// compiler crates depend on SDK crates, so starting the compiler stage before
/// the SDK is fully resolvable would publish crates nobody can build.
pub fn execute(
    ws: &Workspace,
    plan: &Plan,
    index: &dyn IndexClient,
    github: &dyn GitHub,
    target: &Target,
    options: &Options,
) -> Result<Journal> {
    let mut journal = Journal::default();

    if target.is_production() {
        check_no_subject_config(&ws.root)?;
        journal.record("preflight", "config-isolation", "no subject Cargo configuration present");
    }
    configure_cargo_home(target, options)?;

    for stage in &plan.intent.stages {
        // The unit's tag is created here, immediately before its own stage,
        // rather than for every unit at the start of the phase. A permanent
        // failure in the SDK stage would otherwise burn the compiler's tag too,
        // and a tag cannot be moved once the release it names is finalized.
        //
        // A stage with no crates still gets its tag: templates publish nothing,
        // so this is the only place their tag can come from.
        tag_stage(plan, github, &stage.unit, options, &mut journal)?;

        let planned = plan.planned_for(&stage.unit);
        if planned.is_empty() {
            journal.record(&stage.unit, "skip", "stage publishes no crates");
            continue;
        }

        let result = reconcile::reconcile(ws, index, &planned)?;

        for outcome in result.conflicts() {
            journal.record(
                &stage.unit,
                "conflict",
                format!("{} {}: {:?}", outcome.name, outcome.version, outcome.disposition),
            );
        }
        if !result.is_publishable() {
            bail!(
                "stage '{}' has {} conflict(s); resolve them or abandon the release",
                stage.unit,
                result.conflicts().count()
            );
        }

        for outcome in &result.outcomes {
            if outcome.disposition == Disposition::Skip {
                journal.record(
                    &stage.unit,
                    "skip",
                    format!("{} {} already published", outcome.name, outcome.version),
                );
            }
        }

        if result.is_complete() {
            // `cargo publish` errors when every `-p` package already exists, so
            // a fully published stage must not invoke it at all.
            journal.record(&stage.unit, "complete", "every planned version is already published");
            continue;
        }

        journal.record(
            &stage.unit,
            "publish",
            format!("{} crate(s): {}", result.to_publish.len(), result.to_publish.join(", ")),
        );

        if options.dry_run {
            journal.record(&stage.unit, "dry-run", "stopped before publishing");
            continue;
        }

        publish(&ws.root, &result.to_publish, target, options)
            .with_context(|| format!("publishing stage '{}'", stage.unit))?;

        // Verify from the registry rather than trusting the exit status: an
        // upload can be accepted and still not be resolvable.
        let after = reconcile::reconcile(ws, index, &planned)?;
        if !after.is_complete() {
            bail!(
                "stage '{}' reported success but {} crate(s) are still missing: {}",
                stage.unit,
                after.to_publish.len(),
                after.to_publish.join(", ")
            );
        }
        journal.record(&stage.unit, "verified", "every planned version is published");
    }

    Ok(journal)
}

/// Create a stage's tag at the subject commit.
///
/// Creation is fail-closed: an existing ref is only acceptable if it already
/// points at the subject, which is what makes a resume safe and an unexpected
/// tag an incident rather than something to overwrite.
fn tag_stage(
    plan: &Plan,
    github: &dyn GitHub,
    unit: &str,
    options: &Options,
    journal: &mut Journal,
) -> Result<()> {
    let Some(tag) = plan.intent.tags.iter().find(|tag| tag.unit == unit) else {
        journal.record(unit, "no-tag", "the plan declares no tag for this unit");
        return Ok(());
    };

    if options.dry_run {
        journal.record(unit, "dry-run", format!("would create tag '{}'", tag.name));
        return Ok(());
    }

    let outcome = github::create_tag_idempotent(github, &tag.name, &plan.intent.subject)
        .with_context(|| format!("creating the tag for stage '{unit}'"))?;
    let detail = match outcome {
        github::TagOutcome::Created => format!("created '{}' at {}", tag.name, plan.intent.subject),
        github::TagOutcome::AlreadyCorrect => {
            format!("'{}' already points at {}", tag.name, plan.intent.subject)
        }
    };
    journal.record(unit, "tag", detail);
    Ok(())
}

/// Point the executor's Cargo home at the right source of truth.
///
/// Production needs nothing: each stage resolves the previous stage's crates
/// from crates.io, where they were just published. A rehearsal needs source
/// replacement, because the previous stage published to a registry that is not
/// crates.io, and `--index` redirects only the *upload*, never resolution. Get
/// this wrong and the second stage fails looking for a crate that exists —
/// somewhere else.
fn configure_cargo_home(target: &Target, options: &Options) -> Result<()> {
    std::fs::create_dir_all(&options.cargo_home)?;
    let config = options.cargo_home.join("config.toml");

    match target {
        Target::CratesIo => {
            // Leave no replacement behind from an earlier rehearsal.
            if config.exists() {
                std::fs::remove_file(&config)?;
            }
        }
        Target::Rehearsal { index_url, .. } => std::fs::write(
            &config,
            format!(
                    "[source.crates-io]\nreplace-with = \
                     \"rehearsal\"\n\n[source.rehearsal]\nregistry                  = \
                     \"{index_url}\"\n"
                ),
        )
        .with_context(|| format!("failed to write {}", config.display()))?,
    }
    Ok(())
}

/// Refuse to run a credentialed Cargo beside subject-controlled configuration.
///
/// Cargo discovers `.cargo/config.toml` by walking up from the working
/// directory, and such a file can redirect HTTP through a proxy of the
/// subject's choosing. Excluding subject *code* from credentialed jobs is not
/// enough; subject *configuration* reaches further.
pub fn check_no_subject_config(workspace_root: &Path) -> Result<()> {
    let mut directory = Some(workspace_root);
    while let Some(current) = directory {
        for name in ["config.toml", "config"] {
            let candidate = current.join(".cargo").join(name);
            if candidate.exists() {
                bail!(
                    "refusing to publish with {} present: Cargo would honour it, and it can \
                     redirect HTTP away from the registry while a token is in the environment",
                    candidate.display()
                );
            }
        }
        directory = current.parent();
    }
    Ok(())
}

fn publish(
    workspace_root: &Path,
    packages: &[String],
    target: &Target,
    options: &Options,
) -> Result<()> {
    let mut command = Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"));
    command
        .current_dir(workspace_root)
        .env("CARGO_HOME", &options.cargo_home)
        .args(["publish", "--no-verify", "--locked"]);

    match target {
        Target::CratesIo => {
            // The token stays in the environment. Cargo reads
            // CARGO_REGISTRY_TOKEN for crates.io, and the caller is responsible
            // for putting it there and for its lifetime.
            command.args(["--registry", "crates-io"]);
            if std::env::var("CARGO_REGISTRY_TOKEN").is_err() {
                bail!(
                    "CARGO_REGISTRY_TOKEN is not set; production publication expects credentials \
                     in the environment, never on the command line"
                );
            }
        }
        Target::Rehearsal { index_url, token } => {
            command.args(["--index", index_url]).args(["--token", token]);
        }
    }

    for package in packages {
        command.args(["-p", package]);
    }

    let output = command.output().context("failed to run `cargo publish`")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subject_cargo_config_blocks_production_publication() {
        let dir = std::env::temp_dir().join(format!("executor-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ws/.cargo")).unwrap();
        std::fs::write(dir.join("ws/.cargo/config.toml"), "[http]\nproxy = \"evil\"\n").unwrap();

        let err = check_no_subject_config(&dir.join("ws")).unwrap_err().to_string();
        assert!(err.contains("refusing to publish"), "{err}");
        assert!(err.contains("redirect HTTP"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_config_above_the_workspace_is_also_refused() {
        // Cargo walks up, so a parent directory is just as dangerous.
        let dir = std::env::temp_dir().join(format!("executor-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        std::fs::create_dir_all(dir.join("ws")).unwrap();
        std::fs::write(dir.join(".cargo/config.toml"), "").unwrap();

        assert!(check_no_subject_config(&dir.join("ws")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn production_refuses_to_run_without_credentials_in_the_environment() {
        // Guards the invariant that the token is never passed as an argument:
        // if it is not in the environment, there is nowhere else it could be.
        unsafe { std::env::remove_var("CARGO_REGISTRY_TOKEN") };
        let options = Options {
            dry_run: false,
            cargo_home: PathBuf::from("/tmp"),
        };
        let err = publish(Path::new("/tmp"), &["a".into()], &Target::CratesIo, &options)
            .unwrap_err()
            .to_string();
        assert!(err.contains("CARGO_REGISTRY_TOKEN"), "{err}");
        assert!(err.contains("never on the command line"), "{err}");
    }

    #[test]
    fn journals_serialize() {
        let mut journal = Journal::default();
        journal.record("sdk", "publish", "2 crates");
        let json = journal.to_json();
        assert!(json.contains("\"stage\": \"sdk\""), "{json}");
    }
}
