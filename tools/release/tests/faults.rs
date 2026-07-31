//! Failure injection against a real `cargo publish`.
//!
//! The property under test is not "does the registry return the right status" —
//! it is that a release which dies partway through leaves the registry in a
//! state reconciliation reads correctly, and can be resumed without
//! republishing what already landed. Mock clients cannot show that, because the
//! thing being tested is Cargo's actual behaviour when a publish goes wrong.

use std::{fs, path::Path, sync::Arc};

use midenc_release::{
    reconcile::{self, Disposition, Planned},
    registry::{Faults, NoUpstream, Registry, client::SparseIndex},
    workspace::{EdgeKind, Package, Workspace},
};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-faults-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Three crates in a chain: leaf <- mid <- root. A failure on `mid` should
/// leave `leaf` published and `root` untouched.
fn fixture(dir: &Path) {
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"leaf\", \"mid\", \"root\"]\n",
    );
    for (name, dep) in [("leaf", None), ("mid", Some("leaf")), ("root", Some("mid"))] {
        let dependency = dep
            .map(|d| {
                format!(
                    "\n[dependencies]\nfault-{d} = {{ version = \"0.1.0\", path = \"../{d}\" }}\n"
                )
            })
            .unwrap_or_default();
        write(
            &dir.join(name).join("Cargo.toml"),
            &format!(
                "[package]\nname = \"fault-{name}\"\nversion = \"0.1.0\"\nedition = \
                 \"2021\"\nlicense = \"MIT\"\ndescription = \"fixture\"\n{dependency}"
            ),
        );
        write(&dir.join(name).join("src/lib.rs"), "");
    }

    let status = std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
        .current_dir(dir)
        .args(["generate-lockfile"])
        .status()
        .unwrap();
    assert!(status.success());
}

fn cargo_home(dir: &Path, index_url: &str) -> std::path::PathBuf {
    let home = dir.join(".cargo-home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"faults\"\n\n[source.faults]\nregistry = \
             \"{index_url}\"\n"
        ),
    )
    .unwrap();
    home
}

/// Publish `packages` in order, returning whether Cargo succeeded.
fn publish(dir: &Path, home: &Path, index_url: &str, packages: &[&str]) -> (bool, String) {
    let mut command =
        std::process::Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"));
    command
        .current_dir(dir)
        .env("CARGO_HOME", home)
        .args(["publish", "--no-verify", "--allow-dirty"])
        .args(["--index", index_url])
        .args(["--token", "fault-token"]);
    for package in packages {
        command.args(["-p", package]);
    }
    let output = command.output().unwrap();
    (output.status.success(), String::from_utf8_lossy(&output.stderr).to_string())
}

