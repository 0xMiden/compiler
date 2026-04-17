//! Project-assembler support for compiler-generated MASM components.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::{
    Assembler, Library, Path, ProjectSourceInputs, ProjectTargetSelector,
    library::{LibraryExport, ProcedureExport},
};
use miden_mast_package::{
    Package as MastPackage, PackageManifest, Section, SectionId, TargetType, Version,
};
use miden_package_registry::{
    InMemoryPackageRegistry, PackageId, PackageRegistry, PackageStore, Version as RegistryVersion,
    VersionRequirement,
};
use miden_project::{
    Dependency as ProjectDependency, DependencyVersionScheme, Linkage, Package as ProjectPackage,
    Target,
};
use midenc_session::{
    LinkLibrary, Session,
    diagnostics::{Report, Span},
};

use super::{MasmComponent, Package, Rodata, Symbol};
use crate::{intrinsics::INTRINSICS_MODULE_NAMES, masm};

/// Assemble a MASM component through the VM project assembler.
pub(super) fn assemble(
    component: &MasmComponent,
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
    account_component_metadata_bytes: Option<&[u8]>,
    session: &Session,
) -> Result<Package, Report> {
    let mut store = InMemoryPackageRegistry::default();
    let dependencies = register_external_dependencies(
        &mut store,
        &session.options.link_libraries,
        link_libraries,
        link_packages,
    )?;
    let target = build_root_target(component)?;
    let mut assembler = Assembler::new(session.source_manager.clone());
    let sources = prepare_sources(
        component,
        &mut assembler,
        link_libraries,
        link_packages,
        session.get_flag("test_harness"),
        session.source_manager.clone(),
    )?;
    let mut project_assembler = assembler.for_project(
        Arc::<ProjectPackage>::from(
            ProjectPackage::new(session.name.clone(), target).with_dependencies(dependencies),
        ),
        &mut store,
    )?;

    let selector = if component.entrypoint.is_some() {
        ProjectTargetSelector::Executable(&component.id.to_string())
    } else {
        ProjectTargetSelector::Library
    };
    let mut package =
        Arc::unwrap_or_clone(project_assembler.assemble_with_sources(selector, "dev", sources)?);

    package.name = session.name.clone().into();
    attach_account_component_metadata(&mut package, account_component_metadata_bytes);
    extend_rodata_advice_map(&mut package, &component.rodata);
    normalize_library_exports(&mut package)?;
    Ok(package)
}

/// Register externally-linked artifacts in an in-memory package store.
fn register_external_dependencies(
    store: &mut InMemoryPackageRegistry,
    link_library_specs: &[LinkLibrary],
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
) -> Result<Vec<ProjectDependency>, Report> {
    if link_library_specs.len() != link_libraries.len() {
        return Err(Report::msg(
            "loaded link libraries do not match the session link library configuration",
        ));
    }

    let link_library_versions =
        resolve_link_library_versions(link_library_specs, link_libraries, link_packages)?;
    let mut dependencies = BTreeMap::default();
    for ((link_lib, library), version) in link_library_specs
        .iter()
        .zip(link_libraries.iter())
        .zip(link_library_versions.into_iter())
    {
        let package = Arc::from(MastPackage::from_library(
            link_lib.name.to_string().into(),
            version,
            TargetType::Library,
            library.clone(),
            [],
        ));
        let version = publish_external_package(store, package)?;
        push_project_dependency(&mut dependencies, Arc::from(link_lib.name.as_ref()), version)?;
    }
    register_link_packages(store, &mut dependencies, link_packages)?;

    Ok(dependencies.into_values().collect())
}

