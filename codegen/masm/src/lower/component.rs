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
/// A world is not a component, and the difference is what this impl exists to handle: a
/// component's body holds modules, interfaces and functions, while a world's body holds
/// *components* as well. Handing a world's own operation to `MasmComponentBuilder`, which walks a
/// component body, therefore panics the moment it meets the first `builtin.component`.
///
/// So the shape of the world decides how it is lowered:
///
/// - A world holding **no** component is treated as one logical component whose body is the
///   world's, which is what it has always meant here. This is the shape `frontend/masm`'s
///   disassembler produces — it defines modules directly on the world — so it is a live path.
/// - A world holding **one** component is lowered by lowering that component, because a
///   component is what a Miden package is rooted at and carries the identity it is rooted at.
///   Delegating rather than reimplementing is deliberate: the result is then the same
///   [`MasmComponent`] the equivalent standalone `builtin.component` produces, by construction
///   rather than by two implementations agreeing.
/// - A world holding **more than one** component is reported, and that limitation is external to
///   this crate — see `too_many_components`.
///
/// # Top-level items beside the component are normal, and are not an error
///
/// A world is not "a component, optionally". It may hold a component — the current codegen unit —
/// **plus any number of sibling interfaces and modules**, which are either
///
/// - *external dependencies represented in the IR*, which hold declarations only and contribute
///   nothing to the generated Miden Assembly, or
/// - *supporting modules*, which are meant to be translated 1:1 to Miden Assembly modules and
///   linked into the final assembly as ad-hoc modules.
///
/// A world holding a single component is only the *happy path*, and only for the Rust frontend,
/// which compiles to one Wasm component and translates it to one HIR component. Other frontends,
/// the MASM one included, legitimately produce several top-level items. **Neither kind of sibling
/// may fail a build.**
///
/// The first kind is handled: `is_declaration_only` recognizes it and it is ignored, silently,
/// because that is exactly what it is worth. The second kind is **stubbed** — see
/// `report_untranslated_siblings` for the TODO and for why translating it is not a small change.
///
/// Every producer hands this impl a *top-level* world: the world a whole-`builtin.world` `.hir`
/// file parses to, the world `midenc_hir::parse` anchors any other top-level operation at, the
/// world the Wasm frontend builds, and the one `frontend/masm`'s disassembler builds.
impl ToMasmComponent for builtin::World {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
    ) -> Result<MasmComponent, Report> {
        let mut components = Vec::new();
        let mut siblings = Vec::new();
        for op in self.body().entry().body().iter() {
            match op.as_operation_ref().try_downcast_op::<builtin::Component>() {
                Ok(component) => components.push(component),
                Err(op) => siblings.push(op),
            }
        }

        match components.len() {
            0 => world_body_to_masm_component(self, analysis_manager),
            1 => {
                // The analysis manager is rooted at the world, and `AnalysisManager::nest`
                // accepts any proper descendant, so the component impl can nest at its own
                // modules from here exactly as it does when codegen anchors it at the component
                // itself.
                let lowered = components[0].borrow().to_masm_component(analysis_manager)?;
                // Reported after lowering succeeded, so that a build which failed for an
                // unrelated reason is not also told about a limitation it never reached.
                report_untranslated_siblings(self, &siblings);
                Ok(lowered)
            }
            _ => Err(too_many_components(self, &components)),
        }
    }
}

