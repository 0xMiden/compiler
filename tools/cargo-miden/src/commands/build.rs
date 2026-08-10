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
    /// Executes `cargo miden build`, returning the resulting command output.
    pub fn exec(self) -> Result<PathBuf> {
        let (session, metadata_out_dir) = super::session_from_args(&self.args)?;

        let artifact =
            midenc_compile::compile_to_memory(Rc::new(midenc_hir::Context::new(session)))
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;

        match artifact {
            CompiledArtifact::Assembled(package) => {
                let output_path = metadata_out_dir
                    .join(&*package.name)
                    .with_extension(miden_mast_package::Package::EXTENSION);
                package.write_masp_file(&metadata_out_dir).with_context(|| {
                    format!(
                        "failed to write package artifact for {}@{}",
                        &package.name, &package.version
                    )
                })?;
                Ok(output_path)
            }
            _ => unreachable!(),
        }
    }
}
