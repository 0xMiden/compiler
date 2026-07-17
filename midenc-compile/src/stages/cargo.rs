use midenc_session::InputFile;
#[cfg(feature = "std")]
use midenc_session::InputType;

use super::*;

/// Runs `cargo build` to produce a Wasm artifact
pub struct CargoBuildStage;

impl Stage for CargoBuildStage {
    type Input = InputFile;
    type Output = InputFile;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        match input.file {
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            #[cfg(feature = "std")]
            InputType::Real(path) => {
                let options = context.session().options.clone();
                let mut outputs = support::cargo_build(&path, options, None)?;
                Ok(outputs.pop().unwrap())
            }
            InputType::Stdin { .. } => {
                todo!()
            }
        }
    }
}

#[cfg(feature = "std")]
pub mod support {
    use std::{
        boxed::Box,
        path::{Path, PathBuf},
        string::{String, ToString},
        sync::Arc,
        vec::Vec,
    };

    use miden_assembly::{DefaultSourceManager, SourceManager};
    use miden_mast_package::TargetType;
    use midenc_hir::{
        Report,
        diagnostics::{IntoDiagnostic, SourceManagerExt},
    };
    use midenc_session::{
        InputFile, OptLevel, RemapPathPrefix, miden_project, registry::HybridPackageRegistry,
    };
    use tempfile::TempDir;

    use crate::{CompilerResult, cargo::CargoOptions};

    /// Executes a Cargo-based build with the provided compiler options and package registry
    pub fn cargo_build(
        manifest_path: &Path,
        mut compiler_opts: Box<midenc_session::Options>,
        registry: Option<&mut HybridPackageRegistry>,
    ) -> CompilerResult<Vec<InputFile>> {
        // Extract cargo-specific options from parsed Compiler struct
        compiler_opts.manifest_path = Some(manifest_path.to_path_buf());
        let cargo_opts = CargoOptions::from_compiler(&compiler_opts)?;

        let cwd = compiler_opts.current_dir.clone();
        let (project_dir, project_manifest_path) = match compiler_opts.manifest_path.as_mut() {
            Some(manifest_path)
                if manifest_path
                    .file_stem()
                    .is_some_and(|stem| stem.eq_ignore_ascii_case("miden-project")) =>
            {
                let manifest_path = manifest_path.clone();
                let cwd = manifest_path.parent().map(|dir| dir.to_path_buf()).unwrap_or(cwd);
                (cwd, manifest_path)
            }
            Some(cargo_manifest_path) => {
                let Some(project_dir) = cargo_manifest_path.parent() else {
                    return Err(Report::msg(
                        "unable to locate project manifest: --manifest-path specifies a path with \
                         no parent",
                    ));
                };
                let manifest_path = project_dir.join("miden-project.toml");
                let project_dir = project_dir.to_path_buf();
                *cargo_manifest_path = manifest_path.clone();
                (project_dir, manifest_path)
            }
            None => {
                let Ok(cwd) = std::env::current_dir() else {
                    return Err(Report::msg(
                        "unable to locate project manifest: current working directory is \
                         unavailable",
                    ));
                };
                let manifest_path = cwd.join("miden-project.toml");
                compiler_opts.manifest_path = Some(manifest_path.clone());
                (cwd, manifest_path)
            }
        };

        let source_manager =
            Arc::new(DefaultSourceManager::default()) as Arc<dyn SourceManager + Send + Sync>;
        let outputs = if compiler_opts.workspace {
            let source = source_manager.load_file(&project_manifest_path).into_diagnostic()?;
            let workspace = miden_project::Workspace::load(source, &source_manager)?;
            build_workspace(
                &workspace,
                project_dir,
                compiler_opts,
                &cargo_opts,
                None,
                source_manager,
            )?
        } else {
            // Check if the project manifest is a workspace manifest - this requires us to build
            // the entire workspace, rather than a single project. However, we only support this
            // if `--package` was given, as otherwise there is no way for us to select a package
            // to build
            let source = source_manager.load_file(&project_manifest_path).into_diagnostic()?;
            if let miden_project::ast::MidenProject::Workspace(_) =
                miden_project::ast::MidenProject::parse(source.clone())?
            {
                if compiler_opts.packages.is_empty() {
                    return Err(Report::msg(
                        "a workspace manifest was provided, but --workspace was not specified",
                    ));
                }
                let workspace = miden_project::Workspace::load(source, &source_manager)
                    .map(Arc::<miden_project::Workspace>::from)?;
                let mut outputs = Vec::new();
                for requested in compiler_opts.packages.iter() {
                    let Some(package) = workspace.get_member_by_name(requested) else {
                        return Err(Report::msg(format!(
                            "requested pacakge '{requested}' is not a valid workspace member"
                        )));
                    };
                    let mut compiler_opts = compiler_opts.clone();
                    let project = miden_project::Project::WorkspacePackage {
                        package,
                        workspace: workspace.clone(),
                    };
                    modify_midenc_options_for_target(&project, &mut compiler_opts)?;
                    let output = build_project(
                        project,
                        &compiler_opts,
                        &cargo_opts,
                        None,
                        Arc::clone(&source_manager),
                    )?;
                    outputs.push(output);
                }
                outputs
            } else {
                let project =
                    miden_project::Project::load(&project_manifest_path, &source_manager)?;
                let package_name = project.package().name().into_inner();
                if compiler_opts.packages.len() > 1 {
                    return Err(Report::msg(format!(
                        "multiple packages were requested via --package, but the project manifest \
                         only defines a single package ({package_name})"
                    )));
                } else if !compiler_opts.packages.is_empty()
                    && !compiler_opts.packages.iter().any(|p| &*package_name == p)
                {
                    return Err(Report::msg(format!(
                        "the provided project manifest defines a package ({}) that differs from \
                         the one requested via --package ({package_name})",
                        &compiler_opts.packages[0]
                    )));
                }
                modify_midenc_options_for_target(&project, &mut compiler_opts)?;
                let output =
                    build_project(project, &compiler_opts, &cargo_opts, registry, source_manager)?;
                vec![output]
            }
        };

        Ok(outputs)
    }

