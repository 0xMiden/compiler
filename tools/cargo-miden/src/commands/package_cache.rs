use std::rc::Rc;

use anyhow::{Result, anyhow};
use clap::Args;
use midenc_compile::Compiler;
use midenc_session::{InputFile, diagnostics::PrintDiagnostic};

/// Command-line arguments accepted by `cargo miden package-cache`.
///
/// All arguments are parsed by the `midenc` compiler's argument parser, exactly like
/// `cargo miden build`. The printed cache directory therefore matches the directory a build
/// with the same arguments uses.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct PackageCacheCommand {
    /// Arguments parsed by midenc (includes cargo-compatible options).
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl PackageCacheCommand {
    /// Prints the package-cache location and the build-script inputs of the current project.
    ///
    /// The output is line oriented, one `key=value` item per line:
    /// - `cache-dir=<path>` — the fingerprinted package-cache directory of this project;
    /// - `source-dependencies=<count>` — direct dependencies compiled into the cache;
    /// - `watch=<path>` — an input a contract build script must watch (repeated). The list
    ///   ends with this `cargo-miden` binary itself, so a compiler update re-runs the build
    ///   script and rotates the emitted cache path.
    pub fn exec(self) -> Result<()> {
        let cwd = std::env::current_dir()?;
        let compiler_opts =
            Compiler::try_parse_from(cwd.clone(), &self.args).unwrap_or_else(|err| err.exit());

        let manifest_path = match compiler_opts.manifest_path.as_deref() {
            Some(manifest_path) => manifest_path.to_path_buf(),
            None => cwd.join("Cargo.toml"),
        };
        let input = InputFile::from_path(&manifest_path)
            .map_err(|err| anyhow!("failed to read '{}': {err}", manifest_path.display()))?;
        let session = Rc::new(
            compiler_opts
                .into_session(input, None, None)
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?,
        );

        let cache_dir = session.filesystem_package_cache_dir().ok_or_else(|| {
            anyhow!(
                "'{}' does not locate a Miden project, so it has no package cache",
                manifest_path.display()
            )
        })?;
        let inputs = session.package_cache_build_inputs().unwrap_or_default();

        println!("cache-dir={}", cache_dir.display());
        println!("source-dependencies={}", inputs.source_dependency_count);
        for path in &inputs.watch_paths {
            println!("watch={}", path.display());
        }
        if let Ok(current_exe) = std::env::current_exe() {
            println!("watch={}", current_exe.display());
        }
        Ok(())
    }
}
