use alloc::{collections::BTreeMap, sync::Arc};
use core::fmt;

use miden_assembly::Library;
use miden_core::utils::DisplayHex;
use miden_mast_package::Package;
use miden_processor::Digest;
use midenc_hir2::{constants::ConstantData, dialects::builtin, interner::Symbol};
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
    /// The address of the `__stack_pointer` global, if such a global has been defined
    pub stack_pointer: Option<u32>,
    /// The set of modules in this component
    #[allow(clippy::vec_box)]
    pub modules: Vec<Box<masm::Module>>,
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
            // Skip printing the standard library modules and intrinsics
            // modules to focus on the user-defined modules and avoid the
            // stack overflow error when printing large programs
            // https://github.com/0xPolygonMiden/miden-formatting/issues/4
            if INTRINSICS_MODULE_NAMES.contains(&module.name()) {
                continue;
            }
            if ["std"].contains(&module.namespace().as_str()) {
                continue;
            } else {
                writeln!(f, "# mod {}\n", &module.name())?;
                writeln!(f, "{}", module)?;
            }
        }
        Ok(())
    }
}

impl MasmComponent {
    pub fn assemble(
        &self,
        _link_libraries: &[Arc<Library>],
        _link_packages: &BTreeMap<Symbol, Arc<Package>>,
        _session: &Session,
    ) -> Result<miden_mast_package::MastArtifact, Report> {
        todo!()
    }
}

