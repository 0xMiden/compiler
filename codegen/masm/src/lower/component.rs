use alloc::{collections::BTreeSet, sync::Arc};

use miden_assembly::{LibraryPath, ast::InvocationTarget};
use miden_assembly_syntax::parser::WordValue;
use miden_mast_package::ProcedureName;
use midenc_hir::{
    CallConv, FunctionIdent, Op, SourceSpan, Span, Symbol, ValueRef, diagnostics::IntoDiagnostic,
    dialects::builtin, pass::AnalysisManager,
};
use midenc_hir_analysis::analyses::LivenessAnalysis;
use midenc_session::{
    TargetEnv,
    diagnostics::{Report, Spanned},
};
use smallvec::SmallVec;

use crate::{
    TraceEvent,
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

/// 1:1 conversion from HIR component to MASM component
impl ToMasmComponent for builtin::Component {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
    ) -> Result<MasmComponent, Report> {
        // Get the current compiler context
        let context = self.as_operation().context_rc();

        // Run the linker for this component in order to compute its data layout
        let link_info = Linker::default().link(self).map_err(Report::msg)?;

        // Get the library path of the component
        let component_path = link_info.component().to_library_path();

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
                let is_wrapper = component_path.path() == "root_ns:root@1.0.0";
                let path = if is_wrapper {
                    component_path.clone().append_unchecked(entry_id.module)
                } else {
                    // We're compiling a Wasm component and the component id is included
                    // in the entrypoint.
                    LibraryPath::new(entry_id.module).into_diagnostic()?
                };
                Some(masm::InvocationTarget::AbsoluteProcedurePath { name, path })
            }
            None => None,
        };

        // If we have global variables or data segments, we will require a component initializer
        // function, as well as a module to hold component-level functions such as init
        let requires_init = link_info.has_globals() || link_info.has_data_segments();
        let init = if requires_init {
            Some(masm::InvocationTarget::AbsoluteProcedurePath {
                name: masm::ProcedureName::new("init").unwrap(),
                path: component_path,
            })
        } else {
            None
        };

        // Initialize the MASM component with basic information we have already
        let id = link_info.component().clone();

        // Define the initial component modules set
        //
        // The top-level component module is always defined, but may be empty
        let modules =
            vec![Arc::new(masm::Module::new(masm::ModuleKind::Library, id.to_library_path()))];

        let rodata = data_segments_to_rodata(&link_info)?;

        let kernel = if matches!(context.session().options.target, TargetEnv::Rollup { .. }) {
            Some(miden_lib::transaction::TransactionKernel::kernel())
        } else {
            None
        };

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
            id,
            init,
            entrypoint,
            kernel,
            rodata,
            heap_base,
            stack_pointer,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
            init_body: Default::default(),
            invoked_from_init: Default::default(),
        };

        builder.build(self)?;

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
            offset: *s.offset(),
            data: s.initializer().as_slice().to_vec(),
            readonly: *s.readonly(),
        });
    }
    Ok(match merge_data_segments(resolved).map_err(Report::msg)? {
        None => alloc::vec::Vec::new(),
        Some(merged) => {
            let data = alloc::sync::Arc::new(ConstantData::from(merged.data));
            let felts = crate::Rodata::bytes_to_elements(data.as_slice());
            let digest = miden_core::crypto::hash::Rpo256::hash_elements(&felts);
            alloc::vec![crate::Rodata {
                component: link_info.component().clone(),
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
    init_body: Vec<masm::Op>,
    invoked_from_init: BTreeSet<masm::Invoke>,
}

impl MasmComponentBuilder<'_> {
    /// Convert the component body to Miden Assembly
    pub fn build(mut self, component: &builtin::Component) -> Result<(), Report> {
        use masm::{Instruction as Inst, InvocationTarget, Op};

        // If a component-level init is required, emit code to initialize the heap before any other
        // initialization code.
        if self.component.init.is_some() {
            let span = component.span();

            // Heap metadata initialization
            let heap_base = self.component.heap_base;
            self.init_body.push(masm::Op::Inst(Span::new(
                span,
                Inst::Push(masm::Immediate::Value(Span::unknown(heap_base.into()))),
            )));
            let heap_init = masm::ProcedureName::new("heap_init").unwrap();
            let memory_intrinsics = masm::LibraryPath::new("intrinsics::mem").unwrap();
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Trace(TraceEvent::FrameStart.as_u32().into()),
            )));
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Exec(InvocationTarget::AbsoluteProcedurePath {
                    name: heap_init,
                    path: memory_intrinsics,
                }),
            )));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));

            // Data segment initialization
            self.emit_data_segment_initialization();
        }

        // Translate component body
        let region = component.body();
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
        if self.component.init.is_some() {
            let module =
                Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");

            let init_name = masm::ProcedureName::new("init").unwrap();
            let init_body = core::mem::take(&mut self.init_body);
            let init = masm::Procedure::new(
                Default::default(),
                masm::Visibility::Private,
                init_name,
                0,
                masm::Block::new(component.span(), init_body),
            );

            module.define_procedure(masm::Export::Procedure(init))?;
        } else {
            assert!(
                self.init_body.is_empty(),
                "the need for an 'init' function was not expected, but code was generated for one"
            );
        }

        Ok(())
    }

    fn define_interface(&mut self, interface: &builtin::Interface) -> Result<(), Report> {
        let component_path = self.component.id.to_library_path();
        let interface_path = component_path.append_unchecked(interface.name());
        let mut masm_module =
            Box::new(masm::Module::new(masm::ModuleKind::Library, interface_path));
        let builder = MasmModuleBuilder {
            module: &mut masm_module,
            analysis_manager: self
                .analysis_manager
                .nest(interface.as_operation().as_operation_ref()),
            link_info: self.link_info,
            init_body: &mut self.init_body,
            invoked_from_init: &mut self.invoked_from_init,
        };
        builder.build_from_interface(interface)?;

        self.component.modules.push(Arc::from(masm_module));

        Ok(())
    }

    fn define_module(&mut self, module: &builtin::Module) -> Result<(), Report> {
        let component_path = self.component.id.to_library_path();
        let module_path = component_path.append_unchecked(module.name());
        let mut masm_module = Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
        let builder = MasmModuleBuilder {
            module: &mut masm_module,
            analysis_manager: self.analysis_manager.nest(module.as_operation_ref()),
            link_info: self.link_info,
            init_body: &mut self.init_body,
            invoked_from_init: &mut self.invoked_from_init,
        };
        builder.build(module)?;

        self.component.modules.push(Arc::from(masm_module));

        Ok(())
    }

    fn define_function(&mut self, function: &builtin::Function) -> Result<(), Report> {
        let builder = MasmFunctionBuilder::new(function)?;
        let procedure = builder.build(
            function,
            self.analysis_manager.nest(function.as_operation_ref()),
            self.link_info,
        )?;

        let module =
            Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");
        assert_eq!(
            module.path().num_components(),
            1,
            "expected top-level namespace module, but one has not been defined (in '{}' of '{}')",
            module.path(),
            function.path()
        );
        module.define_procedure(masm::Export::Procedure(procedure))?;

        Ok(())
    }

    /// Emit the sequence of instructions necessary to consume rodata from the advice stack and
    /// populate the global heap with the data segments of this component, verifying that the
    /// commitments match.
    fn emit_data_segment_initialization(&mut self) {
        use masm::{Instruction as Inst, InvocationTarget, Op};

        // Emit data segment initialization code
        //
        // NOTE: This depends on the program being executed with the data for all data segments
        // having been placed in the advice map with the same commitment and encoding used here.
        // The program will fail to execute if this is not set up correctly.
        let pipe_preimage_to_memory = masm::ProcedureName::new("pipe_preimage_to_memory").unwrap();
        let std_mem = masm::LibraryPath::new("std::mem").unwrap();

        let span = SourceSpan::default();
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
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::Exec(InvocationTarget::AbsoluteProcedurePath {
                    name: pipe_preimage_to_memory.clone(),
                    path: std_mem.clone(),
                }),
            )));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));
            // drop write_ptr'
            self.init_body.push(Op::Inst(Span::new(span, Inst::Drop)));
        }
    }
}

