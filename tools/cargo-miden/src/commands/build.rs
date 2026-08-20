use std::{path::PathBuf, rc::Rc};

use anyhow::{Context as _, Result, anyhow};
use clap::Args;
use midenc_compile::CompiledArtifact;
use midenc_session::diagnostics::PrintDiagnostic;

/// Command-line arguments accepted by `cargo miden build`.
///
/// All arguments following `build` are parsed by the `midenc` compiler's argument parser.
/// Cargo-specific options (`--release`, `--manifest-path`, `--workspace`, `--package`)
/// are recognized and forwarded to the underlying `cargo build` invocation.
/// All other options are passed to `midenc` for compilation.
#[derive(Clone, Debug, Args)]
#[command(disable_version_flag = true, trailing_var_arg = true)]
pub struct BuildCommand {
    /// Arguments parsed by midenc (includes cargo-compatible options).
    #[arg(value_name = "ARG", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl BuildCommand {
    /// Executes `cargo miden build`, returning the built package's path — or `None` when the
    /// run was deliberately stopped short of a package by `--stop-after` (e.g. the contract
    /// build script staging a consumer's dependencies without compiling the consumer).
    pub fn exec(self) -> Result<Option<PathBuf>> {
        let (session, metadata_out_dir) = super::session_from_args(&self.args)?;

        let artifact =
            match midenc_compile::compile_to_memory(Rc::new(midenc_hir::Context::new(session))) {
                Ok(artifact) => artifact,
                // A `--stop-after` stop is a successful, deliberately partial run; there is
                // no package to materialize.
                Err(err) if err.downcast_ref::<midenc_compile::CompilerStopped>().is_some() => {
                    return Ok(None);
                }
                Err(err) => return Err(anyhow!("{}", PrintDiagnostic::new(err))),
            };

        match artifact {
            CompiledArtifact::Assembled(package) => {
                // Written atomically: dependent projects deserialize this artifact from disk
                // while expanding their own macros, potentially in parallel with a rebuild.
                let output_path =
                    midenc_compile::cargo::write_package_atomic(&package, &metadata_out_dir)
                        .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))
                        .with_context(|| {
                            format!(
                                "failed to write package artifact for {}@{}",
                                &package.name, &package.version
                            )
                        })?;
                Ok(Some(output_path))
            }
            _ => unreachable!(),
        }
    }
}
