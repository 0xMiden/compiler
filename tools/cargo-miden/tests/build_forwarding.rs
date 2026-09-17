//! `cargo miden build` is a thin wrapper: what it does with its arguments is hand them to
//! `midenc` unchanged.
//!
//! This pins a build driven from another directory by `--manifest-path`, through the library
//! and through the binary. The argument forwarding itself — the help a user gets and the tokens a
//! `--` delimiter carries through — is pinned by the lit tests in `tests/lit/cargo-miden`.

use std::{env, fs, process::Command};

use cargo_miden::run;

use crate::utils::{current_dir_lock, project_template_arg};

/// `--manifest-path` names the project to build, from a directory that holds no project at all.
///
/// The scratch directory the build runs from has no manifest of its own, so the only project
/// anything can find is the one the option names — which is what makes this the regression test
/// for a `--manifest-path` that was parsed and then dropped. Run through the wrapper binary, the
/// same build prints the `Compiled …` announcement exactly once: the driver prints it, and the
/// wrapper must not repeat it.
#[test]
fn a_manifest_path_builds_a_project_from_another_directory() {
    let _cwd = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // Signals the integration-test code path to cargo-miden, as every in-process build test does.
    unsafe {
        env::set_var("TEST", "1");
    }

    let scratch = env::temp_dir().join(format!(
        "cargo_miden_manifest_path_{}_{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()
    ));
    fs::create_dir_all(&scratch).unwrap();
    env::set_current_dir(&scratch).unwrap();

    let project_name = "manifest-path-build";
    let created = run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project_name.to_string(),
        project_template_arg("--program"),
    ]
    .into_iter())
    .expect("cargo miden new failed")
    .expect("expected NewCommandOutput");
    let project = scratch.join(created.unwrap_new_output());

    // Deliberately not entering the project: the working directory stays where nothing is
    // buildable.
    let manifest_path = project.join("Cargo.toml");
    let output =
        run(["cargo", "miden", "build", "--manifest-path", manifest_path.to_str().unwrap()]
            .into_iter()
            .map(str::to_string))
        .expect("cargo miden build --manifest-path failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();

    assert_eq!(output.len(), 1, "expected one compiled package, got {output:?}");
    assert!(output[0].is_file(), "the compiled package must exist: {:?}", output[0]);
    assert_eq!(
        output[0].extension().and_then(|extension| extension.to_str()),
        Some("masp"),
        "the build must produce a Miden package: {:?}",
        output[0]
    );

    // The same directory and the same flag, so the build is already cached and only the wrapper's
    // own output is under test.
    let wrapped = Command::new(env!("CARGO_BIN_EXE_cargo-miden"))
        .args(["miden", "build", "--manifest-path", manifest_path.to_str().unwrap()])
        .current_dir(&scratch)
        .output()
        .expect("cargo-miden should run");
    let stdout = String::from_utf8_lossy(&wrapped.stdout);
    assert!(
        wrapped.status.success(),
        "`cargo miden build --manifest-path` failed: {}\n{stdout}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    let announcements =
        stdout.lines().filter(|line| line.starts_with("Compiled ")).collect::<Vec<_>>();
    assert_eq!(
        announcements,
        vec![format!("Compiled {}", output[0].display())],
        "the finished build must be announced exactly once, naming the package it wrote"
    );

    fs::remove_dir_all(scratch).unwrap();
}