/// Determine the semantic version to associate with each raw session link library.
///
/// Linked package inputs record exact dependency versions in their manifests. To keep those package
/// inputs interoperable with raw `-l` libraries, reuse the semantic version already required by
/// any linked packages that depend on the same library digest. Libraries with no discoverable
/// version metadata fall back to `0.0.0`.
fn resolve_link_library_versions(
    link_library_specs: &[LinkLibrary],
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
) -> Result<Vec<Version>, Report> {
    let mut digests = BTreeMap::default();
    let mut required_versions =
        BTreeMap::<PackageId, BTreeMap<Version, BTreeSet<PackageId>>>::default();

    for (link_lib, library) in link_library_specs.iter().zip(link_libraries.iter()) {
        let name = PackageId::from(link_lib.name.as_ref());
        let digest = *library.digest();

        match digests.get(&name) {
            Some(existing) if existing != &digest => {
                return Err(Report::msg(format!(
                    "conflicting session link libraries registered for '{name}'",
                )));
            }
            Some(_) => {}
            None => {
                digests.insert(name.clone(), digest);
            }
        }

        required_versions.entry(name).or_default();
    }

    for package in link_packages.values() {
        for dependency in package.manifest.dependencies() {
            if dependency.kind != TargetType::Library {
                continue;
            }

            let Some(expected_digest) = digests.get(&dependency.name) else {
                continue;
            };
            if &dependency.digest != expected_digest {
                return Err(Report::msg(format!(
                    "linked package '{}' depends on session library '{}' at '{}#{}', but the \
                     loaded session library has digest '{}'",
                    package.name,
                    dependency.name,
                    dependency.version,
                    dependency.digest,
                    expected_digest,
                )));
            }

            required_versions
                .get_mut(&dependency.name)
                .expect("session link library versions should be initialized")
                .entry(dependency.version.clone())
                .or_default()
                .insert(package.name.clone());
        }
    }

    link_library_specs
        .iter()
        .map(|link_lib| {
            let name = PackageId::from(link_lib.name.as_ref());
            let versions = required_versions
                .get(&name)
                .expect("session link library versions should be initialized");

            if versions.is_empty() {
                return Ok(Version::new(0, 0, 0));
            }
            if versions.len() > 1 {
                let details = versions
                    .iter()
                    .map(|(version, packages)| {
                        let packages =
                            packages.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
                        format!("{version} (required by {packages})")
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(Report::msg(format!(
                    "linked packages require conflicting versions for session library '{name}': \
                     {details}",
                )));
            }

            Ok(versions.keys().next().cloned().expect("non-empty map should contain a version"))
        })
        .collect()
}

/// Register package inputs in an order accepted by the in-memory registry.
fn register_link_packages(
    store: &mut InMemoryPackageRegistry,
    dependencies: &mut BTreeMap<Arc<str>, ProjectDependency>,
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
) -> Result<(), Report> {
    let mut pending = BTreeMap::<PackageId, Arc<MastPackage>>::default();
    for package in link_packages.values() {
        pending.insert(package.name.clone(), package.clone());
    }

    while !pending.is_empty() {
        let mut published = Vec::default();

        // The registry validates dependency availability during publication, so we only publish
        // packages whose exact dependency versions are already present and iterate to a fixed point.
        for (name, package) in pending.iter() {
            if !package_dependencies_available(store, package.as_ref()) {
                continue;
            }

            let version = publish_external_package(store, package.clone())?;
            push_project_dependency(dependencies, package.name.clone().into_inner(), version)?;
            published.push(name.clone());
        }

        if published.is_empty() {
            return Err(unresolved_external_packages_report(&pending, store));
        }

        for name in published {
            pending.remove(&name);
        }
    }

    Ok(())
}

/// Return true when all exact dependency versions of `package` are already available.
fn package_dependencies_available(store: &InMemoryPackageRegistry, package: &MastPackage) -> bool {
    package.manifest.dependencies().all(|dependency| {
        let version = RegistryVersion::new(dependency.version.clone(), dependency.digest);
        store.is_version_available(&dependency.name, &version)
    })
}

/// Publish `package`, allowing idempotent reuse of an identical exact version.
fn publish_external_package(
    store: &mut InMemoryPackageRegistry,
    package: Arc<MastPackage>,
) -> Result<RegistryVersion, Report> {
    let version = RegistryVersion::new(package.version.clone(), package.digest());

    if let Some(existing) = store.get_by_semver(&package.name, &package.version) {
        return if existing.version() == &version {
            Ok(version)
        } else {
            Err(Report::msg(format!(
                "package '{}' version '{}' is already registered",
                package.name, package.version
            )))
        };
    }

    store.publish_package(package).map_err(|error| Report::msg(error.to_string()))
}

/// Build a diagnostic for the remaining unpublished external packages.
fn unresolved_external_packages_report(
    pending: &BTreeMap<PackageId, Arc<MastPackage>>,
    store: &InMemoryPackageRegistry,
) -> Report {
    let details = pending
        .values()
        .map(|package| {
            let blocking = package
                .manifest
                .dependencies()
                .filter_map(|dependency| {
                    let version =
                        RegistryVersion::new(dependency.version.clone(), dependency.digest);
                    if store.is_version_available(&dependency.name, &version) {
                        return None;
                    }

                    let mut reason = String::from("missing");
                    if pending.contains_key(&dependency.name) {
                        reason = String::from("pending");
                    }

                    Some(format!("{}@{} ({reason})", dependency.name, version))
                })
                .collect::<Vec<_>>();

            if blocking.is_empty() {
                format!("'{}' could not be published", package.name)
            } else {
                format!("'{}' is blocked by {}", package.name, blocking.join(", "))
            }
        })
        .collect::<Vec<_>>();

    Report::msg(format!(
        "unable to register external packages in dependency order: {}",
        details.join("; ")
    ))
}

/// Append a project dependency while preserving the existing exact resolution.
fn push_project_dependency(
    dependencies: &mut BTreeMap<Arc<str>, ProjectDependency>,
    name: Arc<str>,
    version: RegistryVersion,
) -> Result<(), Report> {
    let dependency = ProjectDependency::new(
        Span::unknown(name.clone()),
        DependencyVersionScheme::Registry(VersionRequirement::Exact(version)),
        Linkage::Dynamic,
    );

    match dependencies.get(name.as_ref()) {
        Some(existing) if existing == &dependency => Ok(()),
        Some(_) => Err(Report::msg(format!(
            "conflicting external dependency registration for '{name}'",
        ))),
        None => {
            dependencies.insert(name, dependency);
            Ok(())
        }
    }
}

/// Build the synthetic root target used to assemble compiler-generated MASM.
fn build_root_target(component: &MasmComponent) -> Result<Target, Report> {
    if component.entrypoint.is_some() {
        return Ok(Target::executable(component.id.to_string()));
    }

    let root = component
        .modules
        .first()
        .ok_or_else(|| Report::msg("component does not contain any MASM modules"))?;
    Ok(Target::library(root.path()))
}

/// Prepare project source inputs while preserving the legacy assembler behavior for intrinsics.
fn prepare_sources(
    component: &MasmComponent,
    assembler: &mut Assembler,
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
    emit_test_harness: bool,
    source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
) -> Result<ProjectSourceInputs, Report> {
    let external_modules = external_module_paths(link_libraries, link_packages);

    // Intrinsics must be linked into the assembler context directly so they do not become part of
    // the assembled package surface.
    let mut support = Vec::with_capacity(component.modules.len());
    for module in component.modules.iter() {
        if external_modules.contains(module.path()) {
            log::warn!(
                target: "assembly",
                "module '{}' is already registered with the assembler as dependency module, \
                 skipping",
                module.path()
            );
            continue;
        }

        if is_intrinsics_module(module) {
            log::debug!(
                target: "assembly",
                "adding intrinsics '{}' to assembler",
                module.path()
            );
            assembler.compile_and_statically_link(module.clone())?;
            continue;
        }

        support.push(Box::new(Arc::unwrap_or_clone(module.clone())));
    }

    if let Some(entrypoint) = component.entrypoint.as_ref() {
        let root = Box::new(Arc::unwrap_or_clone(component.generate_main(
            entrypoint,
            emit_test_harness,
            source_manager,
        )?));
        return Ok(ProjectSourceInputs { root, support });
    }

    let mut modules = support.into_iter();
    let root = modules
        .next()
        .ok_or_else(|| Report::msg("component does not contain any user-defined MASM modules"))?;
    Ok(ProjectSourceInputs {
        root,
        support: modules.collect(),
    })
}

/// Attach serialized account component metadata to the assembled package.
fn attach_account_component_metadata(
    package: &mut Package,
    account_component_metadata_bytes: Option<&[u8]>,
) {
    if let Some(bytes) = account_component_metadata_bytes {
        package
            .sections
            .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, bytes.to_vec()));
    }
}

