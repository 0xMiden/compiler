//! Generates the Rust bindings of a Miden package the project depends on.
//!
//! Behind the `bindgen` feature: the generator and the package reader it needs are most of the
//! compiler's dependency graph, and a project that only stages its package cache or compiles its
//! own stubs must not pay for them.

use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

use miden_mast_package::Package;
use miden_sdk_bindgen::{External, Options};
use midenc_frontend_wasm_metadata::package_cache::{
    DEPENDENCY_MANIFEST_DIR, DEPENDENCY_MAP_SCHEMA, PACKAGE_CACHE_ENV, dependency_map_file_name,
    package_file_name,
};
use midenc_package_interface::PackageInterface;

use crate::stubs;

/// The Rust bindings of one Miden package, for [`generate_bindings`].
#[derive(Clone, Copy, Debug)]
pub struct Bindings<'a> {
    /// The package's name: its key in the project's `miden-project.toml` `[dependencies]`, which
    /// is also the name its compiled package is published under in the package cache.
    pub package: &'a str,
    /// The MASM module path the bindings are generated from, e.g. `::masm_dep`. Every export of
    /// the package must lie under it, and it is stripped from their paths: the module that
    /// includes the bindings stands for it. Empty means the package's root module: the deepest
    /// module every export and every declared module of the package lies under.
    pub root: &'a str,
    /// The absolute Rust path of the items the generated code names (`Felt`, `Word`,
    /// `ElementPtr`, `WordAligned`, `FeltConstant`, `WordConstant`): `::miden::support` in a
    /// project that depends on `miden`.
    pub support: &'a str,
    /// The packages whose types the bindings refer to instead of declaring copies, each as
    /// `(package, root, rust path)`: the package's name and root as for [`Bindings::package`] and
    /// [`Bindings::root`], and the Rust path its own bindings are mounted at, e.g. `crate::other`.
    pub with: &'a [(&'a str, &'a str, &'a str)],
}

