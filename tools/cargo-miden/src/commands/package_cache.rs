use anyhow::{Result, anyhow};
use clap::Args;

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
        let (session, _metadata_out_dir) = super::session_from_args(&self.args)?;

        let cache_dir = session.filesystem_package_cache_dir().ok_or_else(|| {
            anyhow!(
                "the current directory does not locate a Miden project, so it has no package cache"
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
