use clap::Error as ClapError;
use midenc_session::diagnostics::{Diagnostic, Report, miette};

use crate::opt;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error(transparent)]
#[diagnostic()]
pub struct ClapDiagnostic {
    #[from]
    err: ClapError,
}
impl ClapDiagnostic {
    pub fn exit(self) -> ! {
        self.err.exit()
    }
}

/// Run the driver as if it was invoked from the command-line
pub fn run<P, A>(
    cwd: P,
    args: A,
    logger: Box<dyn log::Log>,
    filter: log::LevelFilter,
) -> Result<(), Report>
where
    P: Into<std::path::PathBuf>,
    A: IntoIterator<Item = std::ffi::OsString>,
{
    match opt::HirOpt::run(cwd, args, logger, filter) {
        Err(report) => match report.downcast::<midenc_compile::CompilerStopped>() {
            Ok(_) => Ok(()),
            Err(report) => Err(report),
        },
        result => result,
    }
}
