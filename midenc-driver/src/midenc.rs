use std::{ffi::OsString, path::PathBuf, rc::Rc, sync::Arc};

use clap::{Parser, Subcommand};
use log::Log;
use midenc_compile as compile;
#[cfg(feature = "debug")]
use midenc_debug as debugger;
use midenc_session::{
    diagnostics::{Emitter, Report},
    InputFile,
};

use crate::ClapDiagnostic;

/// This struct provides the command-line interface used by `midenc`
#[derive(Debug, Parser)]
#[command(name = "midenc")]
#[command(author, version, about = "A compiler for Miden Assembly", long_about = None)]
pub struct Midenc {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Compile {
        /// The input file to compile
        ///
        /// You may specify `-` to read from stdin, otherwise you must provide a path
        #[arg(required(true), value_name = "FILE")]
        input: InputFile,
        #[command(flatten)]
        options: compile::Compiler,
    },
    /// Execute a compiled program or library, using the Miden VM.
    #[cfg(feature = "debug")]
    Run {
        /// Specify the path to a Miden program file to execute.
        ///
        /// Miden Assembly programs are emitted by the compiler with a `.masl` extension.
        ///
        /// You may use `-` as a file name to read a file from stdin.
        #[arg(required(true), value_name = "FILE")]
        input: InputFile,
        /// Specify the path to a file containing program inputs.
        ///
        /// Program inputs are stack and advice provider values which the program can
        /// access during execution. The inputs file is a TOML file which describes
        /// what the inputs are, or where to source them from.
        ///
        /// Example:
        ///
        /// [inputs]
        ///
        /// stack = [1, 2, 0x3]
        ///
        #[arg(long, value_name = "FILE")]
        inputs: Option<debugger::DebuggerConfig>,
        /// Number of outputs on the operand stack to print
        #[arg(long, short = 'n', default_value_t = 16)]
        num_outputs: usize,
        /// Arguments to place on the operand stack before calling the program entrypoint.
        ///
        /// Arguments will be pushed on the operand stack in the order of appearance,
        ///
        /// Example: `-- a b` will push `a` on the stack, then `b`.
        ///
        /// These arguments must be valid field element values expressed in decimal format.
        ///
        /// NOTE: These arguments will override any stack values provided via --inputs
        #[arg(last(true), value_name = "ARGV")]
        args: Vec<debugger::Felt>,
        #[command(flatten)]
        options: debugger::Debugger,
    },
    /// Run a program under the interactive Miden VM debugger
    ///
    /// This command starts a TUI-based interactive debugger with the given program loaded.
    #[cfg(feature = "debug")]
    Debug {
        /// Specify the path to a Miden program file to execute.
        ///
        /// Miden Assembly programs are emitted by the compiler with a `.masl` extension.
        ///
        /// You may use `-` as a file name to read a file from stdin.
        #[arg(required(true), value_name = "FILE")]
        input: InputFile,
        /// Specify the path to a file containing program inputs.
        ///
        /// Program inputs are stack and advice provider values which the program can
        /// access during execution. The inputs file is a TOML file which describes
        /// what the inputs are, or where to source them from.
        #[arg(long, value_name = "FILE")]
        inputs: Option<debugger::DebuggerConfig>,
        /// Arguments to place on the operand stack before calling the program entrypoint.
        ///
        /// Arguments will be pushed on the operand stack in the order of appearance,
        ///
        /// Example: `-- a b` will push `a` on the stack, then `b`.
        ///
        /// These arguments must be valid field element values expressed in decimal format.
        ///
        /// NOTE: These arguments will override any stack values provided via --inputs
        #[arg(last(true), value_name = "ARGV")]
        args: Vec<debugger::Felt>,
        #[command(flatten)]
        options: debugger::Debugger,
    },
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
        let command = command.mut_subcommand("compile", midenc_session::flags::register_flags);

        let mut matches = command.try_get_matches_from(args).map_err(ClapDiagnostic::from)?;
        let compile_matches = matches.subcommand_matches("compile").cloned().unwrap_or_default();
        let cli = <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<Self>)
            .map_err(ClapDiagnostic::from)?;

        cli.invoke(cwd.into(), emitter, logger, filter, compile_matches)
    }

    fn invoke(
        self,
        cwd: PathBuf,
        emitter: Option<Arc<dyn Emitter>>,
        logger: Box<dyn Log>,
        filter: log::LevelFilter,
        matches: clap::ArgMatches,
    ) -> Result<(), Report> {
        match self.command {
            Commands::Compile { input, mut options } => {
                log::set_boxed_logger(logger)
                    .unwrap_or_else(|err| panic!("failed to install logger: {err}"));
                log::set_max_level(filter);
                if options.working_dir.is_none() {
                    options.working_dir = Some(cwd);
                }
                let session =
                    options.into_session(vec![input], emitter).with_extra_flags(matches.into());
                compile::compile(Rc::new(session))
            }
            #[cfg(feature = "debug")]
            Commands::Run {
                input,
                inputs,
                args,
                num_outputs,
                mut options,
            } => {
                log::set_boxed_logger(logger)
                    .unwrap_or_else(|err| panic!("failed to install logger: {err}"));
                log::set_max_level(filter);
                if options.working_dir.is_none() {
                    options.working_dir = Some(cwd);
                }
                let session = options.into_session(vec![input], emitter);
                let args = args.into_iter().map(|felt| felt.0).collect();
                debugger::run_noninteractively(inputs, args, num_outputs, Rc::new(session))
            }
            #[cfg(feature = "debug")]
            Commands::Debug {
                input,
                inputs,
                args,
                mut options,
            } => {
                if options.working_dir.is_none() {
                    options.working_dir = Some(cwd);
                }
                let session = options.into_session(vec![input], emitter);
                let args = args.into_iter().map(|felt| felt.0).collect();
                debugger::run(inputs, args, Rc::new(session), logger)
            }
        }
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}