struct MasmModuleBuilder<'a> {
    module: &'a mut masm::Module,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
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
        )?;

        self.module.define_procedure(masm::Export::Procedure(procedure))?;

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
            stack: Default::default(),
        };
        block_emitter.emit_inline(&initializer_block);

        // Sanity checks
        assert_eq!(block_emitter.stack.len(), 1, "expected only global variable value on stack");
        let return_ty = block_emitter.stack.peek().unwrap().ty();
        assert_eq!(
            &return_ty,
            gv.ty(),
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

        let name = function.name();
        let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
            name.span,
            name.as_str().into(),
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

        let sig = function.signature();
        let args = sig.params.iter().map(|param| masm::TypeExpr::from(param.ty.clone())).collect();
        let results = sig
            .results
            .iter()
            .map(|result| masm::TypeExpr::from(result.ty.clone()))
            .collect();
        let signature = masm::FunctionType::new(sig.cc, args, results);

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
    ) -> Result<masm::Procedure, Report> {
        use alloc::collections::BTreeSet;

        use midenc_hir_analysis::analyses::LivenessAnalysis;

        log::trace!(target: "codegen", "lowering {}", function.as_operation());

        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;

        let mut invoked = BTreeSet::default();
        let entry = function.entry_block();
        let mut stack = crate::OperandStack::default();
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
        };

        // For component export functions, invoke the `init` procedure first if needed.
        // It loads the data segments and global vars into memory.
        if function.signature().cc == CallConv::CanonLift
            && (link_info.has_globals() || link_info.has_data_segments())
        {
            let component_path = link_info.component().to_library_path();
            let init = InvocationTarget::AbsoluteProcedurePath {
                name: ProcedureName::new("init").unwrap(),
                path: component_path,
            };
            let span = SourceSpan::default();
            // Add init call to the emitter's target before emitting the function body
            emitter
                .target
                .push(masm::Op::Inst(Span::new(span, masm::Instruction::Exec(init))));
        }

        let mut body = emitter.emit(&entry.borrow());

        if function.signature().cc == CallConv::CanonLift {
            // Truncate the stack to 16 elements on exit in the component export function
            // since it is expected to be `call`ed so it has a requirement to have
            // no more than 16 elements on the stack when it returns.
            // See https://0xmiden.github.io/miden-vm/user_docs/assembly/execution_contexts.html
            // Since the VM's `drop` instruction not letting stack size go beyond the 16 elements
            // we most likely end up with stack size > 16 elements at the end.
            // See https://github.com/0xPolygonMiden/miden-vm/blob/c4acf49510fda9ba80f20cee1a9fb1727f410f47/processor/src/stack/mod.rs?plain=1#L226-L253
            let truncate_stack = InvocationTarget::AbsoluteProcedurePath {
                name: ProcedureName::new("truncate_stack").unwrap(),
                path: masm::LibraryPath::new_from_components(
                    masm::LibraryNamespace::new("std").unwrap(),
                    [masm::Ident::new("sys").unwrap()],
                ),
            };
            let span = SourceSpan::default();
            body.push(masm::Op::Inst(Span::new(span, masm::Instruction::Exec(truncate_stack))));
        }
        let Self {
            span,
            name,
            signature,
            visibility,
            num_locals,
        } = self;

        let mut procedure = masm::Procedure::new(span, visibility, name, num_locals, body);
        procedure.set_signature(signature);
        procedure.extend_invoked(invoked);

        Ok(procedure)
    }
}
