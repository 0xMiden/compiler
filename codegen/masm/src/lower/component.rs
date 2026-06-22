use alloc::{collections::BTreeSet, sync::Arc, vec::Vec};

use miden_assembly::{PathBuf as LibraryPath, ast::InvocationTarget};
use miden_assembly_syntax::{ast::Attribute, parser::WordValue};
use miden_core::operations::DebugVarLocation;
use midenc_hir::{
    FunctionIdent, Op, OpExt, SourceSpan, Span, Symbol, TraceTarget, Type, ValueRef,
    diagnostics::IntoDiagnostic,
    dialects::{
        builtin,
        debuginfo::attributes::{
            SubprogramAttr, decode_frame_base_local_index, encode_frame_base_local_offset,
        },
    },
    interner,
    pass::AnalysisManager,
};
use midenc_hir_analysis::analyses::LivenessAnalysis;
use midenc_session::diagnostics::{Report, Spanned, WrapErr};
use smallvec::SmallVec;

use crate::{
    OperandStack, TraceEvent,
    artifact::MasmComponent,
    emitter::BlockEmitter,
    linker::{LinkInfo, Linker},
    masm,
};

/// This trait represents a conversion pass from some HIR entity to a Miden Assembly component.
pub trait ToMasmComponent {
    fn to_masm_component(&self, analysis_manager: AnalysisManager)
    -> Result<MasmComponent, Report>;
}

/// Derivation of a MASM component from an HIR world
///
/// This currently works by treating all definition-carrying modules in the world as part of a
/// single logical component.
impl ToMasmComponent for builtin::World {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
    ) -> Result<MasmComponent, Report> {
        // Get the current compiler context
        let context = self.as_operation().context_rc();

        // Run the linker for this component in order to compute its data layout
        let link_info = Linker::default().link(None, self.as_operation()).map_err(Report::msg)?;

        // Get the entrypoint, if specified
        let entrypoint = match context.session().options.entrypoint.as_deref() {
            Some(entry) => {
                let entry_id = entry.parse::<FunctionIdent>().map_err(|_| {
                    Report::msg(format!("invalid entrypoint identifier: '{entry}'"))
                })?;
                let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
                    Span::new(entry_id.function.span, entry_id.function.as_str().into()),
                ));

                let path = LibraryPath::new(entry_id.module.as_str()).into_diagnostic()?;
                let qualified = masm::QualifiedProcedureName::new(path.as_path(), name);
                Some(masm::InvocationTarget::Path(Span::new(
                    entry_id.function.span,
                    qualified.into_inner(),
                )))
            }
            None => None,
        };

        // If we have global variables or data segments, we will require a component initializer
        // function, as well as a module to hold component-level functions such as init
        let requires_init = link_info.has_globals() || link_info.has_data_segments();

        // Define the initial component modules set
        //
        // The top-level component module is always defined, but may be empty
        let root =
            Arc::<miden_assembly_syntax::Path>::from(miden_assembly_syntax::Path::new("::init"));
        let init_module = Arc::new(masm::Module::new(masm::ModuleKind::Library, &root));
        let modules = vec![init_module];

        let rodata = data_segments_to_rodata(&link_info)?;

        let kernel = if context.session().options.target_requires_protocol() {
            Some(miden_protocol::transaction::TransactionKernel::kernel())
        } else {
            None
        };
        let emit_executable_main = context.session().is_executable_target();
        let emit_test_harness = context.session().get_flag("test_harness");

        // Compute the first page boundary after the end of the globals table (or reserved memory
        // if no globals) to use as the start of the dynamic heap when the program is executed
        let heap_base = core::cmp::max(
            link_info.reserved_memory_bytes(),
            link_info.globals_layout().next_page_boundary() as usize,
        );
        let heap_base = u32::try_from(heap_base)
            .expect("unable to allocate dynamic heap: global table too large");
        let stack_pointer = link_info.globals_layout().stack_pointer_offset();
        let mut masm_component = MasmComponent {
            id: None,
            root,
            requires_init,
            entrypoint,
            kernel,
            rodata,
            heap_base,
            stack_pointer,
            link_support_modules_privately: false,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
            source_manager: context.session().source_manager.clone(),
            init_body: Default::default(),
            invoked_from_init: Default::default(),
            emit_executable_main,
            emit_test_harness,
        };

        builder.build(self.as_operation())?;

        Ok(masm_component)
    }
}