    fn build_workspace(
        workspace: &miden_project::Workspace,
        _cwd: PathBuf,
        _compiler_opts: Box<midenc_session::Options>,
        _cargo_opts: &CargoOptions,
        _registry: Option<&mut HybridPackageRegistry>,
        _source_manager: Arc<dyn SourceManager>,
    ) -> CompilerResult<Vec<InputFile>> {
        //let metadata = load_metadata(cargo_opts.manifest_path.as_deref())?;

        //let mut packages =
        //   load_component_metadata(&metadata, cargo_opts.packages.iter(), cargo_opts.workspace)?;

        if workspace.members().is_empty() {
            return Err(Report::msg(format!(
                "workspace ({}) contains no members",
                workspace.manifest_path().unwrap_or(Path::new("virtual")).display()
            )));
        }

        todo!("build a dependency graph of the workspace members and build each package")
    }

    fn build_project(
        project: miden_project::Project,
        compiler_opts: &midenc_session::Options,
        cargo_opts: &CargoOptions,
        registry: Option<&mut HybridPackageRegistry>,
        source_manager: Arc<dyn SourceManager + Send + Sync>,
    ) -> CompilerResult<InputFile> {
        let package = project.package();

        let tmp = TempDir::new()
            .map_err(|err| Report::msg(format!("could not create temporary directory: {err}")))?;
        let mut default_registry =
            midenc_session::registry::HybridPackageRegistry::new(compiler_opts)?;
        let registry = registry.unwrap_or(&mut default_registry);
        let dependency_graph = miden_project::ProjectDependencyGraphBuilder::new(&*registry)
            .with_source_manager(source_manager.clone())
            .with_git_cache_root(
                compiler_opts
                    .midenup_home
                    .as_deref()
                    .unwrap_or(tmp.path())
                    .join("git")
                    .join("checkouts"),
            );
        let dependency_graph = dependency_graph.build(package.clone())?;

        crate::cargo::load_cargo_based_source_dependencies(
            &package,
            &dependency_graph,
            registry,
            compiler_opts,
            cargo_opts,
            source_manager,
        )?;

        let rustup_toolchain = crate::rust::rustup_toolchain();
        let cargo_build_args = build_cargo_args(cargo_opts, compiler_opts.optimize);

        // Enable memcopy and 128-bit arithmetic ops
        let mut extra_rust_flags = String::from("-C target-feature=+bulk-memory,+wide-arithmetic");
        // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
        // cfg-based dependency selection.
        extra_rust_flags.push_str(" --cfg miden");
        // Enable errors on missing stub functions
        extra_rust_flags.push_str(" -C link-args=--fatal-warnings");
        // Remove the source file paths in the data segment for panics
        // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
        extra_rust_flags.push_str(" -Zlocation-detail=none");
        // Build with panic=immediate-abort
        extra_rust_flags.push_str(" -Zunstable-options");
        extra_rust_flags.push_str(" -Cpanic=immediate-abort");
        if let Ok(inherited) = std::env::var("RUSTFLAGS")
            && !inherited.is_empty()
        {
            extra_rust_flags.push(' ');
            extra_rust_flags.push_str(&inherited);
        }
        if let Some(explicit) = compiler_opts.rustflags.as_deref() {
            extra_rust_flags.push(' ');
            extra_rust_flags.push_str(explicit);
        }

        let wasi = if compiler_opts.target_requires_protocol() {
            "wasip2"
        } else {
            "wasip1"
        };

        let mut wasm_outputs = run_cargo(
            wasi,
            rustup_toolchain.as_deref(),
            &cargo_build_args,
            [("RUSTFLAGS", extra_rust_flags)],
        )?;

        assert_eq!(wasm_outputs.len(), 1, "expected only one Wasm artifact");
        let wasm_output = wasm_outputs.pop().expect("expected at least one Wasm artifact");

        Ok(InputFile::from_path(wasm_output).unwrap())
    }