/// Generates the Rust bindings of a Miden package this project depends on, and links their stubs.
///
/// The package is read from the Miden package cache: the `MIDENC_PACKAGE_CACHE` a midenc-driven
/// build sets, else the generation [`prepare_package_cache`](crate::prepare_package_cache) staged
/// earlier in this build script. It is found the way the SDK macros find a dependency: through
/// the dependency artifact map the compiler recorded for this project, else as `<package>.masp`
/// in the cache, spelled with `-` or `_`.
///
/// The bindings are written to `<OUT_DIR>/<package>.rs`, for the crate to mount with `include!`.
/// The stubs are written to `<OUT_DIR>/<package>_stubs.rs` and compiled into an archive that every
/// dependent links ([`stubs::compile_stub_archive`]). Every export that produced no code is
/// reported as a `cargo:warning`.
///
/// The generated procedures exist only when compiling for the Miden VM (`target_family = "wasm"`
/// and `cfg(miden)`), so this also declares `cfg(miden)` and sets it when
/// `MIDENC_TARGET_IS_MIDEN_VM` is set, as the SDK's own crates do. The crate root must enable
/// `#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]`, and the module that
/// includes the bindings must allow `non_camel_case_types`, `non_snake_case`,
/// `clippy::too_many_arguments` and `missing_docs`:
///
/// ```ignore
/// // build.rs
/// use miden_sdk_build_script_support::{Bindings, generate_bindings, prepare_package_cache};
///
/// fn main() {
///     prepare_package_cache();
///     generate_bindings(&Bindings {
///         package: "masm-dep",
///         root: "",
///         support: "::miden::support",
///         with: &[],
///     });
/// }
///
/// // src/lib.rs
/// mod masm_dep {
///     // Generated names follow MASM's spelling, and generated items carry no prose docs.
///     #![allow(non_camel_case_types, non_snake_case, clippy::too_many_arguments, missing_docs)]
///     include!(concat!(env!("OUT_DIR"), "/masm-dep.rs"));
/// }
/// ```
///
/// # Panics
///
/// If no package cache is configured, a package is not in it or cannot be read, or the bindings
/// cannot be generated. This runs in a build script, where a panic fails the build with its
/// message.
pub fn generate_bindings(bindings: &Bindings<'_>) {
    println!("cargo:rerun-if-env-changed=MIDENC_TARGET_IS_MIDEN_VM");
    println!("cargo:rustc-check-cfg=cfg(miden)");
    if env::var_os("MIDENC_TARGET_IS_MIDEN_VM").is_some() {
        println!("cargo:rustc-cfg=miden");
    }
    println!("cargo:rerun-if-env-changed={PACKAGE_CACHE_ENV}");

    let cache = crate::configured_package_cache()
        .or_else(crate::selected_package_cache)
        .unwrap_or_else(|| {
            panic!(
                "no Miden package cache to generate the bindings of `{}` from: call \
                 `prepare_package_cache` first, or build with `cargo miden`",
                bindings.package
            )
        });
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let consumer = consumer_name(&manifest_dir);
    let read = |package: &str| {
        let path = locate_package(&cache, &consumer, package).unwrap_or_else(|err| panic!("{err}"));
        println!("cargo:rerun-if-changed={}", path.display());
        read_interface(&path).unwrap_or_else(|err| panic!("{err}"))
    };

    let interface = read(bindings.package);
    let mut externals = Vec::with_capacity(bindings.with.len());
    let mut with = BTreeMap::new();
    for &(package, root, rust_path) in bindings.with {
        let external = read(package);
        let root = root_or_package_root(root, &external);
        with.insert(
            AsRef::<str>::as_ref(&external.name).to_string(),
            External {
                root,
                rust_path: rust_path.to_string(),
            },
        );
        externals.push(external);
    }
    let options = Options {
        root: root_or_package_root(bindings.root, &interface),
        support: bindings.support.to_string(),
        with,
    };
    let externals: Vec<&PackageInterface> = externals.iter().collect();
    let generated =
        miden_sdk_bindgen::generate(&interface, &externals, &options).unwrap_or_else(|err| {
            panic!("failed to generate the bindings of package `{}`: {err}", bindings.package)
        });
    for skipped in &generated.skipped {
        println!(
            "cargo:warning=package `{}` {}: `{}` has no binding: {}",
            AsRef::<str>::as_ref(&interface.name),
            interface.version,
            skipped.path,
            skipped.reason
        );
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let package = bindings.package;
    write_if_changed(&out_dir.join(format!("{package}.rs")), &generated.bindings);
    let stub_source = out_dir.join(format!("{package}_stubs.rs"));
    write_if_changed(&stub_source, &generated.stubs);
    stubs::compile_stub_archive(&stubs::StubArchive {
        crate_name: stub_crate_name(package),
        source: stub_source,
    });
}

/// The name the compiler records this project's dependency artifact map under: the
/// `[package].name` of the `miden-project.toml` in `manifest_dir`, else Cargo's package name.
fn consumer_name(manifest_dir: &Path) -> String {
    fs::read_to_string(manifest_dir.join("miden-project.toml"))
        .ok()
        .and_then(|manifest| manifest.parse::<toml::Table>().ok())
        .and_then(|manifest| {
            manifest.get("package")?.as_table()?.get("name")?.as_str().map(str::to_string)
        })
        .unwrap_or_else(|| env::var("CARGO_PKG_NAME").expect("cargo sets CARGO_PKG_NAME"))
}

/// The package file of the dependency `package` of the project `consumer` in the package cache
/// `cache`: the artifact the compiler's dependency artifact map selected for it, else
/// `<package>.masp` with `-` or `_`, the lookup the SDK macros make.
///
/// A map that exists is authoritative, as it is for the macros: a dependency it does not record
/// means the staged cache is out of date with `miden-project.toml`, not that another file will do.
fn locate_package(cache: &Path, consumer: &str, package: &str) -> Result<PathBuf, String> {
    let map_path = cache.join(DEPENDENCY_MANIFEST_DIR).join(dependency_map_file_name(consumer));
    match fs::read_to_string(&map_path) {
        Ok(map) => return resolve_in_artifact_map(&map, &map_path, cache, package),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "failed to read the compiler's dependency manifest '{}': {err}",
                map_path.display()
            ));
        }
    }

    let mut stems = vec![package.to_string()];
    let underscored = package.replace('-', "_");
    if underscored != package {
        stems.push(underscored);
    }
    for stem in &stems {
        let candidate = cache.join(package_file_name(stem));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let expected = stems
        .iter()
        .map(|stem| format!("'{}'", package_file_name(stem)))
        .collect::<Vec<_>>()
        .join(" or ");
    Err(format!(
        "no package for Miden dependency '{package}' in the package cache '{}': expected \
         {expected}. Declare the dependency in miden-project.toml and build with `cargo miden \
         build`, or call `prepare_package_cache` before `generate_bindings`",
        cache.display()
    ))
}

