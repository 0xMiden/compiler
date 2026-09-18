//! The `miden build` shape on a Rust program: the driver called as a library, from the project's
//! own directory, with no arguments and no wrapper around it.
//!
//! `midenup` aliases `miden build` to the `midenc` binary, so this is the whole command a user of
//! the SDK runs — and it is what failed for every Rust program while the entrypoint inference
//! depended on the manifest flavor: a `Cargo.toml` located by nothing but the working directory
//! had to be recognized as a Miden project root all the same.

use std::{env, ffi::OsString, fs};

use cargo_miden::run;
use midenc_driver::Midenc;

use crate::utils::{current_dir_lock, project_template_arg};

/// A scaffolded Rust program builds through the driver with the program name as its only argument.
///
/// The project is the one `cargo miden new` writes, so the manifest, the sources and the
/// dependencies are the ones a user gets; nothing but the working directory names it to the
/// compiler.
#[test]
fn a_rust_program_is_built_by_the_driver_from_its_own_directory_without_arguments() {
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
        "cargo_miden_miden_build_{}_{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()
    ));
    fs::create_dir_all(&scratch).unwrap();
    env::set_current_dir(&scratch).unwrap();

    let project_name = "miden-build-program";
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

    // The program name and nothing else: the working directory is what locates the project.
    let package = Midenc::exec(project.clone(), [OsString::from("midenc")], None)
        .unwrap_or_else(|err| panic!("midenc failed to build the Rust program: {err}"))
        .expect("a build that ran to completion writes a package");

    assert!(package.is_file(), "the compiled package must exist: {}", package.display());
    assert_eq!(
        package.extension().and_then(|extension| extension.to_str()),
        Some("masp"),
        "the build must produce a Miden package: {}",
        package.display()
    );
    assert!(
        package.starts_with(&project),
        "the package must land under the project's own target directory: {}",
        package.display()
    );

    fs::remove_dir_all(scratch).unwrap();
}
