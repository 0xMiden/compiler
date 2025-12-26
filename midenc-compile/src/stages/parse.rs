#[cfg(feature = "std")]
use alloc::string::ToString;
#[cfg(feature = "std")]
use alloc::{borrow::Cow, string::String};
use alloc::{format, rc::Rc, sync::Arc};
#[cfg(feature = "std")]
use core::fmt;

use miden_assembly::utils::Deserializable;
#[cfg(feature = "std")]
use miden_assembly::utils::ReadAdapter;
#[cfg(feature = "std")]
use midenc_session::{Emit, OutputType, Writer};
#[cfg(feature = "std")]
use midenc_session::{FileName, Path};
use midenc_session::{
    InputFile, InputType,
    diagnostics::{IntoDiagnostic, WrapErr},
};

use super::*;

/// A wrapper that emits WebAssembly text format (WAT).
#[cfg(feature = "std")]
struct WatEmit<'a>(&'a str);

#[cfg(feature = "std")]
impl Emit for WatEmit<'_> {
    fn name(&self) -> Option<midenc_hir::interner::Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Wat
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        if mode != OutputMode::Text {
            anyhow::bail!("wat emission does not support binary mode");
        }
        writer.write_fmt(format_args!("{}", self.0))?;
        Ok(())
    }
}

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
                #[cfg(not(feature = "std"))]
                FileType::Wat => unimplemented!(),
                #[cfg(feature = "std")]
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
                #[cfg(not(feature = "std"))]
                FileType::Wat => unimplemented!(),
                #[cfg(feature = "std")]
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
            #[cfg(feature = "std")]
            ParseOutput::Wasm(ref wasm_input) => {
                self.emit_wat_for_wasm_input(wasm_input, context.session())?;
            }
            #[cfg(not(feature = "std"))]
            ParseOutput::Wasm(_) => (),
            ParseOutput::Library(_) | ParseOutput::Package(_) => (),
        }

        Ok(parsed)
    }
}
impl ParseStage {
    #[cfg(feature = "std")]
    fn emit_wat_for_wasm_input(&self, input: &InputType, session: &Session) -> CompilerResult<()> {
        if !session.should_emit(OutputType::Wat) {
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

        let wat = wasm_to_wat(wasm_bytes.as_ref())?;
        let artifact = WatEmit(&wat);
        session
            .emit(OutputMode::Text, &artifact)
            .into_diagnostic()
            .wrap_err("failed to emit wat output")?;
        Ok(())
    }

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
            LibraryNamespace, LibraryPath,
            ast::{self, Ident, ModuleKind},
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
            LibraryPath,
            ast::{self, ModuleKind},
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

/// Convert a WebAssembly binary to WAT text, filtering out highly variable custom sections.
#[cfg(feature = "std")]
fn wasm_to_wat(wasm_bytes: &[u8]) -> CompilerResult<String> {
    // Disable printing of the various custom sections, e.g. "producers", either because they
    // contain strings which are highly variable (but not important), or because they are debug info
    // related.
    struct NoCustomSectionsPrinter<T: wasmprinter::Print>(T);
    impl<T: wasmprinter::Print> wasmprinter::Print for NoCustomSectionsPrinter<T> {
        fn write_str(&mut self, s: &str) -> std::io::Result<()> {
            self.0.write_str(s)
        }

        fn newline(&mut self) -> std::io::Result<()> {
            self.0.newline()
        }

        fn start_line(&mut self, binary_offset: Option<usize>) {
            self.0.start_line(binary_offset);
        }

        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> std::io::Result<()> {
            self.0.write_fmt(args)
        }

        fn print_custom_section(
            &mut self,
            name: &str,
            binary_offset: usize,
            data: &[u8],
        ) -> std::io::Result<bool> {
            match name {
                "producers" | "target_features" => Ok(true),
                debug if debug.starts_with(".debug") => Ok(true),
                _ => self.0.print_custom_section(name, binary_offset, data),
            }
        }

        fn start_literal(&mut self) -> std::io::Result<()> {
            self.0.start_literal()
        }

        fn start_name(&mut self) -> std::io::Result<()> {
            self.0.start_name()
        }

        fn start_keyword(&mut self) -> std::io::Result<()> {
            self.0.start_keyword()
        }

        fn start_type(&mut self) -> std::io::Result<()> {
            self.0.start_type()
        }

        fn start_comment(&mut self) -> std::io::Result<()> {
            self.0.start_comment()
        }

        fn reset_color(&mut self) -> std::io::Result<()> {
            self.0.reset_color()
        }

        fn supports_async_color(&self) -> bool {
            self.0.supports_async_color()
        }
    }

    // WAT text should be at least ~3x larger than the binary Wasm representation
    let mut wat = String::with_capacity(wasm_bytes.len() * 3);
    let config = wasmprinter::Config::new();
    let mut wasm_printer = NoCustomSectionsPrinter(wasmprinter::PrintFmtWrite(&mut wat));
    config
        .print(wasm_bytes, &mut wasm_printer)
        .into_diagnostic()
        .wrap_err("failed to convert wasm to wat")?;
    Ok(wat)
}
