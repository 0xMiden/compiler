pub mod build;
pub mod new_project;
pub mod test;

use std::{path::PathBuf, rc::Rc};

use anyhow::anyhow;
pub use build::BuildCommand;
use midenc_compile::Compiler;
use midenc_session::{InputFile, Session, diagnostics::PrintDiagnostic};
pub use new_project::NewCommand;
pub use test::TestCommand;

/// Parses midenc-style arguments into a compilation session for the current directory.
///
/// Returns the session together with the metadata output directory
/// (`<target-dir>/<profile>`).
pub(crate) fn session_from_args(args: &[String]) -> anyhow::Result<(Rc<Session>, PathBuf)> {
    let cwd = std::env::current_dir()?;
    let compiler_opts =
        Compiler::try_parse_from(cwd.clone(), args).unwrap_or_else(|err| err.exit());

    let metadata_out_dir = compiler_opts.target_dir.join(&compiler_opts.profile);

    let manifest_path = match compiler_opts.manifest_path.as_deref() {
        Some(manifest_path) => manifest_path.to_path_buf(),
        None => cwd.join("Cargo.toml"),
    };
    let input = InputFile::from_path(&manifest_path)
        .map_err(|err| anyhow!("failed to read '{}': {err}", manifest_path.display()))?;
    // This root session is expected to name one selected package. The package-cache closure
    // walk relies on workspace builds reaching this point once per selected member; an
    // unselected workspace-root manifest is rejected during project preparation.
    let session = Rc::new(
        compiler_opts
            .into_session(input, None, None)
            .map_err(|err| anyhow!("{}", PrintDiagnostic::new(err)))?,
    );
    Ok((session, metadata_out_dir))
}
