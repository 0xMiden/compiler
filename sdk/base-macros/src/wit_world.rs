//! Shared manifest and WIT world helpers used by script-like SDK proc macros.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use heck::ToKebabCase;
use miden_assembly_syntax::ast;
use miden_debug_types::DefaultSourceManager;
use proc_macro2::Span;
use toml::{Value, value::Table};

use crate::{util::strip_line_comment, wit_builder::WitBuilder};

/// Parsed package metadata from the consuming crate's manifest.
pub struct ManifestPackage {
    pub manifest_dir: PathBuf,
    pub package_table: Table,
    pub project_kind: Option<String>,
    pub package: Arc<miden_project::Package>,
    pub target: miden_project::Target,
    pub description: Arc<str>,
    pub supported_types: Vec<String>,
}

impl ManifestPackage {
    pub fn load_or_default(call_site_span: Span) -> Result<Self, syn::Error> {
        let manifest_dir =
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));

        let miden_project_toml_path = manifest_dir.join("miden-project.toml");
        if !miden_project_toml_path.is_file() {
            let target = miden_project::Target::r#virtual(
                Default::default(),
                "default",
                ast::Path::new("empty"),
            );
            return Ok(Self {
                manifest_dir,
                package_table: Default::default(),
                project_kind: None,
                package: Arc::from(miden_project::Package::new("empty", target.clone())),
                target,
                description: Default::default(),
                supported_types: vec![],
            });
        }

        Self::load(call_site_span)
    }

    /// Loads the current crate's `[package]` table from `Cargo.toml`.
    pub(crate) fn load(error_span: Span) -> Result<Self, syn::Error> {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|err| {
            syn::Error::new(error_span, format!("failed to read CARGO_MANIFEST_DIR: {err}"))
        })?);
        let manifest_path = manifest_dir.join("Cargo.toml");
        let manifest_content = fs::read_to_string(&manifest_path).map_err(|err| {
            syn::Error::new(
                error_span,
                format!("failed to read manifest '{}': {err}", manifest_path.display()),
            )
        })?;
        let manifest = manifest_content.parse::<toml::Table>().map_err(|err| {
            syn::Error::new(
                error_span,
                format!("failed to parse manifest '{}': {err}", manifest_path.display()),
            )
        })?;

        let package_table = manifest
            .get("package")
            .and_then(Value::as_table)
            .ok_or_else(|| syn::Error::new(error_span, "manifest missing [package] table"))?
            .clone();
        let project_kind = manifest
            .get("package")
            .and_then(Value::as_table)
            .and_then(|package| package.get("metadata"))
            .and_then(Value::as_table)
            .and_then(|metadata| metadata.get("miden"))
            .and_then(Value::as_table)
            .and_then(|miden| miden.get("project-kind"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        let miden_project_toml_path = manifest_dir.join("miden-project.toml");
        let source_manager = Arc::new(DefaultSourceManager::default());
        let project = miden_project::Project::load(&miden_project_toml_path, &source_manager)
            .map_err(|err| {
                syn::Error::new(
                    error_span,
                    format!(
                        "Failed to read project manifest from {}: {err}",
                        miden_project_toml_path.display()
                    ),
                )
            })?;
        let package = project.package();
        let Some(target) = package.library_target() else {
            return Err(syn::Error::new(
                error_span,
                "Expected miden-project.toml to define a library target",
            ));
        };
        let target = target.inner().clone();

        let description = package.description().unwrap_or_else(|| {
            package_table
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| Arc::from(s.to_string().into_boxed_str()))
                .unwrap_or_default()
        });

        let supported_types = package
            .metadata()
            .get("miden")
            .and_then(|meta| meta.get("supported-types"))
            .and_then(|st| st.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        Ok(Self {
            manifest_dir,
            package_table,
            project_kind,
            package,
            target,
            description,
            supported_types,
        })
    }

    /// Returns the crate name declared in `[package]`.
    pub(crate) fn crate_name(&self, error_span: Span) -> Result<&str, syn::Error> {
        self.package_table
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| syn::Error::new(error_span, "manifest package missing `name`"))
    }

    /// Returns the declared component package identifier from manifest metadata.
    pub(crate) fn component_package(&self) -> String {
        format!("miden:{}", self.package.name().into_inner().to_kebab_case())
    }

    /// Returns the declared component version from manifest metadata.
    pub(crate) fn component_version(&self) -> &miden_mast_package::Version {
        self.package.version().into_inner()
    }

    /// Returns true if Cargo metadata declares this crate as an authentication component.
    pub(crate) fn requires_auth_script(&self) -> bool {
        self.project_kind.as_deref() == Some("authentication-component")
    }

    /// Resolves fully-qualified imports exported by `package.metadata.miden.dependencies`.
    pub(crate) fn collect_miden_dependency_imports(
        &self,
        error_span: Span,
    ) -> Result<Vec<String>, syn::Error> {
        let mut imports = self
            .collect_miden_dependencies(error_span)?
            .into_iter()
            .map(|dependency| dependency.import)
            .collect::<Vec<_>>();
        imports.sort();
        Ok(imports)
    }

    /// Resolves metadata for dependencies declared in `miden-project.toml`.
    pub(crate) fn collect_miden_dependencies(
        &self,
        error_span: Span,
    ) -> Result<Vec<MidenDependency>, syn::Error> {
        collect_miden_dependencies(&self.manifest_dir, &self.package, error_span)
    }
}

