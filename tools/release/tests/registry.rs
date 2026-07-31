//! Protocol tests for the rehearsal registry.
//!
//! These drive a real `cargo publish` against the registry over HTTP, which is
//! what makes them worth their runtime: they exercise Cargo's actual upload
//! format, its index-confirmation polling, and its resolution of interdependent
//! unpublished crates. None of that is reproducible with a mock.
//!
//! No network access is required: the fixture workspace has no third-party
//! dependencies, so the upstream proxy is never consulted.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use midenc_release::registry::{Faults, NoUpstream, Registry};

/// Two interdependent crates at versions that exist on no registry. This is the
/// shape every real release has, and the shape that fails if packages are
/// published in the wrong order.
fn fixture_workspace(dir: &Path) {
    fs::create_dir_all(dir.join("leaf/src")).unwrap();
    fs::create_dir_all(dir.join("root/src")).unwrap();

    fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"leaf\", \"root\"]\n",
    )
    .unwrap();

    for (name, deps) in [
        ("leaf", String::new()),
        (
            "root",
            "[dependencies]\nrehearsal-leaf = { version = \"0.1.0\", path = \"../leaf\" }\n"
                .to_string(),
        ),
    ] {
        fs::write(
            dir.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname = \"rehearsal-{name}\"\nversion = \"0.1.0\"\nedition = \
                 \"2021\"\nlicense = \"MIT\"\ndescription = \"rehearsal fixture\"\n\n{deps}"
            ),
        )
        .unwrap();
        fs::write(dir.join(name).join("src/lib.rs"), "").unwrap();
    }
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").as_deref().unwrap_or("cargo"))
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cargo_publishes_interdependent_crates_and_the_index_serves_them_back() {
    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let workspace = temp_dir("publish");
    fixture_workspace(&workspace);

    // Source replacement redirects dependency *resolution*; `--index` redirects
    // the *upload target*. Both are required: without replacement Cargo resolves
    // the unpublished sibling against crates.io and fails, and without `--index`
    // it uploads to the real crates.io.
    let cargo_home = workspace.join(".cargo-home");
    fs::create_dir_all(&cargo_home).unwrap();
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"rehearsal\"\n\n[source.rehearsal]\nregistry = \
             \"{}\"\n",
            registry.index_url()
        ),
    )
    .unwrap();

    let output = cargo()
        .current_dir(&workspace)
        .env("CARGO_HOME", &cargo_home)
        .args(["publish", "--no-verify", "--allow-dirty"])
        .args(["--index", &registry.index_url()])
        .args(["--token", "rehearsal-token"])
        // Topological order: `leaf` must be published before `root`, which
        // depends on it. Cargo's own ordering is not reliable at scale, so the
        // release tool always supplies this explicitly.
        .args(["-p", "rehearsal-leaf", "-p", "rehearsal-root"])
        .output()
        .expect("cargo publish runs");

    assert!(
        output.status.success(),
        "cargo publish failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(registry.published_versions("rehearsal-leaf"), ["0.1.0"]);
    assert_eq!(registry.published_versions("rehearsal-root"), ["0.1.0"]);
    assert_eq!(registry.stats().published.load(std::sync::atomic::Ordering::Relaxed), 2);

    // The uploaded archive is a real `.crate`: gzip magic, non-trivial size.
    let archive = registry.archive("rehearsal-root", "0.1.0").expect("archive stored");
    assert_eq!(&archive[..2], &[0x1f, 0x8b], "uploaded archive is gzip");
    assert!(archive.len() > 100, "uploaded archive looks truncated");
}

#[test]
fn uploaded_bytes_match_locally_packaged_bytes() {
    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let workspace = temp_dir("parity");
    fixture_workspace(&workspace);

    let cargo_home = workspace.join(".cargo-home");
    fs::create_dir_all(&cargo_home).unwrap();
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"rehearsal\"\n\n[source.rehearsal]\nregistry = \
             \"{}\"\n",
            registry.index_url()
        ),
    )
    .unwrap();

    // Package in production form first: no source replacement, no flags.
    let packaged = cargo()
        .current_dir(&workspace)
        .args(["package", "--no-verify", "--allow-dirty", "-p", "rehearsal-leaf"])
        .output()
        .expect("cargo package runs");
    assert!(
        packaged.status.success(),
        "cargo package failed:\n{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    let reference = fs::read(workspace.join("target/package/rehearsal-leaf-0.1.0.crate")).unwrap();

    let published = cargo()
        .current_dir(&workspace)
        .env("CARGO_HOME", &cargo_home)
        .args(["publish", "--no-verify", "--allow-dirty"])
        .args(["--index", &registry.index_url()])
        .args(["--token", "rehearsal-token"])
        .args(["-p", "rehearsal-leaf"])
        .output()
        .expect("cargo publish runs");
    assert!(
        published.status.success(),
        "cargo publish failed:\n{}",
        String::from_utf8_lossy(&published.stderr)
    );

    let uploaded = registry.archive("rehearsal-leaf", "0.1.0").expect("archive stored");
    assert_eq!(
        uploaded, reference,
        "rehearsal upload differs from production-form packaging; the rehearsal would be testing \
         a variant of the release rather than the release"
    );
}

