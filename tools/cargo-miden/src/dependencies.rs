use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{Package, camino};
use serde::Deserialize;

use crate::{BuildOutput, OutputType, commands::CargoOptions};

/// Defines dependency (the rhs of the dependency `"ns:package" = { path = "..." }` pair)
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct MidenDependencyInfo {
    /// Local path to the cargo-miden project that produces Miden package or Miden package `.masp` file
    path: PathBuf,
}

/// Representation for [package.metadata.miden]
#[derive(Deserialize, Debug, Default)]
struct MidenMetadata {
    #[serde(default)]
    dependencies: HashMap<String, MidenDependencyInfo>,
}

/// Processes Miden dependencies defined in `[package.metadata.miden.dependencies]`
/// for the given package.
///
/// This involves finding dependency projects, recursively building them if necessary,
/// and collecting the paths to the resulting `.masp` package artifacts.
pub fn process_miden_dependencies(
    package: &Package,
    cargo_opts: &CargoOptions,
) -> Result<Vec<PathBuf>> {
    let mut dependency_packages_paths: Vec<PathBuf> = Vec::new();
    // Avoid redundant builds/checks
    let mut processed_dep_paths: HashSet<PathBuf> = HashSet::new();

    log::debug!("Processing Miden dependencies for package '{}'...", package.name);

    // Get the manifest directory from the package
    let manifest_path = &package.manifest_path;
    let manifest_dir = manifest_path
        .parent()
        .with_context(|| format!("Failed to get parent directory for manifest: {manifest_path}"))?;

    // Extract Miden metadata using serde_json
    let miden_metadata: MidenMetadata = package
        .metadata
        .get("miden")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .context("Failed to deserialize [package.metadata.miden]")?
        .unwrap_or_default();

    let dependencies = miden_metadata.dependencies;

    if !dependencies.is_empty() {
        log::debug!("  Found dependencies defined in {manifest_path}");

        for (dep_name, dep_info) in &dependencies {
            let relative_path = &dep_info.path;
            // Resolve relative to the *dependency declaring* manifest's directory
            let utf8_relative_path = match camino::Utf8PathBuf::from_path_buf(relative_path.clone())
            {
                Ok(p) => p,
                Err(e) => {
                    bail!(
                        "Dependency path for '{}' is not valid UTF-8 ({}): {}",
                        dep_name,
                        relative_path.display(),
                        e.to_path_buf().display()
                    );
                }
            };
            let dep_path = manifest_dir.join(&utf8_relative_path);

            let absolute_dep_path =
                fs::canonicalize(dep_path.as_std_path()).with_context(|| {
                    format!("resolving dependency path for '{dep_name}' ({dep_path})")
                })?;

            // Skip if we've already processed this exact path
            if processed_dep_paths.contains(&absolute_dep_path) {
                // Check if the artifact path is already collected, add if not
                if dependency_packages_paths.contains(&absolute_dep_path) {
                    // Already in the list, nothing to do.
                } else {
                    // If it was processed but is a valid .masp file, ensure it's in the final list
                    if absolute_dep_path.is_file()
                        && absolute_dep_path.extension().is_some_and(|ext| ext == "masp")
                    {
                        dependency_packages_paths.push(absolute_dep_path.clone());
                    }
                }
                continue;
            }

            if absolute_dep_path.is_file() {
                // Look for a Miden package .masp file
                if absolute_dep_path.extension().is_some_and(|ext| ext == "masp") {
                    log::debug!(
                        "    - Found pre-compiled dependency '{}': {}",
                        dep_name,
                        absolute_dep_path.display()
                    );
                    if !dependency_packages_paths.iter().any(|p| p == &absolute_dep_path) {
                        dependency_packages_paths.push(absolute_dep_path.clone());
                    }
                    // Mark as processed
                    processed_dep_paths.insert(absolute_dep_path);
                } else {
                    bail!(
                        "Dependency path for '{}' points to a file, but it's not a .masp file: {}",
                        dep_name,
                        absolute_dep_path.display()
                    );
                }
            } else if absolute_dep_path.is_dir() {
                // Build a cargo project
                let dep_manifest_path = absolute_dep_path.join("Cargo.toml");
                if dep_manifest_path.is_file() {
                    log::debug!(
                        "    - Building Miden library dependency project '{}' at {}",
                        dep_name,
                        absolute_dep_path.display()
                    );

                    let mut dep_build_args = vec![
                        "cargo".to_string(),
                        "miden".to_string(),
                        "build".to_string(),
                        "--manifest-path".to_string(),
                        dep_manifest_path.to_string_lossy().to_string(),
                    ];
                    // Inherit release/debug profile from parent build
                    if cargo_opts.release {
                        dep_build_args.push("--release".to_string());
                    }

                    // We expect dependencies to *always* produce Masm libraries (.masp)
                    let command_output = crate::run(dep_build_args.into_iter(), OutputType::Masm)
                        .with_context(|| {
                            format!(
                                "building dependency '{}' at {}",
                                dep_name,
                                absolute_dep_path.display()
                            )
                        })?
                        .ok_or(anyhow!("`cargo miden build` does not produced any output"))?;

                    let build_output = command_output.unwrap_build_output();

                    let artifact_path = match build_output {
                        BuildOutput::Masm { artifact_path } => artifact_path,
                        // We specifically requested Masm, so Wasm output would be an error.
                        BuildOutput::Wasm { artifact_path, .. } => {
                            bail!(
                                "Dependency build for '{}' unexpectedly produced WASM output at \
                                 {}. Expected MASM (.masp)",
                                dep_name,
                                artifact_path.display()
                            );
                        }
                    };
                    log::debug!(
                        "    - Dependency '{}' built successfully. Output: {}",
                        dep_name,
                        artifact_path.display()
                    );
                    // Ensure it's a .masp file and add if unique
                    if artifact_path.extension().is_some_and(|ext| ext == "masp") {
                        if !dependency_packages_paths.iter().any(|p| p == &artifact_path) {
                            dependency_packages_paths.push(artifact_path);
                        } else {
                            bail!(
                                "Dependency build for '{}' produced a duplicate artifact: {}",
                                dep_name,
                                artifact_path.display()
                            );
                        }
                    } else {
                        bail!(
                            "Build output for dependency '{}' is not a .masp file: {}.",
                            dep_name,
                            artifact_path.display()
                        );
                    }
                    // Mark the *directory* as processed
                    processed_dep_paths.insert(absolute_dep_path);
                } else {
                    bail!(
                        "Dependency path for '{}' points to a directory, but it does not contain \
                         a Cargo.toml file: {}",
                        dep_name,
                        absolute_dep_path.display()
                    );
                }
            } else {
                bail!(
                    "Dependency path for '{}' does not exist or is not a file/directory: {}",
                    dep_name,
                    absolute_dep_path.display()
                );
            }
        }
    } else {
        log::debug!("  No Miden dependencies found for package '{}'.", package.name);
    }
    log::debug!(
        "Finished processing Miden dependencies. Packages to link: [{}]",
        dependency_packages_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(dependency_packages_paths)
}