/// Resolved metadata for one Miden package dependency.
#[derive(Debug)]
pub(crate) struct MidenDependency {
    /// Manifest key used for this dependency.
    pub(crate) name: String,
    /// Canonical project root or precompiled package path.
    pub(crate) root: PathBuf,
    /// Fully-qualified WIT import path, including package version.
    pub(crate) import: String,
}

/// Writes a WIT world block with the provided imports and exports.
pub(crate) fn write_world_block(
    wit: &mut WitBuilder,
    world_name: &str,
    imports: &[String],
    exports: &[String],
) {
    wit.world(world_name, |world| {
        for import in imports {
            world.line(&format!("import {import};"));
        }
        if !imports.is_empty() && !exports.is_empty() {
            world.blank_line();
        }
        for export in exports {
            world.line(&format!("export {export};"));
        }
    });
}

/// Collects dependency metadata needed for typed FPI imports.
fn collect_miden_dependencies(
    manifest_dir: &Path,
    package: &miden_project::Package,
    error_span: Span,
) -> Result<Vec<MidenDependency>, syn::Error> {
    let mut dependencies = Vec::new();

    for dependency in package.dependencies() {
        match dependency.scheme() {
            miden_project::DependencyVersionScheme::Path { path, .. } => {
                let absolute_path = manifest_dir.join(path.path());
                let dependency_root = fs::canonicalize(&absolute_path).map_err(|err| {
                    syn::Error::new(
                        error_span,
                        format!(
                            "failed to canonicalize dependency '{}' path '{}': {err}",
                            dependency.name(),
                            absolute_path.display()
                        ),
                    )
                })?;
                let wit_root =
                    dependency_wit_root(manifest_dir, package, dependency, &dependency_root)?;

                let dependency_wit = parse_dependency_wit(&wit_root).map_err(|msg| {
                    syn::Error::new(
                        error_span,
                        format!(
                            "failed to process typed FPI WIT metadata for dependency '{}' from \
                             '{}': {msg}",
                            dependency.name(),
                            wit_root.display()
                        ),
                    )
                })?;

                dependencies.push(MidenDependency {
                    name: dependency.name().to_string(),
                    root: dependency_root,
                    import: qualify_dependency_export(&dependency_wit, error_span)?,
                });
            }
            _ => continue,
        }
    }

    dependencies.sort_by(|a, b| a.import.cmp(&b.import));

    Ok(dependencies)
}

