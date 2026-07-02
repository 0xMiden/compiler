//! Shared manifest and WIT world helpers used by script-like SDK proc macros.

use std::{
    collections::HashSet,
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

use crate::{
    dependency_package::{DependencyWitSource, collect_dependency_wit_sources},
    wit_builder::WitBuilder,
};

/// Parsed package metadata from the consuming crate's manifest.
pub struct ManifestPackage {
    pub manifest_dir: PathBuf,
    pub package_table: Table,
    pub project_kind: Option<String>,
    pub package: Arc<miden_project::Package>,
    pub target: miden_project::Target,
    pub description: Arc<str>,
    /// Whether the crate has a `miden-project.toml`; when false, the package and target metadata
    /// above are synthesized placeholders.
    pub has_miden_project_toml: bool,
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
                .flat_map(|dependency| {
                    dependency
                        .interfaces
                        .iter()
                        .map(|interface| interface.import.clone())
                        .collect::<Vec<_>>()
                })
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
                has_miden_project_toml: false,
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
            has_miden_project_toml: true,
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
            .flat_map(|dependency| {
                dependency
                    .interfaces
                    .iter()
                    .map(|interface| interface.import.clone())
                    .collect::<Vec<_>>()
            })
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
    /// Path of the compiled `.masp` package the dependency metadata was read from.
    pub(crate) package_path: PathBuf,
    /// Exported WIT interfaces loaded from the dependency metadata.
    pub(crate) interfaces: Vec<DependencyInterface>,
}

impl MidenDependency {
    /// Narrows this dependency to the exported interface with the given kebab-case name.
    pub(crate) fn select(&self, interface_name: &str) -> Option<SelectedDependency> {
        self.interfaces
            .iter()
            .find(|interface| interface.name == interface_name)
            .map(|interface| SelectedDependency {
                package_path: self.package_path.clone(),
                interface: interface.clone(),
            })
    }

    /// Names of the interfaces exported by this dependency, for diagnostics.
    pub(crate) fn interface_names(&self) -> Vec<&str> {
        self.interfaces.iter().map(|interface| interface.name.as_str()).collect()
    }
}

/// A dependency narrowed to one selected exported interface.
///
/// This is the unit the `#[account]` and `#[component]` sibling generators consume: one
/// `pkg::Interface` macro argument resolves to one `SelectedDependency`.
#[derive(Debug)]
pub(crate) struct SelectedDependency {
    /// Path of the compiled `.masp` package the dependency metadata was read from.
    pub(crate) package_path: PathBuf,
    /// The selected exported WIT interface.
    pub(crate) interface: DependencyInterface,
}

impl SelectedDependency {
    /// Fully-qualified WIT import path, including package version.
    pub(crate) fn import(&self) -> &str {
        &self.interface.import
    }

    /// WIT type names declared by the selected dependency interface.
    pub(crate) fn type_names(&self) -> &[String] {
        &self.interface.types
    }
}

/// Exported dependency interface metadata derived from WIT.
#[derive(Clone, Debug)]
pub(crate) struct DependencyInterface {
    /// Kebab-case interface name as declared in the dependency WIT.
    pub(crate) name: String,
    /// Fully-qualified WIT import path, including package version.
    pub(crate) import: String,
    /// WIT type names owned by the imported interface.
    pub(crate) types: Vec<String>,
}

/// Renders a standalone inline WIT package whose single world imports the given interfaces.
///
/// Shared by generated FPI and sibling bindings so import-only world formatting remains
/// consistent.
pub(crate) fn import_world_wit(name: &str, imports: &[String]) -> String {
    let mut tokens = format!("package miden:{name}@1.0.0;\n\nworld {name} {{\n");
    for import in imports {
        tokens.push_str("    import ");
        tokens.push_str(import);
        tokens.push_str(";\n");
    }
    tokens.push_str("}\n");
    tokens
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
///
/// The dependency's exported interfaces are read from the component WIT embedded in its compiled
/// `.masp` package, which cargo-miden materializes before the dependent crate's macros expand.
fn collect_miden_dependencies(
    manifest_dir: &Path,
    package: &miden_project::Package,
    error_span: Span,
) -> Result<Vec<MidenDependency>, syn::Error> {
    let mut dependencies = Vec::new();

    for source in collect_dependency_wit_sources(manifest_dir, package)? {
        let dependency_wit = parse_dependency_wit_source(&source.wit).map_err(|msg| {
            syn::Error::new(error_span, dependency_wit_error_message(&source, &msg))
        })?;

        dependencies.push(MidenDependency {
            name: source.name,
            package_path: source.package_path,
            interfaces: dependency_wit.interfaces,
        });
    }

    dependencies.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(dependencies)
}

/// Formats the dependency WIT diagnostic emitted by SDK macros.
fn dependency_wit_error_message(source: &DependencyWitSource, details: &str) -> String {
    format!(
        "failed to load dependency WIT metadata for dependency '{}' (root '{}') from its compiled \
         package '{}': {details}. The SDK macros read the dependency's component WIT embedded in \
         the `.masp` package during Rust macro expansion to construct dependency imports; rebuild \
         the dependency with the current `cargo miden build`.",
        source.name,
        source.root.display(),
        source.package_path.display(),
    )
}

/// WIT metadata extracted from a dependency package.
#[derive(Debug)]
struct DependencyWit {
    interfaces: Vec<DependencyInterface>,
}

/// Parses dependency WIT source and returns metadata for its exported interfaces.
fn parse_dependency_wit_source(wit_source: &str) -> Result<DependencyWit, String> {
    let mut resolve = Resolve::default();
    resolve
        .push_str("miden.wit", crate::manifest_paths::SDK_WIT_SOURCE)
        .map_err(|err| format!("failed to load bundled Miden WIT: {err}"))?;
    let package_id = resolve
        .push_str("package.wit", wit_source)
        .map_err(|err| format!("failed to parse embedded dependency WIT: {err}"))?;

    // Skip exported interfaces that cannot be turned into a referenceable import id (anonymous
    // inline interfaces, or interfaces in an unversioned package) rather than failing the whole
    // dependency: an incidental anonymous export must not break a referenced *named* interface. A
    // reference to a skipped interface still fails later, via `find_interface`, with a precise
    // "does not export a WIT interface named ..." message.
    let interfaces = exported_interfaces(&resolve, package_id)
        .into_iter()
        .filter_map(|interface_id| dependency_interface_metadata(&resolve, interface_id).ok())
        .collect::<Vec<_>>();
    if interfaces.is_empty() {
        return Err("no exported WIT interface found in the embedded dependency WIT".to_string());
    }

    Ok(DependencyWit { interfaces })
}

/// Returns the interfaces exported by the worlds of the parsed package, in declaration order.
fn exported_interfaces(resolve: &Resolve, package_id: PackageId) -> Vec<InterfaceId> {
    let package = &resolve.packages[package_id];
    let mut seen = HashSet::new();
    let mut interfaces = Vec::new();
    for world_id in package.worlds.values() {
        let world = &resolve.worlds[*world_id];
        for item in world.exports.values() {
            if let WorldItem::Interface { id, .. } = item
                && seen.insert(*id)
            {
                interfaces.push(*id);
            }
        }
    }
    interfaces
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
    let name = interface_name.to_string();
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

    Ok(DependencyInterface {
        name,
        import,
        types,
    })
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
        path::{Path, PathBuf},
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use miden_assembly_syntax::{ast, debuginfo::Span as MidenSpan};
    use proc_macro2::Span;

    use super::{ProjectPackageMetadata, collect_miden_dependencies, parse_dependency_wit_source};

    // This WIT matches what the `#[component]` macro embeds into the basic wallet example package.
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

    /// Writes a minimal `.masp` package fixture, optionally embedding `wit` in the WIT section.
    fn write_masp_fixture(package_path: &Path, wit: Option<&str>) {
        use miden_assembly::{
            Assembler, DefaultSourceManager, Parse, ParseOptions, ast::ModuleKind,
        };

        let source_manager = Arc::new(DefaultSourceManager::default());
        let module = "pub proc callee(a: felt) -> felt\n    add.1\nend"
            .parse_with_options(
                source_manager.clone(),
                ParseOptions::new(ModuleKind::Library, "::dep"),
            )
            .expect("fixture module must parse");
        let library = Assembler::new(source_manager)
            .assemble_library([module])
            .expect("fixture library must assemble");
        let mut package = miden_mast_package::Package::from_library(
            miden_mast_package::PackageId::from("basic-wallet"),
            "0.1.0".parse().expect("fixture version must parse"),
            miden_mast_package::TargetType::Library,
            library,
            core::iter::empty(),
        );
        if let Some(wit) = wit {
            package.sections.push(miden_mast_package::Section::new(
                crate::dependency_package::wit_section_id(),
                wit.as_bytes().to_vec(),
            ));
        }

        use miden_protocol::utils::serde::Serializable;
        fs::create_dir_all(package_path.parent().expect("package path must have a parent"))
            .expect("package directory must be created");
        fs::write(package_path, package.to_bytes()).expect("package fixture must be written");
    }

    /// Creates a dependency project root with a compiled package under `target/miden/debug`.
    fn basic_wallet_fixture_root() -> PathBuf {
        let unique = unique_fixture_suffix();
        let root = std::env::temp_dir().join(format!("miden-base-macros-wit-world-{unique}"));
        write_masp_fixture(
            &root.join("target/miden/debug/basic_wallet.masp"),
            Some(BASIC_WALLET_GENERATED_WIT),
        );
        root
    }

    fn empty_fixture_root() -> PathBuf {
        let unique = unique_fixture_suffix();
        let root = std::env::temp_dir().join(format!("miden-base-macros-empty-wit-world-{unique}"));
        fs::create_dir_all(&root).expect("empty fixture directory must be created");
        root
    }

    fn package_with_dependency(package_path: PathBuf) -> Box<miden_project::Package> {
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
        miden_project::Package::new("consumer", target).with_dependencies([dependency])
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

        let dependency_wit = parse_dependency_wit_source(wit).unwrap();

        assert_eq!(dependency_wit.interfaces.len(), 1);
        assert_eq!(dependency_wit.interfaces[0].name, "typed-account");
        assert_eq!(dependency_wit.interfaces[0].import, "miden:typed-account/typed-account@0.0.1");
        assert_eq!(
            dependency_wit.interfaces[0].types,
            vec!["mixed-scalar-record", "options", "amount"]
        );
    }

    #[test]
    fn parses_all_exported_interfaces_and_selects_by_name() {
        let wit = r#"
package miden:multi-account@0.0.1;

interface first-api {
    get-value: func() -> u64;
}

interface second-api {
    set-value: func(value: u64);
}

world multi-account-world {
    export first-api;
    export second-api;
}
"#;

        let dependency_wit = parse_dependency_wit_source(wit).unwrap();
        let dependency = super::MidenDependency {
            name: "multi-account".to_string(),
            package_path: PathBuf::from("/tmp/multi-account/target/miden/debug/multi_account.masp"),
            interfaces: dependency_wit.interfaces,
        };

        assert_eq!(dependency.interface_names(), vec!["first-api", "second-api"]);

        let selected = dependency.select("second-api").expect("interface must be selectable");
        assert_eq!(selected.import(), "miden:multi-account/second-api@0.0.1");
        assert_eq!(selected.package_path, dependency.package_path);

        assert!(dependency.select("missing-api").is_none());
    }

    #[test]
    fn skips_anonymous_exported_interfaces() {
        // The world exports a named interface plus an inline (anonymous) one; the anonymous export
        // must be skipped rather than failing the whole dependency parse.
        let wit = r#"
package miden:mixed-export@0.0.1;

interface named-api {
    get-value: func() -> u64;
}

world mixed-export-world {
    export named-api;
    export inline-api: interface {
        helper: func();
    }
}
"#;

        let dependency_wit = parse_dependency_wit_source(wit).unwrap();

        assert_eq!(dependency_wit.interfaces.len(), 1);
        assert_eq!(dependency_wit.interfaces[0].name, "named-api");
    }

    #[test]
    fn dependency_wit_without_exported_interfaces_reports_error() {
        // An embedded WIT whose world exports nothing referenceable must produce a parse error
        // rather than an empty dependency.
        let wit = r#"
package miden:empty-export@0.0.1;

world empty-export-world {
}
"#;

        let err = parse_dependency_wit_source(wit).unwrap_err();

        assert!(err.contains("no exported WIT interface found"), "unexpected error: {err}");
    }

    #[test]
    fn collects_dependency_interfaces_from_compiled_package() {
        let fixture_root = basic_wallet_fixture_root();
        let dependency_root = fixture_root.clone();

        let package = package_with_dependency(dependency_root.clone());

        let dependencies =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].interface_names(), vec!["basic-wallet"]);
        assert_eq!(dependencies[0].interfaces[0].import, "miden:basic-wallet/basic-wallet@0.1.0");
        assert!(dependencies[0].interfaces[0].types.is_empty());
        assert!(
            dependencies[0].package_path.ends_with("target/miden/debug/basic_wallet.masp"),
            "unexpected package path: {}",
            dependencies[0].package_path.display()
        );

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn file_dependency_reads_wit_from_masp_package() {
        // A dependency that points directly at a `.masp` file is self-contained: the embedded WIT
        // is read from that package with no additional manifest metadata.
        let fixture_root = empty_fixture_root();
        let package_path = fixture_root.join("prebuilt/basic_wallet.masp");
        write_masp_fixture(&package_path, Some(BASIC_WALLET_GENERATED_WIT));

        let package = package_with_dependency(package_path.clone());

        let dependencies =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .unwrap();
        let package_path =
            fs::canonicalize(package_path).expect("package path fixture must canonicalize");

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].package_path, package_path);
        assert_eq!(dependencies[0].interface_names(), vec!["basic-wallet"]);
        assert_eq!(dependencies[0].interfaces[0].import, "miden:basic-wallet/basic-wallet@0.1.0");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn missing_dependency_package_reports_actionable_error() {
        let fixture_root = empty_fixture_root();
        let dependency_root = fixture_root.join("basic-wallet");
        fs::create_dir_all(&dependency_root).expect("dependency fixture directory must be created");
        let package = package_with_dependency(dependency_root);

        let error =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .expect_err("dependency without a compiled package must fail metadata load");
        let message = error.to_string();

        assert!(
            message.contains("could not find a built `.masp` package"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("Miden dependency 'basic-wallet'"),
            "unexpected error: {message}"
        );
        assert!(message.contains("during Rust macro expansion"), "unexpected error: {message}");
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }

    #[test]
    fn package_without_wit_section_reports_rebuild_error() {
        let fixture_root = empty_fixture_root();
        let dependency_root = fixture_root.join("basic-wallet");
        write_masp_fixture(&dependency_root.join("target/miden/debug/basic_wallet.masp"), None);
        let package = package_with_dependency(dependency_root);

        let error =
            collect_miden_dependencies(&fixture_root, &package, proc_macro2::Span::call_site())
                .expect_err("package without an embedded WIT section must fail metadata load");
        let message = error.to_string();

        assert!(message.contains("does not embed component WIT"), "unexpected error: {message}");
        assert!(message.contains("older Miden toolchain"), "unexpected error: {message}");
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");

        fs::remove_dir_all(fixture_root).expect("temporary fixture directory must be removed");
    }
}