    /// Returns the Cargo profile value for a compiler optimization level.
    fn cargo_profile_opt_level(opt_level: OptLevel) -> &'static str {
        match opt_level {
            OptLevel::None | OptLevel::Balanced => "2",
            OptLevel::Basic => "1",
            OptLevel::Max => "3",
            OptLevel::Size => "\"s\"",
            OptLevel::SizeMin => "\"z\"",
        }
    }

    /// Builds the argument vector for the underlying `cargo build` invocation.
    fn build_cargo_args(cargo_opts: &CargoOptions, opt_level: OptLevel) -> Vec<String> {
        let mut args = vec!["build".to_string()];

        // Add build-std flags required for Miden compilation
        args.extend(
            [
                "-Z",
                "build-std=core,alloc,panic_abort",
                "-Z",
                "build-std-features=optimize_for_size",
            ]
            .into_iter()
            .map(|s| s.to_string()),
        );

        // Configure profile settings
        let cfg_pairs: Vec<(&str, &str)> = vec![
            ("profile.dev.panic", "\"abort\""),
            ("profile.dev.opt-level", "1"),
            ("profile.dev.overflow-checks", "false"),
            ("profile.dev.debug", "true"),
            ("profile.dev.debug-assertions", "false"),
            ("profile.release.opt-level", cargo_profile_opt_level(opt_level)),
            ("profile.release.lto", "true"),
            ("profile.release.codegen-units", "1"),
            ("profile.release.panic", "\"abort\""),
        ];

        for (key, value) in cfg_pairs {
            args.push("--config".to_string());
            args.push(format!("{key}={value}"));
        }

        // Forward cargo-specific options
        if cargo_opts.release {
            args.push("--release".to_string());
        }

        if let Some(ref manifest_path) = cargo_opts.manifest_path {
            args.push("--manifest-path".to_string());
            args.push(manifest_path.to_string_lossy().to_string());
        }

        if cargo_opts.workspace {
            args.push("--workspace".to_string());
        }

        for package in &cargo_opts.packages {
            args.push("--package".to_string());
            args.push(package.to_string());
        }

        args
    }

    fn run_cargo<E>(
        wasi: &str,
        toolchain: Option<&str>,
        spawn_args: &[String],
        env: E,
    ) -> CompilerResult<Vec<PathBuf>>
    where
        E: IntoIterator<Item = (&'static str, String)>,
    {
        let cargo_env = std::env::var("CARGO").map(PathBuf::from).ok();
        let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));

        let mut cargo = std::process::Command::new(cargo_path);
        cargo.envs(env);
        if cargo_env.is_none() {
            cargo.arg(format!("+{}", toolchain.unwrap_or("nightly")));
        }

        // This env var is used by crates (e.g. `miden-field`) to distinguish compiling to Wasm
        // for a "real" Wasm runtime vs compiling to Wasm as an intermediate artifact that
        // will be compiled to Miden VM code by `midenc`.
        cargo.env("MIDENC_TARGET_IS_MIDEN_VM", "1");
        cargo.args(spawn_args);

        // Handle the target for buildable commands
        crate::rust::install_wasm32_target(wasi, None)?;

        cargo.arg("--target").arg(format!("wasm32-{wasi}"));

        // It will output the message as json so we can extract the wasm files
        // that will be componentized
        cargo.arg("--message-format").arg("json-render-diagnostics");
        cargo.stdout(std::process::Stdio::piped());
        cargo.stderr(std::process::Stdio::inherit());

        let artifacts = crate::rust::spawn_cargo(cargo, cargo_path)?;

        let outputs: Vec<PathBuf> = artifacts
            .into_iter()
            .filter_map(|a| {
                let path: PathBuf = a.filenames.first().unwrap().clone().into();
                if path.to_str().unwrap().contains("wasm32-wasip") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        Ok(outputs)
    }

    /// Produces the `midenc` CLI flags implied by the detected target environment and project type.
    fn modify_midenc_options_for_target(
        project: &miden_project::Project,
        options: &mut midenc_session::Options,
    ) -> CompilerResult<()> {
        let project = project.package();

        // source paths in debug information.
        let package_source_dir = project.manifest_path().and_then(|path| path.parent());
        if options.debug != midenc_session::DebugInfo::None
            && let Some(source_dir) = package_source_dir
        {
            options.remap_path_prefixes.push(RemapPathPrefix {
                from: source_dir.to_path_buf().into_boxed_path(),
                to: None,
            });
        }

        let target_type = match options.target_type {
            None => project
                .library_target()
                .map(|target| target.ty)
                .unwrap_or(TargetType::Executable),
            Some(target_type) => target_type,
        };

        match target_type {
            TargetType::Executable => {
                let target = if let Some(target_name) = options.target.as_deref() {
                    project
                        .executable_targets()
                        .iter()
                        .find(|t| target_name == &**t.name.inner())
                        .ok_or_else(|| {
                        Report::msg(format!("no executable target name '{target_name}'"))
                    })?
                } else if project.executable_targets().len() == 1 {
                    &project.executable_targets()[0]
                } else {
                    return Err(Report::msg(
                        "ambiguous executable target selection: use --target to select a specific \
                         executable target",
                    ));
                };
                let masm_module_name = target.name.inner().replace('-', "_");
                options.entrypoint = Some(format!("{masm_module_name}::entrypoint"));
            }
            TargetType::Kernel => {
                return Err(Report::msg("kernels are not currently supported via midenc"));
            }
            TargetType::Library | TargetType::AccountComponent | TargetType::Note => (),
            TargetType::TransactionScript => {
                options.entrypoint = Some("miden:base/transaction-script@1.0.0::run".to_string());
            }
            _ => return Err(Report::msg("unsupported --target-type: {target_type}")),
        }
        Ok(())
    }
}