/// Whether `op`, a top-level item of a world, contributes nothing to the generated Miden Assembly.
///
/// This is how an *external dependency represented in the IR* is told apart from a *supporting
/// module*: the former holds declarations only. There is no flag for it — `Symbol::is_declaration`
/// is defined on functions and global variables but not on the modules and interfaces that hold
/// them, so the question has to be asked of the contents.
///
/// Deliberately conservative: anything unrecognized counts as carrying definitions. Guessing wrong
/// in that direction produces a warning about something that did not need one, while guessing
/// wrong in the other direction silently omits code.
fn is_declaration_only(op: &midenc_hir::OperationRef) -> bool {
    /// A body defines nothing if every item in it is itself only a declaration.
    ///
    /// An empty body is vacuously declaration-only, which is the answer we want: an empty module
    /// would lower to an empty Miden Assembly module.
    fn body_is_all_declarations(region: &midenc_hir::Region) -> bool {
        region.entry().body().iter().all(|item| {
            if let Some(function) = item.downcast_ref::<builtin::Function>() {
                function.is_declaration()
            } else if let Some(gv) = item.downcast_ref::<builtin::GlobalVariable>() {
                gv.is_declaration()
            } else {
                // A `builtin::Segment` initializes memory, and so does anything unrecognized as
                // far as this predicate is willing to assume.
                false
            }
        })
    }

    if let Ok(module) = op.try_downcast_op::<builtin::Module>() {
        let module = module.borrow();
        body_is_all_declarations(&module.body())
    } else if let Ok(interface) = op.try_downcast_op::<builtin::Interface>() {
        let interface = interface.borrow();
        body_is_all_declarations(&interface.body())
    } else if let Ok(function) = op.try_downcast_op::<builtin::Function>() {
        let function = function.borrow();
        function.is_declaration()
    } else {
        false
    }
}

/// Warn about supporting modules beside a component that this crate does not yet translate.
///
/// **This is a deliberate stub, not a rejection and not a silent drop.** The intended semantics,
/// recorded here so the next person has the design rather than having to rederive it:
///
/// > A top-level module beside the component is a *supporting module*. It should be emitted 1:1 as
/// > a Miden Assembly module and linked into the final assembly as an ad-hoc module — which, once
/// > it is in [`MasmComponent::modules`], is what happens already: `MasmComponent::source_inputs`
/// > puts every module whose path is not the component root into `support`.
///
/// TODO(codegen): translate these 1:1 instead of warning.
///
/// # Why it is not a small change, and why it is not attempted here
///
/// The obstacle is **linking, not emission**. [`Linker::link`] walks only the direct
/// `builtin::Module` children of the operation it is handed, so the [`LinkInfo`] the component
/// impl computes — `link(Some(id), <the component>)` — cannot see a sibling of the *world*. And
/// `LinkInfo` is what assigns every global variable its address and every data segment its offset,
/// so a sibling lowered against a `LinkInfo` of its own would lay its globals over the component's.
///
/// Doing it properly therefore means one link over the world, with the component's id, and a
/// component impl that accepts a pre-computed [`LinkInfo`] rather than computing its own — plus a
/// decision, which is not this crate's to make alone, about whether supporting modules share the
/// component's globals table, heap base and `init`. Until that lands, omitting them loudly is the
/// honest behaviour: the happy path is unaffected, and anyone who hits this is told exactly what
/// was left out and why.
///
/// Declaration-only siblings are *not* reported. They are ignored by design, and warning about
/// them would make the normal case noisy.
fn report_untranslated_siblings(world: &builtin::World, siblings: &[midenc_hir::OperationRef]) {
    let untranslated = siblings
        .iter()
        .filter(|op| !is_declaration_only(op))
        .collect::<SmallVec<[_; 4]>>();
    if untranslated.is_empty() {
        return;
    }

    let mut diagnostic = world
        .as_operation()
        .context()
        .diagnostics()
        .diagnostic(miden_assembly::diagnostics::Severity::Warning)
        .with_message(
            "top-level items carrying definitions beside a component are not yet translated to \
             Miden Assembly",
        );
    // The first label has to be the primary one; the builder asserts on that ordering.
    for (index, op) in untranslated.into_iter().enumerate() {
        let op = op.borrow();
        let label = format!("this '{}' is omitted from the generated package", op.name());
        diagnostic = if index == 0 {
            diagnostic.with_primary_label(op.span(), label)
        } else {
            diagnostic.with_secondary_label(op.span(), label)
        };
    }
    diagnostic
        .with_help(
            "a module declared beside a component in a world is a supporting module, and is meant \
             to be emitted 1:1 as a Miden Assembly module and linked in as an ad-hoc module. That \
             is not implemented yet, so this build omits it, and code that calls into it will \
             fail to resolve. Top-level items that only declare symbols — external dependencies \
             represented in the IR — contribute no Miden Assembly and are ignored by design; they \
             are not reported here.",
        )
        .emit();
}

