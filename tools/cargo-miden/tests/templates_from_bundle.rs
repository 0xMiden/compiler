//! `cargo miden new` with no `--template-path` must render the template bundle.
//!
//! Every other test in this crate passes `--template-path`, which short-circuits
//! resolution entirely. That is why the bundle shipped for a full release while
//! `cargo miden new` was still cloning two external repositories at hardcoded
//! tags, and nothing noticed: the templates were tested, the wiring was not.
//!
//! These tests exercise the default path — the one an actual user takes.

use std::{fs, path::Path};

use cargo_miden::{bundle, run};

use crate::utils::current_dir_lock;

fn scratch(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cargo-miden-bundle-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

/// Sets or clears process-wide environment variables, restoring every one of
/// them when dropped.
///
/// A `Drop` guard rather than a pair of statements, because restoring on the
/// *success* path only is worse than useless here. These variables are read by
/// every `cargo` and `git` subprocess the rest of this binary spawns, so a
/// panic that leaves `http_proxy` pointing at a dead port turns one failing
/// test into a cascade of unrelated network failures in the tests that follow
/// it — precisely when a legible failure matters most.
///
/// Previous values are saved and put back, rather than simply cleared: a
/// developer or runner behind a corporate proxy already has these set, and
/// deleting them would break everything downstream just as effectively.
///
/// Safety: these tests all hold the current-directory lock, so they do not race
/// each other. That is not a proof of soundness — `set_var` is unsafe because
/// of threads anywhere in the process, which a lock between tests cannot
/// constrain — but it is the same bargain the rest of this test binary makes.
struct EnvGuard(Vec<(String, Option<String>)>);

impl EnvGuard {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn set(mut self, name: &str, value: &str) -> Self {
        self.remember(name);
        unsafe { std::env::set_var(name, value) };
        self
    }

    fn unset(mut self, name: &str) -> Self {
        self.remember(name);
        unsafe { std::env::remove_var(name) };
        self
    }

    fn remember(&mut self, name: &str) {
        self.0.push((name.to_string(), std::env::var(name).ok()));
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // Reverse order, so a variable touched twice ends at its original value.
        for (name, previous) in self.0.drain(..).rev() {
            match previous {
                Some(value) => unsafe { std::env::set_var(&name, value) },
                None => unsafe { std::env::remove_var(&name) },
            }
        }
    }
}

/// `cargo miden new` injects a `--compiler-path` pointing at this checkout when
/// `TEST` is set, rewriting the generated manifest's SDK dependency into a path
/// dependency. Other tests in this binary set `TEST` and never clear it, so
/// whether these tests observe a version requirement or a path would otherwise
/// depend on execution order. They are about what a *user* gets.
fn without_test_env() -> EnvGuard {
    EnvGuard::new().unset("TEST")
}

/// The `miden` requirement the embedded bundle declares its templates carry.
///
/// This is the discriminator that makes these tests meaningful. Directory names
/// are the same in the external template repositories, so comparing structure
/// alone would pass even if `cargo miden new` went back to cloning them; the
/// requirement is pinned to the SDK this bundle was released against and
/// differs from whatever those repositories carry at their pinned tags.
fn bundle_sdk_requirement(bundle_root: &Path) -> String {
    let manifest =
        fs::read_to_string(bundle_root.join("bundle.toml")).expect("read the bundle manifest");
    for line in manifest.lines() {
        if let Some(value) = line
            .trim()
            .strip_prefix("sdk-requirement")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            return value.trim().trim_matches('"').to_string();
        }
    }
    panic!("the bundle declares no sdk-requirement");
}

/// The SDK version required by every `miden` dependency in a generated
/// project's manifests.
///
/// The *version* is extracted and compared for equality rather than searching
/// the line for a substring. `requirement_for` yields a bare `major.minor` for
/// a stable SDK, so a substring check would go vacuous the moment 0.14.0 ships:
/// `"0.14"` is contained in `"0.14.0-rc.1"`, and in `"1.0.14"`. This is the only
/// assertion that distinguishes the bundle from the external repositories the
/// resolver used to clone, so it has to be exact.
fn sdk_requirements(project: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![project.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                let text = fs::read_to_string(&path).unwrap_or_default();
                found.extend(text.lines().filter_map(quoted_sdk_version));
            }
        }
    }
    found
}

/// The quoted value of a `miden = ...` dependency line, in either the plain
/// (`miden = "0.14"`) or table (`miden = { version = "0.14" }`) form.
fn quoted_sdk_version(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("miden ") && !line.starts_with("miden=") {
        return None;
    }
    let (_, rest) = line.split_once('"')?;
    let (value, _) = rest.split_once('"')?;
    Some(value.to_string())
}

