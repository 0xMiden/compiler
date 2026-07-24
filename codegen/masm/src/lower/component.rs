use alloc::{collections::BTreeSet, sync::Arc, vec::Vec};

use miden_assembly::{PathBuf as LibraryPath, ast::InvocationTarget};
use miden_assembly_syntax::{
    ast::{Attribute, DebugVarLocation},
    parser::WordValue,
};
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
    Event, OperandStack,
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
        let toplevel_namespaces = self
            .body()
            .entry()
            .body()
            .iter()
            .filter_map(|op| {
                if op.is::<builtin::Module>() || op.is::<builtin::Component>() {
                    Some(op.as_operation_ref())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let init = if requires_init {
            let name = masm::ProcedureName::new("init").unwrap();
            let qualified = match toplevel_namespaces.len() {
                1 => {
                    let namespace =
                        toplevel_namespaces[0].borrow().symbol_name_if_symbol().unwrap();
                    masm::QualifiedProcedureName::new(format!("::{namespace}").as_str(), name)
                }
                _ => masm::QualifiedProcedureName::new("::init", name),
            };
            Some(masm::InvocationTarget::Path(Span::new(
                SourceSpan::default(),
                qualified.into_inner(),
            )))
        } else {
            None
        };

        // Define the initial component modules set
        //
        // The top-level component module is always defined, but may be empty
        let root = match toplevel_namespaces.len() {
            1 => {
                let namespace = toplevel_namespaces[0].borrow().symbol_name_if_symbol().unwrap();
                Arc::from(
                    masm::PathBuf::new(&format!("::{namespace}"))
                        .expect("invalid namespace")
                        .into_boxed_path(),
                )
            }
            _ => Arc::<masm::Path>::from(masm::Path::new("::init")),
        };
        let init_module = Arc::new(masm::Module::new(masm::ModuleKind::Library, &root));
        let modules = vec![init_module];

        let rodata = data_segments_to_rodata(&link_info)?;

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
            init,
            entrypoint,
            rodata,
            heap_base,
            stack_pointer,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
            source_manager: context.session().source_manager.clone(),
            init_body: Default::default(),
            invoked_from_init: Default::default(),
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
        let component_path = id
            .to_library_path()
            .to_absolute()
            .map_err(|err| {
                Report::msg(format!("unable to canonicalize '{}': {err}", &id.to_library_path()))
            })?
            .into_owned();

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
                    component_path.join(entry_id.module.as_str())
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
        let init = if requires_init {
            let name = masm::ProcedureName::new("init").unwrap();
            let qualified = masm::QualifiedProcedureName::new(&component_path, name);
            Some(masm::InvocationTarget::Path(Span::new(
                SourceSpan::default(),
                qualified.into_inner(),
            )))
        } else {
            None
        };

        // Define the initial component modules set
        //
        // The top-level component module is always defined, but may be empty
        let root: Arc<miden_assembly_syntax::Path> = component_path.into_boxed_path().into();
        let root_module = Arc::new(masm::Module::new(masm::ModuleKind::Library, &root));
        let modules = vec![root_module];

        let rodata = data_segments_to_rodata(&link_info)?;

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
            id: Some(id),
            root,
            init,
            entrypoint,
            rodata,
            heap_base,
            stack_pointer,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
            source_manager: context.session().source_manager.clone(),
            init_body: Default::default(),
            invoked_from_init: Default::default(),
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
    source_manager: Arc<dyn midenc_session::SourceManager>,
    init_body: Vec<masm::Op>,
    invoked_from_init: BTreeSet<masm::Invoke>,
}

impl MasmComponentBuilder<'_> {
    /// Convert the component body to Miden Assembly
    pub fn build(mut self, component: &midenc_hir::Operation) -> Result<(), Report> {
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
            let heap_init = {
                let name = masm::ProcedureName::new("heap_init").unwrap();
                let module = masm::LibraryPath::new("::intrinsics::mem").unwrap();
                let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
                InvocationTarget::Path(Span::new(span, qualified.into_inner()))
            };
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            )));
            self.init_body.push(Op::Inst(Span::new(span, Inst::Exec(heap_init))));
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()),
            )));

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
        if self.component.init.is_some() {
            let module =
                Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");

            let init_name = masm::ProcedureName::new("init").unwrap();
            let init_body = core::mem::take(&mut self.init_body);
            let init = masm::Procedure::new(
                Default::default(),
                masm::Visibility::Public,
                init_name,
                0,
                masm::Block::new(component.span(), init_body),
            )
            .with_signature(masm::FunctionType::new(
                midenc_hir::CallConv::Fast,
                vec![],
                vec![],
            ));

            module
                .define_procedure(init, self.source_manager.clone())
                .into_diagnostic()
                .wrap_err("failed to define component `init` procedure")?;
        } else {
            assert!(
                self.init_body.is_empty(),
                "the need for an 'init' function was not expected, but code was generated for one"
            );
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
        let module_path = module.path().to_library_path();
        let module_path = module_path.to_absolute().unwrap();
        let trace_target = TraceTarget::category("codegen");
        log::debug!(target: &trace_target, "defining module '{module_path}'");
        /*
        let visibility = match *module.get_visibility() {
            midenc_hir::Visibility::Public => masm::Visibility::Public,
            midenc_hir::Visibility::Internal | midenc_hir::Visibility::Private => {
                masm::Visibility::Private
            }
        };
         */
        let visibility = masm::Visibility::Public;
        let module_index = if let Some(rest) = module_path.strip_prefix(&self.component.root) {
            self.define_module_tree(rest, Some(0), visibility)?
        } else {
            self.define_module_tree(&module_path, None, visibility)?
        };

        let masm_module = Arc::get_mut(&mut self.component.modules[module_index])
            .expect("expected unique reference");
        let builder = MasmModuleBuilder {
            module: masm_module,
            analysis_manager: self.analysis_manager.nest(module.as_operation_ref()),
            link_info: self.link_info,
            source_manager: self.source_manager.clone(),
            init_body: &mut self.init_body,
            invoked_from_init: &mut self.invoked_from_init,
        };
        builder.build(module)?;

        Ok(())
    }

    fn define_module_tree(
        &mut self,
        module_path: &masm::Path,
        mut parent: Option<usize>,
        visibility: masm::Visibility,
    ) -> Result<usize, Report> {
        let trace_target = TraceTarget::category("codegen");
        let mut path = masm::PathBuf::with_capacity(256);
        if let Some(parent) = parent {
            path = self.component.modules[parent].path().to_path_buf();
        }
        let mut components = module_path.components().peekable();
        while let Some(component) = components.next() {
            let name = component.unwrap().as_str();
            // Ignore the root component
            if name == "::" {
                continue;
            }
            path.push_component(name);
            if !path.is_absolute() {
                path = path.to_absolute().unwrap().into_owned();
            }
            // Use the input visibility for the last module we crate, for parent modules, we must
            // specify public visibility so that references to this module are valid.
            let visibility = if components.peek().is_none() {
                visibility
            } else {
                masm::Visibility::Public
            };
            let module_path = &path;
            if let Some(parent_index) = parent {
                let parent_module = Arc::get_mut(&mut self.component.modules[parent_index])
                    .expect("expected unique reference");
                if parent_module.submodules().iter().any(|sm| sm.name.as_str() == name) {
                    // Already defined, look up the submodule as the new `parent`
                    parent = Some(
                        self.component
                            .modules
                            .iter()
                            .position(|m| m.path() == module_path.as_path())
                            .expect(
                                "submodule was already defined, but not registered with component",
                            ),
                    );
                } else {
                    // Create the submodule
                    let submodule =
                        Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
                    let name = masm::Ident::new(submodule.name()).unwrap();
                    log::debug!(target: &trace_target, "declaring submodule '{name}' of '{}'", parent_module.path());
                    parent_module.declare_submodule(name, visibility)?;
                    parent = Some(self.component.modules.len());
                    self.component.modules.push(Arc::from(submodule));
                }
            } else {
                log::debug!(target: &trace_target, "declaring module '{module_path}'");
                let module = Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
                parent = Some(self.component.modules.len());
                self.component.modules.push(Arc::from(module));
            }
        }

        Ok(parent.unwrap())
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
        use masm::{Instruction as Inst, InvocationTarget, Op};

        // Emit data segment initialization code
        //
        // NOTE: This depends on the program being executed with the data for all data segments
        // having been placed in the advice map with the same commitment and encoding used here.
        // The program will fail to execute if this is not set up correctly.
        let span = SourceSpan::default();
        let pipe_preimage_to_memory = {
            let name = masm::ProcedureName::new("pipe_preimage_to_memory").unwrap();
            let module = masm::LibraryPath::new("::miden::core::mem").unwrap();
            let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
            InvocationTarget::Path(Span::new(span, qualified.into_inner()))
        };
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
                Inst::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            )));
            self.init_body
                .push(Op::Inst(Span::new(span, Inst::Exec(pipe_preimage_to_memory.clone()))));
            self.init_body.push(Op::Inst(Span::new(
                span,
                Inst::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()),
            )));
            // drop write_ptr'
            self.init_body.push(Op::Inst(Span::new(span, Inst::Drop)));
        }
    }
}

