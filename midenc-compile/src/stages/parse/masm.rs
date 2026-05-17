use miden_assembly::ProjectSourceInputs;
use midenc_session::OutputMode;

use super::*;
use crate::CompilerStopped;

/// Parses Miden Assembly sources for project assembly
pub struct ParseMasmStage;

impl Stage for ParseMasmStage {
    type Input = InputFile;
    type Output = ProjectSourceInputs;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let file_type = input.file_type();
        if !matches!(input.file_type(), midenc_session::FileType::Masm) {
            return Err(Report::msg(format!(
                "invalid input file: expected '.masm', got {file_type}"
            )));
        }
        let module = match input.file {
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            #[cfg(feature = "std")]
            InputType::Real(path) => self.parse_masm_from_file(path.as_ref(), context.clone())?,
            InputType::Stdin { name, input } => {
                self.parse_masm_from_bytes(name.as_str(), &input, context.clone())?
            }
        };

        context.session().emit(OutputMode::Text, &module).into_diagnostic()?;

        if context.session().parse_only() {
            log::debug!("stopping compiler early (parse-only=true)");
            return Err(CompilerStopped("parse-only").into());
        }

        Ok(ProjectSourceInputs {
            root: module,
            support: Default::default(),
        })
    }
}

impl ParseMasmStage {
    #[cfg(feature = "std")]
    fn parse_masm_from_file(
        &self,
        path: &Path,
        context: Rc<Context>,
    ) -> CompilerResult<Box<miden_assembly::ast::Module>> {
        use miden_assembly::ast::{self, Ident, ModuleKind};
        use miden_mast_package::TargetType;

        // Construct library path for MASM module
        let module_name = Ident::new(path.file_stem().unwrap().to_str().unwrap())
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "failed to construct valid module identifier from path '{}'",
                    path.display()
                )
            })?;

        // Parse AST
        let kind = match context.session().options.target_type.unwrap_or_default() {
            TargetType::Executable => ModuleKind::Executable,
            TargetType::Kernel => ModuleKind::Kernel,
            _ => ModuleKind::Library,
        };
        let mut parser = ast::Module::parser(kind);
        let ast = parser.parse_file(
            module_name.as_str(),
            path,
            context.session().source_manager.clone(),
        )?;

        Ok(ast)
    }

    fn parse_masm_from_bytes(
        &self,
        name: &str,
        bytes: &[u8],
        context: Rc<Context>,
    ) -> CompilerResult<Box<miden_assembly::ast::Module>> {
        use miden_assembly::{
            PathBuf as LibraryPath,
            ast::{self, ModuleKind},
        };

        let source = core::str::from_utf8(bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("input '{name}' contains invalid utf-8"))?;

        // Construct library path for MASM module
        let name = LibraryPath::new(name).into_diagnostic()?;

        // Parse AST
        let kind = match context.session().options.target_type.unwrap_or_default() {
            TargetType::Executable => ModuleKind::Executable,
            TargetType::Kernel => ModuleKind::Kernel,
            _ => ModuleKind::Library,
        };
        let mut parser = ast::Module::parser(kind);
        let ast =
            parser.parse_str(name.as_path(), source, context.session().source_manager.clone())?;
        Ok(ast)
    }
}
