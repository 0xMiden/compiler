//! The executor driving a sealed plan to completion.
//!
//! These run against the rehearsal registry, which is as close to production as
//! anything can get without publishing for real: the same Cargo, the same
//! upload protocol, the same index confirmation. What they cannot cover is
//! crates.io itself accepting the upload, and the Trusted Publishing identity
//! behind it.

use std::{fs, path::Path, sync::Arc};

use midenc_release::{
    closure,
    executor::{self, Options, Target},
    intent::{Intent, Stage, Tag},
    plan::{self, Plan},
    registry::{Faults, NoUpstream, Registry, client::SparseIndex},
    workspace::{EdgeKind, Package, Workspace},
};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-executor-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// An SDK-then-compiler shape: `comp` depends on `sdk`, so the stages must run
/// in that order or the compiler stage publishes crates nobody can resolve.
fn fixture(dir: &Path) {
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"sdk\", \"comp\"]\n",
    );
    write(
        &dir.join("sdk/Cargo.toml"),
        "[package]\nname = \"exec-sdk\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \
         \"MIT\"\ndescription = \"fixture\"\n",
    );
    write(&dir.join("sdk/src/lib.rs"), "");
    write(
        &dir.join("comp/Cargo.toml"),
        "[package]\nname = \"exec-comp\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \
         \"MIT\"\ndescription = \"fixture\"\n\n[dependencies]\nexec-sdk = { version = \"0.1.0\", \
         path = \"../sdk\" }\n",
    );
    write(&dir.join("comp/src/lib.rs"), "");

    let status = std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
        .current_dir(dir)
        .args(["generate-lockfile"])
        .status()
        .unwrap();
    assert!(status.success());
}

fn workspace(dir: &Path) -> Workspace {
    Workspace {
        root: dir.to_path_buf(),
        packages: [("exec-sdk", vec![]), ("exec-comp", vec!["exec-sdk"])]
            .into_iter()
            .map(|(name, deps)| {
                (
                    name.to_string(),
                    Package {
                        version: "0.1.0".into(),
                        manifest_path: dir.join("Cargo.toml"),
                        local_deps: deps
                            .into_iter()
                            .map(|d: &str| (d.to_string(), EdgeKind::Required))
                            .collect(),
                        publishable: true,
                    },
                )
            })
            .collect(),
    }
}

fn intent() -> Intent {
    Intent {
        schema_version: 1,
        subject: "abc123".into(),
        candidate_digest: "cand".into(),
        stages: vec![
            Stage {
                unit: "sdk".into(),
                version: "0.1.0".into(),
                prerelease: false,
                packages: vec!["exec-sdk".into()],
            },
            Stage {
                unit: "compiler".into(),
                version: "0.1.0".into(),
                prerelease: false,
                packages: vec!["exec-comp".into()],
            },
        ],
        tags: vec![
            Tag {
                unit: "sdk".into(),
                name: "sdk/v0.1.0".into(),
            },
            Tag {
                unit: "compiler".into(),
                name: "v0.1.0".into(),
            },
        ],
    }
}

/// Seal a plan by actually packaging, so the digests are real.
fn sealed_plan(dir: &Path) -> Plan {
    let built = closure::verify(
        dir,
        &closure::Options {
            packages: vec!["exec-sdk".into(), "exec-comp".into()],
            build_consumer: false,
            allow_upstream: false,
            cache_dir: None,
        },
    )
    .unwrap();
    plan::seal(&intent(), &built).unwrap()
}

fn options(dir: &Path, dry_run: bool) -> Options {
    let cargo_home = dir.join(".cargo-home-exec");
    fs::create_dir_all(&cargo_home).unwrap();
    Options {
        dry_run,
        cargo_home,
    }
}

fn rehearsal(registry: &Registry) -> Target {
    Target::Rehearsal {
        index_url: registry.index_url(),
        token: "rehearsal".into(),
    }
}

