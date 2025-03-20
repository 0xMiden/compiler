use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use core::fmt;

use miden_assembly::{ast::InvocationTarget, Library};
use miden_core::{utils::DisplayHex, Program};
use miden_mast_package::{MastArtifact, Package, ProcedureName};
use miden_processor::Digest;
use midenc_hir::{constants::ConstantData, dialects::builtin, interner::Symbol};
use midenc_session::{
    diagnostics::{Report, SourceSpan, Span},
    Session,
};

use crate::{lower::NativePtr, masm};

pub struct MasmComponent {
    pub id: builtin::ComponentId,
    /// The symbol name of the component initializer function
    ///
    /// This function is responsible for initializing global variables and writing data segments
    /// into memory at program startup, and at cross-context call boundaries (in callee prologue).
    pub init: Option<masm::InvocationTarget>,
    /// The symbol name of the program entrypoint, if this component is executable.
    ///
    /// If unset, it indicates that the component is a library, even if it could be made executable.
    pub entrypoint: Option<masm::InvocationTarget>,
    /// The kernel library to link against
    pub kernel: Option<masm::KernelLibrary>,
    /// The rodata segments of this component keyed by the offset of the segment
    pub rodata: Vec<Rodata>,
    /// The address of the start of the global heap
    pub heap_base: u32,
    /// The address of the `__stack_pointer` global, if such a global has been defined
    pub stack_pointer: Option<u32>,
    /// The set of modules in this component
    pub modules: Vec<Arc<masm::Module>>,
}

/// Represents a read-only data segment, combined with its content digest
#[derive(Clone, PartialEq, Eq)]
pub struct Rodata {
    /// The component to which this read-only data segment belongs
    pub component: builtin::ComponentId,
    /// The content digest computed for `data`
    pub digest: Digest,
    /// The address at which the data for this segment begins
    pub start: NativePtr,
    /// The raw binary data for this segment
    pub data: Arc<ConstantData>,
}
impl fmt::Debug for Rodata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rodata")
            .field("digest", &format_args!("{}", DisplayHex::new(&self.digest.as_bytes())))
            .field("start", &self.start)
            .field_with("data", |f| {
                f.debug_struct("ConstantData")
                    .field("len", &self.data.len())
                    .finish_non_exhaustive()
            })
            .finish()
    }
}
impl Rodata {
    pub fn size_in_bytes(&self) -> usize {
        self.data.len()
    }

    pub fn size_in_felts(&self) -> usize {
        self.data.len().next_multiple_of(4) / 4
    }

    pub fn size_in_words(&self) -> usize {
        self.size_in_felts().next_multiple_of(4) / 4
    }

    /// Attempt to convert this rodata object to its equivalent representation in felts
    ///
    /// The resulting felts will be in padded out to the nearest number of words, i.e. if the data
    /// only takes up 3 felts worth of bytes, then the resulting `Vec` will contain 4 felts, so that
    /// the total size is a valid number of words.
    pub fn to_elements(&self) -> Result<Vec<miden_processor::Felt>, String> {
        use miden_core::FieldElement;
        use miden_processor::Felt;

        let data = self.data.as_slice();
        let mut felts = Vec::with_capacity(data.len() / 4);
        let mut iter = data.iter().copied().array_chunks::<4>();
        felts.extend(iter.by_ref().map(|bytes| Felt::new(u32::from_le_bytes(bytes) as u64)));
        if let Some(remainder) = iter.into_remainder() {
            let mut chunk = [0u8; 4];
            for (i, byte) in remainder.into_iter().enumerate() {
                chunk[i] = byte;
            }
            felts.push(Felt::new(u32::from_le_bytes(chunk) as u64));
        }

        let padding = (self.size_in_words() * 4).abs_diff(felts.len());
        felts.resize(felts.len() + padding, Felt::ZERO);

        Ok(felts)
    }
}

inventory::submit! {
    midenc_session::CompileFlag::new("test_harness")
        .long("test-harness")
        .action(midenc_session::FlagAction::SetTrue)
        .help("If present, causes the code generator to emit extra code for the VM test harness")
        .help_heading("Testing")
}