/// The artifact the dependency artifact map `map` (read from `map_path`) records for `package`.
fn resolve_in_artifact_map(
    map: &str,
    map_path: &Path,
    cache: &Path,
    package: &str,
) -> Result<PathBuf, String> {
    let map = map.parse::<toml::Table>().map_err(|err| {
        format!(
            "failed to parse the compiler's dependency manifest '{}': {err}",
            map_path.display()
        )
    })?;
    let schema = map.get("schema").and_then(toml::Value::as_integer);
    if schema != Some(DEPENDENCY_MAP_SCHEMA) {
        return Err(format!(
            "the compiler's dependency manifest '{}' has an unsupported schema {schema:?}; the \
             package cache was staged by an incompatible toolchain",
            map_path.display()
        ));
    }
    let Some(entry) = map
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get(package))
    else {
        return Err(format!(
            "dependency '{package}' is not recorded in the compiler's dependency manifest '{}': \
             declare it in miden-project.toml's [dependencies], and rebuild so the package cache \
             is staged again",
            map_path.display()
        ));
    };
    if let Some(file) = entry.get("package").and_then(toml::Value::as_str) {
        Ok(cache.join(file))
    } else if let Some(path) = entry.get("path").and_then(toml::Value::as_str) {
        Ok(PathBuf::from(path))
    } else {
        Err(format!(
            "the compiler's dependency manifest '{}' has a malformed entry for dependency \
             '{package}'",
            map_path.display()
        ))
    }
}

/// The interface of the package in the file `path`.
fn read_interface(path: &Path) -> Result<PackageInterface, String> {
    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read the Miden package '{}': {err}", path.display()))?;
    let package = Package::read_from_bytes_trusted(&bytes).map_err(|err| {
        format!("failed to decode the Miden package '{}': {err}; rebuild it", path.display())
    })?;
    Ok(PackageInterface::from_package(&package))
}

/// `root`, or the root module of `package` when `root` is empty.
fn root_or_package_root(root: &str, package: &PackageInterface) -> String {
    if !root.is_empty() {
        return root.to_string();
    }
    package_root(package).unwrap_or_else(|err| panic!("{err}"))
}

/// The root module of `package`: the deepest module every export and every declared module lies
/// under.
fn package_root(package: &PackageInterface) -> Result<String, String> {
    let name: &str = package.name.as_ref();
    let mut modules = package
        .modules
        .iter()
        .map(|module| &**module)
        .chain(package.procedures.iter().filter_map(|item| item.path.parent()))
        .chain(package.types.iter().filter_map(|item| item.path.parent()))
        .chain(package.constants.iter().filter_map(|item| item.path.parent()));
    let no_root = || {
        format!(
            "the modules of package `{name}` share no root module; name the one to generate the \
             bindings from in `Bindings::root`"
        )
    };
    let Some(mut root) = modules.next() else {
        return Err(format!("package `{name}` declares no module and exports nothing to bind"));
    };
    for module in modules {
        while module.strip_prefix(root).is_none() {
            root = root.parent().ok_or_else(no_root)?;
        }
    }
    // A path made of the root prefix alone names no module.
    if root.first().is_none() {
        return Err(no_root());
    }
    Ok(root.to_string())
}

/// Writes `contents` to `path` unless the file already holds them.
///
/// An unchanged file keeps its modification time. The stub source is watched
/// (`cargo:rerun-if-changed`), and Cargo treats a watched file written during the script's own
/// run as changed afterwards, so rewriting it every time would re-run the script on every build.
fn write_if_changed(path: &Path, contents: &str) {
    if fs::read_to_string(path).is_ok_and(|existing| existing == contents) {
        return;
    }
    fs::write(path, contents)
        .unwrap_or_else(|err| panic!("failed to write '{}': {err}", path.display()));
}

