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
use wit_bindgen_core::wit_parser::{
    InterfaceId, PackageId, Resolve, Type as WitType, TypeDefKind, TypeOwner, WorldItem,
};

use crate::wit_builder::WitBuilder;

/// Parsed package metadata from the consuming crate's manifest.
pub struct ManifestPackage {
    pub manifest_dir: PathBuf,
    pub package_table: Table,
    pub project_kind: Option<String>,
    pub package: Arc<miden_project::Package>,
    pub target: miden_project::Target,
    pub description: Arc<str>,
}

/// Project package metadata needed to resolve dependency WIT imports.
pub(crate) struct ProjectPackageMetadata {
    pub(crate) manifest_dir: PathBuf,
    pub(crate) package: Arc<miden_project::Package>,
}

impl ProjectPackageMetadata {
    /// Loads the current crate's Miden project package, or an empty package if none exists.
    pub(crate) fn load_or_default(error_span: Span) -> Result<Self, syn::Error> {
        let manifest_dir =
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));
        Self::load_or_default_from_dir(manifest_dir, error_span)
    }

    /// Loads a Miden project package from `manifest_dir`, or an empty package if none exists.
    fn load_or_default_from_dir(
        manifest_dir: PathBuf,
        error_span: Span,
    ) -> Result<Self, syn::Error> {
        let miden_project_toml_path = manifest_dir.join("miden-project.toml");
        if !miden_project_toml_path.is_file() {
            let target = miden_project::Target::r#virtual(
                Default::default(),
                "default",
                ast::Path::new("empty"),
            );
            return Ok(Self {
                manifest_dir,
                package: Arc::from(miden_project::Package::new("empty", target)),
            });
        }

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

        Ok(Self {
            manifest_dir,
            package: project.package(),
        })
    }

    /// Resolves dependency imports for tests that exercise default project metadata.
    #[cfg(test)]
    fn collect_miden_dependency_imports(
        &self,
        error_span: Span,
    ) -> Result<Vec<String>, syn::Error> {
        let mut imports =
            collect_miden_dependencies(&self.manifest_dir, &self.package, error_span)?
                .into_iter()
                .map(|dependency| dependency.import().to_owned())
                .collect::<Vec<_>>();
        imports.sort();
        Ok(imports)
    }
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

        Ok(Self {
            manifest_dir,
            package_table,
            project_kind,
            package,
            target,
            description,
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
            .map(|dependency| dependency.import().to_owned())
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
    /// Exported WIT interface loaded from the dependency metadata.
    pub(crate) interface: DependencyInterface,
}

impl MidenDependency {
    /// Fully-qualified WIT import path, including package version.
    pub(crate) fn import(&self) -> &str {
        &self.interface.import
    }

    /// WIT type names declared by the imported dependency interface.
    pub(crate) fn type_names(&self) -> &[String] {
        &self.interface.types
    }
}

/// Exported dependency interface metadata derived from WIT.
#[derive(Debug)]
pub(crate) struct DependencyInterface {
    /// Fully-qualified WIT import path, including package version.
    pub(crate) import: String,
    /// WIT type names owned by the imported interface.
    pub(crate) types: Vec<String>,
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

/// Collects dependency metadata needed for SDK-generated dependency imports.
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
                        dependency_wit_error_message(dependency, &dependency_root, &wit_root, &msg),
                    )
                })?;

                dependencies.push(MidenDependency {
                    name: dependency.name().to_string(),
                    root: dependency_root,
                    interface: dependency_wit.interface,
                });
            }
            _ => continue,
        }
    }

    dependencies.sort_by(|a, b| a.import().cmp(b.import()));

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
        return canonicalize_dependency_wit_path(manifest_dir, dependency, wit_path, error_span);
    }

    if dependency_root.is_file() {
        return Err(syn::Error::new(
            error_span,
            format!(
                "dependency '{}' points to file '{}', which can be used as a `.masp` package \
                 artifact but cannot supply dependency WIT metadata; add a matching \
                 package.metadata.miden.dependencies entry with a `wit` path to the dependency's \
                 generated WIT",
                dependency.name(),
                dependency_root.display()
            ),
        ));
    }

    Ok(dependency_root.to_path_buf())
}