#[test]
fn a_plan_publishes_stage_by_stage_and_verifies_each() {
    let dir = temp_dir("full");
    fixture(&dir);
    let plan = sealed_plan(&dir);

    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let index = SparseIndex::new(registry.index_url());
    let journal = executor::execute(
        &workspace(&dir),
        &plan,
        &index,
        &rehearsal(&registry),
        &options(&dir, false),
    )
    .unwrap();

    assert_eq!(registry.published_versions("exec-sdk"), ["0.1.0"]);
    assert_eq!(registry.published_versions("exec-comp"), ["0.1.0"]);

    let stages: Vec<&str> = journal
        .entries
        .iter()
        .filter(|e| e.action == "publish")
        .map(|e| e.stage.as_str())
        .collect();
    assert_eq!(stages, ["sdk", "compiler"], "the sdk stage must precede the compiler stage");
    assert_eq!(
        journal.entries.iter().filter(|e| e.action == "verified").count(),
        2,
        "each stage is verified from the registry, not from cargo's exit status"
    );
}

#[test]
fn a_dry_run_publishes_nothing() {
    let dir = temp_dir("dry");
    fixture(&dir);
    let plan = sealed_plan(&dir);

    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let index = SparseIndex::new(registry.index_url());
    let journal = executor::execute(
        &workspace(&dir),
        &plan,
        &index,
        &rehearsal(&registry),
        &options(&dir, true),
    )
    .unwrap();

    assert!(registry.published_versions("exec-sdk").is_empty());
    assert!(journal.entries.iter().any(|e| e.action == "dry-run"));
}

/// The resume property, driven by the executor rather than by hand.
#[test]
fn a_run_that_dies_in_the_second_stage_resumes_without_republishing_the_first() {
    let dir = temp_dir("resume");
    fixture(&dir);
    let plan = sealed_plan(&dir);

    let registry =
        Registry::start(0, Faults::unauthorized(&["exec-comp"]), Arc::new(NoUpstream)).unwrap();
    let index = SparseIndex::new(registry.index_url());

    let err = executor::execute(
        &workspace(&dir),
        &plan,
        &index,
        &rehearsal(&registry),
        &options(&dir, false),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("publishing stage 'compiler'"), "{err}");
    assert_eq!(registry.published_versions("exec-sdk"), ["0.1.0"], "the sdk stage completed");
    assert!(registry.published_versions("exec-comp").is_empty());

    // Fix the cause and run the same plan again. The sdk stage must recognise
    // itself as complete and not invoke Cargo, which would fail outright.
    registry.set_faults(Faults::default());
    let journal = executor::execute(
        &workspace(&dir),
        &plan,
        &index,
        &rehearsal(&registry),
        &options(&dir, false),
    )
    .unwrap();

    let sdk: Vec<&str> = journal
        .entries
        .iter()
        .filter(|e| e.stage == "sdk")
        .map(|e| e.action.as_str())
        .collect();
    assert!(sdk.contains(&"complete"), "the sdk stage should be recognised as done: {sdk:?}");
    assert!(!sdk.contains(&"publish"), "it must not be republished: {sdk:?}");
    assert_eq!(registry.published_versions("exec-comp"), ["0.1.0"]);
}

/// A version held by someone else must stop the release rather than be skipped.
#[test]
fn a_checksum_conflict_stops_the_release_before_publishing() {
    let dir = temp_dir("conflict");
    fixture(&dir);
    let plan = sealed_plan(&dir);

    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();

    // Someone else already holds exec-sdk 0.1.0, with different bytes.
    let foreign = midenc_release::registry::client::StubIndex::new().publish(
        "exec-sdk",
        "0.1.0",
        "not-our-bytes",
        false,
    );

    let err = executor::execute(
        &workspace(&dir),
        &plan,
        &foreign,
        &rehearsal(&registry),
        &options(&dir, false),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("conflict"), "{err}");
    assert!(
        registry.published_versions("exec-comp").is_empty(),
        "a conflict in the first stage must prevent the second from starting"
    );
}