/// 1:1 conversion from HIR component to MASM component
impl ToMasmComponent for builtin::Component {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
    ) -> Result<MasmComponent, Report> {
        // Get the current compiler context
        let context = self.as_operation().context_rc();

        // Run the linker for this component in order to compute its data layout
        let id = self.id();
        let link_info = Linker::default()
            .link(Some(id.clone()), self.as_operation())
            .map_err(Report::msg)?;

        // Get the library path of the component
        let component_path = id.to_library_path();

        // Get the entrypoint, if specified
        let entrypoint = match context.session().options.entrypoint.as_deref() {
            Some(entry) => {
                let entry_id = entry.parse::<FunctionIdent>().map_err(|_| {
                    Report::msg(format!("invalid entrypoint identifier: '{entry}'"))
                })?;
                let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
                    Span::new(entry_id.function.span, entry_id.function.as_str().into()),
                ));

                // Check if we're inside the synthetic "wrapper" component used for pure Rust
                // compilation. Since the user does not know about it, their entrypoint does not
                // include the synthetic component path. We append the user-provided path to the
                // root component path here if needed.
                //
                // TODO(pauls): Narrow this to only be true if the target env is not 'rollup', we
                // cannot currently do so because we do not have sufficient Cargo metadata yet in
                // 'cargo miden build' to detect the target env, and we default it to 'rollup'
                let is_wrapper = id.is_synthetic_wrapper();
                let path = if is_wrapper {
                    let mut path = component_path.clone();
                    path.push(entry_id.module.as_str());
                    path
                } else {
                    // We're compiling a Wasm component and the component id is included
                    // in the entrypoint.
                    LibraryPath::new(entry_id.module.as_str()).into_diagnostic()?
                };
                let qualified = masm::QualifiedProcedureName::new(path.as_path(), name);
                Some(masm::InvocationTarget::Path(Span::new(
                    entry_id.function.span,
                    qualified.into_inner(),
                )))
            }
            None => None,
        };

        // If we have global variables or data segments, we will require a component initializer
        // function, as well as a module to hold component-level functions such as init
        let requires_init = link_info.has_globals() || link_info.has_data_segments();

        // Define the initial component modules set
        //
        // The top-level component module is always defined, but may be empty
        let root: Arc<miden_assembly_syntax::Path> =
            id.to_library_path().to_absolute().into_owned().into();
        let modules = vec![Arc::new(masm::Module::new(masm::ModuleKind::Library, &root))];

        let rodata = data_segments_to_rodata(&link_info)?;

        let kernel = if context.session().options.target_requires_protocol() {
            Some(miden_protocol::transaction::TransactionKernel::kernel())
        } else {
            None
        };
        let emit_executable_main = context.session().is_executable_target();
        let emit_test_harness = context.session().get_flag("test_harness");

        // Compute the first page boundary after the end of the globals table (or reserved memory
        // if no globals) to use as the start of the dynamic heap when the program is executed
        let heap_base = core::cmp::max(
            link_info.reserved_memory_bytes(),
            link_info.globals_layout().next_page_boundary() as usize,
        );
        let heap_base = u32::try_from(heap_base)
            .expect("unable to allocate dynamic heap: global table too large");
        let stack_pointer = link_info.globals_layout().stack_pointer_offset();
        let link_support_modules_privately = !id.is_synthetic_wrapper();
        let mut masm_component = MasmComponent {
            id: Some(id),
            root,
            requires_init,
            entrypoint,
            kernel,
            rodata,
            heap_base,
            stack_pointer,
            link_support_modules_privately,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
            source_manager: context.session().source_manager.clone(),
            init_body: Default::default(),
            invoked_from_init: Default::default(),
            emit_executable_main,
            emit_test_harness,
        };

        builder.build(self.as_operation())?;

        Ok(masm_component)
    }
}

fn data_segments_to_rodata(link_info: &LinkInfo) -> Result<Vec<crate::Rodata>, Report> {
    use midenc_hir::constants::ConstantData;

    use crate::data_segments::{ResolvedDataSegment, merge_data_segments};
    let mut resolved = SmallVec::<[ResolvedDataSegment; 2]>::new();
    for sref in link_info.segment_layout().iter() {
        let s = sref.borrow();
        resolved.push(ResolvedDataSegment {
            offset: *s.get_offset(),
            data: s.initializer().as_slice().to_vec(),
            readonly: *s.get_readonly(),
        });
    }
    Ok(match merge_data_segments(resolved).map_err(Report::msg)? {
        None => alloc::vec::Vec::new(),
        Some(merged) => {
            let data = alloc::sync::Arc::new(ConstantData::from(merged.data));
            let felts = crate::Rodata::bytes_to_elements(data.as_slice());
            let digest = miden_core::crypto::hash::Poseidon2::hash_elements(&felts);
            alloc::vec![crate::Rodata {
                component: link_info.component().cloned().unwrap_or(builtin::ComponentId {
                    namespace: interner::Symbol::intern("root_ns"),
                    name: interner::Symbol::intern("root"),
                    version: midenc_hir::version::Version::new(1, 0, 0)
                }),
                digest,
                start: super::NativePtr::from_ptr(merged.offset),
                data,
            }]
        }
    })
}

struct MasmComponentBuilder<'a> {
    component: &'a mut MasmComponent,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
    source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
    init_body: Vec<masm::Op>,
    invoked_from_init: BTreeSet<masm::Invoke>,
    emit_executable_main: bool,
    emit_test_harness: bool,
}

