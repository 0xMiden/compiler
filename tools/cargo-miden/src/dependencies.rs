use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use anyhow::{anyhow, bail, Context, Result};
use cargo_component::{config::CargoArguments, PackageComponentMetadata};
use cargo_metadata::camino;
use serde::Deserialize;

use crate::{BuildOutput, OutputType}; // Import run for recursive calls

/// Defines dependency (the rhs of the dependency `"ns:package" = { path = "..." }` pair)
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct MidenDependencyInfo {
    /// Local path to the cargo-miden project that produces Miden package or Miden package `.masp` file
    path: PathBuf,
}

type MidenDependencies = HashMap<String, MidenDependencyInfo>;

#[derive(Deserialize, Debug)]
struct TomlMidenMetadata {
    #[serde(default)]
    dependencies: Option<MidenDependencies>,
}

#[derive(Deserialize, Debug)]
struct TomlMetadata {
    #[serde(default)]
    miden: Option<TomlMidenMetadata>,
}

#[derive(Deserialize, Debug)]
struct TomlPackage {
    metadata: Option<TomlMetadata>,
}

#[derive(Deserialize, Debug)]
struct TomlManifest {
    package: Option<TomlPackage>,
}

/// Processes Miden dependencies defined in `[package.metadata.miden.dependencies]`
/// for the given packages.
///
/// This involves finding dependency projects, recursively building them if necessary,
/// and collecting the paths to the resulting `.masp` package artifacts.
pub fn process_miden_dependencies(
    packages: &[PackageComponentMetadata],
    cargo_args: &CargoArguments,
) -> Result<Vec<PathBuf>> {
    let mut all_dependency_library_paths: Vec<PathBuf> = Vec::new();
    let mut processed_dep_paths: HashSet<PathBuf> = HashSet::new(); // Avoid redundant builds/checks

    log::debug!("Processing Miden dependencies...");

    for package_info in packages {
        let manifest_path = &package_info.package.manifest_path;
        let manifest_dir = manifest_path.parent().with_context(|| {
            format!("Failed to get parent directory for manifest: {}", manifest_path)
        })?;

        let toml_str = fs::read_to_string(manifest_path.as_std_path())
            .with_context(|| format!("Failed to read manifest from: {}", manifest_path))?;
        let toml_manifest = toml::from_str::<TomlManifest>(&toml_str)?;
        let dependencies = toml_manifest
            .package
            .and_then(|p| p.metadata)
            .and_then(|m| m.miden)
            .and_then(|mid| mid.dependencies)
            .unwrap_or_default();

        if !dependencies.is_empty() {
            log::debug!(
                "  Processing dependencies for package '{}' defined in {}...",
                package_info.package.name,
                manifest_path
            );

            for (dep_name, dep_info) in &dependencies {
                let relative_path = &dep_info.path;
                // Resolve relative to the *dependency declaring* manifest's directory
                // Convert relative PathBuf with Utf8Path
                let utf8_relative_path =
                    match camino::Utf8PathBuf::from_path_buf(relative_path.clone()) {
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
                        format!("resolving dependency path for '{}' ({})", dep_name, dep_path)
                    })?;

                // Skip if we've already processed this exact path
                if processed_dep_paths.contains(&absolute_dep_path) {
                    // Check if the artifact path is already collected, add if not
                    if all_dependency_library_paths.contains(&absolute_dep_path) {
                        // Already in the list, nothing to do.
                    } else {
                        // If it was processed but is a valid .masp file, ensure it's in the final list
                        if absolute_dep_path.is_file()
                            && absolute_dep_path.extension().is_some_and(|ext| ext == "masp")
                        {
                            all_dependency_library_paths.push(absolute_dep_path.clone());
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
                        if !all_dependency_library_paths.iter().any(|p| p == &absolute_dep_path) {
                            all_dependency_library_paths.push(absolute_dep_path.clone());
                        }
                        // Mark as processed
                        processed_dep_paths.insert(absolute_dep_path);
                    } else {
                        bail!(
                            "Dependency path for '{}' points to a file, but it's not a .masp \
                             file: {}",
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
                        if cargo_args.release {
                            dep_build_args.push("--release".to_string());
                        }
                        // Dependencies should always be built as libraries
                        dep_build_args.push("--lib".to_string());

                        // We expect dependencies to *always* produce Masm libraries (.masp)
                        let command_output =
                            crate::run(dep_build_args.into_iter(), OutputType::Masm)
                                .with_context(|| {
                                    format!(
                                        "building dependency '{}' at {}",
                                        dep_name,
                                        absolute_dep_path.display()
                                    )
                                })?
                                .ok_or(anyhow!(
                                    "`cargo miden build` does not produced any output"
                                ))?;

                        let build_output = command_output.unwrap_build_output();

                        let artifact_path = match build_output {
                            BuildOutput::Masm { artifact_path } => artifact_path,
                            // We specifically requested Masm, so Wasm output would be an error.
                            BuildOutput::Wasm { artifact_path, .. } => {
                                bail!(
                                    "Dependency build for '{}' unexpectedly produced WASM output \
                                     at {}. Expected MASM (.masp)",
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
                            if !all_dependency_library_paths.iter().any(|p| p == &artifact_path) {
                                all_dependency_library_paths.push(artifact_path);
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
                            "Dependency path for '{}' points to a directory, but it does not \
                             contain a Cargo.toml file: {}",
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
        }
    }
    log::debug!(
        "Finished processing Miden dependencies. Packages to link: [{}]",
        all_dependency_library_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(all_dependency_library_paths)
}