/*
impl MasmComponent {
    // Assemble this program to MAST
    pub fn assemble(
        &self,
        link_libraries: &[Arc<Library>],
        link_packages: &BTreeMap<Symbol, Package>,
        session: &Session,
    ) -> Result<Arc<miden_core::Program>, Report> {
        use miden_assembly::{Assembler, CompileOptions};

        let debug_mode = session.options.emit_debug_decorators();

        log::debug!(
            "assembling executable with entrypoint '{}' (debug_mode={})",
            self.entrypoint,
            debug_mode
        );
        let mut assembler =
            Assembler::new(session.source_manager.clone()).with_debug_mode(debug_mode);

        let mut lib_modules = Vec::new();
        // Link extra libraries
        for library in self.library.libraries.iter() {
            for module in library.module_infos() {
                log::debug!("registering '{}' with assembler", module.path());
                lib_modules.push(module.path().to_string());
            }
            assembler.add_library(library)?;
        }

        // Assemble library
        let mut modules: Vec<&Module> = self.library.modules.iter().collect();
        // Sort modules to ensure intrinsics are first since the target compiled module imports them
        modules.sort_by_key(|m| (!INTRINSICS_MODULE_NAMES.contains(&m.id.as_str()), &m.id));
        for module in modules.iter() {
            if lib_modules.contains(&module.id.as_str().to_string()) {
                log::warn!(
                    "module '{}' is already registered with the assembler as library's module, \
                     skipping",
                    module.id
                );
                continue;
            }
            log::debug!("adding '{}' to assembler", module.id.as_str());
            let kind = module.kind;
            let module = module.to_ast(debug_mode).map(Box::new)?;
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
        let main = self.generate_main(self.entrypoint, emit_test_harness);
        let main = main.to_ast(debug_mode).map(Box::new)?;
        let program = assembler.assemble_program(main)?;
        let advice_map: AdviceMap = self
            .rodatas()
            .iter()
            .map(|rodata| (rodata.digest, rodata.to_elements()))
            .collect();
        let new_prog = program.with_advice_map(advice_map);
        Ok(Arc::new(new_prog))
    }

    // Assemble this library to MAST
    pub fn assemble_library(&self, session: &Session) -> Result<Arc<CompiledLibrary>, Report> {
        use miden_assembly::Assembler;

        let debug_mode = session.options.emit_debug_decorators();
        log::debug!(
            "assembling library of {} modules (debug_mode={})",
            self.modules().count(),
            debug_mode
        );

        let mut assembler =
            Assembler::new(session.source_manager.clone()).with_debug_mode(debug_mode);

        let mut lib_modules = Vec::new();
        // Link extra libraries
        for library in self.libraries.iter() {
            for module in library.module_infos() {
                log::debug!("registering '{}' with assembler", module.path());
                lib_modules.push(module.path().to_string());
            }
            assembler.add_library(library)?;
        }

        // Assemble library
        let mut modules = Vec::with_capacity(self.modules.len());
        for module in self.modules.iter() {
            let module_id = module.id.as_str();
            if lib_modules.contains(&module_id.to_string()) {
                log::warn!(
                    "module '{}' is already registered with the assembler as library's module, \
                     skipping",
                    module_id
                );
                continue;
            }
            log::debug!("adding '{}' to assembler", module.id.as_str());
            let module = module.to_ast(debug_mode).map(Box::new)?;
            modules.push(module);
        }
        let lib = assembler.assemble_library(modules)?;
        let advice_map: AdviceMap = self
            .rodatas()
            .iter()
            .map(|rodata| (rodata.digest, rodata.to_elements()))
            .collect();
        let mut mast_forest = lib.mast_forest().as_ref().clone();
        mast_forest.advice_map_mut().extend(advice_map);
        let converted_exports = recover_wasm_cm_interfaces(&lib);
        let lib = CompiledLibrary::new(Arc::new(mast_forest), converted_exports)?;
        Ok(Arc::new(lib))
    }

    /// Generate an executable module which when run expects the raw data segment data to be
    /// provided on the advice stack in the same order as initialization, and the operands of
    /// the entrypoint function on the operand stack.
    fn generate_main(&self, entrypoint: FunctionIdent, emit_test_harness: bool) -> Box<Module> {
        let mut exe = Box::new(Module::new(LibraryNamespace::Exec.into(), ModuleKind::Executable));
        let start_id = FunctionIdent {
            module: Ident::with_empty_span(Symbol::intern(LibraryNamespace::EXEC_PATH)),
            function: Ident::with_empty_span(Symbol::intern(ProcedureName::MAIN_PROC_NAME)),
        };
        let start_sig = Signature::new([], []);
        let mut start = Box::new(Function::new(start_id, start_sig));
        {
            let body = start.body_mut();
            // Initialize dynamic heap
            body.push(Op::PushU32(self.heap_base), SourceSpan::default());
            body.push(
                Op::Exec("intrinsics::mem::heap_init".parse().unwrap()),
                SourceSpan::default(),
            );
            // Initialize data segments from advice stack
            self.emit_data_segment_initialization(body);
            // Possibly initialize test harness
            if emit_test_harness {
                self.emit_test_harness(body);
            }
            // Invoke the program entrypoint
            body.push(Op::Exec(entrypoint), SourceSpan::default());
        }
        exe.push_back(start);
        exe
    }

    fn emit_test_harness(&self, block: &mut Block) {
        let span = SourceSpan::default();

        // Advice Stack: [dest_ptr, num_words, ...]
        block.push(Op::AdvPush(2), span); // => [num_words, dest_ptr] on operand stack
        block.push(Op::Exec("std::mem::pipe_words_to_memory".parse().unwrap()), span);
        // Drop HASH
        block.push(Op::Dropw, span);
        // Drop dest_ptr
        block.push(Op::Drop, span);
    }

    /// Emit the sequence of instructions necessary to consume rodata from the advice stack and
    /// populate the global heap with the data segments of this program, verifying that the
    /// commitments match.
    fn emit_data_segment_initialization(&self, block: &mut miden_assembly::ast::Block) {
        use miden_assembly::ast::{Op, Instruction as Inst};

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
        let pipe_preimage_to_memory = "std::mem::pipe_preimage_to_memory".parse().unwrap();
        for rodata in self.rodata.iter() {
            let span = SourceSpan::default();

            // Move rodata from advice map to advice stack
            block.push(Op::Inst(Inst::PushWord(rodata.digest.into()), span); // COM
            block.push(Op::AdvInjectPushMapVal, span);
            // write_ptr
            block.push(Op::PushU32(rodata.start.waddr), span);
            // num_words
            block.push(Op::PushU32(rodata.size_in_words() as u32), span);
            // [num_words, write_ptr, COM, ..] -> [write_ptr']
            block.push(Op::Exec(pipe_preimage_to_memory), span);
            // drop write_ptr'
            block.push(Op::Drop, span);
        }
    }
}
 */
