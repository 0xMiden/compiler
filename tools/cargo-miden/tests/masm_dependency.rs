//! Coverage for a Miden Assembly target that is **not** the root of the build.
//!
//! Every other `.masm`-rooted project in this repository is the target of the build itself, so
//! nothing exercised a MASM target reached as a *dependency* of another project. That is the
//! shape that matters for the frontend-neutral pipeline: registering a `"masm"` frontend
//! displaces the assembler's own built-in [`MasmSourceProvider`] for every MASM target in every
//! dependency graph, not merely for MASM roots. A regression there would be invisible to a
//! suite made entirely of MASM roots and Rust roots with Rust dependencies.
//!
//! The root here is a Rust project, so the build also registers two frontends at once and has
//! to derive a per-target role across package boundaries: the Rust root is
//! `TargetRole::Root`, the MASM library is a `TargetRole::Dependency`.

use std::{
    env, fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cargo_miden::run;

use crate::utils::{current_dir_lock, project_template_arg};

/// The Miden Assembly library the root project depends on.
///
/// It declares a submodule so that the dependency's sources are a module *tree* rather than a
/// single file: walking the tree is part of what the displaced provider does, and a
/// single-module fixture would not notice its absence.
const DEPENDENCY_ROOT: &str = "\
pub mod support

pub proc entry() -> u32
    push.1
    exec.support::clean
end
";

/// The submodule [`DEPENDENCY_ROOT`] declares.
const DEPENDENCY_SUPPORT: &str = "\
pub proc clean
    push.1
    u32wrapping_add
end
";

/// Write the Miden Assembly dependency project into `dir`, named `name`.
fn write_masm_dependency(dir: &Path, name: &str) {
    fs::create_dir_all(dir.join("lib")).expect("create masm dependency source dir");
    fs::write(
        dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[lib]
path = "lib/mod.masm"

[dependencies]
"#
        ),
    )
    .expect("write masm dependency manifest");
    fs::write(dir.join("lib/mod.masm"), DEPENDENCY_ROOT).expect("write masm dependency root");
    fs::write(dir.join("lib/support.masm"), DEPENDENCY_SUPPORT)
        .expect("write masm dependency submodule");
}

/// Rewrite the Miden manifest of the scaffolded project in `project_dir` as a plain Rust
/// library target that depends on the Miden Assembly project `dependency` at `relative_path`.
///
/// The manifest is replaced rather than edited because the shape matters twice over:
///
/// * A **library** target, not the `[[bin]]` `cargo miden new --program` writes. The template's
///   `src/lib.rs` compiles to a Wasm library, and the assembler rejects a library root module
///   for an executable target.
/// * A **plain** library, not an account component. A component root can hold a WIT-less
///   dependency (the SDK macros skip it during WIT collection), but the plain shape keeps the
///   fixture free of SDK macros entirely, so the test exercises the compiler's MASM-dependency
///   pipeline and nothing else.
fn write_library_manifest(project_dir: &Path, name: &str, dependency: &str, relative_path: &str) {
    fs::write(
        project_dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[lib]
# A core Wasm module carries the Wasm frontend's synthetic wrapper component identity, which
# the assembler requires the declared namespace to match. See
# `tests/integration/src/end_to_end/support.rs`, which writes the same namespace for the same
# reason.
namespace = "root_ns:root@1.0.0"
path = "src/lib.rs"

[dependencies]
{dependency} = {{ path = "{relative_path}" }}
"#
        ),
    )
    .expect("write the root project's Miden manifest");
}

/// A Rust project with a Miden Assembly path dependency builds, and the dependency is assembled.
#[test]
fn build_rust_project_with_masm_path_dependency() {
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // signal integration tests to the cargo-miden code path
    unsafe {
        env::set_var("TEST", "1");
    }

    // The dependency package is materialized under the *root project's* target directory, which
    // `Makefile.toml` redirects workspace-wide. Unset it so this build uses its own.
    let restore_target_dir = env::var_os("CARGO_TARGET_DIR");
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let restore_dir = env::current_dir().unwrap();
    let root = env::temp_dir().join(format!(
        "cargo_miden_masm_dep_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    env::set_current_dir(&root).unwrap();

    let dependency_name = "masm-dep";
    write_masm_dependency(&root.join("masm_dep"), dependency_name);

    let project_name = "masm_dep_root";
    let output = run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project_name.to_string(),
        project_template_arg("--program"),
    ]
    .into_iter())
    .expect("cargo miden new failed")
    .expect("expected NewCommandOutput");
    let project_path = match output {
        // Relative to the directory the command ran in, which this test leaves before asserting.
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => root.join(project_path),
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };

    write_library_manifest(&project_path, project_name, dependency_name, "../masm_dep");

    env::set_current_dir(&project_path).unwrap();
    let build_started_at = SystemTime::now();
    let export_dir = crate::utils::exported_packages_dir(&project_path);
    let result = crate::utils::with_package_cache_env(&export_dir, || {
        run(["cargo", "miden", "build"].into_iter().map(|s| s.to_string()))
    });
    env::set_current_dir(&restore_dir).unwrap();

    match restore_target_dir {
        Some(val) => unsafe { env::set_var("CARGO_TARGET_DIR", val) },
        None => unsafe { env::remove_var("CARGO_TARGET_DIR") },
    }

    let artifacts = result
        .expect("cargo miden build with a masm path dependency failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    assert_eq!(artifacts.len(), 1, "expected a single root package artifact, got {artifacts:?}");

    // The root's own package building is not by itself evidence that the dependency was built
    // through a source provider: it would also hold if the dependency had been resolved from a
    // registry, or skipped. A materialized `.masp` for the dependency is produced only by
    // assembling it from its Miden Assembly sources and publishing it into the adopted
    // package-cache directory, which the compiler leaves in place.
    // `.masp` is `miden_mast_package::Package::EXTENSION`, spelled inline because cargo-miden
    // no longer links miden-mast-package.
    let dependency_package = export_dir.join(format!("{dependency_name}.masp"));
    assert!(
        dependency_package.exists(),
        "expected the masm dependency to be assembled and materialized at {}",
        dependency_package.display()
    );
    let modified = dependency_package.metadata().unwrap().modified().unwrap();
    let attribution_floor =
        build_started_at.checked_sub(Duration::from_secs(1)).unwrap_or(UNIX_EPOCH);
    assert!(
        modified >= attribution_floor,
        "expected this build to rewrite {}, but its modification time {modified:?} predates the \
         one-second-tolerant build attribution floor {attribution_floor:?}",
        dependency_package.display()
    );

    fs::remove_dir_all(&root).unwrap();
}
