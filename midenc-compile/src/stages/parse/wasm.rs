use midenc_frontend_wasm::{FrontendOutput, WasmTranslationConfig};
#[cfg(feature = "std")]
use midenc_frontend_wasm::{WatEmit, wasm_to_wat};
#[cfg(feature = "std")]
use midenc_session::Session;

use super::*;
use crate::CompilerStopped;

/// Parses Wasm binaries or WebAssembly Text to an HIR component
#[derive(Default)]
pub struct ParseWasmStage;

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
        let source_provenance = self.source_provenance_from_wasm_input(&input)?;

        #[cfg(feature = "std")]
        self.emit_wat_for_wasm_input(&input, context.session())?;

        // Parse and translate the component WebAssembly using the constructed World
        let world = {
            let mut builder = OpBuilder::new(context.clone());
            let world_builder = builder.create::<builtin::World, ()>(SourceSpan::default());
            world_builder()?
        };

        let FrontendOutput {
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
                let config = WasmTranslationConfig {
                    source_name: name.file_stem().unwrap().to_owned().into(),
                    remap_path_prefixes: context.session().options.remap_path_prefixes.clone(),
                    world: Some(world),
                    generate_native_debuginfo: context.session().options.emit_source_locations(),
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
            return Err(CompilerStopped("parse-only").into());
        } else if context.session().options.link_only {
            log::debug!("stopping compiler early (link-only=true)");
            return Err(CompilerStopped("link-only").into());
        }

        Ok(MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes,
            #[cfg(feature = "std")]
            source_provenance,
        })
    }
}

impl ParseWasmStage {
    #[cfg(feature = "std")]
    fn emit_wat_for_wasm_input(&self, input: &InputType, session: &Session) -> CompilerResult<()> {
        use midenc_session::OutputMode;

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
    fn source_provenance_from_wasm_input(
        &self,
        input: &InputType,
    ) -> CompilerResult<miden_assembly::ProjectSourceProvenanceInputs> {
        use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};

        let wasm_bytes: Cow<'_, [u8]> = match input {
            InputType::Real(path) => {
                Cow::Owned(std::fs::read(path).into_diagnostic().wrap_err_with(|| {
                    format!("failed to read wasm input from '{}'", path.display())
                })?)
            }
            InputType::Stdin { input, .. } => Cow::Borrowed(input),
        };

        let content = wasm_to_wat(wasm_bytes.as_ref())
            .into_diagnostic()
            .wrap_err("failed to convert wasm to wat")?
            .into_boxed_str();

        let root = match input {
            InputType::Real(path) => SourceFileProvenance {
                path: path.clone().into_boxed_path(),
                content,
            },
            InputType::Stdin { name, .. } => SourceFileProvenance {
                path: name.as_path().to_path_buf().into_boxed_path(),
                content,
            },
        };
        Ok(ProjectSourceProvenanceInputs {
            root,
            support: Default::default(),
        })
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
    ) -> CompilerResult<FrontendOutput> {
        use std::io::Read;

        log::debug!("parsing hir from wasm at {}", path.display());
        let mut file = std::fs::File::open(path)
            .into_diagnostic()
            .wrap_err("could not open input for reading")?;
        let mut bytes = Vec::with_capacity(1024);
        file.read_to_end(&mut bytes).into_diagnostic()?;
        let file_name = path.file_stem().unwrap().to_str().unwrap().to_owned();

        let config = WasmTranslationConfig {
            source_name: file_name.into(),
            remap_path_prefixes: context.session().options.remap_path_prefixes.clone(),
            world: Some(world),
            generate_native_debuginfo: context.session().options.emit_source_locations(),
            ..Default::default()
        };
        self.parse_hir_from_wasm_bytes(&bytes, context, &config)
    }

    fn parse_hir_from_wasm_bytes(
        &self,
        bytes: &[u8],
        context: Rc<Context>,
        config: &WasmTranslationConfig,
    ) -> CompilerResult<wasm::FrontendOutput> {
        let outpub = midenc_frontend_wasm::translate(bytes, config, context.clone())?;
        log::debug!(
            "parsed hir component from wasm bytes with first module name: {}",
            outpub.component.borrow().id()
        );

        Ok(outpub)
    }
}