#[test]
fn config_json_points_clients_back_at_this_registry() {
    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let url = registry.index_url();
    let host = url.trim_start_matches("sparse+http://").trim_end_matches('/');

    let body = http_get(host, "/config.json");
    assert!(body.contains(host), "config.json should advertise {host}: {body}");
    assert!(body.contains("\"dl\""), "config.json needs a download endpoint: {body}");
}

#[test]
fn unknown_crates_are_not_found_when_there_is_no_upstream() {
    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let url = registry.index_url();
    let host = url.trim_start_matches("sparse+http://").trim_end_matches('/');

    let response = http_get_status(host, "/se/rd/serde");
    assert_eq!(response, 404);
}

fn http_get(host: &str, path: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

fn http_get_status(host: &str, path: &str) -> u16 {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

/// The property that makes a resume safe: after a partial publish, reconciling
/// against live registry state yields exactly the crates that remain, in
/// dependency order. This is the same code path a first attempt takes.
#[test]
fn reconciliation_after_a_partial_publish_yields_only_what_remains() {
    use midenc_release::{
        reconcile::{self, Disposition, Planned},
        registry::client::SparseIndex,
        workspace::{EdgeKind, Package, Workspace},
    };

    let registry = Registry::start(0, Faults::default(), Arc::new(NoUpstream)).unwrap();
    let workspace_dir = temp_dir("reconcile");
    fixture_workspace(&workspace_dir);

    let cargo_home = workspace_dir.join(".cargo-home");
    fs::create_dir_all(&cargo_home).unwrap();
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"rehearsal\"\n\n[source.rehearsal]\nregistry = \
             \"{}\"\n",
            registry.index_url()
        ),
    )
    .unwrap();

    // Simulate an attempt that published `leaf` and then died before `root`.
    let published = cargo()
        .current_dir(&workspace_dir)
        .env("CARGO_HOME", &cargo_home)
        .args(["publish", "--no-verify", "--allow-dirty"])
        .args(["--index", &registry.index_url()])
        .args(["--token", "rehearsal-token"])
        .args(["-p", "rehearsal-leaf"])
        .output()
        .expect("cargo publish runs");
    assert!(
        published.status.success(),
        "cargo publish failed:\n{}",
        String::from_utf8_lossy(&published.stderr)
    );

    let ws = Workspace {
        root: workspace_dir.clone(),
        packages: [("rehearsal-leaf", vec![]), ("rehearsal-root", vec!["rehearsal-leaf"])]
            .into_iter()
            .map(|(name, deps)| {
                (
                    name.to_string(),
                    Package {
                        version: "0.1.0".into(),
                        local_deps: deps
                            .into_iter()
                            .map(|d: &str| (d.to_string(), EdgeKind::Required))
                            .collect(),
                        publishable: true,
                    },
                )
            })
            .collect(),
    };

    let planned: Vec<Planned> = ["rehearsal-leaf", "rehearsal-root"]
        .iter()
        .map(|name| Planned {
            name: name.to_string(),
            version: "0.1.0".into(),
            expected_cksum: None,
        })
        .collect();

    let index = SparseIndex::new(registry.index_url());
    let result = reconcile::reconcile(&ws, &index, &planned).unwrap();

    assert!(result.is_publishable(), "no conflicts expected");
    assert!(!result.is_complete(), "root has not been published yet");
    assert_eq!(result.to_publish, ["rehearsal-root"]);

    let leaf = result.outcomes.iter().find(|o| o.name == "rehearsal-leaf").unwrap();
    assert_eq!(leaf.disposition, Disposition::Skip, "already-published crates are skipped");
}
