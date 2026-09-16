//! `cargo miden build` is a thin wrapper: what it does with its arguments is hand them to
//! `midenc` unchanged.
//!
//! These tests pin that end to end — the help a user gets, the tokens a `--` delimiter carries
//! through, and a build driven from another directory by `--manifest-path`.

use std::{env, fs, process::Command};

use cargo_miden::run;

use crate::utils::{current_dir_lock, project_template_arg};

/// `cargo miden build` forwards its arguments to `midenc`, help included.
///
/// The wrapper parses nothing of its own, so the help a user gets is the compiler's — which is
/// the only place the forwarded options are documented. Two options no wrapper stub ever had
/// stand in for "this is midenc's help": if they are there, the arguments reached the driver.
#[test]
fn build_help_is_the_compilers_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-miden"))
        .args(["miden", "build", "--help"])
        .output()
        .expect("cargo-miden should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`cargo miden build --help` failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    for option in ["--manifest-path", "--working-dir"] {
        assert!(
            stdout.contains(option),
            "expected midenc's help, which documents `{option}`, but got:\n{stdout}"
        );
    }
}

/// A leading `--` reaches `midenc` instead of being eaten by the wrapper's own parse.
///
/// `--` is how a user names an input file that starts with a hyphen, so what follows it must be
/// taken as an input and not as an option: `build -- --help` has to fail on the input `--help`,
/// not print the compiler's help. Clap strips that delimiter, so the wrapper forwards the raw
/// tokens instead of clap's parse of them.
#[test]
fn a_leading_double_dash_is_forwarded_rather_than_consumed() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-miden"))
        .args(["miden", "build", "--", "--help"])
        .output()
        .expect("cargo-miden should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "`cargo miden build -- --help` must not succeed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("invalid input file"),
        "`--help` after `--` must be rejected as an input file, but got:\n{stderr}"
    );
}

/// `--manifest-path` names the project to build, from a directory that holds no project at all.
///
/// The scratch directory the build runs from has no manifest of its own, so the only project
/// anything can find is the one the option names — which is what makes this the regression test
/// for a `--manifest-path` that was parsed and then dropped.
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

    fs::remove_dir_all(scratch).unwrap();
}
