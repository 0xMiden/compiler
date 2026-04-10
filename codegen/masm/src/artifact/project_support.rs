//! Project-assembler support for compiler-generated MASM components.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::ToString,
    sync::Arc,
    vec::Vec,
};

use miden_assembly::{Assembler, ProjectSourceInputs, ProjectTargetSelector};
use miden_mast_package::{Package as MastPackage, TargetType, Version};
use miden_package_registry::{
    PackageProvider, PackageRecord, PackageRegistry, PackageStore, PackageVersions,
    Version as RegistryVersion, VersionRequirement,
};
use miden_project::{
    Dependency as ProjectDependency, DependencyVersionScheme, Linkage, Package as ProjectPackage,
    Target,
};
use midenc_session::{
    Session,
    diagnostics::{Report, Span},
};

use super::{
    MasmComponent, Package, Symbol, attach_account_component_metadata, extend_rodata_advice_map,
    normalize_library_exports,
};

/// Assemble a MASM component through the VM project assembler.
pub(super) fn assemble(
    component: &MasmComponent,
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
    account_component_metadata_bytes: Option<&[u8]>,
    session: &Session,
) -> Result<Package, Report> {
    let mut store = VirtualPackageStore::default();
    let dependencies =
        register_external_dependencies(&mut store, link_libraries, link_packages, session)?;
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
        ProjectTargetSelector::Executable("main")
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
    store: &mut VirtualPackageStore,
    link_libraries: &[Arc<miden_assembly::Library>],
    link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
    session: &Session,
) -> Result<Vec<ProjectDependency>, Report> {
    if session.options.link_libraries.len() != link_libraries.len() {
        return Err(Report::msg(
            "loaded link libraries do not match the session link library configuration",
        ));
    }

    let mut dependencies = BTreeMap::default();
    for (link_lib, library) in session.options.link_libraries.iter().zip(link_libraries.iter()) {
        let package = Arc::from(MastPackage::from_library(
            link_lib.name.to_string().into(),
            Version::new(0, 0, 0),
            TargetType::Library,
            library.clone(),
            [],
        ));
        let version = store.add_package(package)?;
        push_project_dependency(&mut dependencies, Arc::from(link_lib.name.as_ref()), version)?;
    }
    for package in link_packages.values() {
        let version = store.add_package(package.clone())?;
        push_project_dependency(&mut dependencies, package.name.clone().into_inner(), version)?;
    }

    Ok(dependencies.into_values().collect())
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
        return Ok(Target::executable("main"));
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

/// A minimal in-memory package store for compiler-provided dependencies.
#[derive(Default)]
struct VirtualPackageStore {
    index: BTreeMap<miden_package_registry::PackageId, PackageVersions>,
    packages: BTreeMap<(miden_package_registry::PackageId, RegistryVersion), Arc<MastPackage>>,
}

impl VirtualPackageStore {
    /// Register a package and return its exact version.
    fn add_package(&mut self, package: Arc<MastPackage>) -> Result<RegistryVersion, Report> {
        let version = RegistryVersion::new(package.version.clone(), package.digest());
        let record = package_record(package.as_ref(), version.clone());

        if let Some(existing) = self
            .index
            .get(&package.name)
            .and_then(|versions| versions.get(&package.version))
        {
            if existing.version() != &version {
                return Err(Report::msg(format!(
                    "package '{}' version '{}' is already registered",
                    package.name, package.version
                )));
            }
        } else {
            self.index
                .entry(package.name.clone())
                .or_default()
                .insert(package.version.clone(), record);
        }

        self.packages.insert((package.name.clone(), version.clone()), package);
        Ok(version)
    }
}

impl PackageRegistry for VirtualPackageStore {
    fn available_versions(
        &self,
        package: &miden_package_registry::PackageId,
    ) -> Option<&PackageVersions> {
        self.index.get(package)
    }
}

impl PackageProvider for VirtualPackageStore {
    fn load_package(
        &self,
        package: &miden_package_registry::PackageId,
        version: &RegistryVersion,
    ) -> Result<Arc<MastPackage>, Report> {
        self.packages
            .get(&(package.clone(), version.clone()))
            .cloned()
            .ok_or_else(|| Report::msg(format!("missing package '{package}' at '{version}'")))
    }
}

impl PackageStore for VirtualPackageStore {
    type Error = Report;

    fn publish_package(
        &mut self,
        package: Arc<MastPackage>,
    ) -> Result<RegistryVersion, Self::Error> {
        self.add_package(package)
    }
}

/// Build the registry metadata record for a package.
fn package_record(package: &MastPackage, version: RegistryVersion) -> PackageRecord {
    let dependencies = package.manifest.dependencies().map(|dependency| {
        (
            dependency.name.clone(),
            VersionRequirement::Exact(RegistryVersion::new(
                dependency.version.clone(),
                dependency.digest,
            )),
        )
    });

    match package.description.as_deref() {
        Some(description) => {
            PackageRecord::new(version, dependencies).with_description(description)
        }
        None => PackageRecord::new(version, dependencies),
    }
}