/// Resolves an explicit dependency WIT path from manifest metadata.
fn canonicalize_dependency_wit_path(
    manifest_dir: &Path,
    dependency: &miden_project::Dependency,
    path: &str,
    error_span: Span,
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
            format!(
                "failed to resolve dependency WIT metadata for dependency '{}' from \
                 package.metadata.miden.dependencies.{}.wit = '{}': '{}': {err}. The SDK macro \
                 needs the dependency's generated WIT file or directory during Rust macro \
                 expansion; generate the dependency WIT or update the `wit` path.",
                dependency.name(),
                dependency.name(),
                path,
                absolute_path.display()
            ),
        )
    })
}

/// Parses the first exported WIT interface exposed by a dependency root or WIT file.
fn parse_dependency_wit(root: &Path) -> Result<DependencyWit, String> {
    if root.is_file() {
        return parse_dependency_wit_path(root)?.ok_or_else(|| {
            format!("WIT file '{}' does not contain a world export", root.display())
        });
    }

    let wit_dirs = dependency_wit_candidate_paths(root);
    let mut parser_errors = Vec::new();
    for path in &wit_dirs {
        if !path.exists() {
            continue;
        }
        match parse_dependency_wit_path(path) {
            Ok(Some(info)) => return Ok(info),
            Ok(None) => {}
            Err(err) => {
                parser_errors.push(format!("'{}': {err}", path.display()));
            }
        }
    }

    if parser_errors.is_empty() {
        Err("no WIT world definition found".to_string())
    } else {
        Err(format!(
            "no WIT world definition found; parser errors: {}",
            parser_errors.join("; ")
        ))
    }
}

/// Returns the WIT paths searched for generated dependency metadata.
fn dependency_wit_candidate_paths(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        vec![root.to_path_buf(), root.join("wit"), root.join("target/generated-wit")]
    }
}