impl fmt::Display for MasmComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::intrinsics::INTRINSICS_MODULE_NAMES;

        for module in self.modules.iter() {
            // Don't print empty modules
            //
            // NOTE(pauls): This is a temporary workaround for the fact that component init
            // functions require a module, and we are not yet emitting component init functions,
            // so the generated module is empty.
            if module.exported_procedures().next().is_none() {
                continue;
            }

            // Skip printing the standard library modules and intrinsics
            // modules to focus on the user-defined modules and avoid the
            // stack overflow error when printing large programs
            // https://github.com/0xPolygonMiden/miden-formatting/issues/4
            let module_name = module.path().path();
            if INTRINSICS_MODULE_NAMES.contains(&module_name.as_ref()) {
                continue;
            }
            if ["std"].contains(&module.namespace().as_str()) {
                continue;
            } else {
                writeln!(f, "# mod {}\n", &module_name)?;
                writeln!(f, "{}", module)?;
            }
        }
        Ok(())
    }
}

impl MasmComponent {
    pub fn assemble(
        &self,
        link_libraries: &[Arc<Library>],
        link_packages: &BTreeMap<Symbol, Arc<Package>>,
        session: &Session,
    ) -> Result<MastArtifact, Report> {
        if let Some(entrypoint) = self.entrypoint.as_ref() {
            self.assemble_program(entrypoint, link_libraries, link_packages, session)
                .map(MastArtifact::Executable)
        } else {
            self.assemble_library(link_libraries, link_packages, session)
                .map(MastArtifact::Library)
        }
    }

    fn assemble_program(
        &self,
        entrypoint: &InvocationTarget,
        link_libraries: &[Arc<Library>],
        _link_packages: &BTreeMap<Symbol, Arc<Package>>,
        session: &Session,
    ) -> Result<Arc<Program>, Report> {
        use miden_assembly::{Assembler, CompileOptions};

        let debug_mode = session.options.emit_debug_decorators();

        log::debug!(
            "assembling executable with entrypoint '{}' (debug_mode={})",
            entrypoint,
            debug_mode
        );
        let mut assembler =
            Assembler::new(session.source_manager.clone()).with_debug_mode(debug_mode);

        let mut lib_modules = BTreeSet::default();
        // Link extra libraries
        for library in link_libraries.iter().cloned() {
            for module in library.module_infos() {
                log::debug!("registering '{}' with assembler", module.path());
                lib_modules.insert(module.path().clone());
            }
            assembler.add_library(library)?;
        }

        // Assemble library
        let mut modules: Vec<Arc<masm::Module>> = self.modules.clone();
        // Sort modules to ensure intrinsics are first since the target compiled module imports them
        modules.sort_by_key(|m| {
            let name = m.path().path().into_owned();
            let is_intrinsic = crate::intrinsics::INTRINSICS_MODULE_NAMES.contains(&name.as_str());
            (!is_intrinsic, name)
        });
        for module in modules.iter().cloned() {
            if lib_modules.contains(module.path()) {
                log::warn!(
                    "module '{}' is already registered with the assembler as library's module, \
                     skipping",
                    module.path()
                );
                continue;
            }
            log::debug!("adding '{}' to assembler", module.path());
            let kind = module.kind();
            assembler.add_module_with_options(
                module,
                CompileOptions {
                    kind,
                    warnings_as_errors: false,
                    path: None,
                },
            )?;
        }

        let emit_test_harness = session.get_flag("test_harness");
        let main = self.generate_main(entrypoint, emit_test_harness)?;
        let program = assembler.assemble_program(main)?;
        let advice_map: miden_core::AdviceMap = self
            .rodata
            .iter()
            .map(|rodata| {
                rodata.to_elements().map_err(Report::msg).map(|felts| (rodata.digest, felts))
            })
            .try_collect()?;
        Ok(Arc::new(program.with_advice_map(advice_map)))
    }