impl MasmComponentBuilder<'_> {
    /// Convert the component body to Miden Assembly
    pub fn build(mut self, component: &midenc_hir::Operation) -> Result<(), Report> {
        use masm::{Instruction as Inst, Op};

        // If a component-level init is required, emit code to initialize the heap before any other
        // initialization code.
        if self.component.requires_init {
            let span = component.span();

            // Heap metadata initialization
            let heap_base = self.component.heap_base;
            self.init_body.push(masm::Op::Inst(Span::new(
                span,
                Inst::Push(masm::Immediate::Value(Span::unknown(heap_base.into()))),
            )));
            let heap_init = qualified_procedure_target("::intrinsics::mem", "heap_init", span);
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Trace(TraceEvent::FrameStart.as_u32().into()),
            )));
            self.init_body.push(Op::Inst(Span::new(span, Inst::Exec(heap_init))));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));

            // Data segment initialization
            self.emit_data_segment_initialization();
        }

        // Translate component body
        let region = component.region(0);
        let block = region.entry();
        for op in block.body() {
            if let Some(module) = op.downcast_ref::<builtin::Module>() {
                self.define_module(module)?;
            } else if let Some(interface) = op.downcast_ref::<builtin::Interface>() {
                self.define_interface(interface)?;
            } else if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else {
                panic!(
                    "invalid component-level operation: '{}' is not supported in a component body",
                    op.name()
                )
            }
        }

        // Finalize the component-level init, if required
        if self.component.requires_init {
            let init_body = core::mem::take(&mut self.init_body);
            let invoked_from_init = core::mem::take(&mut self.invoked_from_init);
            let root_module =
                Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");

            define_init_procedure(
                root_module,
                init_body,
                invoked_from_init,
                component.span(),
                self.source_manager.clone(),
            )
            .wrap_err("failed to define component `init` procedure")?;
        } else {
            assert!(
                self.init_body.is_empty(),
                "the need for an 'init' function was not expected, but code was generated for one"
            );
        }

        if self.emit_executable_main
            && let Some(entrypoint) = self.component.entrypoint.clone()
        {
            let span = component.span();
            let root = self.component.root.clone();
            let init = self.component.requires_init.then(|| local_procedure_target("init", span));
            let entrypoint = localize_root_invocation_target(&entrypoint, root.as_ref());
            let root_module =
                Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");

            define_main_procedure(
                root_module,
                init,
                entrypoint,
                self.emit_test_harness,
                span,
                self.source_manager.clone(),
            )
            .wrap_err("failed to define executable `main` procedure")?;
        }

        Ok(())
    }

    fn define_interface(&mut self, interface: &builtin::Interface) -> Result<(), Report> {
        let interface_path = if let Some(id) = self.component.id.as_ref() {
            let mut path = id.to_library_path();
            path.push(interface.name().as_str());
            path
        } else {
            interface.path().to_library_path()
        };
        let mut masm_module =
            Box::new(masm::Module::new(masm::ModuleKind::Library, interface_path));
        let builder = MasmModuleBuilder {
            module: &mut masm_module,
            analysis_manager: self
                .analysis_manager
                .nest(interface.as_operation().as_operation_ref()),
            link_info: self.link_info,
            source_manager: self.source_manager.clone(),
            init_body: &mut self.init_body,
            invoked_from_init: &mut self.invoked_from_init,
        };
        builder.build_from_interface(interface)?;

        self.component.modules.push(Arc::from(masm_module));

        Ok(())
    }

    fn define_module(&mut self, module: &builtin::Module) -> Result<(), Report> {
        let module_path = if let Some(id) = self.component.id.as_ref() {
            let mut path = id.to_library_path();
            path.push(module.name().as_str());
            path
        } else {
            module.path().to_library_path()
        };
        let mut masm_module = Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
        let builder = MasmModuleBuilder {
            module: &mut masm_module,
            analysis_manager: self.analysis_manager.nest(module.as_operation_ref()),
            link_info: self.link_info,
            source_manager: self.source_manager.clone(),
            init_body: &mut self.init_body,
            invoked_from_init: &mut self.invoked_from_init,
        };
        builder.build(module)?;

        self.component.modules.push(Arc::from(masm_module));

        Ok(())
    }

    fn define_function(&mut self, function: &builtin::Function) -> Result<(), Report> {
        let builder = MasmFunctionBuilder::new(function)?;
        let init = self
            .component
            .requires_init
            .then(|| local_procedure_target("init", function.span()));
        let procedure = builder.build(
            function,
            self.analysis_manager.nest(function.as_operation_ref()),
            self.link_info,
            init,
        )?;

        let module =
            Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");
        let expected_path_len = if module.path().is_absolute() { 2 } else { 1 };
        assert_eq!(
            module.path().len(),
            expected_path_len,
            "expected top-level namespace module, but one has not been defined (in '{}' of '{}')",
            module.path(),
            function.path()
        );
        module
            .define_procedure(procedure, self.source_manager.clone())
            .into_diagnostic()
            .wrap_err("failed to define MASM procedure")?;

        Ok(())
    }

    /// Emit the sequence of instructions necessary to consume rodata from the advice stack and
    /// populate the global heap with the data segments of this component, verifying that the
    /// commitments match.
    fn emit_data_segment_initialization(&mut self) {
        use masm::{Instruction as Inst, Op};

        // Emit data segment initialization code
        //
        // NOTE: This depends on the program being executed with the data for all data segments
        // having been placed in the advice map with the same commitment and encoding used here.
        // The program will fail to execute if this is not set up correctly.
        let span = SourceSpan::default();
        let pipe_preimage_to_memory =
            qualified_procedure_target("::miden::core::mem", "pipe_preimage_to_memory", span);
        for rodata in self.component.rodata.iter() {
            // Push the commitment hash (`COM`) for this data onto the operand stack

            // WARNING: These two are equivalent, shouldn't this be a no-op?
            let word = rodata.digest.as_elements();
            let word_value = [word[0], word[1], word[2], word[3]];

            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Push(masm::Immediate::Value(Span::unknown(WordValue(word_value).into()))),
            )));
            // Move rodata from the advice map, using the commitment as key, to the advice stack
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::SysEvent(masm::SystemEventNode::PushMapVal))));
            // write_ptr
            assert!(rodata.start.is_word_aligned(), "rodata segments must be word-aligned");
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Push(masm::Immediate::Value(Span::unknown(rodata.start.addr.into()))),
            )));
            // num_words
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Push(masm::Immediate::Value(Span::unknown(
                    (rodata.size_in_words() as u32).into(),
                ))),
            )));
            // [num_words, write_ptr, COM, ..] -> [write_ptr']
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Trace(TraceEvent::FrameStart.as_u32().into()),
            )));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Exec(pipe_preimage_to_memory.clone()))));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));
            // drop write_ptr'
            self.init_body.push(Op::Inst(Span::new(span, Inst::Drop)));
        }
    }
}