/// The report for a world declaring more than one component.
///
/// The **one** shape this impl rejects, and the blocker is external to this crate rather than a
/// gap in it: a Miden package's metadata can currently describe a single component, so a build
/// emits one component per package. Two components in a world would have to become two packages.
/// Work on multi-component packages is happening elsewhere; until it lands there is nothing this
/// crate could do with the second component but invent merge semantics, which would be worse than
/// saying so.
///
/// Note what this is *not*: a claim that worlds are single-component by nature. They are not, and
/// sibling interfaces and modules are ordinary — see the docs on `ToMasmComponent for
/// builtin::World`. Only a second *component* stops a build.
///
/// The wording matters as much as the rejection, so the message says what is unimplemented and the
/// help says who is unblocking it, rather than implying the input is wrong.
fn too_many_components(world: &builtin::World, components: &[builtin::ComponentRef]) -> Report {
    // The limitation belongs in the *message*, not only in the help: a `Report` built from a
    // diagnostic renders its message alone under `Display`, which is all a caller that only
    // formats the error ever sees.
    let mut diagnostic = world
        .as_operation()
        .context()
        .diagnostics()
        .diagnostic(miden_assembly::diagnostics::Severity::Error)
        .with_message(format!(
            "lowering a world containing {} components is not yet implemented",
            components.len()
        ))
        .with_primary_label(world.span(), "in this world");
    for component in components {
        let component = component.borrow();
        diagnostic = diagnostic.with_secondary_label(component.span(), "this component");
    }
    diagnostic
        .with_help(
            "this is a known limitation of the compiler rather than a problem with this input: a \
             Miden package's metadata can currently describe only one component, so a build emits \
             one component per package. Support for multiple components in a package is being \
             worked on; until it lands, compile each component separately.",
        )
        .into_report()
}

