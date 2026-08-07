//! Tests for the `cargo miden package-cache` build-script query.

use std::{env, fs, path::Path, process::Command};

/// Writes a minimal Miden project with the given `[dependencies]` tail.
fn write_project(dir: &Path, name: &str, dependencies: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("miden-project.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n\n[lib]\npath = \
             \"src/lib.rs\"\n{dependencies}"
        ),
    )
    .unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"),
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), "").unwrap();
}

#[test]
fn package_cache_command_prints_cache_dir_and_build_script_inputs() {
    let scratch =
        env::temp_dir().join(format!("cargo_miden_package_cache_cmd_{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let root = scratch.join("root");
    let dependency = scratch.join("dependency");
    write_project(
        &root,
        "root",
        "\n[dependencies]\nregistry-dep = \"*\"\ndependency = { path = \"../dependency\" }\n",
    );
    write_project(&dependency, "dependency", "");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-miden"))
        .args(["miden", "package-cache", "--release"])
        .current_dir(&root)
        .output()
        .expect("failed to run cargo-miden");
    assert!(
        output.status.success(),
        "package-cache failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    let cache_dir = stdout
        .lines()
        .find_map(|line| line.strip_prefix("cache-dir="))
        .expect("the output must name the cache directory");
    let cache_dir = Path::new(cache_dir);
    let canonical_root = root.canonicalize().unwrap();
    assert!(
        cache_dir.starts_with(canonical_root.join("target").join("miden").join("packages")),
        "the cache must live in the owned project layout, got '{}'",
        cache_dir.display()
    );
    let fingerprint = cache_dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(fingerprint.len(), 16, "the cache directory must be a fingerprint");
    assert!(
        fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "the fingerprint must be lowercase hexadecimal, got '{fingerprint}'"
    );

    let source_dependencies = stdout
        .lines()
        .find_map(|line| line.strip_prefix("source-dependencies="))
        .expect("the output must report the source-dependency count");
    assert_eq!(source_dependencies, "1", "only the source-project dependency counts");

    let watches: Vec<&Path> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("watch="))
        .map(Path::new)
        .collect();
    let watched = |suffix: &str| watches.iter().any(|path| path.ends_with(suffix));
    assert!(watched("root/miden-project.toml"), "watch list: {watches:?}");
    assert!(watched("dependency/miden-project.toml"), "watch list: {watches:?}");
    assert!(watched("dependency/src"), "watch list: {watches:?}");
    assert!(!watched("root/src"), "root sources must not be watched: {watches:?}");
    assert!(
        watches.iter().any(|path| path.ends_with("cargo-miden")),
        "the tool binary itself must be watched: {watches:?}"
    );

    let _ = fs::remove_dir_all(&scratch);
}
