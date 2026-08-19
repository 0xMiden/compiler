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

use std::{env, fs, path::Path, time::SystemTime};

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

/// Write the same Miden Assembly project into `dir`, with its sources at the project root.
///
/// The `[lib] path` names `mod.masm` directly in the project root. The complete-input contract
/// watches the whole local project tree, so this layout also proves a sibling module is covered
/// without predicting its individual path.
fn write_masm_dependency_at_root(dir: &Path, name: &str) {
    fs::create_dir_all(dir).expect("create masm dependency dir");
    fs::write(
        dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[lib]
path = "mod.masm"

[dependencies]
"#
        ),
    )
    .expect("write masm dependency manifest");
    fs::write(dir.join("mod.masm"), DEPENDENCY_ROOT).expect("write masm dependency root");
    fs::write(dir.join("support.masm"), DEPENDENCY_SUPPORT)
        .expect("write masm dependency submodule");
}

/// Write a MASM project whose declared library target lives outside its manifest directory.
fn write_masm_dependency_with_external_sources(project_dir: &Path, source_dir: &Path, name: &str) {
    fs::create_dir_all(project_dir).expect("create external-source project dir");
    fs::create_dir_all(source_dir).expect("create external source dir");
    let relative_source = Path::new("..").join(source_dir.file_name().unwrap()).join("mod.masm");
    fs::write(
        project_dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[lib]
path = "{}"

[dependencies]
"#,
            relative_source.display()
        ),
    )
    .expect("write external-source dependency manifest");
    fs::write(source_dir.join("mod.masm"), DEPENDENCY_ROOT)
        .expect("write external dependency root");
    fs::write(source_dir.join("support.masm"), DEPENDENCY_SUPPORT)
        .expect("write external dependency submodule");
}

/// Write a MASM project whose declared root is a file symlink into an external module tree.
#[cfg(unix)]
fn write_masm_dependency_with_symlinked_sources(project_dir: &Path, source_dir: &Path, name: &str) {
    use std::os::unix::fs::symlink;

    fs::create_dir_all(project_dir).expect("create symlink-source project dir");
    fs::create_dir_all(source_dir).expect("create symlink source dir");
    fs::write(
        project_dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[lib]
path = "mod.masm"

[dependencies]
"#,
        ),
    )
    .expect("write symlink-source dependency manifest");
    fs::write(source_dir.join("mod.masm"), DEPENDENCY_ROOT).expect("write symlink dependency root");
    fs::write(source_dir.join("support.masm"), DEPENDENCY_SUPPORT)
        .expect("write symlink dependency submodule");
    symlink(source_dir.join("mod.masm"), project_dir.join("mod.masm"))
        .expect("symlink dependency root into its project");
}

/// Rewrite the Miden manifest of the scaffolded project in `project_dir` as a plain Rust
/// library target that depends on the given Miden Assembly projects (`name`, `relative_path`
/// pairs).
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
fn write_library_manifest(project_dir: &Path, name: &str, dependencies: &[(&str, &str)]) {
    let dependency_lines = dependencies
        .iter()
        .map(|(dependency, relative_path)| {
            format!("{dependency} = {{ path = \"{relative_path}\" }}\n")
        })
        .collect::<String>();
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
{dependency_lines}"#
        ),
    )
    .expect("write the root project's Miden manifest");
}