/// Define the private component initializer in `module`.
fn define_init_procedure(
    module: &mut masm::Module,
    body: Vec<masm::Op>,
    invoked: BTreeSet<masm::Invoke>,
    span: SourceSpan,
    source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
) -> Result<(), Report> {
    let init_name = masm::ProcedureName::new("init").unwrap();
    let mut init = masm::Procedure::new(
        Default::default(),
        masm::Visibility::Private,
        init_name,
        0,
        masm::Block::new(span, body),
    )
    .with_signature(masm::FunctionType::new(midenc_hir::CallConv::Fast, vec![], vec![]));
    init.extend_invoked(invoked);
    module.define_procedure(init, source_manager).into_diagnostic()?;
    Ok(())
}

/// Define the generated executable `main` procedure in the component root module.
///
/// The generated entry procedure invokes the component initializer, optional VM test harness
/// initialization, selected entrypoint, and stack truncation shim in order.
fn define_main_procedure(
    module: &mut masm::Module,
    init: Option<masm::InvocationTarget>,
    entrypoint: masm::InvocationTarget,
    emit_test_harness: bool,
    span: SourceSpan,
    source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
) -> Result<(), Report> {
    use masm::{Instruction as Inst, Op};

    // The generated entrypoint claims the reserved `main` procedure name in the component root
    // module, which also holds the lifted Component Model export wrappers. A component export
    // named `main` would otherwise surface as an opaque symbol conflict during assembly, so reject
    // it here with an actionable error.
    let main_name = masm::ProcedureName::main();
    if module.procedures().any(|proc| proc.name().as_str() == main_name.as_str()) {
        return Err(Report::msg(format!(
            "cannot generate executable entrypoint: component root module '{}' already defines a \
             procedure named `{}`; rename the conflicting component export or build a library \
             instead",
            module.path(),
            main_name.as_str(),
        )));
    }

    let mut invoked = Vec::new();
    let body = {
        let mut block = masm::Block::new(span, Vec::with_capacity(64));

        // Invoke component initializer, if present
        if let Some(init) = init {
            invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, init.clone()));
            block.push(Op::Inst(Span::new(span, Inst::Exec(init))));
        }

        // Initialize test harness, if requested
        if emit_test_harness {
            emit_test_harness_initialization(&mut block);
        }

        // Invoke the program entrypoint
        block.push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameStart.as_u32().into()))));
        invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, entrypoint.clone()));
        block.push(Op::Inst(Span::new(span, Inst::Exec(entrypoint))));
        block.push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));

        // Truncate the stack to 16 elements on exit
        let truncate_stack =
            qualified_procedure_target("::miden::core::sys", "truncate_stack", span);
        invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, truncate_stack.clone()));
        block.push(Op::Inst(Span::new(span, Inst::Exec(truncate_stack))));
        block
    };
    let mut main =
        masm::Procedure::new(span, masm::Visibility::Public, masm::ProcedureName::main(), 0, body);
    main.extend_invoked(invoked);
    module.define_procedure(main, source_manager).into_diagnostic()?;
    Ok(())
}

/// Emit VM test harness initializer loading into the generated executable entry block.
fn emit_test_harness_initialization(block: &mut masm::Block) {
    use masm::{Instruction as Inst, IntValue, Op, PushValue};
    use miden_core::Felt;

    let span = SourceSpan::default();

    let pipe_words_to_memory =
        qualified_procedure_target("::miden::core::mem", "pipe_words_to_memory", span);

    // Step 1: Get the number of initializers to run
    // => [inits] on operand stack
    block.push(Op::Inst(Span::new(span, Inst::AdvPush)));

    // Step 2: Evaluate the initial state of the loop condition `inits > 0`
    // => [inits, inits]
    block.push(Op::Inst(Span::new(span, Inst::Dup0)));
    // => [inits > 0, inits]
    block.push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
    block.push(Op::Inst(Span::new(span, Inst::Gt)));

    // Step 3: Loop until `inits == 0`
    let mut loop_body = Vec::with_capacity(16);

    // State of operand stack on entry to `loop_body`: [inits]
    // State of advice stack on entry to `loop_body`: [dest_ptr, num_words, ...]
    //
    // Step 3a: Compute next value of `inits`, i.e. `inits'`
    // => [inits - 1]
    loop_body.push(Op::Inst(Span::new(span, Inst::SubImm(Felt::ONE.into()))));

    // Step 3b: Copy initializer data to memory
    // => [num_words, dest_ptr, inits']
    loop_body.push(Op::Inst(Span::new(span, Inst::AdvPush)));
    loop_body.push(Op::Inst(Span::new(span, Inst::AdvPush)));
    // => [C, B, A, dest_ptr, inits'] on operand stack
    loop_body.push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameStart.as_u32().into()))));
    loop_body.push(Op::Inst(Span::new(span, Inst::Exec(pipe_words_to_memory))));
    loop_body.push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));
    // Drop C, B, A
    loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
    loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
    loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
    // => [inits']
    loop_body.push(Op::Inst(Span::new(span, Inst::Drop)));

    // Step 3c: Evaluate loop condition `inits' > 0`
    // => [inits', inits']
    loop_body.push(Op::Inst(Span::new(span, Inst::Dup0)));
    // => [inits' > 0, inits']
    loop_body.push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
    loop_body.push(Op::Inst(Span::new(span, Inst::Gt)));

    // Step 4: Enter (or skip) loop
    block.push(Op::While {
        span,
        body: masm::Block::new(span, loop_body),
    });

    // Step 5: Drop `inits` after loop is evaluated
    block.push(Op::Inst(Span::new(span, Inst::Drop)));
}

