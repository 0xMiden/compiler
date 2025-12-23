#![deny(warnings)]

mod midenc;

pub use clap::Error as ClapError;
use log::Log;
pub use midenc_session::diagnostics;
use midenc_session::diagnostics::{Diagnostic, Report, miette};

pub use self::midenc::Midenc;

/// A convenience alias for `Result<T, Report>`
pub type DriverResult<T> = Result<T, Report>;

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
    logger: Box<dyn Log>,
    filter: log::LevelFilter,
) -> Result<(), Report>
where
    P: Into<std::path::PathBuf>,
    A: IntoIterator<Item = std::ffi::OsString>,
{
    setup_diagnostics();

    match Midenc::run(cwd, args, logger, filter) {
        Err(report) => match report.downcast::<midenc_compile::CompilerStopped>() {
            Ok(_) => Ok(()),
            Err(report) => Err(report),
        },
        result => result,
    }
}

fn setup_diagnostics() {
    use diagnostics::ReportHandlerOpts;

    let result =
        diagnostics::reporting::set_hook(Box::new(|_| Box::new(ReportHandlerOpts::new().build())));
    if result.is_ok() {
        diagnostics::reporting::set_panic_hook();
    }
}