/// A Rust project with a Miden Assembly path dependency builds, and the dependency is assembled.
#[test]
fn build_rust_project_with_masm_path_dependency() {
    struct RestoreCargoMiden(Option<std::ffi::OsString>);
    impl Drop for RestoreCargoMiden {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => unsafe { env::set_var("CARGO_MIDEN", value) },
                None => unsafe { env::remove_var("CARGO_MIDEN") },
            }
        }
    }

    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // signal integration tests to the cargo-miden code path
    unsafe {
        env::set_var("TEST", "1");
    }
    // The test invokes cargo-miden in-process, so provide the collector an explicit stable
    // launcher identity rather than making this otherwise-complete MASM graph opaque because a
    // future `cargo miden` plugin could shadow the current executable in PATH.
    let _restore_cargo_miden = RestoreCargoMiden(env::var_os("CARGO_MIDEN"));
    unsafe {
        env::set_var("CARGO_MIDEN", env::current_exe().unwrap());
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
    // A second dependency with its sources directly in the project root.
    let flat_dependency_name = "masm-dep-flat";
    write_masm_dependency_at_root(&root.join("masm_dep_flat"), flat_dependency_name);
    // And a third whose target escapes its manifest directory, so the invalidation record must
    // include the resolved source tree rather than assuming every target is contained.
    let external_dependency_name = "masm-dep-external";
    write_masm_dependency_with_external_sources(
        &root.join("masm_dep_external"),
        &root.join("shared_masm"),
        external_dependency_name,
    );
    #[cfg(unix)]
    let symlink_dependency_name = "masm-dep-symlink";
    #[cfg(unix)]
    write_masm_dependency_with_symlinked_sources(
        &root.join("masm_dep_symlink"),
        &root.join("shared_symlink_masm"),
        symlink_dependency_name,
    );

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

    let mut dependencies = vec![
        (dependency_name, "../masm_dep"),
        (flat_dependency_name, "../masm_dep_flat"),
        (external_dependency_name, "../masm_dep_external"),
    ];
    #[cfg(unix)]
    dependencies.push((symlink_dependency_name, "../masm_dep_symlink"));
    write_library_manifest(&project_path, project_name, &dependencies);

    let mut workspace_members =
        vec!["masm_dep", "masm_dep_flat", "masm_dep_external", "masm_dep_root"];
    #[cfg(unix)]
    workspace_members.push("masm_dep_symlink");
    let members = workspace_members
        .iter()
        .map(|member| format!("\"{member}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(root.join("miden-project.toml"), format!("[workspace]\nmembers = [{members}]\n"))
        .expect("write the Miden workspace manifest");

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
    crate::utils::assert_written_by_this_build(&dependency_package, build_started_at);
    let flat_dependency_package = export_dir.join(format!("{flat_dependency_name}.masp"));
    assert!(
        flat_dependency_package.exists(),
        "expected the root-level masm dependency to be assembled and materialized at {}",
        flat_dependency_package.display()
    );
    let external_dependency_package = export_dir.join(format!("{external_dependency_name}.masp"));
    assert!(
        external_dependency_package.exists(),
        "expected the external-source masm dependency to be assembled and materialized at {}",
        external_dependency_package.display()
    );
    #[cfg(unix)]
    assert!(
        export_dir.join(format!("{symlink_dependency_name}.masp")).exists(),
        "expected the symlink-source MASM dependency to be assembled and materialized"
    );

    // Local MASM project trees are complete, selectively watchable inputs. Recording each
    // project root recursively observes edits, removals, new sibling modules, and a newly created
    // WIT directory without reconstructing the assembler's individual file reads.
    let inputs_file = export_dir.join("miden-deps").join("build-inputs");
    let inputs = fs::read_to_string(&inputs_file).unwrap_or_else(|err| {
        panic!(
            "expected the compiler-recorded build inputs at {}: {err}",
            inputs_file.display()
        )
    });
    let tree_has = |suffix: &str| {
        inputs.lines().any(|line| {
            line.strip_prefix("tree\t")
                .is_some_and(|path| Path::new(path).ends_with(suffix))
        })
    };
    assert!(
        tree_has("masm_dep"),
        "expected the subdirectory dependency's project tree in:\n{inputs}"
    );
    assert!(
        tree_has("masm_dep_flat"),
        "expected the root-level dependency's project tree in:\n{inputs}"
    );
    assert!(
        tree_has("shared_masm"),
        "expected the dependency's external source tree in:\n{inputs}"
    );
    #[cfg(unix)]
    assert!(
        tree_has("shared_symlink_masm"),
        "expected the dependency's canonical symlink source tree in:\n{inputs}"
    );
    assert!(
        !inputs.lines().any(|line| line.starts_with("opaque\t")),
        "local MASM dependencies must remain selectively invalidated:\n{inputs}"
    );

    fs::remove_dir_all(&root).unwrap();
}
