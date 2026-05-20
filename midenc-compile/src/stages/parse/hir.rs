use super::*;
use crate::CompilerStopped;

/// Parses arbitrary HIR
pub struct ParseHirStage;

impl Stage for ParseHirStage {
    type Input = InputFile;
    type Output = midenc_hir::OperationRef;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let file_type = input.file_type();
        if !matches!(input.file_type(), midenc_session::FileType::Hir) {
            return Err(Report::msg(format!(
                "invalid input file: expected '.hir', got {file_type}"
            )));
        }
        let op = match input.file {
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            #[cfg(feature = "std")]
            InputType::Real(path) => {
                let config = midenc_hir::parse::ParserConfig {
                    context: context.clone(),
                    verify: true,
                };
                midenc_hir::parse::parse_file_any(config, path)?
            }
            InputType::Stdin { name, input } => {
                let config = midenc_hir::parse::ParserConfig {
                    context: context.clone(),
                    verify: true,
                };
                let source = core::str::from_utf8(&input)
                    .map_err(|err| Report::msg(format!("failed to parse {name}: {err}")))?;
                midenc_hir::parse::parse_any(config, Uri::new(name.as_str()), source)?
            }
        };

        {
            let op = op.borrow();
            crate::emit_hir_if_requested(&op, context.clone())?;
        }

        if context.session().parse_only() {
            log::debug!("stopping compiler early (parse-only=true)");
            return Err(CompilerStopped("parse-only").into());
        }

        Ok(op)
    }
}
