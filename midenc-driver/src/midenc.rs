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
#[command(author, version, about = "A compiler for Miden Assembly", long_about = None)]
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
        let command = <Self as clap::CommandFactory>::command();
        let command = midenc_session::flags::register_flags(command);

        let mut matches = command.try_get_matches_from(args).map_err(ClapDiagnostic::from)?;
        let Self { input, mut options } =
            <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
                .map_err(format_error::<Self>)
                .map_err(ClapDiagnostic::from)?;

        log::set_boxed_logger(logger)
            .unwrap_or_else(|err| panic!("failed to install logger: {err}"));
        log::set_max_level(filter);
        if options.working_dir.is_none() {
            options.working_dir = Some(cwd.into());
        }
        let session = Rc::new(
            options
                .into_session(Vec::from_iter(input), emitter)
                .with_extra_flags(matches.into()),
        );
        let context = Rc::new(Context::new(session));
        compile::compile(context)
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}