/// Derive a MASM component by treating `world`'s body as a component body.
///
/// The meaning a world has always had here, and correct only when the world declares no
/// component of its own: every definition-carrying module in it belongs to one logical
/// component, which has no identity beyond the namespace those modules sit in.
fn world_body_to_masm_component(
    world: &builtin::World,
    analysis_manager: AnalysisManager,
) -> Result<MasmComponent, Report> {
    // Get the current compiler context
    let context = world.as_operation().context_rc();

    // Run the linker for this component in order to compute its data layout
    let link_info = Linker::default().link(None, world.as_operation()).map_err(Report::msg)?;

    // Get the entrypoint, if specified
    let entrypoint = match context.session().options.entrypoint.as_deref() {
        Some(entry) => {
            let entry_id = entry
                .parse::<FunctionIdent>()
                .map_err(|_| Report::msg(format!("invalid entrypoint identifier: '{entry}'")))?;
            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
                entry_id.function.span,
                entry_id.function.as_str().into(),
            )));

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
    let toplevel_namespaces = world
        .body()
        .entry()
        .body()
        .iter()
        // Only modules: this function is reached only for a world that declares no component,
        // so a `builtin::Component` arm here would be unreachable.
        .filter_map(|op| {
            if op.is::<builtin::Module>() {
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
                let namespace = toplevel_namespaces[0].borrow().symbol_name_if_symbol().unwrap();
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
    let heap_base =
        u32::try_from(heap_base).expect("unable to allocate dynamic heap: global table too large");
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

    builder.build(world.as_operation())?;

    Ok(masm_component)
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
    use alloc::{format, rc::Rc, string::String};

    use midenc_hir::{Context, OperationRef, PointerType, diagnostics::Uri};
    use midenc_session::{
        InputFile, Options, Session,
        diagnostics::{CaptureEmitter, DefaultSourceManager},
    };

    use super::*;

    // -------------------------------------------------------------------------------------
    // Fixtures.
    //
    // Task 7's, copied from `midenc-compile/src/pipeline/frontends/hir.rs` rather than shared:
    // `midenc-compile` depends on this crate, so nothing here can import from it. Its report
    // records which shapes parse — in particular that a component id is *one quoted*
    // symbol-path component, because `ComponentId::try_from` splits the `:` and the `@` back
    // out of it itself.
    // -------------------------------------------------------------------------------------

    /// A component, written on its own — the other half of the equivalence [`WORLD`] pins.
    const COMPONENT: &str = r#"
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module private @test {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
"#;

    /// [`COMPONENT`] inside the world that declares it — the *shape* `--emit=hir` writes, and so
    /// the shape a whole-world `.hir` file has.
    ///
    /// Not its literal text, though: `OpPrinter for builtin::Component` prints the id **bare**
    /// (`@hir_ns:test@1.0.0`) while the parser requires it **quoted**, so `--emit=hir` output
    /// holding a component does not re-parse. The quoting here works around that; the defect
    /// is recorded as a `TODO(hir)` on that printer.
    const WORLD: &str = r#"
builtin.world {
    builtin.component private @"hir_ns:test@1.0.0" {
        builtin.module private @test {
            builtin.function public extern("C") @main() {
                builtin.ret;
            };
        };
    };
};
"#;

    /// [`WORLD`] with an *external dependency represented in the IR* beside its component.
    ///
    /// The sibling's one function is declared and not defined — an empty body is what
    /// `Symbol::is_declaration` keys on, and therefore what `is_declaration_only` asks about.
    ///
    /// The braces are empty rather than absent on purpose. `builtin.function` carries the
    /// `SingleRegion` trait, so a function written with no region at all fails verification
    /// ("requires exactly one region, but got 0") even though that is precisely how the printer
    /// writes a declaration. An empty region is a body with no blocks, which is what
    /// `is_declaration` means.
    const WORLD_WITH_DECLARATION_ONLY_SIBLING: &str = r#"
builtin.world {
    builtin.component private @"hir_ns:test@1.0.0" {
        builtin.module private @test {
            builtin.function public extern("C") @main() {
                builtin.ret;
            };
        };
    };
    builtin.module public @external_dep {
        builtin.function public extern("C") @sibling() {
        };
    };
};
"#;

    /// [`WORLD`] with a *supporting module* beside its component.
    ///
    /// Identical to [`WORLD_WITH_DECLARATION_ONLY_SIBLING`] but for the sibling's body, which is
    /// the single bit `is_declaration_only` decides on.
    const WORLD_WITH_SUPPORTING_SIBLING: &str = r#"
builtin.world {
    builtin.component private @"hir_ns:test@1.0.0" {
        builtin.module private @test {
            builtin.function public extern("C") @main() {
                builtin.ret;
            };
        };
    };
    builtin.module public @supporting {
        builtin.function public extern("C") @sibling() {
            builtin.ret;
        };
    };
};
"#;

    /// A world declaring two components, which is what this crate does not implement.
    const TWO_COMPONENT_WORLD: &str = r#"
builtin.world {
    builtin.component private @"hir_ns:first@1.0.0" {
        builtin.module private @first {
            builtin.function public extern("C") @main() {
                builtin.ret;
            };
        };
    };
    builtin.component private @"hir_ns:second@1.0.0" {
        builtin.module private @second {
            builtin.function public extern("C") @other() {
                builtin.ret;
            };
        };
    };
};
"#;

    /// A bare `builtin.module`, which the parser likewise anchors at a world of its own.
    ///
    /// That world holds no component at all, which is the shape `frontend/masm`'s disassembler
    /// produces — `declare_modules` defines modules directly on the world — and therefore the
    /// live path this change must leave alone.
    const MODULE: &str = r#"
builtin.module public @lib {
    builtin.function public extern("C") @main() {
        builtin.ret;
    };
};
"#;

    /// [`MODULE`] twice over: a component-less world declaring **several** top-level modules.
    ///
    /// Derived from the shared fixture rather than written out, so it cannot drift from the
    /// single-module shape it is the counterpart of — the same construction
    /// `a_hir_root_declaring_several_top_level_modules_declares_nothing` uses in
    /// `midenc-compile/src/pipeline/prepare.rs`, which is the preparation half of the same
    /// question.
    fn two_module_world() -> String {
        format!("builtin.world {{{}{}}};\n", MODULE, MODULE.replace("@lib", "@second"))
    }

    /// A library target whose namespace is `namespace`, as [`MasmComponent::source_inputs`]
    /// receives one.
    fn library_target(namespace: &str) -> midenc_session::miden_project::Target {
        midenc_session::miden_project::Target::library(
            Arc::<masm::Path>::from(
                masm::LibraryPath::new(namespace)
                    .unwrap()
                    .to_absolute()
                    .unwrap()
                    .into_owned()
                    .into_boxed_path(),
            ),
            Uri::new("lib.hir"),
        )
    }

    /// Parse `text`, returning the top-level operation it holds.
    ///
    /// `verify: true` matches what the `.hir` frontend does, since HIR that arrives as text has
    /// not been through any of the builders that maintain the IR's invariants.
    fn parse(context: &Rc<Context>, text: &str) -> OperationRef {
        let config = midenc_hir::parse::ParserConfig {
            context: context.clone(),
            verify: true,
        };
        midenc_hir::parse::parse_any(config, Uri::new("test.hir"), text)
            .expect("the fixture should parse")
    }

    /// Lower `world`, as `pipeline::backend::codegen` does when extraction named no component:
    /// the analysis manager is rooted at the world, not at anything inside it.
    fn lower_world(world: builtin::WorldRef) -> Result<MasmComponent, Report> {
        let analysis_manager = AnalysisManager::new(world.as_operation_ref(), None);
        let world = world.borrow();
        world.to_masm_component(analysis_manager)
    }

    /// Parse `text`, whose top-level operation must be a `builtin.world`.
    fn parse_world(context: &Rc<Context>, text: &str) -> builtin::WorldRef {
        parse(context, text)
            .try_downcast_op::<builtin::World>()
            .unwrap_or_else(|_| panic!("the fixture should parse as a world"))
    }

    /// The world the parser anchored `op` at.
    ///
    /// Only for fixtures whose top-level operation is *not* a world; one that is comes back as
    /// the root, with nothing above it. See [`parse_world`].
    fn anchoring_world(op: OperationRef) -> builtin::WorldRef {
        op.parent_op()
            .expect("the parser anchors every non-world top-level operation at a world it creates")
            .try_downcast_op::<builtin::World>()
            .unwrap_or_else(|_| panic!("and that anchor is a world"))
    }

    /// A context whose session captures its diagnostics instead of printing them.
    ///
    /// Needed because the sibling stub's whole observable behaviour is a *warning*: it must not
    /// fail the build and must not be silent, and neither half is checkable against a session
    /// that writes to stderr.
    fn capturing_context() -> (Rc<Context>, alloc::sync::Arc<CaptureEmitter>) {
        let emitter = alloc::sync::Arc::new(CaptureEmitter::new());
        let options = alloc::boxed::Box::new(Options::default());
        let source_manager = alloc::sync::Arc::new(DefaultSourceManager::default());
        let session =
            Session::new(InputFile::empty(), options, Some(emitter.clone()), source_manager)
                .expect("should build a session");
        (Rc::new(Context::new(Rc::new(session))), emitter)
    }

    /// Everything a caller can observe about a lowered component, as one comparable value.
    fn summarize(component: &MasmComponent) -> String {
        format!(
            "id: {:?}\nroot: {}\ninit: {:?}\nentrypoint: {:?}\nheap_base: {}\nstack_pointer: \
             {:?}\nrodata: {:?}\n{component}",
            component.id.as_ref().map(|id| id.to_string()),
            component.root,
            component.init,
            component.entrypoint,
            component.heap_base,
            component.stack_pointer,
            component.rodata,
        )
    }

    /// A world holding a single component lowers to exactly what that component lowers to.
    ///
    /// The defect: the world's *own* operation used to be handed to
    /// [`MasmComponentBuilder::build`], which walks a component *body* and accepts only
    /// modules, interfaces and functions — so it panicked with "invalid component-level
    /// operation: 'builtin.component' is not supported in a component body" on the first
    /// component it met.
    ///
    /// The equality is the point, and it is why the fix delegates rather than reimplements: a
    /// component is what a Miden package is rooted at, so the world around it must not change
    /// the answer.
    ///
    /// The world here is parsed from `.hir` text rather than taken from the parser's anchor, so
    /// the world under test is the one the file declares — the shape `--emit=hir` writes. See
    /// [`WORLD`] for why the id is quoted here but is not in what `--emit=hir` actually prints.
    #[test]
    fn a_world_holding_one_component_lowers_as_that_component() {
        let context = Rc::new(Context::default());
        let from_world =
            lower_world(parse_world(&context, WORLD)).expect("a single-component world lowers");

        // A second context, so that neither lowering can be reading anything the other cached.
        let context = Rc::new(Context::default());
        let op = parse(&context, COMPONENT);
        let component = op
            .try_downcast_op::<builtin::Component>()
            .unwrap_or_else(|_| panic!("the fixture parses as a component"));
        let analysis_manager = AnalysisManager::new(op, None);
        let from_component = component
            .borrow()
            .to_masm_component(analysis_manager)
            .expect("and so does the component on its own");

        assert_eq!(
            summarize(&from_world),
            summarize(&from_component),
            "a world holding one component must lower to what that component lowers to"
        );
    }

    /// And a component on its own still lowers rooted at its own id.
    ///
    /// The discriminating half. Without it the equality above could be satisfied by breaking
    /// the *component* path to match the world's — no id, a root taken from the enclosing
    /// namespace — which is the path every Wasm, Rust and manifest build takes.
    #[test]
    fn a_component_lowers_rooted_at_its_own_id() {
        let context = Rc::new(Context::default());
        let op = parse(&context, COMPONENT);
        let component = op
            .try_downcast_op::<builtin::Component>()
            .unwrap_or_else(|_| panic!("the fixture parses as a component"));
        let analysis_manager = AnalysisManager::new(op, None);
        let lowered = component
            .borrow()
            .to_masm_component(analysis_manager)
            .expect("the component lowers");

        let id = lowered.id.as_ref().expect("a component knows its own id");
        assert_eq!(id.to_string(), "hir_ns:test@1.0.0");
        assert_eq!(
            lowered.root.to_string(),
            "::\"hir_ns:test@1.0.0\"",
            "a component's Miden Assembly is rooted at its id, as one quoted path component"
        );
        assert!(
            format!("{lowered}").contains("main"),
            "and its function must have been lowered: {lowered}"
        );
    }

    /// A world declaring more than one component is reported, not merged and not panicked on.
    #[test]
    fn a_world_declaring_two_components_is_reported_as_unimplemented() {
        let context = Rc::new(Context::default());
        let op = parse(&context, TWO_COMPONENT_WORLD);
        let world = op
            .try_downcast_op::<builtin::World>()
            .unwrap_or_else(|_| panic!("the fixture parses as a world"));
        let err = lower_world(world)
            .err()
            .expect("lowering two components into one package is not implemented");

        let msg = format!("{err}");
        assert!(
            msg.contains("lowering a world containing 2 components"),
            "the report must say what it found, and how many of them: {msg}"
        );
        assert!(
            msg.contains("not yet implemented"),
            "and must read as a limitation of the compiler rather than a malformed input: {msg}"
        );
    }

    /// A declaration-only sibling is ignored entirely, and changes nothing about the result.
    ///
    /// This is an *external dependency represented in the IR* — normal, expected, and worth
    /// nothing to code generation. The assertion is the strong one: the world lowers to exactly
    /// what the same component lowers to with no sibling at all, so the sibling cannot have
    /// leaked into the output. And nothing is warned about, because warning here would make the
    /// ordinary case noisy.
    #[test]
    fn a_declaration_only_sibling_is_ignored() {
        let (context, emitter) = capturing_context();
        let world = parse_world(&context, WORLD_WITH_DECLARATION_ONLY_SIBLING);
        let with_sibling = lower_world(world).expect("a declaration-only sibling must not fail");

        let context = Rc::new(Context::default());
        let alone =
            lower_world(parse_world(&context, WORLD)).expect("and neither must its absence");

        assert_eq!(
            summarize(&with_sibling),
            summarize(&alone),
            "a sibling that only declares symbols contributes no Miden Assembly"
        );
        assert!(
            emitter.captured().is_empty(),
            "and it is ignored by design, so it must not be reported: {}",
            emitter.captured()
        );
    }

    /// A sibling module carrying definitions is omitted, loudly — the deliberate stub.
    ///
    /// Both halves matter, and each is a thing the owner ruled out doing:
    ///
    /// - it must **not** fail the build, because supporting modules beside a component are a
    ///   legitimate shape that other frontends produce, and
    /// - it must **not** be silent, because omitting a module that carries definitions leaves
    ///   callers of it unresolvable at assembly time.
    ///
    /// When `report_untranslated_siblings`'s TODO is done — the sibling emitted 1:1 as a Miden
    /// Assembly module and linked in as an ad-hoc module — this test should be replaced by one
    /// asserting the module reaches `MasmComponent::modules`, not deleted.
    #[test]
    fn a_sibling_module_with_definitions_is_omitted_with_a_warning() {
        let (context, emitter) = capturing_context();
        let world = parse_world(&context, WORLD_WITH_SUPPORTING_SIBLING);

        let lowered = lower_world(world)
            .expect("a supporting module beside a component must not fail the build");
        assert!(
            !context.session().diagnostics.has_errors(),
            "and must not be reported as an error either"
        );

        let captured = emitter.captured();
        assert!(
            captured.contains("not yet translated to Miden Assembly"),
            "the omission must be reported, not silent: {captured}"
        );
        assert!(
            captured.contains("supporting"),
            "and the report must name the item that was left out: {captured}"
        );

        // The component itself still lowered, so the warning is about the sibling alone.
        assert_eq!(
            lowered.id.as_ref().map(|id| id.to_string()).as_deref(),
            Some("hir_ns:test@1.0.0")
        );
        assert!(
            !format!("{lowered}").contains("sibling"),
            "the stub omits the sibling, and this pins that until the TODO is done: {lowered}"
        );
    }

    /// A world holding no component at all still lowers as one logical component.
    ///
    /// The other live path this change must not disturb: `frontend/masm`'s disassembler builds
    /// exactly this shape, and `frontend/masm/tests/e2e.rs` lowers it back through this impl.
    #[test]
    fn a_world_of_modules_still_lowers_as_a_component_body() {
        let context = Rc::new(Context::default());
        let module = parse(&context, MODULE);
        let lowered = lower_world(anchoring_world(module))
            .expect("a world of modules lowers as it always did");

        assert!(lowered.id.is_none(), "a world declares no component id of its own");
        assert_eq!(
            lowered.root.to_string(),
            "::lib",
            "its root is the single top-level namespace it holds"
        );
        assert!(
            format!("{lowered}").contains("main"),
            "and the module's function must have been lowered: {lowered}"
        );
    }

    /// A component-less world's Miden Assembly is rooted at the *target's* namespace.
    ///
    /// Lowering has no target and so cannot answer this: with several top-level modules
    /// `world_body_to_masm_component` falls through to the placeholder `::init`, which is not a
    /// name any source declares and which therefore no synthesized namespace can equal. Since
    /// `load_target_sources` rejects a root module that does not sit exactly at its target's
    /// namespace, such a build could not assemble at all. The first assertion pins that lowering
    /// still produces the placeholder, which is what makes the second one about
    /// [`MasmComponent::source_inputs`] rather than about lowering.
    ///
    /// # What this does *not* claim
    ///
    /// The second assertion pins the limitation that comes with it, so the next person reads it
    /// here rather than rediscovering it. With several top-level modules the placeholder root is
    /// an **empty** module and the real ones are its *siblings*, not its children —
    /// `define_module` finds their absolute paths do not begin with the root and defines them
    /// top-level — so moving the root moves nothing else, and they stay outside the namespace.
    /// Such a build therefore still does not assemble; what it no longer does is fail on a
    /// namespace no source could have produced.
    ///
    /// TODO(codegen): decide what a world of several top-level modules should *be*. Nesting them
    /// under the target's namespace would rename every procedure in them, which is not this
    /// change's to do; rejecting the shape outright may well be the better answer.
    #[test]
    fn a_world_of_several_modules_is_rooted_at_the_target_namespace() {
        let context = Rc::new(Context::default());
        let lowered = lower_world(parse_world(&context, &two_module_world()))
            .expect("a world of several modules lowers");
        assert_eq!(
            lowered.root.to_string(),
            "::init",
            "lowering has no target to root at, so it still picks its placeholder"
        );

        let target = library_target("::example");
        let sources = lowered
            .source_inputs(&target, context.session())
            .expect("and its source inputs are what the assembler is handed");

        assert_eq!(
            sources.root.path(),
            target.namespace.inner().as_ref(),
            "a world declaring no component has no identity of its own, so its root is the \
             namespace its target names"
        );
        assert_eq!(
            sources
                .support
                .iter()
                .map(|module| module.path().to_string())
                .collect::<Vec<_>>(),
            vec!["::lib", "::second"],
            "and the modules the world declares are siblings of the placeholder rather than \
             children of it, so they do not move with it"
        );
    }

    /// A component-less world whose root already agrees with its target comes back unchanged.
    ///
    /// The single-module shape, end to end from `.hir`: lowering roots at `::{module}`, and
    /// preparation's `.hir` scan reads that same module's name, so the two normally agree and
    /// subsuming this case into the same rule costs the common case nothing.
    ///
    /// What this pins is the *outcome* — that nothing observable moved — which is what a caller
    /// sees. It does not pin the equality guard in `MasmComponent::source_inputs`, and cannot:
    /// this fixture's one procedure calls nothing, so there is no call target whose rewriting
    /// would be detectable, and re-rooting to the same path is lossless anyway. The guard itself
    /// is pinned by `a_component_less_world_already_at_the_target_namespace_is_left_alone` in
    /// `artifact.rs`, against a fixture that does have callees.
    #[test]
    fn a_world_of_one_module_already_at_its_targets_namespace_is_left_alone() {
        let context = Rc::new(Context::default());
        let module = parse(&context, MODULE);
        let lowered = lower_world(anchoring_world(module)).expect("a world of one module lowers");
        let emitted = format!("{}", lowered.modules[0]);

        let target = library_target("::lib");
        assert_eq!(
            lowered.root.as_ref(),
            target.namespace.inner().as_ref(),
            "the module's own name and the target's namespace must really be the same path, or \
             this test is about some other case"
        );

        let sources = lowered
            .source_inputs(&target, context.session())
            .expect("its source inputs are what the assembler is handed");

        assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
        assert_eq!(format!("{}", sources.root), emitted, "and nothing in it moved");
    }

    /// A world holding a component keeps that component's id, whatever its target is called.
    ///
    /// The discriminating half of the two above, at the seam that decides it: re-rooting is
    /// justified only for a component-less world, whose modules have no identity beyond the
    /// namespace they sit in. An authored component id *is* the code's identity — every dependent
    /// addresses its procedures through it — so a target named something else must not silently
    /// rename them, and this is the shape every Wasm and Rust build produces.
    #[test]
    fn a_world_holding_one_component_keeps_that_components_id() {
        let context = Rc::new(Context::default());
        let lowered =
            lower_world(parse_world(&context, WORLD)).expect("a single-component world lowers");

        let target = library_target("::example");
        let sources = lowered
            .source_inputs(&target, context.session())
            .expect("its source inputs are what the assembler is handed");

        assert_eq!(
            sources.root.path().to_string(),
            "::\"hir_ns:test@1.0.0\"",
            "an authored component's root is its own library path, and a target named otherwise \
             must fail the assembler's root-module check rather than be quietly accommodated"
        );
    }

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
