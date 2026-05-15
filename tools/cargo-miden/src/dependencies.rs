use std::{collections::BTreeSet, sync::Arc};

use anyhow::{Context, Result, anyhow, bail};
use miden_package_registry::PackageStore;
use midenc_session::{
    SourceManager,
    diagnostics::{PrintDiagnostic, serde::Deserializable},
    miden_project,
};

use crate::{BuildOutput, OutputType, commands::CargoOptions};

pub fn load_cargo_based_source_dependencies(
    package: &miden_project::Package,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
    source_manager: &dyn SourceManager,
) -> Result<()> {
    log::debug!(target: "cargo-miden", "processing Cargo-based source dependencies for {}@{}", package.name(), package.version());

    let package_id = package.name().into_inner();
    let Some(node) = dependency_graph.get(&package_id) else {
        log::debug!(target: "cargo-miden", "{package_id} has no dependencies");
        return Ok(());
    };

    let mut visited = BTreeSet::new();
    for dep in node.dependencies.iter() {
        if !visited.insert(dep.dependency.clone()) {
            continue;
        }
        let Some(dependency) = dependency_graph.get(&dep.dependency) else {
            bail!("unable to resolve dependency on '{}'", &dep.dependency);
        };
        match &dependency.provenance {
            miden_project::ProjectDependencyNodeProvenance::Preassembled { path, selected } => {
                log::debug!(target: "cargo-miden", "resolved dependency {}@{selected} to preassembled package at {}", &dependency.name, path.display());
            }
            miden_project::ProjectDependencyNodeProvenance::Registry {
                requirement,
                selected: _,
            } => {
                log::debug!(target: "cargo-miden", "expecting dependency {} {requirement} to be in registry", &dependency.name);
            }
            miden_project::ProjectDependencyNodeProvenance::Source(proj) => match proj {
                miden_project::ProjectSource::Real { manifest_path, .. } => {
                    let project = miden_project::Project::load(manifest_path, source_manager)
                        .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;
                    let project_package = project.package();
                    let target = project_package.library_target().ok_or_else(|| {
                        anyhow!(
                            "invalid dependency '{}': no a library target",
                            project_package.name()
                        )
                    })?;
                    if target.path.is_none() {
                        cargo_build(
                            &project_package,
                            target.inner(),
                            manifest_path,
                            registry,
                            options,
                            cargo_opts,
                        )?;
                    }
                }
                miden_project::ProjectSource::Virtual { .. } => {
                    unreachable!("virtual manifests are only possible at the top-level")
                }
            },
        }
    }

    Ok(())
}

fn cargo_build(
    package: &miden_project::Package,
    target: &miden_project::Target,
    manifest_path: &std::path::Path,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    options: &midenc_session::Options,
    cargo_opts: &CargoOptions,
) -> Result<()> {
    let mut nested_options = Box::new(options.clone());
    nested_options.manifest_path = Some(manifest_path.to_path_buf());
    nested_options.name = Some(target.name.to_string());
    nested_options.target_type = Some(target.ty);
    // Inherit release/debug profile from parent build
    if cargo_opts.release {
        nested_options.profile = "release".to_string();
    }

    // We expect dependencies to *always* produce packages (.masp)
    let command_output =
        crate::BuildCommand::exec_from_options(nested_options, Some(registry), OutputType::Masm)
            .with_context(|| {
                format!("building dependency '{}' at {}", package.name(), manifest_path.display())
            })?
            .ok_or(anyhow!("`cargo miden build` does not produced any output"))?;

    let mut build_output = command_output.unwrap_build_output();

    if build_output.len() > 1 {
        bail!(
            "expected '{}' to produce a single artifact - got {build_output:#?}",
            package.name()
        );
    }

    let artifact_path = match build_output
        .pop()
        .ok_or_else(|| anyhow!("expected '{}' to produce an artifact", package.name()))?
    {
        BuildOutput::Masm { artifact_path } => artifact_path,
        // We specifically requested Masm, so Wasm output would be an error.
        BuildOutput::Wasm { artifact_path, .. } => {
            bail!(
                "Dependency build for '{}' unexpectedly produced WASM output at {}. Expected MASM \
                 (.masp)",
                package.name(),
                artifact_path.display()
            );
        }
    };

    log::debug!(
        "    - Dependency '{}' built successfully. Output: {}",
        package.name(),
        artifact_path.display()
    );

    let bytes = std::fs::read(&artifact_path).map_err(|err| {
        anyhow!(
            "failed to read package for '{}' from {}: {err}",
            package.name(),
            artifact_path.display()
        )
    })?;
    let loaded = Arc::new(miden_mast_package::Package::read_from_bytes(&bytes).map_err(|err| {
        anyhow!("invalid package for '{}' at {}: {err}", package.name(), artifact_path.display())
    })?);

    registry
        .publish_package(loaded)
        .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;

    Ok(())
}