/// Convert a root-module absolute invocation target into a local target.
fn localize_root_invocation_target(
    target: &masm::InvocationTarget,
    root: &masm::LibraryPathRef,
) -> masm::InvocationTarget {
    if let masm::InvocationTarget::Path(path) = target
        && path.parent().is_some_and(|parent| parent == root)
        && let Some(name) = path.last()
    {
        return local_procedure_target(name, target.span());
    }

    target.clone()
}

/// Build an invocation target for a procedure in the current module.
fn local_procedure_target(name: &str, span: SourceSpan) -> masm::InvocationTarget {
    masm::InvocationTarget::Symbol(masm::Ident::from_raw_parts(Span::new(span, name.into())))
}

/// Build an invocation target for a fully-qualified procedure in another module (e.g. a stdlib or
/// intrinsics procedure referenced by absolute path).
fn qualified_procedure_target(
    module: &str,
    name: &str,
    span: SourceSpan,
) -> masm::InvocationTarget {
    let name = masm::ProcedureName::new(name).unwrap();
    let module = masm::LibraryPath::new(module).unwrap();
    let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
    InvocationTarget::Path(Span::new(span, qualified.into_inner()))
}

struct MasmModuleBuilder<'a> {
    module: &'a mut masm::Module,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
    source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
    init_body: &'a mut Vec<masm::Op>,
    invoked_from_init: &'a mut BTreeSet<masm::Invoke>,
}

impl MasmModuleBuilder<'_> {
    pub fn build(mut self, module: &builtin::Module) -> Result<(), Report> {
        let region = module.body();
        let block = region.entry();
        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else if let Some(gv) = op.downcast_ref::<builtin::GlobalVariable>() {
                self.emit_global_variable_initializer(gv)?;
            } else if op.is::<builtin::Segment>() {
                continue;
            } else {
                panic!(
                    "invalid module-level operation: '{}' is not legal in a MASM module body",
                    op.name()
                )
            }
        }

        Ok(())
    }

    pub fn build_from_interface(mut self, interface: &builtin::Interface) -> Result<(), Report> {
        let region = interface.body();
        let block = region.entry();
        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else {
                panic!(
                    "invalid interface-level operation: '{}' is not legal in a MASM module body",
                    op.name()
                )
            }
        }

        Ok(())
    }

    fn define_function(&mut self, function: &builtin::Function) -> Result<(), Report> {
        let builder = MasmFunctionBuilder::new(function)?;

        let procedure = builder.build(
            function,
            self.analysis_manager.nest(function.as_operation_ref()),
            self.link_info,
            None,
        )?;

        self.module
            .define_procedure(procedure, self.source_manager.clone())
            .map_err(|e| Report::msg(e.to_string()))?;

        Ok(())
    }

    fn emit_global_variable_initializer(
        &mut self,
        gv: &builtin::GlobalVariable,
    ) -> Result<(), Report> {
        // We don't emit anything for declarations
        if gv.is_declaration() {
            return Ok(());
        }

        // We compute liveness for global variables independently
        let analysis_manager = self.analysis_manager.nest(gv.as_operation_ref());
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        // Emit the initializer block
        let initializer_region = gv.region(0);
        let initializer_block = initializer_region.entry();

        let mut block_emitter = BlockEmitter {
            liveness: &liveness,
            link_info: self.link_info,
            invoked: self.invoked_from_init,
            target: Default::default(),
            stack: OperandStack::new(gv.as_operation().context_rc()),
            trace_target: TraceTarget::category("codegen")
                .with_relevant_symbol(gv.name().as_symbol()),
        };
        block_emitter.emit_inline(&initializer_block);

        // Sanity checks
        assert_eq!(block_emitter.stack.len(), 1, "expected only global variable value on stack");
        let return_ty = block_emitter.stack.peek().unwrap().ty();
        assert_eq!(
            &return_ty,
            &*gv.get_ty(),
            "expected initializer to return value of same type as declaration"
        );

        // Write the initialized value to the computed storage offset for this global
        let computed_addr = self
            .link_info
            .globals_layout()
            .get_computed_addr(gv.as_global_var_ref())
            .expect("undefined global variable");
        block_emitter.emitter().store_imm(computed_addr, gv.span());

        // Extend the generated init function with the code to initialize this global
        let mut body = core::mem::take(&mut block_emitter.target);
        self.init_body.append(&mut body);

        Ok(())
    }
}

struct MasmFunctionBuilder {
    span: midenc_hir::SourceSpan,
    name: masm::ProcedureName,
    signature: masm::FunctionType,
    visibility: masm::Visibility,
    num_locals: u16,
}

