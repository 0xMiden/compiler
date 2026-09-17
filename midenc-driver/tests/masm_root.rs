//! A Miden Assembly project as the root of a `midenc` build, located by nothing but the working
//! directory — the `miden build` shape, with no arguments at all.
//!
//! Every Miden Assembly project elsewhere in the workspace's tests is either handed to the
//! compiler by manifest path or reached as a dependency of a Rust root. This pins the default
//! locator: a directory holding a `miden-project.toml` and no `Cargo.toml` names its project by
//! the Miden manifest, the Miden Assembly frontend builds it, and a package comes out. The driver
//! is called as a library, which is what `midenc` and `cargo miden build` both do.

use std::{ffi::OsString, fs};

use midenc_driver::Midenc;

/// The library's root module, with a submodule so that the sources are a module tree.
const ROOT: &str = "\
pub mod support

pub proc entry() -> u32
    push.1
    exec.support::clean
end
";

/// The submodule [`ROOT`] declares.
const SUPPORT: &str = "\
pub proc clean
    push.1
    u32wrapping_add
end
";

/// The project's manifest: a library rooted at `lib/mod.masm`, with nothing to depend on.
const MANIFEST: &str = r#"[package]
name = "masm-root"
version = "0.1.0"

[lib]
path = "lib/mod.masm"

[dependencies]
"#;

#[test]
fn a_masm_project_is_built_from_its_own_directory_without_arguments() {
    let scratch = tempfile::TempDir::new().unwrap();
    let project = scratch.path().join("masm-root");
    fs::create_dir_all(project.join("lib")).unwrap();
    fs::write(project.join("miden-project.toml"), MANIFEST).unwrap();
    fs::write(project.join("lib/mod.masm"), ROOT).unwrap();
    fs::write(project.join("lib/support.masm"), SUPPORT).unwrap();
    assert!(
        !project.join("Cargo.toml").exists(),
        "the project must be MASM-only, so that only the Miden manifest can name it"
    );

    // The program name and nothing else: the working directory is what locates the project.
    let package = Midenc::exec(project.clone(), [OsString::from("midenc")], None)
        .unwrap_or_else(|err| panic!("midenc failed to build the MASM project: {err}"))
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
}