/// Rewrite library exports to preserve Wasm component-model interface names.
fn normalize_library_exports(package: &mut Package) -> Result<(), Report> {
    if !package.kind.is_library() {
        return Ok(());
    }

    let dependencies = package.manifest.dependencies().cloned().collect::<Vec<_>>();
    let exports = recover_wasm_cm_interfaces(package.mast.as_ref());
    package.mast = Arc::new(Library::new(package.mast.mast_forest().clone(), exports)?);
    package.manifest = PackageManifest::from_library(package.mast.as_ref())
        .with_dependencies(dependencies)
        .map_err(|error| Report::msg(error.to_string()))?;
    Ok(())
}

/// Extend the package advice map with the component's rodata segments.
fn extend_rodata_advice_map(package: &mut Package, rodata: &[Rodata]) {
    if rodata.is_empty() {
        return;
    }

    let advice_map = rodata.iter().map(|segment| (segment.digest, segment.to_elements())).collect();
    Arc::make_mut(&mut package.mast).extend_advice_map(advice_map);
}

/// Try to recognize Wasm CM interfaces and transform those exports to have Wasm interface encoded
/// as module name.
///
/// Temporary workaround for:
///
/// 1. Temporary exporting multiple interfaces from the same(Wasm core) module (an interface is
///    encoded in the function name)
///
/// 2. Assembler using the current module name to generate exports.
///
fn recover_wasm_cm_interfaces(lib: &Library) -> BTreeMap<Arc<Path>, LibraryExport> {
    let mut exports = BTreeMap::new();
    for export in lib.exports() {
        let path = export.path();
        let Some(proc_export) = export.as_procedure() else {
            exports.insert(path, export.clone());
            continue;
        };

        let Some(module) = proc_export.path.parent() else {
            exports.insert(path, export.clone());
            continue;
        };
        let Some(proc_name) = proc_export.path.last() else {
            exports.insert(path, export.clone());
            continue;
        };

        if INTRINSICS_MODULE_NAMES.contains(&module.as_str()) || proc_name.starts_with("cabi") {
            // Preserve intrinsics modules and internal Wasm CM `cabi_*` functions
            exports.insert(path, export.clone());
            continue;
        }

        if let Some((component, interface)) = proc_name.rsplit_once('/') {
            // Wasm CM interface
            let (interface, function) =
                interface.rsplit_once('#').expect("invalid wasm component model identifier");

            // Derive a new module path in which the Wasm CM interface name is encoded as part of
            // the module path, rather than being encoded in the procedure name.
            let mut module_path = component.to_string();
            module_path.push_str("::");
            module_path.push_str(interface);
            let module_path = masm::LibraryPath::new(&module_path)
                .expect("invalid wasm component model identifier");

            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
                Span::unknown(Arc::from(function)),
            ));
            let qualified = masm::QualifiedProcedureName::new(module_path.as_path(), name);
            let qualified = qualified.into_inner();

            let mut new_export = ProcedureExport::new(proc_export.node, qualified.clone())
                .with_attributes(proc_export.attributes.clone());
            if let Some(signature) = proc_export.signature.clone() {
                new_export = new_export.with_signature(signature);
            }

            exports.insert(qualified, LibraryExport::Procedure(new_export));
        } else {
            // Non-Wasm CM interface, preserve as is
            exports.insert(path, export.clone());
        }
    }
    exports
}