/// Formats the dependency WIT diagnostic emitted by SDK macros.
fn dependency_wit_error_message(
    dependency: &miden_project::Dependency,
    dependency_root: &Path,
    wit_root: &Path,
    details: &str,
) -> String {
    let candidates = dependency_wit_candidate_paths(wit_root)
        .into_iter()
        .map(|path| format!("'{}'", path.display()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "failed to load dependency WIT metadata for dependency '{}' (dependency root '{}', WIT \
         root '{}'): {details}. The SDK macro needs the dependency's generated WIT during Rust \
         macro expansion to construct dependency imports. Searched for WIT world definitions in: \
         {candidates}. Generate the dependency WIT by compiling the dependency component, or set \
         package.metadata.miden.dependencies.{}.wit to the generated WIT file or directory.",
        dependency.name(),
        dependency_root.display(),
        wit_root.display(),
        dependency.name()
    )
}

/// WIT metadata extracted from a dependency package.
struct DependencyWit {
    interface: DependencyInterface,
}

/// Parses one WIT path and returns dependency metadata when it exports an interface.
fn parse_dependency_wit_path(path: &Path) -> Result<Option<DependencyWit>, String> {
    let mut resolve = Resolve::default();
    resolve
        .push_str("miden.wit", crate::manifest_paths::SDK_WIT_SOURCE)
        .map_err(|err| format!("failed to load bundled Miden WIT: {err}"))?;
    let (package_id, _) = resolve
        .push_path(path)
        .map_err(|err| format!("failed to parse WIT path '{}': {err}", path.display()))?;

    let Some(interface_id) = first_exported_interface(&resolve, package_id) else {
        return Ok(None);
    };

    Ok(Some(DependencyWit {
        interface: dependency_interface_metadata(&resolve, interface_id)?,
    }))
}

/// Returns the first interface exported by any world in the parsed package.
fn first_exported_interface(resolve: &Resolve, package_id: PackageId) -> Option<InterfaceId> {
    let package = &resolve.packages[package_id];
    package.worlds.values().find_map(|world_id| {
        let world = &resolve.worlds[*world_id];
        world.exports.values().find_map(|item| match item {
            WorldItem::Interface { id, .. } => Some(*id),
            WorldItem::Function(_) | WorldItem::Type { .. } => None,
        })
    })
}

/// Builds explicit metadata for an exported dependency interface.
fn dependency_interface_metadata(
    resolve: &Resolve,
    interface_id: InterfaceId,
) -> Result<DependencyInterface, String> {
    let interface = &resolve.interfaces[interface_id];
    let package_id = interface
        .package
        .ok_or_else(|| "exported dependency interface is not owned by a WIT package".to_string())?;
    let package = &resolve.packages[package_id];
    if package.name.version.is_none() {
        return Err(format!("WIT package '{}' is missing a version suffix", package.name));
    }
    let interface_name = interface.name.as_deref().ok_or_else(|| {
        format!("exported interface in WIT package '{}' is anonymous", package.name)
    })?;
    let import = package.name.interface_id(interface_name);
    let types = interface
        .types
        .iter()
        .filter_map(|(name, type_id)| {
            if is_dependency_interface_type(resolve, interface_id, *type_id) {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    Ok(DependencyInterface { import, types })
}

/// Returns true when a type belongs to the dependency interface rather than a `use` import.
fn is_dependency_interface_type(
    resolve: &Resolve,
    interface_id: InterfaceId,
    type_id: wit_bindgen_core::wit_parser::TypeId,
) -> bool {
    let ty = &resolve.types[type_id];
    if !matches!(ty.owner, TypeOwner::Interface(owner) if owner == interface_id) {
        return false;
    }

    if let TypeDefKind::Type(WitType::Id(alias_target)) = ty.kind {
        matches!(
            resolve.types[alias_target].owner,
            TypeOwner::Interface(owner) if owner == interface_id
        )
    } else {
        true
    }
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

    use super::{ProjectPackageMetadata, collect_miden_dependencies, parse_dependency_wit};

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

    /// Returns a fixture directory name unique across both threads and test processes: a bare
    /// timestamp can collide when parallel tests hit the same clock tick, causing one test to
    /// observe (or remove) another's fixture tree.
    fn unique_fixture_suffix() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after unix epoch")
            .as_nanos();
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{pid}-{nanos}-{count}")
    }

    fn basic_wallet_fixture_root() -> PathBuf {
        let unique = unique_fixture_suffix();
        let root = std::env::temp_dir().join(format!("miden-base-macros-wit-world-{unique}"));
        let generated_wit_dir = root.join("target/generated-wit");
        fs::create_dir_all(&generated_wit_dir).expect("generated-wit directory must be created");
        fs::write(generated_wit_dir.join("miden-basic-wallet.wit"), BASIC_WALLET_GENERATED_WIT)
            .expect("basic wallet fixture must be written");
        root
    }

    fn empty_fixture_root() -> PathBuf {
        let unique = unique_fixture_suffix();
        let root = std::env::temp_dir().join(format!("miden-base-macros-empty-wit-world-{unique}"));
        fs::create_dir_all(&root).expect("empty fixture directory must be created");
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
    fn project_package_metadata_defaults_without_miden_project_manifest() {
        let fixture_root = empty_fixture_root();
        let metadata = ProjectPackageMetadata::load_or_default_from_dir(
            fixture_root.clone(),
            Span::call_site(),
        )
        .expect("missing miden-project.toml should use empty dependency metadata");

        let imports = metadata
            .collect_miden_dependency_imports(Span::call_site())
            .expect("empty dependency metadata should collect successfully");

        assert!(imports.is_empty());

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn project_package_metadata_allows_executable_project_without_imports() {
        let fixture_root = empty_fixture_root();
        fs::write(
            fixture_root.join("miden-project.toml"),
            r#"
[package]
name = "script"
version = "0.0.1"

[[bin]]
name = "script"
path = "<virtual>"
"#,
        )
        .expect("executable project manifest fixture must be written");
        let metadata = ProjectPackageMetadata::load_or_default_from_dir(
            fixture_root.clone(),
            Span::call_site(),
        )
        .expect("executable project metadata should load without a library target");

        let imports = metadata
            .collect_miden_dependency_imports(Span::call_site())
            .expect("executable project without dependencies should collect successfully");

        assert!(imports.is_empty());

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn parses_exported_interface_type_names_with_wit_parser() {
        let fixture_root = empty_fixture_root();
        let wit_dir = fixture_root.join("target/generated-wit");
        fs::create_dir_all(&wit_dir).expect("generated-wit directory must be created");
        let wit = r#"
package miden:typed-account@0.0.1;

use miden:base/core-types@1.0.0;

interface typed-account {
    use core-types.{felt};

    record mixed-scalar-record {
        value: u32,
    }

    flags options {
        enabled,
    }

    type amount = u64;
    echo: func(arg: mixed-scalar-record) -> mixed-scalar-record;
}

world typed-account-world {
    export typed-account;
}
"#;
        fs::write(wit_dir.join("typed-account.wit"), wit).expect("typed WIT fixture");

        let dependency_wit = parse_dependency_wit(&fixture_root).unwrap();

        assert_eq!(dependency_wit.interface.import, "miden:typed-account/typed-account@0.0.1");
        assert_eq!(
            dependency_wit.interface.types,
            vec!["mixed-scalar-record", "options", "amount"]
        );

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn parses_generated_component_world_from_dependency_root() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_wit = parse_dependency_wit(&fixture_root).unwrap();

        assert_eq!(dependency_wit.interface.import, "miden:basic-wallet/basic-wallet@0.1.0");
        assert!(dependency_wit.interface.types.is_empty());

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn parses_direct_generated_wit_directory() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_wit =
            parse_dependency_wit(&fixture_root.join("target/generated-wit")).unwrap();

        assert_eq!(dependency_wit.interface.import, "miden:basic-wallet/basic-wallet@0.1.0");
        assert!(dependency_wit.interface.types.is_empty());

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
        assert_eq!(dependencies[0].import(), "miden:basic-wallet/basic-wallet@0.1.0");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn missing_dependency_wit_reports_actionable_sdk_macro_error() {
        let fixture_root = empty_fixture_root();
        let dependency_root = fixture_root.join("basic-wallet");
        fs::create_dir_all(&dependency_root).expect("dependency fixture directory must be created");
        let package = package_with_dependency(dependency_root, None);

        let error =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .expect_err("dependency without generated WIT must fail dependency metadata load");
        let message = error.to_string();

        assert!(
            message
                .contains("failed to load dependency WIT metadata for dependency 'basic-wallet'"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("The SDK macro needs the dependency's generated WIT"),
            "unexpected error: {message}"
        );
        assert!(message.contains("target/generated-wit"), "unexpected error: {message}");
        assert!(
            message.contains("package.metadata.miden.dependencies.basic-wallet.wit"),
            "unexpected error: {message}"
        );
        assert!(message.contains("Generate the dependency WIT"), "unexpected error: {message}");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn missing_explicit_dependency_wit_reports_actionable_sdk_macro_error() {
        let fixture_root = empty_fixture_root();
        let dependency_root = fixture_root.join("basic-wallet");
        fs::create_dir_all(&dependency_root).expect("dependency fixture directory must be created");
        let package = package_with_dependency(
            dependency_root,
            Some(PathBuf::from("target/generated-wit/missing.wit")),
        );

        let error =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .expect_err(
                    "missing explicit dependency WIT path must fail dependency metadata load",
                );
        let message = error.to_string();

        assert!(
            message.contains(
                "failed to resolve dependency WIT metadata for dependency 'basic-wallet'"
            ),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("package.metadata.miden.dependencies.basic-wallet.wit"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("target/generated-wit/missing.wit"),
            "unexpected error: {message}"
        );
        assert!(
            message
                .contains("The SDK macro needs the dependency's generated WIT file or directory"),
            "unexpected error: {message}"
        );
        assert!(message.contains("update the `wit` path"), "unexpected error: {message}");

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
                .expect_err("artifact-only dependency must not provide dependency WIT metadata");
        let message = error.to_string();

        assert!(message.contains("points to file"), "unexpected error: {message}");
        assert!(
            message.contains("package.metadata.miden.dependencies"),
            "unexpected error: {message}"
        );
        assert!(message.contains("dependency WIT metadata"), "unexpected error: {message}");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }
}