impl MasmFunctionBuilder {
    pub fn new(function: &builtin::Function) -> Result<Self, Report> {
        use midenc_hir::{Symbol, Visibility};

        let name = *function.get_name();
        let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
            name.span,
            name.as_ref().into(),
        )));
        let visibility = match function.visibility() {
            Visibility::Public => masm::Visibility::Public,
            // TODO(pauls): Support internal visibility in MASM
            Visibility::Internal => masm::Visibility::Public,
            Visibility::Private => masm::Visibility::Private,
        };
        let locals_required = function.locals().iter().map(|ty| ty.size_in_felts()).sum::<usize>();
        let num_locals = u16::try_from(locals_required).map_err(|_| {
            let context = function.as_operation().context();
            context
                .diagnostics()
                .diagnostic(miden_assembly::diagnostics::Severity::Error)
                .with_message("cannot emit masm for function")
                .with_primary_label(
                    function.span(),
                    "local storage exceeds procedure limit: no more than u16::MAX elements are \
                     supported",
                )
                .into_report()
        })?;

        let signature =
            semantic_debug_signature(function).unwrap_or_else(|| lowered_signature(function));

        Ok(Self {
            span: function.span(),
            name,
            signature,
            visibility,
            num_locals,
        })
    }

    pub fn build(
        self,
        function: &builtin::Function,
        analysis_manager: AnalysisManager,
        link_info: &LinkInfo,
        init_target: Option<masm::InvocationTarget>,
    ) -> Result<masm::Procedure, Report> {
        use alloc::collections::BTreeSet;

        use midenc_hir_analysis::analyses::LivenessAnalysis;

        let demangled_symbol_name = midenc_hir::demangle::demangle(function.get_name().as_str());
        let trace_target = TraceTarget::category("codegen")
            .with_relevant_symbol(midenc_hir::SymbolName::intern(demangled_symbol_name));

        log::trace!(target: &trace_target, "lowering {}", function.as_operation());

        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        let mut invoked = BTreeSet::default();
        let entry = function.entry_block();
        let mut stack = crate::OperandStack::new(function.as_operation().context_rc());
        {
            let entry_block = entry.borrow();
            for arg in entry_block.arguments().iter().rev().copied() {
                stack.push(arg as ValueRef);
            }
        }
        let mut emitter = BlockEmitter {
            liveness: &liveness,
            link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target,
        };

        // Component export wrappers (Component Model calling convention) invoke the `init`
        // procedure first to load data segments and global vars into memory.
        //
        // Every such wrapper is lowered in the component root module, where the caller supplies a
        // root-local `init` target. Support-module functions are never Component Model exports and
        // always pass `None`, so reaching this branch without a target indicates a broken lowering
        // invariant rather than a user error.
        if function.signature().cc.is_wasm_canonical_abi()
            && (link_info.has_globals() || link_info.has_data_segments())
        {
            let init = init_target.ok_or_else(|| {
                Report::msg(
                    "internal error: Component Model export wrapper lowered without a root-local \
                     `init` target",
                )
            })?;
            let span = SourceSpan::default();
            // Add init call to the emitter's target before emitting the function body
            emitter.invoked.insert(masm::Invoke::new(masm::InvokeKind::Exec, init.clone()));
            emitter
                .target
                .push(masm::Op::Inst(Span::new(span, masm::Instruction::Exec(init))));
        }

        let mut body = emitter.emit(&entry.borrow());

        if function.signature().cc.is_wasm_canonical_abi() {
            // Truncate the stack to 16 elements on exit in the component export function
            // since it is expected to be `call`ed so it has a requirement to have
            // no more than 16 elements on the stack when it returns.
            // See https://0xmiden.github.io/miden-vm/user_docs/assembly/execution_contexts.html
            // Since the VM's `drop` instruction not letting stack size go beyond the 16 elements
            // we most likely end up with stack size > 16 elements at the end.
            // See https://github.com/0xPolygonMiden/miden-vm/blob/c4acf49510fda9ba80f20cee1a9fb1727f410f47/processor/src/stack/mod.rs?plain=1#L226-L253
            let truncate_stack = qualified_procedure_target(
                "::miden::core::sys",
                "truncate_stack",
                SourceSpan::default(),
            );
            let span = SourceSpan::default();
            invoked.insert(masm::Invoke::new(masm::InvokeKind::Exec, truncate_stack.clone()));
            body.push(masm::Op::Inst(Span::new(span, masm::Instruction::Exec(truncate_stack))));
        }
        let Self {
            span,
            name,
            signature,
            visibility,
            num_locals,
        } = self;

        // Align num_locals to WORD_SIZE, matching the assembler's FMP frame sizing.
        // num_locals already counts all HIR locals (including those allocated for params).
        // The assembler rounds up to next_multiple_of(WORD_SIZE) when advancing FMP
        // (see fmp.rs fmp_start_frame_sequence and mem_ops.rs locaddr), so we must use
        // the same alignment for debug var offset computation.
        let aligned_num_locals = num_locals.next_multiple_of(miden_core::WORD_SIZE as u16);

        // Resolve FrameBase global_index → Miden memory address.
        // Use the stack pointer offset from the linker's global layout.
        let stack_pointer_addr = link_info.globals_layout().stack_pointer_offset();

        // Patch DebugVar Local locations to compute FMP offset.
        // During lowering, Local(idx) stores the raw WASM local index.
        // Now convert to FMP offset: idx - aligned_num_locals
        // This matches locaddr.N which computes -(aligned_num_locals - N).
        patch_debug_var_locals_in_block(&mut body, aligned_num_locals, stack_pointer_addr);

        // If a function body after lowering produces a MASM procedure with an empty body aside
        // from debug decorators, then we must emit a `nop` at the end of the block which will
        // act as the anchor for those decorators. Such a procedure is basically useless, as it is
        // just passing through arguments as results - but the assembler currently rejects empty
        // procedures (not counting decorators), so we must handle this edge case.
        if !block_has_real_instructions(&body) {
            body.push(masm::Op::Inst(Span::unknown(masm::Instruction::Nop)));
        }

        let mut procedure = masm::Procedure::new(span, visibility, name, num_locals, body);
        procedure.set_signature(signature);
        for attribute in ["auth_script", "note_script"] {
            if function.has_attribute(attribute) {
                procedure
                    .attributes_mut()
                    .insert(Attribute::Marker(masm::Ident::new(attribute).unwrap()));
            }
        }
        procedure.extend_invoked(invoked);

        Ok(procedure)
    }
}

