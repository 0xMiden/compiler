use std::{ffi::OsString, path::PathBuf, rc::Rc, sync::Arc};

use clap::Parser;
use log::Log;
use midenc_compile as compile;
use midenc_hir::Context;
use midenc_session::{
    InputFile,
    diagnostics::{Emitter, Report},
};

use crate::ClapDiagnostic;

/// This struct provides the command-line interface used by `midenc`
#[derive(Debug, Parser)]
#[command(name = "midenc")]
#[command(
    author,
    version,
    about = "A compiler for Miden Assembly",
    long_about = None,
    arg_required_else_help = false,
)]
pub struct Midenc {
    /// The input file to compile
    ///
    /// You may specify `-` to read from stdin, otherwise you must provide a path
    #[arg(value_name = "FILE")]
    input: Option<InputFile>,
    #[command(flatten)]
    options: compile::Compiler,
}

impl Midenc {
    /// Run one compilation of `args`, as if `midenc` had been invoked with them in `cwd`.
    ///
    /// This is the library entry point: it installs nothing global — no logger, no diagnostics
    /// hook — so an embedder keeps whatever it installed itself. `args` is the full argument
    /// vector including the program name, exactly as a process receives it.
    ///
    /// The path of the package that was written, if one was, for a caller that has to say where
    /// its build went; see [`compile::compile`]. A run deliberately stopped short of a package by
    /// `--stop-after` is a success with nothing to name, not a failure.
    pub fn exec<P, A>(
        cwd: P,
        args: A,
        emitter: Option<Arc<dyn Emitter>>,
    ) -> Result<Option<PathBuf>, Report>
    where
        P: Into<PathBuf>,
        A: IntoIterator<Item = OsString>,
    {
        let command = <Self as clap::CommandFactory>::command();
        let command = midenc_session::flags::register_flags(command);

        let mut matches = command.try_get_matches_from(args).map_err(ClapDiagnostic::from)?;
        let compile_matches = matches.clone();
        let Self { input, options } =
            <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
                .map_err(format_error::<Self>)
                .map_err(ClapDiagnostic::from)?;

        let mut options = options.into_options(cwd.into());
        options.set_extra_flags(compile_matches.into());

        let input = options.resolve_input(input)?;

        let session = Rc::new(options.into_session(input, emitter, None)?);
        let context = Rc::new(Context::new(session));
        match compile::compile(context) {
            Err(report) => match report.downcast::<compile::CompilerStopped>() {
                Ok(_) => Ok(None),
                Err(report) => Err(report),
            },
            result => result,
        }
    }

    /// Run `midenc` from the command line, logging through `logger` at `filter`.
    pub fn run<P, A>(
        cwd: P,
        args: A,
        logger: Box<dyn Log>,
        filter: log::LevelFilter,
    ) -> Result<(), Report>
    where
        P: Into<PathBuf>,
        A: IntoIterator<Item = OsString>,
    {
        Self::run_with_emitter(cwd, args, None, logger, filter)
    }

    /// The same as [`run`](Self::run), with diagnostics rendered by `emitter`.
    pub fn run_with_emitter<P, A>(
        cwd: P,
        args: A,
        emitter: Option<Arc<dyn Emitter>>,
        logger: Box<dyn Log>,
        filter: log::LevelFilter,
    ) -> Result<(), Report>
    where
        P: Into<PathBuf>,
        A: IntoIterator<Item = OsString>,
    {
        log::set_boxed_logger(logger)
            .unwrap_or_else(|err| panic!("failed to install logger: {err}"));
        log::set_max_level(filter);

        Self::exec(cwd, args, emitter).map(|_| ())
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}