    fn assemble_library(
        &self,
        link_libraries: &[Arc<Library>],
        _link_packages: &BTreeMap<Symbol, Arc<Package>>,
        session: &Session,
    ) -> Result<Arc<Library>, Report> {
        use miden_assembly::Assembler;

        let debug_mode = session.options.emit_debug_decorators();
        log::debug!(
            "assembling library of {} modules (debug_mode={})",
            self.modules.len(),
            debug_mode
        );

        let mut assembler =
            Assembler::new(session.source_manager.clone()).with_debug_mode(debug_mode);

        let mut lib_modules = Vec::new();
        // Link extra libraries
        for library in link_libraries.iter().cloned() {
            for module in library.module_infos() {
                log::debug!("registering '{}' with assembler", module.path());
                lib_modules.push(module.path().clone());
            }
            assembler.add_library(library)?;
        }

        // Assemble library
        let mut modules = Vec::with_capacity(self.modules.len());
        for module in self.modules.iter().cloned() {
            if lib_modules.contains(module.path()) {
                log::warn!(
                    "module '{}' is already registered with the assembler as library's module, \
                     skipping",
                    module.path()
                );
                continue;
            }
            log::debug!("adding '{}' to assembler", module.path());
            modules.push(module);
        }
        let lib = assembler.assemble_library(modules)?;
        let advice_map: miden_core::AdviceMap = self
            .rodata
            .iter()
            .map(|rodata| {
                rodata.to_elements().map_err(Report::msg).map(|felts| (rodata.digest, felts))
            })
            .try_collect()?;

        let converted_exports = recover_wasm_cm_interfaces(&lib);

        // Get a reference to the library MAST, then drop the library so we can obtain a mutable
        // reference so we can modify its advice map data
        let mut mast_forest = lib.mast_forest().clone();
        drop(lib);
        {
            let mast = Arc::get_mut(&mut mast_forest).expect("expected unique reference");
            mast.advice_map_mut().extend(advice_map);
        }

        // Reconstruct the library with the updated MAST
        Ok(Library::new(mast_forest, converted_exports).map(Arc::new)?)
    }

    /// Generate an executable module which when run expects the raw data segment data to be
    /// provided on the advice stack in the same order as initialization, and the operands of
    /// the entrypoint function on the operand stack.
    fn generate_main(
        &self,
        entrypoint: &InvocationTarget,
        emit_test_harness: bool,
    ) -> Result<Arc<masm::Module>, Report> {
        use masm::{Instruction as Inst, Op};

        let mut exe = Box::new(masm::Module::new_executable());
        let span = SourceSpan::default();
        let body = {
            let mut block = masm::Block::new(span, Vec::with_capacity(64));
            // Initialize dynamic heap
            block.push(Op::Inst(Span::new(span, Inst::PushU32(self.heap_base))));
            let heap_init = masm::ProcedureName::new("heap_init").unwrap();
            let memory_intrinsics = masm::LibraryPath::new("intrinsics::mem").unwrap();
            block.push(Op::Inst(Span::new(
                span,
                Inst::Exec(InvocationTarget::AbsoluteProcedurePath {
                    name: heap_init,
                    path: memory_intrinsics,
                }),
            )));
            // Initialize data segments from advice stack
            self.emit_data_segment_initialization(&mut block);
            // Possibly initialize test harness
            if emit_test_harness {
                self.emit_test_harness(&mut block);
            }
            // Invoke the program entrypoint
            block.push(Op::Inst(Span::new(span, Inst::Exec(entrypoint.clone()))));
            // Truncate the stack to 16 elements on exit
            let truncate_stack = InvocationTarget::AbsoluteProcedurePath {
                name: ProcedureName::new("truncate_stack").unwrap(),
                path: masm::LibraryPath::new_from_components(
                    masm::LibraryNamespace::new("std").unwrap(),
                    [masm::Ident::new("sys").unwrap()],
                ),
            };
            block.push(Op::Inst(Span::new(span, Inst::Exec(truncate_stack))));
            block
        };
        let start = masm::Procedure::new(
            span,
            masm::Visibility::Public,
            masm::ProcedureName::main(),
            0,
            body,
        );
        exe.define_procedure(masm::Export::Procedure(start))?;
        Ok(Arc::from(exe))
    }

    fn emit_test_harness(&self, block: &mut masm::Block) {
        use masm::{Instruction as Inst, Op};

        let span = SourceSpan::default();

        let pipe_words_to_memory = masm::ProcedureName::new("pipe_words_to_memory").unwrap();
        let std_mem = masm::LibraryPath::new("std::mem").unwrap();

        // Advice Stack: [dest_ptr, num_words, ...]

        // => [num_words, dest_ptr] on operand stack
        block.push(Op::Inst(Span::new(span, Inst::AdvPush(2.into()))));
        block.push(Op::Inst(Span::new(
            span,
            Inst::Exec(InvocationTarget::AbsoluteProcedurePath {
                name: pipe_words_to_memory,
                path: std_mem,
            }),
        )));
        // Drop HASH
        block.push(Op::Inst(Span::new(span, Inst::DropW)));
        // Drop dest_ptr
        block.push(Op::Inst(Span::new(span, Inst::Drop)));
    }

