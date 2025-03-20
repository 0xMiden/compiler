#[cfg(feature = "std")]
use alloc::string::ToString;
use alloc::{format, rc::Rc, sync::Arc};

use miden_assembly::utils::Deserializable;
#[cfg(feature = "std")]
use miden_assembly::utils::ReadAdapter;
use midenc_session::{
    diagnostics::{IntoDiagnostic, WrapErr},
    InputFile, InputType,
};
#[cfg(feature = "std")]
use midenc_session::{FileName, Path};

use super::*;

/// This represents the output of the parser, depending on the type of input that was parsed/loaded.
pub enum ParseOutput {
    /// We found a WebAssembly binary representing a component or core module.
    ///
    /// This input type is processed in a later stage, here we are only interested in other input
    /// types.
    Wasm(InputType),
    /// A single Miden Assembly module was given as an input
    Module(Arc<miden_assembly::ast::Module>),
    /// A MAST library was given as an input
    Library(Arc<miden_assembly::Library>),
    /// A Miden package was given as an input
    Package(Arc<miden_mast_package::Package>),
}

/// This stage of compilation is where we parse input files into the earliest representation
/// supported by the input file type. Later stages will handle lowering as needed.
pub struct ParseStage;

impl Stage for ParseStage {
    type Input = InputFile;
    type Output = ParseOutput;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_session::{FileType, InputType};

        let file_type = input.file_type();
        let parsed = match input.file {
            #[cfg(not(feature = "std"))]
            InputType::Real(_path) => unimplemented!(),
            #[cfg(feature = "std")]
            InputType::Real(path) => match file_type {
                FileType::Hir => {
                    Err(Report::msg("invalid input: hir parsing is temporarily unsupported"))
                }
                FileType::Wasm => Ok(ParseOutput::Wasm(InputType::Real(path))),
                FileType::Wat => self.parse_wasm_from_wat_file(path.as_ref()),
                FileType::Masm => self.parse_masm_from_file(path.as_ref(), context.clone()),
                FileType::Mast => miden_assembly::Library::deserialize_from_file(&path)
                    .map(Arc::new)
                    .map(ParseOutput::Library)
                    .map_err(|err| {
                        Report::msg(format!(
                            "invalid input: could not deserialize mast library: {err}"
                        ))
                    }),
                FileType::Masp => {
                    let mut file = std::fs::File::open(&path).map_err(|err| {
                        Report::msg(format!("cannot open {} for reading: {err}", path.display()))
                    })?;
                    let mut adapter = ReadAdapter::new(&mut file);
                    miden_mast_package::Package::read_from(&mut adapter)
                        .map(Arc::new)
                        .map(ParseOutput::Package)
                        .map_err(|err| {
                            Report::msg(format!(
                                "failed to load mast package from {}: {err}",
                                path.display()
                            ))
                        })
                }
            },
            InputType::Stdin { name, input } => match file_type {
                FileType::Hir => {
                    Err(Report::msg("invalid input: hir parsing is temporarily unsupported"))
                }
                FileType::Wasm => Ok(ParseOutput::Wasm(InputType::Stdin { name, input })),
                FileType::Wat => {
                    let wasm = wat::parse_bytes(&input)
                        .into_diagnostic()
                        .wrap_err("failed to parse wat")?;
                    Ok(ParseOutput::Wasm(InputType::Stdin {
                        name,
                        input: wasm.into_owned(),
                    }))
                }
                FileType::Masm => {
                    self.parse_masm_from_bytes(name.as_str(), &input, context.clone())
                }
                FileType::Mast => miden_assembly::Library::read_from_bytes(&input)
                    .map(Arc::new)
                    .map(ParseOutput::Library)
                    .map_err(|err| {
                        Report::msg(format!(
                            "invalid input: could not deserialize mast library: {err}"
                        ))
                    }),
                FileType::Masp => miden_mast_package::Package::read_from_bytes(&input)
                    .map(Arc::new)
                    .map(ParseOutput::Package)
                    .map_err(|err| {
                        Report::msg(format!(
                            "invalid input: failed to load mast package from {name}: {err}"
                        ))
                    }),
            },
        }?;

        match parsed {
            ParseOutput::Module(ref module) => {
                context.session().emit(OutputMode::Text, module).into_diagnostic()?;
            }
            ParseOutput::Wasm(_) | ParseOutput::Library(_) | ParseOutput::Package(_) => (),
        }

        Ok(parsed)
    }
}
impl ParseStage {
    #[cfg(feature = "std")]
    fn parse_wasm_from_wat_file(&self, path: &Path) -> CompilerResult<ParseOutput> {
        let wasm = wat::parse_file(path).into_diagnostic().wrap_err("failed to parse wat")?;
        Ok(ParseOutput::Wasm(InputType::Stdin {
            name: FileName::from(path.to_path_buf()),
            input: wasm,
        }))
    }

    #[cfg(feature = "std")]
    fn parse_masm_from_file(
        &self,
        path: &Path,
        context: Rc<Context>,
    ) -> CompilerResult<ParseOutput> {
        use miden_assembly::{
            ast::{self, Ident, ModuleKind},
            LibraryNamespace, LibraryPath,
        };

        // Construct library path for MASM module
        let module_name = Ident::new(path.file_stem().unwrap().to_str().unwrap())
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "failed to construct valid module identifier from path '{}'",
                    path.display()
                )
            })?;
        let namespace = path
            .parent()
            .map(|dir| {
                LibraryNamespace::User(dir.to_str().unwrap().to_string().into_boxed_str().into())
            })
            .unwrap_or(LibraryNamespace::Anon);
        let name = LibraryPath::new_from_components(namespace, [module_name]);

        // Parse AST
        let mut parser = ast::Module::parser(ModuleKind::Library);
        let ast = parser.parse_file(name, path, &context.session().source_manager)?;

        Ok(ParseOutput::Module(Arc::from(ast)))
    }

    fn parse_masm_from_bytes(
        &self,
        name: &str,
        bytes: &[u8],
        context: Rc<Context>,
    ) -> CompilerResult<ParseOutput> {
        use miden_assembly::{
            ast::{self, ModuleKind},
            LibraryPath,
        };

        let source = core::str::from_utf8(bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("input '{name}' contains invalid utf-8"))?;

        // Construct library path for MASM module
        let name = LibraryPath::new(name).into_diagnostic()?;

        // Parse AST
        let mut parser = ast::Module::parser(ModuleKind::Library);
        let ast = parser.parse_str(name, source, &context.session().source_manager)?;

        Ok(ParseOutput::Module(Arc::from(ast)))
    }
}