struct MasmModuleBuilder<'a> {
    module: &'a mut masm::Module,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
    source_manager: Arc<dyn midenc_session::SourceManager>,
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

        // For component export functions, invoke the `init` procedure first if needed.
        // It loads the data segments and global vars into memory.
        if function.signature().cc.is_wasm_canonical_abi()
            && (link_info.has_globals() || link_info.has_data_segments())
        {
            // Resolve `init` symbolically within the containing module instead of through a
            // fully-qualified component path, which depends on the (user-editable)
            // `[lib].namespace` matching the component's library identity.
            //
            // INVARIANT: this relies on the canonical-ABI export wrappers being emitted into the
            // root component module — the same module where `MasmComponentBuilder` defines
            // `init` (`self.component.modules[0]`); the inner lifted functions in interface and
            // core child modules carry no init prologue. If export wrappers ever move into child
            // modules, this symbol stops resolving and the init target must be threaded in as a
            // qualified path instead. A user-exported method named `init` collides with the
            // generated procedure at definition time ("symbol conflict: found duplicate
            // definitions"), so it cannot silently shadow this target.
            let init = InvocationTarget::Symbol("init".parse().unwrap());
            // Add init call to the emitter's target before emitting the function body; `emit`
            // also registers the invocation so the assembler can resolve the symbolic target.
            emitter.emitter().emit(masm::Instruction::Exec(init), SourceSpan::default());
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
            let truncate_stack = {
                let name = masm::ProcedureName::new("truncate_stack").unwrap();
                let module = masm::LibraryPath::new("::miden::core::sys").unwrap();
                let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
                InvocationTarget::Path(Span::new(SourceSpan::default(), qualified.into_inner()))
            };
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
        for attribute in ["account_procedure", "auth_script", "note_script", "transaction_script"] {
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

    let args = ty.params().iter().cloned().map(masm::TypeExpr::from).collect();
    let results = ty.results().iter().cloned().map(masm::TypeExpr::from).collect();
    Some(masm::FunctionType::new(ty.calling_convention(), args, results))
}

/// Returns true if the block contains at least one real (non-decorator) instruction.
///
/// DebugVar instructions are decorator-only and don't produce MAST nodes. If a procedure
/// body contains only DebugVar ops, the assembler will reject it.
fn block_has_real_instructions(block: &masm::Block) -> bool {
    block.iter().any(|op| match op {
        masm::Op::Inst(inst) => !matches!(inst.inner(), masm::Instruction::DebugVar(_)),
        masm::Op::If {
            then_blk, else_blk, ..
        } => block_has_real_instructions(then_blk) || block_has_real_instructions(else_blk),
        masm::Op::While { body, .. } => block_has_real_instructions(body),
        masm::Op::DoWhile {
            body, condition, ..
        } => block_has_real_instructions(body) || block_has_real_instructions(condition),
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
            masm::Op::DoWhile {
                body, condition, ..
            } => {
                patch_debug_var_locals_in_block(body, aligned_num_locals, stack_pointer_addr);
                patch_debug_var_locals_in_block(condition, aligned_num_locals, stack_pointer_addr);
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
    use midenc_hir::PointerType;

    use super::*;

    #[test]
    fn type_expr_from_hir_pointer_conversion_preserves_address_space() {
        for addrspace in [masm::types::AddressSpace::Byte, masm::types::AddressSpace::Element] {
            let ty = Type::from(PointerType::new_with_address_space(Type::U32, addrspace));

            let masm::TypeExpr::Ptr(ptr) = masm::TypeExpr::from(ty) else {
                panic!("expected pointer type expression");
            };
            assert_eq!(ptr.address_space(), addrspace);
        }
    }
}
