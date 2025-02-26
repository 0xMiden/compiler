use alloc::{borrow::ToOwned, collections::BTreeMap, sync::Arc, vec::Vec};
use std::path::Path;

use midenc_hir2::{interner::Symbol, BuilderExt, OpBuilder, SourceSpan};
use midenc_session::{
    diagnostics::{Severity, Spanned},
    InputType, ProjectType,
};

use super::*;

pub struct LinkOutput {
    /// The IR world in which all components/modules are represented as declarations or definitions.
    pub world: builtin::WorldRef,
    /// The IR component which is the primary input being compiled
    pub component: builtin::ComponentRef,
    /// The set of Miden Assembly sources to be provided to the assembler to satisfy link-time
    /// dependencies
    pub masm: Vec<Arc<miden_assembly::ast::Module>>,
    /// The set of MAST libraries to be provided to the assembler to satisfy link-time dependencies
    ///
    /// These are either given via `-l`, or as inputs
    pub mast: Vec<Arc<miden_assembly::Library>>,
    /// The set of link libraries provided to the compiler as MAST packages
    pub packages: BTreeMap<Symbol, Arc<miden_mast_package::Package>>,
}

/// This stage gathers together the parsed inputs, constructs a [World] representing all of the
/// parsed non-Wasm inputs and specified link libraries, and then parses the Wasm input(s) in the
/// context of that world. If successful, there are no undefined symbols present in the program.
///
/// This stage also ensures that any builtins/intrinsics are represented in the IR.
pub struct LinkStage;

impl Stage for LinkStage {
    type Input = Vec<ParseOutput>;
    type Output = LinkOutput;

    fn run(&mut self, inputs: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        // Construct an empty world
        let world = {
            let mut builder = OpBuilder::new(context.clone());
            let world_builder = builder.create::<builtin::World, ()>(SourceSpan::default());
            world_builder()?
        };

        // Construct the empty linker outputs
        let mut masm = Vec::default();
        let mut mast = Vec::default();
        let mut packages = BTreeMap::default();

        // Visit each input, validate it, and update the linker outputs accordingly
        let mut component_wasm = None;
        for input in inputs {
            match input {
                ParseOutput::Wasm(wasm) => {
                    if component_wasm.is_some() {
                        return Err(Report::msg(
                            "only a single wasm input can be provided at a time",
                        ));
                    }
                    component_wasm = Some(wasm);
                }
                ParseOutput::Module(module) => {
                    if matches!(context.session().options.project_type, ProjectType::Library if module.is_executable())
                    {
                        return Err(context
                            .diagnostics()
                            .diagnostic(Severity::Error)
                            .with_message("invalid input")
                            .with_primary_label(
                                module.span(),
                                "cannot pass executable modules as input when compiling a library",
                            )
                            .into_report());
                    } else if module.is_executable() {
                        // If a module is executable, we do not need to represent it in the world
                        // as it is by definition unreachable from any symbols outside of itself.
                        masm.push(module);
                    } else {
                        // We represent library modules in the world so that the symbols are
                        // resolvable.
                        todo!("need type information for masm procedures")
                    }
                }
                ParseOutput::Library(lib) => {
                    mast.push(lib);
                }
                ParseOutput::Package(package) => {
                    packages.insert(Symbol::intern(&package.name), package);
                }
            }
        }

        // Load link libraries now
        for link_lib in context.session().options.link_libraries.iter() {
            log::debug!(
                "registering link library '{}' ({}, from {:#?}) with linker",
                link_lib.name,
                link_lib.kind,
                link_lib.path.as_ref()
            );
            let lib = link_lib.load(context.session()).map(Arc::new)?;
            mast.push(lib);
        }

        // Parse and translate the component WebAssembly using the constructed World
        let component_wasm =
            component_wasm.ok_or_else(|| Report::msg("expected at least one wasm input"))?;
        let component = match component_wasm {
            InputType::Real(path) => parse_hir_from_wasm_file(&path, world, context.clone())?,
            InputType::Stdin { name, input } => {
                let config = wasm::WasmTranslationConfig {
                    source_name: name.file_stem().unwrap().to_owned().into(),
                    world: Some(world),
                    ..Default::default()
                };
                parse_hir_from_wasm_bytes(&input, context.clone(), &config)?
            }
        };

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

        Ok(LinkOutput {
            world,
            component,
            masm,
            mast,
            packages,
        })
    }
}

fn parse_hir_from_wasm_file(
    path: &Path,
    world: builtin::WorldRef,
    context: Rc<Context>,
) -> CompilerResult<builtin::ComponentRef> {
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
        world: Some(world),
        ..Default::default()
    };
    parse_hir_from_wasm_bytes(&bytes, context, &config)
}

fn parse_hir_from_wasm_bytes(
    bytes: &[u8],
    context: Rc<Context>,
    config: &wasm::WasmTranslationConfig,
) -> CompilerResult<builtin::ComponentRef> {
    let component = wasm::translate(bytes, config, context.clone())?;
    log::debug!(
        "parsed hir component from wasm bytes with first module name: {}",
        component.borrow().id()
    );

    Ok(component)
}