/// Directories directly inside a bundle template, which survive rendering
/// unchanged and so identify what the project was generated from.
fn directories(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .expect("read template directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The default scaffold comes from the bundle's `project/`, compared against the
/// embedded copy rather than a list written here, so the assertion cannot drift
/// from the templates it is checking.
#[test]
fn new_without_a_template_path_renders_the_bundle_project() {
    let dir = scratch("project");
    let extracted = dir.join("bundle");
    let root = bundle::extract(&extracted).expect("extract the embedded bundle");
    let expected = directories(&bundle::template_path(&root, None));
    assert!(
        !expected.is_empty(),
        "the bundle's project template has no directories to compare against"
    );

    let _guard = current_dir_lock();
    let _clean_env = without_test_env();
    let project = dir.join("scaffold");
    run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project.display().to_string(),
    ]
    .into_iter())
    .expect("`cargo miden new` with default template resolution");

    let generated = directories(&project);
    for name in &expected {
        assert!(
            generated.contains(name),
            "'{name}' is in the bundle's project template but not in the generated project \
             ({generated:?}); `cargo miden new` is not rendering the bundle"
        );
    }

    // Structure alone would also match the external template repositories this
    // used to clone, so pin the content too.
    let required = bundle_sdk_requirement(&root);
    let found = sdk_requirements(&project);
    assert!(!found.is_empty(), "the generated project depends on no SDK at all");
    for requirement in &found {
        assert_eq!(
            requirement, &required,
            "the generated project requires SDK {requirement:?}, but the bundle declares its \
             templates carry {required:?}; these templates did not come from the bundle"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

/// `--account` and friends come from `rust/<name>/template/`, which is a
/// different path through resolution than the default scaffold.
#[test]
fn new_with_a_named_template_renders_it_from_the_bundle() {
    let dir = scratch("account");
    let _guard = current_dir_lock();
    let _clean_env = without_test_env();
    let project = dir.join("acct");

    run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project.display().to_string(),
        "--account".to_string(),
    ]
    .into_iter())
    .expect("`cargo miden new --account` with default template resolution");

    assert!(project.join("Cargo.toml").is_file(), "no manifest was rendered");
    assert!(project.join("src").is_dir(), "the account template renders a src directory");

    // The rendered manifest must require the SDK the bundle was released
    // against; the templates carry that requirement and nothing else supplies it.
    let extracted = dir.join("bundle");
    let root = bundle::extract(&extracted).expect("extract the embedded bundle");
    let required = bundle_sdk_requirement(&root);

    let manifest = fs::read_to_string(project.join("Cargo.toml")).expect("read the manifest");
    assert!(
        manifest.contains(&required),
        "the generated manifest does not require the bundle's SDK (\"{required}\"), so it did not \
         come from the bundle:\n{manifest}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// The guard has to survive a panic, which is the entire reason it is a guard:
/// these variables are read by every `cargo` and `git` subprocess the rest of
/// this binary spawns, so leaking one turns a single failure into a cascade.
#[test]
fn the_environment_guard_restores_on_panic() {
    let _lock = current_dir_lock();

    // A pre-existing value must come back, not just be cleared.
    let sentinel = "cargo-miden-guard-probe";
    unsafe { std::env::set_var(sentinel, "original") };

    // Silence the deliberate panic, so a passing run does not print a backtrace
    // that reads like a real failure.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicked = std::panic::catch_unwind(|| {
        let _guard = EnvGuard::new().set(sentinel, "overridden").unset("TEST");
        assert_eq!(std::env::var(sentinel).as_deref(), Ok("overridden"));
        panic!("as a test would");
    });
    std::panic::set_hook(hook);
    assert!(panicked.is_err(), "the probe must actually panic");

    assert_eq!(
        std::env::var(sentinel).as_deref(),
        Ok("original"),
        "the guard must restore the previous value while unwinding"
    );
    unsafe { std::env::remove_var(sentinel) };
}

/// The embedded bundle is the floor: with no network, project creation still
/// works. Enforced by pointing the resolver at a dead proxy rather than by
/// trusting that CI happens to be offline.
#[test]
fn new_works_without_network_access() {
    let dir = scratch("offline");
    let _guard = current_dir_lock();
    let project = dir.join("offline-project");

    // Held across the call, so the proxy is restored even if `run` panics --
    // it can, via the `expect`s in extraction and rendering.
    let _no_network = EnvGuard::new()
        .set("https_proxy", "http://127.0.0.1:9")
        .set("http_proxy", "http://127.0.0.1:9");

    run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project.display().to_string(),
    ]
    .into_iter())
    .expect("`cargo miden new` must fall back to the embedded bundle when GitHub is absent");
    assert!(project.join("Cargo.toml").is_file());

    let _ = fs::remove_dir_all(&dir);
}