/// The crate name of `package`'s stub archive: the package name as an identifier, plus `_stubs`.
fn stub_crate_name(package: &str) -> String {
    let name: String = package
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}_stubs")
}

#[cfg(test)]
mod tests {
    use midenc_frontend_wasm_metadata::package_cache::{BUILD_INPUTS_FILE, BUILD_INPUTS_SCHEMA};

    use super::*;

    /// The spellings `prepare_package_cache` carries inline, so that the crate stays
    /// dependency-free without this feature, are the shared cache contract's — the build-inputs
    /// header's schema number included.
    #[test]
    fn the_inline_cache_contract_spellings_are_the_shared_ones() {
        assert_eq!(crate::PACKAGE_CACHE_ENV, PACKAGE_CACHE_ENV);
        assert_eq!(crate::DEPENDENCY_MANIFEST_DIR, DEPENDENCY_MANIFEST_DIR);
        assert_eq!(crate::BUILD_INPUTS_FILE, BUILD_INPUTS_FILE);
        assert_eq!(
            crate::BUILD_INPUTS_HEADER,
            format!("miden-build-inputs\t{BUILD_INPUTS_SCHEMA}")
        );
    }

    /// A package cache holding `files`, each a path relative to the cache and its contents.
    fn cache_with(label: &str, files: &[(&str, &str)]) -> PathBuf {
        let cache = crate::tests::scratch(label);
        for (path, contents) in files {
            let path = cache.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        cache
    }

    #[test]
    fn the_artifact_map_names_the_package_the_compiler_selected() {
        let map = r#"
schema = 1

[dependencies]
masm-dep = { package = "masm-dep@0.1.0.masp", version = "0.1.0", wit = false }
prebuilt = { path = "/elsewhere/prebuilt.masp", version = "1.0.0" }
"#;
        let cache = cache_with(
            "artifact-map",
            &[
                ("miden-deps/consumer.deps.toml", map),
                ("masm-dep@0.1.0.masp", ""),
                // The name alone is not what the compiler selected.
                ("masm-dep.masp", ""),
            ],
        );
        assert_eq!(
            locate_package(&cache, "consumer", "masm-dep"),
            Ok(cache.join("masm-dep@0.1.0.masp"))
        );
        assert_eq!(
            locate_package(&cache, "consumer", "prebuilt"),
            Ok(PathBuf::from("/elsewhere/prebuilt.masp"))
        );
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_dependency_the_artifact_map_does_not_record_is_not_looked_up_by_name() {
        let map = "schema = 1\n\n[dependencies]\nother = { package = \"other.masp\" }\n";
        let cache = cache_with(
            "artifact-map-miss",
            &[("miden-deps/consumer.deps.toml", map), ("masm-dep.masp", "")],
        );
        let err = locate_package(&cache, "consumer", "masm-dep").unwrap_err();
        assert!(err.contains("dependency 'masm-dep' is not recorded"), "{err}");
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn without_an_artifact_map_the_package_is_found_by_name_with_either_spelling() {
        let cache = cache_with("by-name", &[("masm_dep.masp", "")]);
        // Another consumer's map does not apply to this one.
        fs::create_dir_all(cache.join("miden-deps")).unwrap();
        fs::write(cache.join("miden-deps/other.deps.toml"), "schema = 1\n").unwrap();
        assert_eq!(locate_package(&cache, "consumer", "masm-dep"), Ok(cache.join("masm_dep.masp")));

        let err = locate_package(&cache, "consumer", "missing-dep").unwrap_err();
        assert!(err.contains("expected 'missing-dep.masp' or 'missing_dep.masp'"), "{err}");
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn the_root_is_the_deepest_module_every_export_lies_under() {
        use midenc_package_interface::testing::{FIXTURE_SOURCE, assemble_fixture};

        for root in ["::masm_dep", "::outer::inner"] {
            let package = assemble_fixture("masm-dep", root, FIXTURE_SOURCE);
            let interface = PackageInterface::from_package(&package);
            assert_eq!(package_root(&interface).as_deref(), Ok(root));
        }
    }

    #[test]
    fn a_stub_crate_is_named_after_its_package_as_an_identifier() {
        assert_eq!(stub_crate_name("masm-dep"), "masm_dep_stubs");
        assert_eq!(stub_crate_name("my.dep_2"), "my_dep_2_stubs");
    }
}