fn workspace(dir: &Path) -> Workspace {
    Workspace {
        root: dir.to_path_buf(),
        packages: [
            ("fault-leaf", vec![]),
            ("fault-mid", vec!["fault-leaf"]),
            ("fault-root", vec!["fault-mid"]),
        ]
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

fn planned() -> Vec<Planned> {
    ["fault-leaf", "fault-mid", "fault-root"]
        .iter()
        .map(|name| Planned {
            name: name.to_string(),
            version: "0.1.0".into(),
            expected_cksum: None,
        })
        .collect()
}

/// The central property: a stage that dies partway leaves a readable state, and
/// resuming publishes exactly the remainder.
#[test]
fn a_release_rejected_mid_stage_resumes_with_only_what_is_missing() {
    let dir = temp_dir("mid-stage");
    fixture(&dir);

    // `fault-mid` has no trusted publisher configured. This cannot be
    // preflighted -- the token carries no crate list -- so it surfaces here.
    let registry =
        Registry::start(0, Faults::unauthorized(&["fault-mid"]), Arc::new(NoUpstream)).unwrap();
    let home = cargo_home(&dir, &registry.index_url());

    let (ok, stderr) =
        publish(&dir, &home, &registry.index_url(), &["fault-leaf", "fault-mid", "fault-root"]);
    assert!(!ok, "publication must fail when a crate is rejected");
    assert!(stderr.contains("not valid for crate"), "{stderr}");

    // Partial state: the dependency landed, the rest did not.
    assert_eq!(registry.published_versions("fault-leaf"), ["0.1.0"]);
    assert!(registry.published_versions("fault-mid").is_empty());
    assert!(registry.published_versions("fault-root").is_empty());

    // Reconciliation reads that state correctly.
    let index = SparseIndex::new(registry.index_url());
    let result = reconcile::reconcile(&workspace(&dir), &index, &planned()).unwrap();
    assert!(result.is_publishable(), "a partial publish is not a conflict");
    assert_eq!(result.to_publish, ["fault-mid", "fault-root"]);

    let leaf = result.outcomes.iter().find(|o| o.name == "fault-leaf").unwrap();
    assert_eq!(
        leaf.disposition,
        Disposition::Skip,
        "already-published crates are not republished"
    );

    // Fix the cause and resume against the *same* registry, so the partial
    // publication is still there. Only the remainder is attempted, which is
    // what makes the resume safe: republishing `fault-leaf` would fail outright,
    // and it is also what `fault-mid` needs in order to resolve at all.
    registry.set_faults(Faults::default());
    let (ok, stderr) = publish(&dir, &home, &registry.index_url(), &result.to_publish_refs());
    assert!(ok, "the resume must succeed:\n{stderr}");

    assert_eq!(registry.published_versions("fault-mid"), ["0.1.0"]);
    assert_eq!(registry.published_versions("fault-root"), ["0.1.0"]);
    assert_eq!(
        registry.published_versions("fault-leaf"),
        ["0.1.0"],
        "the crate published before the failure was not published twice"
    );

    // And a second reconciliation now reports the release complete.
    let result = reconcile::reconcile(&workspace(&dir), &index, &planned()).unwrap();
    assert!(result.is_complete(), "nothing remains; Cargo must not be invoked again");
}

/// Cargo waits for a published version to appear in the index before publishing
/// its dependents. Propagation delay must therefore be survivable rather than
/// fatal.
#[test]
fn delayed_index_visibility_is_survivable() {
    let dir = temp_dir("delayed");
    fixture(&dir);

    let registry = Registry::start(0, Faults::delayed_visibility(2), Arc::new(NoUpstream)).unwrap();
    let home = cargo_home(&dir, &registry.index_url());

    let (ok, stderr) = publish(&dir, &home, &registry.index_url(), &["fault-leaf", "fault-mid"]);
    assert!(ok, "cargo should poll until the version is visible:\n{stderr}");
    assert_eq!(registry.published_versions("fault-mid"), ["0.1.0"]);
}

/// A rejected upload must not leave a phantom index entry behind. If it did,
/// reconciliation would skip a crate that was never published.
#[test]
fn a_rejected_upload_leaves_no_trace() {
    let dir = temp_dir("no-trace");
    fixture(&dir);

    let registry =
        Registry::start(0, Faults::unauthorized(&["fault-leaf"]), Arc::new(NoUpstream)).unwrap();
    let home = cargo_home(&dir, &registry.index_url());

    let (ok, _) = publish(&dir, &home, &registry.index_url(), &["fault-leaf"]);
    assert!(!ok);
    assert!(
        registry.published_versions("fault-leaf").is_empty(),
        "a rejected upload must not be recorded"
    );

    let index = SparseIndex::new(registry.index_url());
    let result = reconcile::reconcile(&workspace(&dir), &index, &planned()).unwrap();
    assert_eq!(result.to_publish, ["fault-leaf", "fault-mid", "fault-root"]);
}

#[test]
fn an_expired_token_stops_the_stage_where_it_expired() {
    let dir = temp_dir("expired");
    fixture(&dir);

    // Credentials outlive exactly one upload.
    let registry = Registry::start(0, Faults::expiring_after(1), Arc::new(NoUpstream)).unwrap();
    let home = cargo_home(&dir, &registry.index_url());

    let (ok, stderr) =
        publish(&dir, &home, &registry.index_url(), &["fault-leaf", "fault-mid", "fault-root"]);
    assert!(!ok, "an expired token must fail the stage");
    assert!(stderr.contains("expired") || stderr.contains("401"), "{stderr}");
    assert_eq!(registry.published_versions("fault-leaf"), ["0.1.0"], "the first upload landed");
    assert!(registry.published_versions("fault-mid").is_empty());
}

#[test]
fn rate_limiting_and_server_errors_are_reported_not_swallowed() {
    for (label, faults) in
        [("rate-limit", Faults::rate_limited(50)), ("transient", Faults::transient(50))]
    {
        let dir = temp_dir(label);
        fixture(&dir);
        let registry = Registry::start(0, faults, Arc::new(NoUpstream)).unwrap();
        let home = cargo_home(&dir, &registry.index_url());

        let (ok, _) = publish(&dir, &home, &registry.index_url(), &["fault-leaf"]);
        assert!(!ok, "{label}: a persistently failing registry must fail the publish");
        assert!(
            registry.published_versions("fault-leaf").is_empty(),
            "{label}: nothing should be recorded"
        );
    }
}
