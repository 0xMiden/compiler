use std::{ffi::OsString, path::PathBuf, rc::Rc, sync::Arc};

use clap::Parser;
use log::Log;
use midenc_compile as compile;
use midenc_hir::{
    Context, OpPrintingFlags, OperationRef, diagnostics::Uri, parse::ParserConfig,
    pass::IRPrintingConfig, print::AsmPrinter,
};
use midenc_session::{
    Emit, FileType, InputFile, InputType, OutputTypeSpec,
    diagnostics::{Emitter, Report},
};

use crate::{
    ClapDiagnostic,
    pipeline::{Anchor, PassPipeline},
};

/// This struct provides the command-line interface used by `hir-opt`
#[derive(Debug, Parser)]
#[command(name = "hir-opt")]
#[command(author, version, about = "A tool for parsing and rewriting HIR", long_about = None)]
pub struct HirOpt {
    /// The input file to parse and optionally rewrite
    ///
    /// You may specify `-` to read from stdin, otherwise you must provide a path
    #[arg(value_name = "FILE")]
    input: InputFile,
    /// Enable verification of parsed HIR and in the pass pipeline
    #[arg(long, default_value_t = true, help_heading = "Compiler")]
    verify: bool,
    /// An optional pass pipeline to apply
    #[arg(
        long,
        short = 'p',
        value_name = "PIPELINE",
        default_value = "any",
        default_missing_value = "any",
        help_heading = "Compiler"
    )]
    pass_pipeline: PassPipeline,
    /// The standard compiler options
    #[command(flatten)]
    options: compile::Compiler,
}

impl HirOpt {
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
        let Self {
            input,
            verify,
            pass_pipeline,
            mut options,
        } = <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<Self>)
            .map_err(ClapDiagnostic::from)?;

        log::set_boxed_logger(logger)
            .unwrap_or_else(|err| panic!("failed to install logger: {err}"));
        log::set_max_level(filter);
        if options.working_dir.is_none() {
            options.working_dir = Some(cwd.into());
        }
        if options.output_types.is_empty() && !options.stdout {
            options.stdout = true;
            options.output_types.push(OutputTypeSpec::Typed {
                output_type: midenc_session::OutputType::Hir,
                path: Some(midenc_session::OutputFile::Stdout),
            });
        }
        let session =
            Rc::new(options.into_session(vec![input], emitter).with_extra_flags(matches.into()));
        let context = Rc::new(Context::new(session));

        let input = &context.session().inputs[0];
        if input.file_type() != FileType::Hir && input.is_real() {
            return Err(Report::msg("invalid input file: expected HIR source file"));
        }

        let mut pm = pass_pipeline
            .load(context.clone())?
            .enable_ir_printing(IRPrintingConfig::try_from(&context.session().options)?);
        pm.enable_verifier(verify);

        let config = ParserConfig {
            context: context.clone(),
            verify,
        };
        let parsed = match &input.file {
            InputType::Real(path) => match pass_pipeline.anchor {
                Anchor::Any => midenc_hir::parse::parse_file_any(config, path)?,
                Anchor::Operation { dialect, opcode } => {
                    let dialect = config.context.get_registered_dialect(dialect);
                    let Some(name) =
                        dialect.registered_ops().iter().find(|name| name.name() == opcode).cloned()
                    else {
                        return Err(Report::msg(format!(
                            "invalid anchor: unknown operation type '{}.{opcode}'",
                            dialect.name()
                        )));
                    };
                    midenc_hir::parse::parse_file_anchored(name, config, path)?
                }
            },
            InputType::Stdin { name, input } => {
                let source = core::str::from_utf8(input).map_err(|err| {
                    Report::msg(format!("unable to load input: invalid utf8 ({err})"))
                })?;
                let uri = Uri::new(name.as_str());
                match pass_pipeline.anchor {
                    Anchor::Any => midenc_hir::parse::parse_any(config, uri, source)?,
                    Anchor::Operation { dialect, opcode } => {
                        let dialect = config.context.get_registered_dialect(dialect);
                        let Some(name) = dialect
                            .registered_ops()
                            .iter()
                            .find(|name| name.name() == opcode)
                            .cloned()
                        else {
                            return Err(Report::msg(format!(
                                "invalid anchor: unknown operation type '{}.{opcode}'",
                                dialect.name()
                            )));
                        };
                        midenc_hir::parse::parse_anchored(name, config, uri, source)?
                    }
                }
            }
        };

        pm.run(parsed)?;

        let emitter = EmitHir { op: parsed };
        context
            .session()
            .emit(midenc_session::OutputMode::Text, &emitter)
            .map_err(|err| Report::msg(format!("failed to emit HIR output: {err}")))?;

        Ok(())
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}

struct EmitHir {
    op: OperationRef,
}

impl Emit for EmitHir {
    fn name(&self) -> Option<midenc_hir::interner::Symbol> {
        None
    }

    fn output_type(&self, _mode: midenc_session::OutputMode) -> midenc_session::OutputType {
        midenc_session::OutputType::Hir
    }

    fn write_to<W: midenc_session::Writer>(
        &self,
        mut writer: W,
        _mode: midenc_session::OutputMode,
        _session: &midenc_session::Session,
    ) -> anyhow::Result<()> {
        let operation = self.op.borrow();
        let flags = OpPrintingFlags::default();
        let mut printer = AsmPrinter::new(operation.context_rc(), &flags);
        printer.print_operation(operation);
        write!(&mut writer, "{}", printer.finish())
    }
}