/// Return the set of modules already supplied by external dependencies.
fn external_module_paths(
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
) -> BTreeSet<miden_assembly::PathBuf> {
    let mut paths = BTreeSet::default();
    for library in link_libraries {
        for module in library.module_infos() {
            paths.insert(module.path().to_path_buf());
        }
    }
    for package in link_packages.values() {
        for module in package.mast.module_infos() {
            paths.insert(module.path().to_path_buf());
        }
    }
    paths
}

/// Return true when the module belongs to the compiler's intrinsics namespace.
fn is_intrinsics_module(module: &miden_assembly::ast::Module) -> bool {
    module.path().as_str().trim_start_matches("::").starts_with("intrinsics")
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeMap, sync::Arc, vec};

    use miden_mast_package::{Dependency, PackageId};
    use midenc_session::{LinkLibrary, STDLIB};

    use super::*;

    /// Build a synthetic library package backed by the standard library MAST.
    fn test_package(
        name: &str,
        version: Version,
        dependencies: impl IntoIterator<Item = Dependency>,
    ) -> Arc<MastPackage> {
        Arc::from(MastPackage::from_library(
            PackageId::from(name),
            version,
            TargetType::Library,
            STDLIB.clone(),
            dependencies,
        ))
    }

    #[test]
    fn register_external_dependencies_reuses_link_package_library_versions() {
        let stdlib = STDLIB.clone();
        let std_digest = *stdlib.digest();
        let dependent = test_package(
            "dep",
            Version::new(1, 0, 0),
            [Dependency {
                name: PackageId::from("std"),
                kind: TargetType::Library,
                version: Version::new(1, 2, 3),
                digest: std_digest,
            }],
        );
        let link_packages = BTreeMap::from([(Symbol::intern("dep"), dependent)]);

        let mut store = InMemoryPackageRegistry::default();
        let link_library_specs = vec![LinkLibrary::std()];
        let dependencies = register_external_dependencies(
            &mut store,
            &link_library_specs,
            &[stdlib],
            &link_packages,
        )
        .expect("external dependency registration should succeed");

        let std_version = RegistryVersion::new(Version::new(1, 2, 3), std_digest);
        assert!(store.is_version_available(&PackageId::from("std"), &std_version));
        assert!(
            !store.is_semver_available(&PackageId::from("std"), &Version::new(0, 0, 0)),
            "expected the inferred version to replace the synthetic 0.0.0 fallback"
        );

        let std_dependency = dependencies
            .into_iter()
            .find(|dependency| dependency.name().as_ref() == "std")
            .expect("root project should register the std dependency");
        assert_eq!(std_dependency.required_version(), VersionRequirement::Exact(std_version));
    }

    #[test]
    fn register_external_dependencies_rejects_conflicting_library_versions() {
        let stdlib = STDLIB.clone();
        let std_digest = *stdlib.digest();
        let dep_a = test_package(
            "dep-a",
            Version::new(1, 0, 0),
            [Dependency {
                name: PackageId::from("std"),
                kind: TargetType::Library,
                version: Version::new(1, 2, 3),
                digest: std_digest,
            }],
        );
        let dep_b = test_package(
            "dep-b",
            Version::new(1, 0, 0),
            [Dependency {
                name: PackageId::from("std"),
                kind: TargetType::Library,
                version: Version::new(2, 0, 0),
                digest: std_digest,
            }],
        );
        let link_packages =
            BTreeMap::from([(Symbol::intern("dep-a"), dep_a), (Symbol::intern("dep-b"), dep_b)]);

        let mut store = InMemoryPackageRegistry::default();
        let link_library_specs = vec![LinkLibrary::std()];
        let error = register_external_dependencies(
            &mut store,
            &link_library_specs,
            &[stdlib],
            &link_packages,
        )
        .expect_err("conflicting link-library versions should fail");

        assert!(
            error.to_string().contains("conflicting versions for session library 'std'"),
            "unexpected error: {error}"
        );
    }
}