/// Returns the WIT root for a dependency, honoring explicit Miden project metadata.
fn dependency_wit_root(
    manifest_dir: &Path,
    package: &miden_project::Package,
    dependency: &miden_project::Dependency,
    dependency_root: &Path,
) -> Result<PathBuf, syn::Error> {
    let error_span = Span::call_site();
    if let Some(wit_path) = package
        .metadata()
        .get("miden")
        .and_then(|meta| meta.get("dependencies"))
        .and_then(|value| value.as_table())
        .and_then(|dependencies| dependencies.get(dependency.name().as_ref()))
        .and_then(|config| config.as_table())
        .and_then(|config| config.get("wit"))
    {
        let wit_path = wit_path.as_str().ok_or_else(|| {
            syn::Error::new(
                error_span,
                format!(
                    "invalid miden-project.toml configuration: expected \
                     package.metadata.miden.dependencies.{}.wit to be a string",
                    dependency.name()
                ),
            )
        })?;
        return canonicalize_manifest_path(
            manifest_dir,
            wit_path,
            error_span,
            &format!("dependency '{}' WIT path", dependency.name()),
        );
    }

    if dependency_root.is_file() {
        return Err(syn::Error::new(
            error_span,
            format!(
                "dependency '{}' points to file '{}', which can be used as a `.masp` package \
                 artifact but cannot supply typed FPI WIT metadata; add a matching \
                 package.metadata.miden.dependencies entry with a `wit` path to the dependency's \
                 generated WIT",
                dependency.name(),
                dependency_root.display()
            ),
        ));
    }

    Ok(dependency_root.to_path_buf())
}

/// Resolves a path from manifest metadata relative to `manifest_dir`.
fn canonicalize_manifest_path(
    manifest_dir: &Path,
    path: &str,
    error_span: Span,
    label: &str,
) -> Result<PathBuf, syn::Error> {
    let raw_path = Path::new(path);
    let absolute_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        manifest_dir.join(raw_path)
    };
    fs::canonicalize(&absolute_path).map_err(|err| {
        syn::Error::new(
            error_span,
            format!("failed to canonicalize {label} '{}': {err}", absolute_path.display()),
        )
    })
}

/// Parses the first exported WIT world exposed by a dependency root or WIT file.
fn parse_dependency_wit(root: &Path) -> Result<DependencyWit, String> {
    if root.is_file() {
        return parse_wit_file(root)?.ok_or_else(|| {
            format!("WIT file '{}' does not contain a world export", root.display())
        });
    }

    let direct_wit_dir = root.to_path_buf();
    let default_wit_dir = root.join("wit");
    let generated_wit_dir = root.join("target/generated-wit");
    let wit_dirs = [direct_wit_dir, default_wit_dir, generated_wit_dir];
    for wit_dir in &wit_dirs {
        if !wit_dir.exists() {
            continue;
        }
        let mut entries = fs::read_dir(wit_dir)
            .map_err(|err| format!("failed to read '{}': {err}", wit_dir.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("failed to iterate '{}': {err}", wit_dir.display()))?;

        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "deps") {
                    continue;
                }
                continue;
            }

            if path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
                continue;
            }

            if let Some(info) = parse_wit_file(&path)? {
                return Ok(info);
            }
        }
    }

    Err(format!("no WIT world definition found in directories '{wit_dirs:?}'"))
}

/// WIT package identifier plus exported world entries extracted from a dependency.
struct DependencyWit {
    package: String,
    version: Option<String>,
    exports: Vec<String>,
}

/// Parses a single WIT file and returns its exported world metadata.
fn parse_wit_file(path: &Path) -> Result<Option<DependencyWit>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read WIT file '{}': {err}", path.display()))?;

    let (package, version) = match extract_package_identifier(&contents) {
        Some(parts) => parts,
        None => return Ok(None),
    };

    let exports = extract_world_exports(&contents);
    if exports.is_empty() {
        return Ok(None);
    }

    Ok(Some(DependencyWit {
        package,
        version,
        exports,
    }))
}

