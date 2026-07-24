use miden_assembly::ProjectSourceInputs;
use midenc_hir::Spanned;
use midenc_session::OutputMode;

use super::*;
use crate::CompilerStopped;

pub struct MasmSources {
    pub inputs: ProjectSourceInputs,
    #[cfg(feature = "std")]
    pub provenance: miden_assembly::ProjectSourceProvenanceInputs,
}

/// Parses Miden Assembly sources for project assembly
pub struct ParseMasmStage;

impl Stage for ParseMasmStage {
    type Input = InputFile;
    type Output = MasmSources;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let file_type = input.file_type();
        if !matches!(input.file_type(), midenc_session::FileType::Masm) {
            return Err(Report::msg(format!(
                "invalid input file: expected '.masm', got {file_type}"
            )));
        }
        let inputs = match input.file {
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            #[cfg(feature = "std")]
            InputType::Real(path) => self.parse_masm_from_file(path.as_ref(), context.clone())?,
            InputType::Stdin { name, input } => {
                self.parse_masm_from_bytes(name.as_str(), &input, context.clone())?
            }
        };

        #[cfg(feature = "std")]
        let provenance = {
            use alloc::string::ToString;
            let root = {
                let source_file =
                    context.session().source_manager.get(inputs.root.span().source_id()).unwrap();
                miden_assembly::SourceFileProvenance {
                    path: source_file.uri().to_path().unwrap().into_boxed_path(),
                    content: source_file.as_str().to_string().into_boxed_str(),
                }
            };
            let mut support = Vec::with_capacity(inputs.support.len());
            for module in inputs.support.iter() {
                let source_file =
                    context.session().source_manager.get(module.span().source_id()).unwrap();
                support.push(miden_assembly::SourceFileProvenance {
                    path: source_file.uri().to_path().unwrap().into_boxed_path(),
                    content: source_file.as_str().to_string().into_boxed_str(),
                });
            }
            miden_assembly::ProjectSourceProvenanceInputs { root, support }
        };

        for module in inputs.support.iter() {
            context.session().emit(OutputMode::Text, module).into_diagnostic()?;
        }
        context.session().emit(OutputMode::Text, &inputs.root).into_diagnostic()?;

        if context.session().parse_only() {
            log::debug!("stopping compiler early (parse-only=true)");
            return Err(CompilerStopped("parse-only").into());
        }

        Ok(MasmSources {
            inputs,
            #[cfg(feature = "std")]
            provenance,
        })
    }
}

impl ParseMasmStage {
    #[cfg(feature = "std")]
    fn parse_masm_from_file(
        &self,
        path: &Path,
        context: Rc<Context>,
    ) -> CompilerResult<ProjectSourceInputs> {
        use miden_assembly::ast::ModuleKind;
        use miden_mast_package::TargetType;

        let kind = match context.session().options.target_type.unwrap_or_default() {
            TargetType::Executable => ModuleKind::Executable,
            TargetType::Kernel => ModuleKind::Kernel,
            _ => ModuleKind::Library,
        };
        let warnings_as_errors =
            context.session().options.diagnostics.warnings.warnings_as_errors();
        let (root, support) = miden_assembly_syntax::parser::read_modules_from_root(
            path,
            None,
            Some(kind),
            context.source_manager(),
            warnings_as_errors,
        )?;
        Ok(ProjectSourceInputs { root, support })
    }

    fn parse_masm_from_bytes(
        &self,
        name: &str,
        bytes: &[u8],
        context: Rc<Context>,
    ) -> CompilerResult<ProjectSourceInputs> {
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
        let mut parser = ast::Module::parser(Some(kind));
        let root = parser.parse_str(
            Some(name.as_path()),
            source,
            context.session().source_manager.clone(),
        )?;
        Ok(ProjectSourceInputs {
            root,
            support: Default::default(),
        })
    }
}
