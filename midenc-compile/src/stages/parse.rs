#[cfg(feature = "std")]
use alloc::{borrow::Cow, format, rc::Rc};
use alloc::{borrow::ToOwned, boxed::Box, vec::Vec};

use miden_assembly::ProjectSourceInputs;
use miden_mast_package::TargetType;
#[cfg(feature = "std")]
use midenc_frontend_wasm::{WatEmit, wasm_to_wat};
use midenc_hir::diagnostics::Uri;
#[cfg(feature = "std")]
use midenc_session::{FileName, Path};
use midenc_session::{
    InputFile, InputType,
    diagnostics::{IntoDiagnostic, WrapErr},
};

use super::*;

/// Parses any input that can be converted to a [MidenComponent]
pub struct ParseComponentStage;

impl Stage for ParseComponentStage {
    type Input = InputFile;
    type Output = MidenComponent;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_session::FileType;

        let file_type = input.file_type();
        match file_type {
            FileType::Hir => {
                let mut stage = ParseHirStage.map(super::extract_miden_component_or_bail);
                stage.run(input, context)
            }
            FileType::Wasm | FileType::Wat => {
                let mut stage = ParseWasmStage;
                stage.run(input, context)
            }
            file_type => Err(Report::msg(format!(
                "unsupported file type '{file_type}' for parsing miden components"
            ))),
        }
    }
}

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
            return Err(CompilerStopped.into());
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
        let kind = match context.session().options.target_type {
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
        let kind = match context.session().options.target_type {
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
            return Err(CompilerStopped.into());
        }

        Ok(op)
    }
}

/// Parses Wasm binaries or WebAssembly Text to an HIR component
pub struct ParseWasmStage;

#[derive(Clone)]
pub struct MidenComponent {
    pub world: builtin::WorldRef,
    pub component: Option<builtin::ComponentRef>,
    pub account_component_metadata_bytes: Option<Vec<u8>>,
}

impl Stage for ParseWasmStage {
    type Input = InputFile;
    type Output = MidenComponent;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_hir::{BuilderExt, OpBuilder, SourceSpan};

        let is_wat = match input.file_type() {
            midenc_session::FileType::Wat => true,
            midenc_session::FileType::Wasm => false,
            file_type => {
                return Err(Report::msg(format!(
                    "invalid input file: expected '.hir', got {file_type}"
                )));
            }
        };
        let input = match input.file {
            #[cfg(feature = "std")]
            InputType::Real(path) if is_wat => self.parse_wasm_from_wat_file(path.as_ref())?,
            #[cfg(feature = "std")]
            InputType::Stdin { name, input } if is_wat => {
                let wasm =
                    wat::parse_bytes(&input).into_diagnostic().wrap_err("failed to parse wat")?;
                InputType::Stdin {
                    name,
                    input: wasm.into_owned(),
                }
            }
            input => input,
        };

        #[cfg(feature = "std")]
        {
            self.emit_wat_for_wasm_input(&input, context.session())?;
        }

        // Parse and translate the component WebAssembly using the constructed World
        let world = {
            let mut builder = OpBuilder::new(context.clone());
            let world_builder = builder.create::<builtin::World, ()>(SourceSpan::default());
            world_builder()?
        };

        let wasm::FrontendOutput {
            component,
            account_component_metadata_bytes,
        } = match input {
            #[cfg(feature = "std")]
            InputType::Real(path) => {
                self.parse_hir_from_wasm_file(&path, world, context.clone())?
            }
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            InputType::Stdin { name, input } => {
                let config = wasm::WasmTranslationConfig {
                    source_name: name.file_stem().unwrap().to_owned().into(),
                    trim_path_prefixes: context.session().options.trim_path_prefixes.clone(),
                    world: Some(world),
                    ..Default::default()
                };
                self.parse_hir_from_wasm_bytes(&input, context.clone(), &config)?
            }
        };

        {
            use midenc_hir::Op;
            let component = component.borrow();
            crate::emit_hir_if_requested(component.as_operation(), context.clone())?;
        }

        if context.session().parse_only() {
            log::debug!("stopping compiler early (parse-only=true)");
            return Err(CompilerStopped.into());
        } else if context.session().analyze_only() {
            log::debug!("stopping compiler early (analyze-only=true)");
            return Err(CompilerStopped.into());
        } else if context.session().options.link_only {
            log::debug!("stopping compiler early (link-only=true)");
            return Err(CompilerStopped.into());
        }

        Ok(MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes,
        })
    }
}

impl ParseWasmStage {
    #[cfg(feature = "std")]
    fn emit_wat_for_wasm_input(&self, input: &InputType, session: &Session) -> CompilerResult<()> {
        if !session.should_emit(midenc_session::OutputType::Wat) {
            return Ok(());
        }

        let wasm_bytes: Cow<'_, [u8]> = match input {
            InputType::Real(path) => {
                Cow::Owned(std::fs::read(path).into_diagnostic().wrap_err_with(|| {
                    format!("failed to read wasm input from '{}'", path.display())
                })?)
            }
            InputType::Stdin { input, .. } => Cow::Borrowed(input),
        };

        let wat = wasm_to_wat(wasm_bytes.as_ref())
            .into_diagnostic()
            .wrap_err("failed to convert wasm to wat")?;
        let artifact = WatEmit(&wat);
        session
            .emit(OutputMode::Text, &artifact)
            .into_diagnostic()
            .wrap_err("failed to emit wat output")?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn parse_wasm_from_wat_file(&self, path: &Path) -> CompilerResult<InputType> {
        let wasm = wat::parse_file(path).into_diagnostic().wrap_err("failed to parse wat")?;
        Ok(InputType::Stdin {
            name: FileName::from(path.to_path_buf()),
            input: wasm,
        })
    }

    #[cfg(feature = "std")]
    fn parse_hir_from_wasm_file(
        &self,
        path: &Path,
        world: builtin::WorldRef,
        context: Rc<Context>,
    ) -> CompilerResult<wasm::FrontendOutput> {
        use std::io::Read;

        log::debug!("parsing hir from wasm at {}", path.display());
        let mut file = std::fs::File::open(path)
            .into_diagnostic()
            .wrap_err("could not open input for reading")?;
        let mut bytes = Vec::with_capacity(1024);
        file.read_to_end(&mut bytes).into_diagnostic()?;
        let file_name = path.file_stem().unwrap().to_str().unwrap().to_owned();

        let config = wasm::WasmTranslationConfig {
            source_name: file_name.into(),
            trim_path_prefixes: context.session().options.trim_path_prefixes.clone(),
            world: Some(world),
            ..Default::default()
        };
        self.parse_hir_from_wasm_bytes(&bytes, context, &config)
    }

    fn parse_hir_from_wasm_bytes(
        &self,
        bytes: &[u8],
        context: Rc<Context>,
        config: &wasm::WasmTranslationConfig,
    ) -> CompilerResult<wasm::FrontendOutput> {
        let outpub = wasm::translate(bytes, config, context.clone())?;
        log::debug!(
            "parsed hir component from wasm bytes with first module name: {}",
            outpub.component.borrow().id()
        );

        Ok(outpub)
    }
}