    /// Emit the sequence of instructions necessary to consume rodata from the advice stack and
    /// populate the global heap with the data segments of this program, verifying that the
    /// commitments match.
    fn emit_data_segment_initialization(&self, block: &mut masm::Block) {
        use masm::{Instruction as Inst, Op};

        // Emit data segment initialization code
        //
        // NOTE: This depends on the program being executed with the data for all data
        // segments having been placed in the advice map with the same commitment and
        // encoding used here. The program will fail to execute if this is not set up
        // correctly.
        //
        // TODO(pauls): To facilitate automation of this, we should emit an inputs file to
        // disk that maps each segment to a commitment and its data encoded as binary. This
        // can then be loaded into the advice provider during VM init.
        let pipe_preimage_to_memory = masm::ProcedureName::new("pipe_preimage_to_memory").unwrap();
        let std_mem = masm::LibraryPath::new("std::mem").unwrap();

        let span = SourceSpan::default();
        for rodata in self.rodata.iter() {
            // Move rodata from advice map to advice stack
            block.push(Op::Inst(Span::new(span, Inst::PushWord(rodata.digest.into())))); // COM
            block
                .push(Op::Inst(Span::new(span, Inst::SysEvent(masm::SystemEventNode::PushMapVal))));
            // write_ptr
            block.push(Op::Inst(Span::new(span, Inst::PushU32(rodata.start.waddr))));
            // num_words
            block.push(Op::Inst(Span::new(span, Inst::PushU32(rodata.size_in_words() as u32))));
            // [num_words, write_ptr, COM, ..] -> [write_ptr']
            block.push(Op::Inst(Span::new(
                span,
                Inst::Exec(InvocationTarget::AbsoluteProcedurePath {
                    name: pipe_preimage_to_memory.clone(),
                    path: std_mem.clone(),
                }),
            )));
            // drop write_ptr'
            block.push(Op::Inst(Span::new(span, Inst::Drop)));
        }
    }
}

/// Try to recognize Wasm CM interfaces and transform those exports to have Wasm interface encoded
/// as module name.
///
/// Temporary workaround for:
///
/// 1. Temporary exporting multiple interfaces from the same(Wasm core) module (an interface is
///    encoded in the function name)
///
/// 2. Assembler using the current module name to generate exports.
///
fn recover_wasm_cm_interfaces(
    lib: &Library,
) -> BTreeMap<masm::QualifiedProcedureName, miden_processor::MastNodeId> {
    use crate::intrinsics::INTRINSICS_MODULE_NAMES;

    let mut exports = BTreeMap::new();
    for export in lib.exports() {
        let export_node_id = lib.get_export_node_id(export);
        if INTRINSICS_MODULE_NAMES.contains(&export.module.to_string().as_str())
            || export.name.as_str().starts_with("cabi")
        {
            // Preserve intrinsics modules and internal Wasm CM `cabi_*` functions
            exports.insert(export.clone(), export_node_id);
            continue;
        }

        if let Some((component, interface)) = export.name.as_str().rsplit_once('/') {
            // Wasm CM interface
            let (interface, function) =
                interface.rsplit_once('#').expect("invalid wasm component model identifier");

            let mut component_parts = component.split(':').map(Arc::from);
            let ns = masm::LibraryNamespace::User(
                component_parts.next().expect("invalid wasm component model identifier"),
            );
            let component_parts = component_parts
                .map(Span::unknown)
                .map(masm::Ident::new_unchecked)
                .chain([masm::Ident::new_unchecked(Span::unknown(Arc::from(interface)))]);
            let path = masm::LibraryPath::new_from_components(ns, component_parts);
            let name = masm::ProcedureName::new_unchecked(masm::Ident::new_unchecked(
                Span::unknown(Arc::from(function)),
            ));
            let new_export = masm::QualifiedProcedureName::new(path, name);
            exports.insert(new_export, export_node_id);
        } else {
            // Non-Wasm CM interface, preserve as is
            exports.insert(export.clone(), export_node_id);
        }
    }
    exports
}
