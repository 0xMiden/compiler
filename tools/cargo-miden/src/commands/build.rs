use std::{path::PathBuf, rc::Rc};

use anyhow::{Context as _, Result, anyhow};
use clap::Args;
use midenc_compile::{Compiler, stages::Artifact};
use midenc_session::{InputFile, diagnostics::PrintDiagnostic};

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
        // Parse all arguments using midenc's Compiler parser.
        // This gives us a structured representation of all options.
        let cwd = std::env::current_dir()?;
        let compiler_opts =
            Compiler::try_parse_from(cwd.clone(), &self.args).unwrap_or_else(|err| err.exit());

        let metadata_out_dir = compiler_opts.target_dir.join("miden").join(&compiler_opts.profile);
        if !metadata_out_dir.exists() {
            std::fs::create_dir_all(&metadata_out_dir)?;
        }

        let manifest_path = match compiler_opts.manifest_path.as_deref() {
            Some(manifest_path) => manifest_path.to_path_buf(),
            None => std::env::var("CARGO_MANIFEST_DIR")
                .ok()
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok())
                .expect("no Cargo manifest available")
                .join("Cargo.toml"),
        };
        let input = InputFile::from_path(&manifest_path).unwrap();
        let session = Rc::new(
            compiler_opts
                .into_session(input, None, None)
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?,
        );

        let artifact =
            midenc_compile::compile_to_memory(Rc::new(midenc_hir::Context::new(session)))
                .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?;

        match artifact {
            Artifact::Assembled(package) => {
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
