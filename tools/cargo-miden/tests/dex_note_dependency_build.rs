//! Integration test for a note that is built as a dependency of another project.
//!
//! A note carries its author codec only when it is the root target of the build. A note that
//! another project pulls in as a source dependency gets the note storage schema, but no codec
//! section: the codec crate's inputs are outside the build provenance, so a stored dependency
//! copy could keep a stale codec, and every dependency assembly would otherwise run a nested
//! cargo build.

use std::{env, fs, path::Path};

use cargo_miden::run;
use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::{
    package_note_codec_section_id, package_note_storage_schema_section_id,
};
use midenc_integration_test_support::{
    example_build_lock, wasm_target_is_installed, workspace_root,
};

use crate::utils::{RestoreEnvironment, current_dir_lock, with_package_cache_env};

#[test]
fn dex_note_built_as_a_dependency_has_the_schema_but_no_codec() {
    if !wasm_target_is_installed() {
        eprintln!("skipping DEX note dependency build test: wasm32-wasip2 is not installed");
        return;
    }
    // The command reads the process working directory, so serialize cwd changes.
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    // Clear the outer override. The scratch project then uses its own target layout.
    let _restore_environment = RestoreEnvironment::new(["CARGO_TARGET_DIR"]);
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let workspace = workspace_root();
    let project_dir = tempfile::tempdir().expect("failed to create the dependent project dir");
    let dependent = project_dir.path().join("counter-contract");
    scaffold_dependent_project(&workspace, &dependent);

    // A caller-provided package cache is adopted and left in place, so the dependency packages
    // this build publishes can be read after the compiler exits.
    let cache_dir = tempfile::tempdir().expect("failed to create the package cache dir");
    let export_dir = cache_dir.path().to_path_buf();

    env::set_current_dir(&dependent).unwrap();
    let result = {
        let _build_lock = example_build_lock(&workspace);
        with_package_cache_env(&export_dir, || {
            run(["cargo", "miden", "build", "--release"].into_iter().map(str::to_owned))
        })
    };

    let output = result
        .expect("cargo miden build for the dex-note dependent failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    assert_eq!(output.len(), 1, "expected one dependent package artifact, got {output:?}");
    assert!(output[0].exists(), "the dependent package was not written to {:?}", output[0]);

    let dependency_package = export_dir.join("dex-note.masp");
    assert!(
        dependency_package.exists(),
        "expected the dex-note dependency package at {}; the cache holds {:?}",
        dependency_package.display(),
        exported_file_names(&export_dir)
    );
    let package = Package::deserialize_from_file(&dependency_package)
        .expect("failed to read the dex-note dependency package");

    let schema_id = package_note_storage_schema_section_id();
    assert!(
        package.sections.iter().any(|section| section.id == schema_id),
        "the dex-note dependency package has no note storage schema section"
    );
    let codec_id = package_note_codec_section_id();
    assert!(
        !package.sections.iter().any(|section| section.id == codec_id),
        "the dex-note dependency package must carry no note codec section"
    );
}

/// Copies the counter-contract example to `destination` and makes it depend on `dex-note`.
///
/// The copy is built outside the compiler workspace, so every relative path in its manifests is
/// rewritten to the workspace copy it names.
fn scaffold_dependent_project(workspace: &Path, destination: &Path) {
    let source = workspace.join("examples/counter-contract");
    copy_project(&source, destination);

    for manifest in ["Cargo.toml", "miden-project.toml"] {
        let path = destination.join(manifest);
        let rewritten = absolutize_manifest_paths(&fs::read_to_string(&path).unwrap(), &source);
        fs::write(&path, rewritten).unwrap();
    }

    let manifest = destination.join("miden-project.toml");
    let contents = fs::read_to_string(&manifest).unwrap();
    let dex_note = workspace.join("examples/dex-note");
    let with_dependency = contents.replace(
        "[dependencies]\n",
        &format!("[dependencies]\ndex-note = {{ path = \"{}\" }}\n", dex_note.display()),
    );
    assert_ne!(with_dependency, contents, "the example manifest has no `[dependencies]` table");
    fs::write(&manifest, with_dependency).unwrap();
}

/// Copies a project directory, skipping build outputs.
fn copy_project(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_project(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Rewrites every relative `path = "..."` value to the location it names under `manifest_dir`.
fn absolutize_manifest_paths(manifest: &str, manifest_dir: &Path) -> String {
    const KEY: &str = "path = \"";

    let mut out = String::with_capacity(manifest.len());
    let mut rest = manifest;
    while let Some(start) = rest.find(KEY) {
        let (head, tail) = rest.split_at(start + KEY.len());
        out.push_str(head);
        let end = tail.find('"').expect("unterminated path value in the manifest");
        let (value, tail) = tail.split_at(end);
        let path = Path::new(value);
        if path.is_absolute() {
            out.push_str(value);
        } else {
            let resolved = manifest_dir.join(path);
            // `src/lib.rs` and the like must stay relative to the copy, not point back to it.
            match resolved.canonicalize() {
                Ok(resolved) if !resolved.starts_with(manifest_dir) => {
                    out.push_str(&resolved.display().to_string())
                }
                _ => out.push_str(value),
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The file names the build left in the package cache, for assertion messages.
fn exported_file_names(export_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(export_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .collect()
}