fn lowered_signature(function: &builtin::Function) -> masm::FunctionType {
    let sig = function.signature();
    let args = sig.params.iter().map(|param| masm::TypeExpr::from(param.ty.clone())).collect();
    let results = sig
        .results
        .iter()
        .map(|result| masm::TypeExpr::from(result.ty.clone()))
        .collect();
    masm::FunctionType::new(sig.cc, args, results)
}

fn semantic_debug_signature(function: &builtin::Function) -> Option<masm::FunctionType> {
    let subprogram = function
        .as_operation()
        .get_attribute("di.subprogram")?
        .try_downcast_attr::<SubprogramAttr>()
        .ok()?;
    let subprogram = subprogram.borrow();
    let Type::Function(ty) = subprogram.ty.as_ref()? else {
        return None;
    };

    let args = ty.params().iter().map(component_abi_type_expr_from_hir).collect();
    let results = ty.results().iter().map(component_abi_type_expr_from_hir).collect();
    Some(masm::FunctionType::new(ty.calling_convention(), args, results))
}

/// Convert HIR types from a Component Model/WIT signature into MASM syntax types.
///
/// This intentionally differs from `From<Type> for TypeExpr`, which describes the lowered MASM
/// representation and expands wide integer primitives like `u64`/`u128` into 32-bit limb arrays.
/// Component export metadata should preserve the Component ABI shape instead, including nominal
/// struct and field names used by debuggers and typed clients.
///
/// TODO(pauls): Remove once miden-vm#XXXX is merged and ships in the next stable release,
/// expected to be v0.24.
fn component_abi_type_expr_from_hir(ty: &Type) -> masm::TypeExpr {
    match ty {
        Type::Array(array) => masm::TypeExpr::Array(masm::ArrayType::new(
            component_abi_type_expr_from_hir(array.element_type()),
            array.len(),
        )),
        Type::Struct(struct_ty) => {
            let name = struct_ty.name().and_then(|name| masm::Ident::new(name.as_ref()).ok());
            let fields = struct_ty.fields().iter().enumerate().map(|(index, field)| {
                let name = field
                    .name
                    .as_deref()
                    .map(masm::Ident::new)
                    .and_then(Result::ok)
                    .unwrap_or_else(|| masm::Ident::new(format!("field{index}")).unwrap());
                masm::StructField {
                    span: SourceSpan::UNKNOWN,
                    name,
                    ty: component_abi_type_expr_from_hir(&field.ty),
                }
            });
            masm::TypeExpr::Struct(
                masm::StructType::new(name, fields)
                    .with_repr(Span::unknown(struct_ty.repr()))
                    .with_span(SourceSpan::UNKNOWN),
            )
        }
        Type::Ptr(ptr) => masm::TypeExpr::Ptr(
            masm::PointerType::new(component_abi_type_expr_from_hir(ptr.pointee()))
                .with_address_space(ptr.addrspace()),
        ),
        Type::Function(_) => masm::TypeExpr::Ptr(masm::PointerType::new(
            masm::TypeExpr::Primitive(Span::unknown(Type::Felt)),
        )),
        Type::List(element_ty) => masm::TypeExpr::Ptr(
            masm::PointerType::new(component_abi_type_expr_from_hir(element_ty))
                .with_address_space(masm::types::AddressSpace::Byte),
        ),
        Type::Unknown | Type::Never | Type::F64 => panic!("unrepresentable type value: {ty}"),
        ty => masm::TypeExpr::Primitive(Span::unknown(ty.clone())),
    }
}

/// Returns true if the block contains at least one real (non-decorator) instruction.
///
/// DebugVar instructions are decorator-only and don't produce MAST nodes. If a procedure
/// body contains only DebugVar ops, the assembler will reject it.
fn block_has_real_instructions(block: &masm::Block) -> bool {
    block.iter().any(|op| match op {
        masm::Op::Inst(inst) => !matches!(
            inst.inner(),
            masm::Instruction::Debug(_)
                | masm::Instruction::DebugVar(_)
                | masm::Instruction::Trace(_)
        ),
        masm::Op::If {
            then_blk, else_blk, ..
        } => block_has_real_instructions(then_blk) || block_has_real_instructions(else_blk),
        masm::Op::While { body, .. } => block_has_real_instructions(body),
        masm::Op::Repeat { body, .. } => block_has_real_instructions(body),
    })
}