/// Extracts the declared WIT package identifier from a source file.
fn extract_package_identifier(contents: &str) -> Option<(String, Option<String>)> {
    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim_start();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            let token = rest.trim_end_matches(';').trim();
            if let Some((name, version)) = token.split_once('@') {
                return Some((name.trim().to_string(), Some(version.trim().to_string())));
            }
            return Some((token.to_string(), None));
        }
    }
    None
}

/// Extracts world export declarations from a WIT source file.
fn extract_world_exports(contents: &str) -> Vec<String> {
    let mut exports = Vec::new();

    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim();
        if let Some(rest) = trimmed.strip_prefix("export ") {
            let export = rest.trim_end_matches(';').trim();
            if !export.is_empty() {
                exports.push(export.to_string());
            }
        }
    }

    exports
}

/// Resolves the dependency's first world export to a fully-qualified import path.
fn qualify_dependency_export(
    dependency_wit: &DependencyWit,
    error_span: Span,
) -> Result<String, syn::Error> {
    let export = dependency_wit.exports.first().ok_or_else(|| {
        syn::Error::new(
            error_span,
            format!(
                "dependency package '{}' did not export any interfaces in its world definition",
                dependency_wit.package
            ),
        )
    })?;

    if export.contains(':') {
        return Ok(export.clone());
    }

    let version = dependency_wit.version.as_deref().ok_or_else(|| {
        syn::Error::new(
            error_span,
            format!("WIT package '{}' is missing a version suffix", dependency_wit.package),
        )
    })?;

    Ok(format!("{}/{export}@{version}", dependency_wit.package))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use miden_assembly_syntax::{ast, debuginfo::Span as MidenSpan};
    use proc_macro2::Span;
    use toml::{Value, value::Table};

    use super::{
        collect_miden_dependencies, extract_package_identifier, extract_world_exports,
        parse_dependency_wit, qualify_dependency_export,
    };

    // This WIT is generated for the basic wallet example at examples/basic-wallet/target/generated-wit/miden-basic-wallet.wit
    const BASIC_WALLET_GENERATED_WIT: &str = r#"// This file is auto-generated by the `#[component]` macro.
// Do not edit this file manually.

package miden:basic-wallet@0.1.0;

use miden:base/core-types@1.0.0;

interface basic-wallet {
    use core-types.{asset, note-idx};

    receive-asset: func(asset: asset);
    move-asset-to-note: func(asset: asset, note-idx: note-idx);
}

world basic-wallet-world {
    export basic-wallet;
}
"#;

    fn basic_wallet_fixture_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("miden-base-macros-wit-world-{unique}"));
        let generated_wit_dir = root.join("target/generated-wit");
        fs::create_dir_all(&generated_wit_dir).expect("generated-wit directory must be created");
        fs::write(generated_wit_dir.join("miden-basic-wallet.wit"), BASIC_WALLET_GENERATED_WIT)
            .expect("basic wallet fixture must be written");
        root
    }

    fn package_with_dependency(
        package_path: PathBuf,
        wit_path: Option<PathBuf>,
    ) -> Box<miden_project::Package> {
        let target = miden_project::Target::r#virtual(
            Default::default(),
            "default",
            ast::Path::new("empty"),
        );
        let dependency = miden_project::Dependency::new(
            MidenSpan::unknown(Arc::<str>::from("basic-wallet")),
            miden_project::DependencyVersionScheme::Path {
                path: MidenSpan::unknown(miden_project::Uri::new(package_path.to_string_lossy())),
                version: None,
            },
            miden_project::Linkage::Dynamic,
        );
        let package =
            miden_project::Package::new("consumer", target).with_dependencies([dependency]);

        if let Some(wit_path) = wit_path {
            package.with_metadata(miden_metadata_dependencies(
                "basic-wallet",
                wit_path.to_string_lossy().as_ref(),
            ))
        } else {
            package
        }
    }

    fn miden_metadata_dependencies(
        dependency_name: &str,
        wit_path: &str,
    ) -> miden_project::MetadataSet {
        let mut dependency_config = Table::new();
        dependency_config.insert("wit".to_string(), Value::String(wit_path.to_string()));

        let mut dependencies = Table::new();
        dependencies.insert(dependency_name.to_string(), Value::Table(dependency_config));

        let mut miden_metadata = miden_project::Metadata::default();
        miden_metadata.insert(
            MidenSpan::unknown(Arc::<str>::from("dependencies")),
            MidenSpan::unknown(Value::Table(dependencies)),
        );

        let mut metadata = miden_project::MetadataSet::default();
        metadata.insert(MidenSpan::unknown(Arc::<str>::from("miden")), miden_metadata);
        metadata
    }

    #[test]
    fn parses_generated_component_wit_fixture_contents() {
        let package = extract_package_identifier(BASIC_WALLET_GENERATED_WIT);
        let exports = extract_world_exports(BASIC_WALLET_GENERATED_WIT);

        assert_eq!(package, Some(("miden:basic-wallet".into(), Some("0.1.0".into()))));
        assert_eq!(exports, vec!["basic-wallet"]);
    }

    #[test]
    fn parses_generated_component_world_from_dependency_root() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_wit = parse_dependency_wit(&fixture_root).unwrap();

        assert_eq!(dependency_wit.package, "miden:basic-wallet");
        assert_eq!(dependency_wit.version.as_deref(), Some("0.1.0"));
        assert_eq!(dependency_wit.exports, vec!["basic-wallet"]);

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn parses_direct_generated_wit_directory() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_wit =
            parse_dependency_wit(&fixture_root.join("target/generated-wit")).unwrap();

        assert_eq!(dependency_wit.package, "miden:basic-wallet");
        assert_eq!(dependency_wit.version.as_deref(), Some("0.1.0"));
        assert_eq!(dependency_wit.exports, vec!["basic-wallet"]);

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn qualifies_generated_component_export_as_dependency_import() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_wit = parse_dependency_wit(&fixture_root).unwrap();

        let import = qualify_dependency_export(&dependency_wit, Span::call_site()).unwrap();

        assert_eq!(import, "miden:basic-wallet/basic-wallet@0.1.0");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn miden_file_dependency_uses_project_wit_metadata() {
        let fixture_root = basic_wallet_fixture_root();
        let package_path = fixture_root.join("target/release/basic_wallet.masp");
        fs::create_dir_all(package_path.parent().expect("package path must have a parent"))
            .expect("package directory must be created");
        fs::write(&package_path, b"package bytes").expect("package fixture must be written");

        let package = package_with_dependency(
            package_path.clone(),
            Some(fixture_root.join("target/generated-wit")),
        );

        let dependencies =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .unwrap();
        let package_path =
            fs::canonicalize(package_path).expect("package path fixture must canonicalize");

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].root, package_path);
        assert_eq!(dependencies[0].import, "miden:basic-wallet/basic-wallet@0.1.0");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn miden_file_dependency_without_project_wit_reports_typed_fpi_error() {
        let fixture_root = basic_wallet_fixture_root();
        let package_path = fixture_root.join("target/release/basic_wallet.masp");
        fs::create_dir_all(package_path.parent().expect("package path must have a parent"))
            .expect("package directory must be created");
        fs::write(&package_path, b"package bytes").expect("package fixture must be written");

        let package = package_with_dependency(package_path, None);

        let error =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .expect_err("artifact-only dependency must not provide typed FPI metadata");
        let message = error.to_string();

        assert!(message.contains("points to file"), "unexpected error: {message}");
        assert!(
            message.contains("package.metadata.miden.dependencies"),
            "unexpected error: {message}"
        );
        assert!(message.contains("typed FPI WIT metadata"), "unexpected error: {message}");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }
}