/// Recursively patch DebugVar locations in a block.
///
/// Converts `Local(idx)` where idx is the raw WASM local index to `Local(offset)` where
/// `offset = idx - aligned_num_locals` (the FMP-relative offset, typically negative). This matches
/// the assembler's `locaddr.N` formula, i.e. `FMP - aligned_num_locals + N`.
///
/// Also resolves `FrameBase { global_index, byte_offset }` by replacing the WASM global index with
/// the resolved Miden memory address of the stack pointer.
fn patch_debug_var_locals_in_block(
    block: &mut masm::Block,
    aligned_num_locals: u16,
    stack_pointer_addr: Option<u32>,
) {
    for op in block.iter_mut() {
        match op {
            masm::Op::Inst(span_inst) => {
                // Use DerefMut to get mutable access to the inner Instruction
                if let masm::Instruction::DebugVar(info) = &mut **span_inst {
                    if let DebugVarLocation::Local(idx) = info.value_location() {
                        // Convert raw WASM local index to FMP offset
                        let fmp_offset = *idx - (aligned_num_locals as i16);
                        info.set_value_location(DebugVarLocation::Local(fmp_offset));
                    } else if let DebugVarLocation::FrameBase {
                        global_index,
                        byte_offset,
                    } = info.value_location()
                    {
                        let byte_offset = *byte_offset;
                        if let Some(local_index) = decode_frame_base_local_index(*global_index) {
                            if let Ok(local_index) = i16::try_from(local_index) {
                                let local_offset = local_index - (aligned_num_locals as i16);
                                info.set_value_location(DebugVarLocation::FrameBase {
                                    global_index: encode_frame_base_local_offset(local_offset),
                                    byte_offset,
                                });
                            }
                        } else {
                            // Resolve FrameBase: replace WASM global index with
                            // the Miden memory address of the stack pointer global.
                            if let Some(resolved_addr) = stack_pointer_addr {
                                info.set_value_location(DebugVarLocation::FrameBase {
                                    global_index: resolved_addr,
                                    byte_offset,
                                });
                            }
                        }
                    }
                }
            }
            masm::Op::If {
                then_blk, else_blk, ..
            } => {
                patch_debug_var_locals_in_block(then_blk, aligned_num_locals, stack_pointer_addr);
                patch_debug_var_locals_in_block(else_blk, aligned_num_locals, stack_pointer_addr);
            }
            masm::Op::While {
                body: while_body, ..
            } => {
                patch_debug_var_locals_in_block(while_body, aligned_num_locals, stack_pointer_addr);
            }
            masm::Op::Repeat {
                body: repeat_body, ..
            } => {
                patch_debug_var_locals_in_block(
                    repeat_body,
                    aligned_num_locals,
                    stack_pointer_addr,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use midenc_hir::{PointerType, StructType, TypeRepr};

    use super::*;

    #[test]
    fn type_expr_from_hir_pointer_conversion_preserves_address_space() {
        for addrspace in [masm::types::AddressSpace::Byte, masm::types::AddressSpace::Element] {
            let ty = Type::from(PointerType::new_with_address_space(Type::U32, addrspace));

            let masm::TypeExpr::Ptr(ptr) = component_abi_type_expr_from_hir(&ty) else {
                panic!("expected pointer type expression");
            };
            assert_eq!(ptr.address_space(), addrspace);

            let masm::TypeExpr::Ptr(ptr) = masm::TypeExpr::from(ty) else {
                panic!("expected pointer type expression");
            };
            assert_eq!(ptr.address_space(), addrspace);
        }
    }

    #[test]
    fn component_abi_type_conversion_preserves_wide_primitives() {
        let masm::TypeExpr::Primitive(ty) = component_abi_type_expr_from_hir(&Type::U64) else {
            panic!("expected primitive component ABI type");
        };
        assert_eq!(ty.inner(), &Type::U64);

        let masm::TypeExpr::Array(ty) = masm::TypeExpr::from(Type::U64) else {
            panic!("expected lowered MASM type");
        };
        assert_eq!(ty.arity, 2);
        let masm::TypeExpr::Primitive(element_ty) = ty.elem.as_ref() else {
            panic!("expected primitive array element type");
        };
        assert_eq!(element_ty.inner(), &Type::U32);
    }

    #[test]
    fn component_abi_type_conversion_preserves_nominal_struct_metadata() {
        let ty = Type::Struct(Arc::new(StructType::from_parts(
            Some(Arc::from("miden:base/core-types@1.0.0/account-id")),
            TypeRepr::Default,
            [
                (Arc::<str>::from("prefix"), Type::Felt),
                (Arc::<str>::from("suffix"), Type::Felt),
            ],
        )));

        let masm::TypeExpr::Struct(struct_ty) = component_abi_type_expr_from_hir(&ty) else {
            panic!("expected struct component ABI type");
        };
        assert_eq!(
            struct_ty.name.as_ref().map(|name| name.as_str()),
            Some("miden:base/core-types@1.0.0/account-id"),
        );
        assert_eq!(struct_ty.fields[0].name.as_str(), "prefix");
        assert_eq!(struct_ty.fields[1].name.as_str(), "suffix");

        let masm::TypeExpr::Struct(struct_ty) = masm::TypeExpr::from(ty) else {
            panic!("expected lowered struct type");
        };
        assert!(struct_ty.name.is_none());
        assert_eq!(struct_ty.fields[0].name.as_str(), "field0");
        assert_eq!(struct_ty.fields[1].name.as_str(), "field1");
    }
}
