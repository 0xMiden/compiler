mod native_ptr;

use alloc::sync::Arc;

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_hir2::{
    dialects::builtin, pass::AnalysisManager, FunctionIdent, Op, OpExt, Operation, Region, Span,
    SymbolTable, Value, ValueRef,
};
use midenc_session::diagnostics::{Report, Severity, Spanned};
use smallvec::SmallVec;

pub use self::native_ptr::NativePtr;
use crate::{
    artifact::MasmComponent,
    emitter::BlockEmitter,
    linker::{LinkInfo, Linker},
    masm,
};

pub trait ToMasmComponent {
    fn to_masm_component(&self, analysis_manager: AnalysisManager)
        -> Result<MasmComponent, Report>;
}

pub trait HirLowering: Op {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report>;

    /// Return the absolute number of `while.true` loops that must be escaped in order to return to
    /// the top-level of the current function.
    ///
    /// If `op` itself is a loop, it is not counted in the returned depth, as it is designed to
    /// answer the question from the perspective of transfer of control _after_ executing `op`.
    #[inline]
    fn loop_depth(&self) -> usize {
        compute_loop_depth(self.as_operation())
    }
}

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
                let name = masm::ProcedureName::new_unchecked(masm::Ident::new_unchecked(
                    Span::new(entry_id.function.span, entry_id.function.as_str().into()),
                ));
                let path = component_path.clone().append_unchecked(entry_id.module);
                Some(masm::InvocationTarget::AbsoluteProcedurePath { name, path })
            }
            None => None,
        };

        // If we have global variables or data segments, we will require a component initializer
        // function, as well as a module to hold component-level functions such as init
        let requires_init = link_info.has_globals() || link_info.has_data_segments();
        let mut modules = Vec::default();
        if requires_init {
            modules.push(Arc::new(masm::Module::new(
                masm::ModuleKind::Library,
                component_path.clone(),
            )));
        }
        let init = if requires_init {
            Some(masm::InvocationTarget::AbsoluteProcedurePath {
                name: masm::ProcedureName::new("init").unwrap(),
                path: component_path.clone(),
            })
        } else {
            None
        };

        // Initialize the MASM component with basic information we have already
        let id = link_info.component().clone();

        // Compute the first page boundary after the end of the globals table to use as the start
        // of the dynamic heap when the program is executed
        let heap_base = link_info.reserved_memory_bytes()
            + link_info.globals_layout().next_page_boundary() as usize;
        let heap_base = u32::try_from(heap_base)
            .expect("unable to allocate dynamic heap: global table too large");
        let stack_pointer = link_info.globals_layout().stack_pointer_offset();
        let mut masm_component = MasmComponent {
            id,
            init,
            entrypoint,
            kernel: None,
            rodata: Default::default(),
            heap_base,
            stack_pointer,
            modules,
        };
        let builder = MasmComponentBuilder {
            analysis_manager,
            component: &mut masm_component,
            link_info: &link_info,
        };

        builder.build(self)?;

        Ok(masm_component)
    }
}

struct MasmComponentBuilder<'a> {
    component: &'a mut MasmComponent,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
}

impl MasmComponentBuilder<'_> {
    /// Convert the component body to Miden Assembly
    pub fn build(mut self, component: &builtin::Component) -> Result<(), Report> {
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
            "expected top-level namespace module, but one has not been defined"
        );

        module.define_procedure(masm::Export::Procedure(procedure))?;

        Ok(())
    }
}

struct MasmModuleBuilder<'a> {
    module: &'a mut masm::Module,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
}

impl MasmModuleBuilder<'_> {
    pub fn build(mut self, module: &builtin::Module) -> Result<(), Report> {
        let region = module.body();
        let block = region.entry();
        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else if op.is::<builtin::Segment>() || op.is::<builtin::GlobalVariable>() {
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
}

struct MasmFunctionBuilder {
    span: midenc_hir2::SourceSpan,
    name: masm::ProcedureName,
    visibility: masm::Visibility,
    num_locals: u16,
}

impl MasmFunctionBuilder {
    pub fn new(function: &builtin::Function) -> Result<Self, Report> {
        use midenc_hir2::{Symbol, Visibility};

        let name = function.name();
        let name = masm::ProcedureName::new_unchecked(masm::Ident::new_unchecked(Span::new(
            name.span,
            name.as_str().into(),
        )));
        let visibility = match function.visibility() {
            Visibility::Public => masm::Visibility::Public,
            // TODO(pauls): Support internal visibility in MASM
            Visibility::Internal => masm::Visibility::Public,
            Visibility::Private => masm::Visibility::Private,
        };
        let num_locals = u16::try_from(function.num_locals()).map_err(|_| {
            let context = function.as_operation().context();
            context
                .diagnostics()
                .diagnostic(miden_assembly::diagnostics::Severity::Error)
                .with_message("cannot emit masm for function")
                .with_primary_label(
                    function.span(),
                    "too many locals: no more than u16::MAX are supported",
                )
                .into_report()
        })?;

        Ok(Self {
            span: function.span(),
            name,
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

        use midenc_hir2::dataflow::analyses::LivenessAnalysis;

        log::trace!(target: "codegen", "lowering {}", function.as_operation());

        let liveness =
            analysis_manager.get_analysis_for::<LivenessAnalysis, builtin::Function>()?;

        let mut invoked = BTreeSet::default();
        let entry = function.entry_block();
        let mut stack = crate::OperandStack::default();
        {
            let entry_block = entry.borrow();
            for arg in entry_block.arguments().iter().rev().copied() {
                stack.push(arg as ValueRef);
            }
        }
        let emitter = BlockEmitter {
            function,
            liveness: &liveness,
            link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
        };

        let body = emitter.emit(&entry.borrow());

        let Self {
            span,
            name,
            visibility,
            num_locals,
        } = self;

        let mut procedure = masm::Procedure::new(span, visibility, name, num_locals, body);

        procedure.extend_invoked(invoked);

        Ok(procedure)
    }
}

impl HirLowering for builtin::Ret {
    fn emit(&self, block_emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let argc = self.num_operands();
        let loop_level = self.loop_depth();
        let mut emitter = block_emitter.emitter();

        // Upon return, the operand stack should only contain the function result(s),
        // so empty the stack before proceeding.
        emitter.truncate_stack(argc, span);

        // If we're in a loop, push N zeroes on the stack, where N is the current loop depth
        for _ in 0..loop_level {
            emitter.literal(false, span);
        }

        Ok(())
    }
}

impl HirLowering for builtin::RetImm {
    fn emit(&self, block_emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let loop_level = self.loop_depth();
        let mut emitter = block_emitter.emitter();

        // Upon return, the operand stack should only contain the function result(s),
        // so empty the stack before proceeding.
        emitter.truncate_stack(0, span);

        // We need to push the return value on the stack at this point.
        emitter.literal(*self.value(), span);

        // If we're in a loop, push N zeroes on the stack, where N is the current loop depth
        for _ in 0..loop_level {
            emitter.literal(false, span);
        }

        Ok(())
    }
}

impl HirLowering for scf::If {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let cond = self.condition().as_value_ref();

        // Ensure `cond` is on top of the stack, and remove it at the same time
        assert_eq!(
            emitter.stack.pop().unwrap().as_value(),
            Some(cond),
            "expected {} on top of the stack",
            cond
        );

        let then_body = self.then_body();
        let else_body = self.else_body();

        emit_if(emitter, self.as_operation(), &then_body, &else_body)
    }
}

fn emit_if(
    emitter: &mut BlockEmitter<'_>,
    op: &Operation,
    then_body: &Region,
    else_body: &Region,
) -> Result<(), Report> {
    let span = op.span();
    let then_dest = then_body.entry();
    let else_dest = else_body.entry_block_ref();

    let (then_stack, then_blk) = {
        let mut then_emitter = emitter.nest();
        then_emitter.emit_inline(&then_dest);
        // Rename the yielded values on the stack for us to check against
        let mut then_stack = then_emitter.stack.clone();
        for (index, result) in op.results().all().into_iter().enumerate() {
            then_stack.rename(index, *result as ValueRef);
        }
        let then_block = then_emitter.into_emitted_block(then_dest.span());
        (then_stack, then_block)
    };

    let (else_stack, else_blk) = match else_dest {
        None => {
            assert!(
                op.results().is_empty(),
                "an elided 'hir.if' else block requires the '{}' to have no results",
                op.name()
            );
            let else_block = masm::Block::new(span, Default::default());
            let mut else_stack = emitter.stack.clone();
            for (index, result) in op.results().all().into_iter().enumerate() {
                else_stack.rename(index, *result as ValueRef);
            }

            (else_stack, else_block)
        }
        Some(dest) => {
            let dest = dest.borrow();
            let mut else_emitter = emitter.nest();
            else_emitter.emit_inline(&dest);
            // Rename the yielded values on the stack for us to check against
            let mut else_stack = else_emitter.stack.clone();
            for (index, result) in op.results().all().into_iter().enumerate() {
                else_stack.rename(index, *result as ValueRef);
            }

            let else_block = else_emitter.into_emitted_block(dest.span());
            (else_stack, else_block)
        }
    };

    if then_stack != else_stack {
        panic!(
            "unexpected observable stack effect leaked from regions of {op}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
        "
        );
    }

    println!("stack on exit from {op}: {then_stack:#?}");
    emitter.stack = then_stack;

    emitter.emit_op(masm::Op::If {
        span,
        then_blk,
        else_blk,
    });

    Ok(())
}

impl HirLowering for scf::While {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();

        // Emit as follows:
        //
        // hir.while <operands> {
        //     <before>
        // } do {
        //     <after>
        // }
        //
        // to:
        //
        // push.1
        // while.true
        //     <before>
        //     if.true
        //         <after>
        //         push.1
        //     else
        //         push.0
        //     end
        // end
        let num_condition_forwarded_operands = self.condition_op().borrow().forwarded().len();
        let (stack_on_loop_exit, loop_body) = {
            let before = self.before();
            let before_block = before.entry();
            let input_stack = emitter.stack.clone();

            let mut body_emitter = emitter.nest();

            // Rename the 'hir.while' operands to match the 'before' region's entry block args
            assert_eq!(self.operands().len(), before_block.num_arguments());
            for (index, arg) in before_block.arguments().iter().copied().enumerate() {
                body_emitter.stack.rename(index, arg as ValueRef);
            }
            let before_stack = body_emitter.stack.clone();

            // Emit the 'before' block, which represents the loop header
            body_emitter.emit_inline(&before_block);

            // Remove the 'hir.condition' condition flag from the operand stack, but do not emit any
            // instructions to do so, as this will be handled by the 'while.true' instruction
            body_emitter.stack.drop();

            // Take a snapshot of the stack at this point, as it represents the state of the stack
            // on exit from the loop, and perform the following modifications:
            //
            // 1. Rename the forwarded condition operands to the 'hir.while' results
            // 2. Check that all values on the operand stack at this point have definitions which
            //    dominate the successor (i.e. the next op after the 'hir.while' op). We can do this
            //    cheaply by asserting that all of the operands were present on the stack before the
            //    'hir.while', or are a result, as any new operands are by definition something
            //    introduced within the loop itself
            let mut stack_on_loop_exit = body_emitter.stack.clone();
            // 1
            assert_eq!(num_condition_forwarded_operands, self.num_results());
            for (index, result) in self.results().all().iter().copied().enumerate() {
                stack_on_loop_exit.rename(index, result as ValueRef);
            }
            // 2
            for (index, value) in stack_on_loop_exit.iter().rev().enumerate() {
                let value = value.as_value().unwrap();
                let is_result = self.results().all().iter().any(|r| *r as ValueRef == value);
                let is_dominating_def = input_stack.find(&value).is_some();
                assert!(
                    is_result || is_dominating_def,
                    "{value} at stack depth {index} incorrectly escapes its dominance frontier"
                );
            }

            let enter_loop_body = {
                let mut body_emitter = body_emitter.nest();

                // Rename the `hir.condition` forwarded operands to match the 'after' region's entry block args
                let after = self.after();
                let after_block = after.entry();
                assert_eq!(num_condition_forwarded_operands, after_block.num_arguments());
                for (index, arg) in after_block.arguments().iter().copied().enumerate() {
                    body_emitter.stack.rename(index, arg as ValueRef);
                }

                // Emit the "after" block
                body_emitter.emit_inline(&after_block);

                // At this point, control yields from "after" back to "before" to re-evaluate the loop
                // condition. We must ensure that the yielded operands are renamed just as before, then
                // push a `push.1` on the stack to re-enter the loop to retry the condition
                assert_eq!(self.yield_op().borrow().yielded().len(), before_block.num_arguments());
                for (index, arg) in before_block.arguments().iter().copied().enumerate() {
                    body_emitter.stack.rename(index, arg as ValueRef);
                }

                // TODO: Until spills pass is reintroduced, we need to emit code at region terminators
                // that ensures the stack content is uniform across all exits. Previously, the spills
                // pass performed a type of register allocation and coalesced live register sets at
                // basic block boundaries, so the stack was always ordered to keep those sets aligned.
                // Currently, without that pass, anything that is not explicitly returned from a
                // region can be in different orders when exiting from different regions of the same
                // op.
                //
                // In order to determine the order, we must first emit all of the instructions of
                // the block and update the stack for exit, and then we ask the operand stack solver
                // to solve for the whole stack, so that it will emit the necessary instructions to
                // fix up any parts of the stack that are out of place.
                if before_stack != body_emitter.stack {
                    panic!(
                        "unexpected observable stack effect leaked from regions of {}

stack on entry to 'before': {before_stack:#?}
stack on exit from 'after': {:#?}
                            ",
                        self.as_operation(),
                        &body_emitter.stack
                    );
                }

                // Re-enter the "before" block to retry the condition
                body_emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::PushU8(1))));

                body_emitter.into_emitted_block(span)
            };

            let exit_loop_body = {
                let mut body_emitter = body_emitter.nest();

                // Exit the loop
                body_emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::PushU8(0))));

                body_emitter.into_emitted_block(span)
            };

            body_emitter.emit_op(masm::Op::If {
                span,
                then_blk: enter_loop_body,
                else_blk: exit_loop_body,
            });

            (stack_on_loop_exit, body_emitter.into_emitted_block(span))
        };

        emitter.stack = stack_on_loop_exit;

        // Always enter loop on first iteration
        emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::PushU8(1))));
        emitter.emit_op(masm::Op::While {
            span,
            body: loop_body,
        });

        Ok(())
    }
}

impl HirLowering for scf::IndexSwitch {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.index_switch' is done by lowering to a sequence of if/else ops, comparing
        // the selector against each non-default case to determine whether control should enter
        // that block. The final else contains the default case.
        log::trace!(target: "index_switch", "{}", self.as_operation());
        let span = self.span();
        let mut cases = self.cases().iter().copied().collect::<SmallVec<[_; 4]>>();
        cases.sort();

        // We have N cases, plus a default case
        //
        // 1. If we have exactly 1 non-default case, we can lower to an `hir.if`
        // 2. If we have N non-default non-contiguous (or N < 3 contiguous) cases, lower to:
        //
        //      if selector == case1 {
        //          <case1 body>
        //      } else {
        //          if selector == case2 {
        //              <case2 body>
        //          } else {
        //              if selector == caseN {
        //                  <caseN body>
        //              } else {
        //                  <default>
        //              }
        //          }
        //      }
        //
        //      if selector < case3 {
        //         if selector == case1 {
        //             <case1 body>
        //         } else {
        //             <case2 body>
        //         }
        //      } else {
        //         if selector < case4 {
        //            <case3 body>
        //         } else {
        //            if selector == case4 {
        //               <case4 body>
        //            } else {
        //               <default>
        //            }
        //         }
        //      }
        //
        // 3. If we have N non-default contiguous cases, use binary search to reduce search space:
        //
        //      if selector < case3 {
        //         if selector == case1 {
        //             <case1 body>
        //         } else {
        //             <case2 body>
        //         }
        //      } else {
        //         if selector < case4 {
        //            <case3 body>
        //         } else {
        //            if selector == case4 {
        //               <case4 body>
        //            } else {
        //               <default>
        //            }
        //         }
        //      }
        //
        // We do not try to use the binary search approach with non-contiguous cases, as we would
        // be forced to emit duplicate copies of the fallback branch, and it isn't clear the size
        // tradeoff would be worth it without branch hints.

        assert!(!cases.is_empty());
        if cases.len() == 1 {
            // Emit `selector == case0`
            emitter.emit_op(masm::Op::Inst(Span::new(
                span,
                masm::Instruction::EqImm(masm::ImmFelt::Value(Span::new(span, cases[0].into()))),
            )));

            // Emit as 'hir.if'
            let then_body = self.get_case_region(0);
            let else_body = self.default_region();
            return emit_if(emitter, self.as_operation(), &then_body.borrow(), &else_body);
        }

        /*
               let (_, is_contiguous) =
                   cases.iter().skip(1).copied().fold((cases[0], true), |(prev_case, acc), case| {
                       let is_succ = prev_case + 1 == case;
                       (case, is_succ && acc)
                   });
        */
        // Emit binary-search-optimized 'hir.if' sequence
        //
        // Partition such that the condition for the `then` branch guarantees that no fallback
        // branch is needed, i.e. an even number of cases must be in the first partition
        let midpoint = cases[0].midpoint(cases[cases.len() - 1]);
        let partition_point = core::cmp::min(
            cases.len(),
            cases.partition_point(|item| *item < midpoint).next_multiple_of(2),
        );
        let (a, b) = cases.split_at(partition_point);
        emit_binary_search(self, emitter, a, b, midpoint)
    }
}

fn emit_binary_search(
    op: &scf::IndexSwitch,
    emitter: &mut BlockEmitter<'_>,
    a: &[u32],
    b: &[u32],
    midpoint: u32,
) -> Result<(), Report> {
    let span = op.span();

    match a {
        [] => {
            match b {
                [] => {
                    // There is only a single case to emit, so we can just emit an 'hir.if' with fallback
                    //
                    // Emit `selector == b[0]`
                    emitter.emit_op(masm::Op::Inst(Span::new(
                        span,
                        masm::Instruction::EqImm(masm::ImmFelt::Value(Span::new(
                            span,
                            b[0].into(),
                        ))),
                    )));

                    // Emit as 'hir.if'
                    let then_index = op.get_case_index_for_selector(b[0]).unwrap();
                    let then_body = op.get_case_region(then_index);
                    let else_body = op.default_region();
                    emit_if(emitter, op.as_operation(), &then_body.borrow(), &else_body)
                }
                [then_case, else_case] => {
                    // We can emit 'b' as an 'hir.if' with no fallback
                    //
                    // Emit `selector == then_case`
                    emitter.emit_op(masm::Op::Inst(Span::new(
                        span,
                        masm::Instruction::EqImm(masm::ImmFelt::Value(Span::new(
                            span,
                            (*then_case).into(),
                        ))),
                    )));

                    // Emit as 'hir.if'
                    let then_index = op.get_case_index_for_selector(*then_case).unwrap();
                    let then_body = op.get_case_region(then_index);
                    let else_index = op.get_case_index_for_selector(*else_case).unwrap();
                    let else_body = op.get_case_region(else_index);
                    emit_if(emitter, op.as_operation(), &then_body.borrow(), &else_body.borrow())
                }
                _ => panic!(
                    "unexpected partitioning of switch cases: a = empty, b = {b:#?}, midpoint = \
                     {midpoint}"
                ),
            }
        }
        [then_case, else_case] if b.is_empty() => {
            // We can emit 'a' as an 'hir.if' with no fallback
            //
            // Emit `selector == then_case`
            emitter.emit_op(masm::Op::Inst(Span::new(
                span,
                masm::Instruction::EqImm(masm::ImmFelt::Value(Span::new(
                    span,
                    (*then_case).into(),
                ))),
            )));

            // Emit as 'hir.if'
            let then_index = op.get_case_index_for_selector(*then_case).unwrap();
            let then_body = op.get_case_region(then_index);
            let else_index = op.get_case_index_for_selector(*else_case).unwrap();
            let else_body = op.get_case_region(else_index);
            emit_if(emitter, op.as_operation(), &then_body.borrow(), &else_body.borrow())
        }
        [_then_case, _else_case] => {
            // We need to emit an 'hir.if' to split the search at the midpoint, and emit 'a' in
            // the then region, and then recurse with 'b' on the else region
            //
            // Emit `selector < partition_point`
            emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::PushU32(midpoint))));
            emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::U32Lt)));
            let (then_blk, then_stack) = {
                let mut then_emitter = emitter.nest();
                emit_binary_search(op, &mut then_emitter, a, &[], midpoint)?;
                let then_stack = then_emitter.stack.clone();
                (then_emitter.into_emitted_block(span), then_stack)
            };
            let (else_blk, else_stack) = {
                let mut else_emitter = emitter.nest();
                let midpoint = b[0].midpoint(b[b.len() - 1]);
                let partition_point = core::cmp::min(
                    b.len(),
                    b.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (b_then, b_else) = b.split_at(partition_point);
                emit_binary_search(op, &mut else_emitter, b_then, b_else, midpoint)?;
                let else_stack = else_emitter.stack.clone();
                (else_emitter.into_emitted_block(span), else_stack)
            };

            if then_stack != else_stack {
                panic!(
                    "unexpected observable stack effect leaked from regions of {}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
                ",
                    op.as_operation()
                );
            }

            emitter.stack = then_stack;

            emitter.emit_op(masm::Op::If {
                span,
                then_blk,
                else_blk,
            });

            Ok(())
        }
        a => {
            emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::PushU32(midpoint))));
            emitter.emit_op(masm::Op::Inst(Span::new(span, masm::Instruction::U32Lt)));
            let (then_blk, then_stack) = {
                let mut then_emitter = emitter.nest();
                let midpoint = a[0].midpoint(a[a.len() - 1]);
                let partition_point = core::cmp::min(
                    a.len(),
                    a.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (a_then, a_else) = a.split_at(partition_point);
                emit_binary_search(op, &mut then_emitter, a_then, a_else, midpoint)?;
                let then_stack = then_emitter.stack.clone();
                (then_emitter.into_emitted_block(span), then_stack)
            };
            let (else_blk, else_stack) = {
                let mut else_emitter = emitter.nest();
                let midpoint = b[0].midpoint(b[b.len() - 1]);
                let partition_point = core::cmp::min(
                    b.len(),
                    b.partition_point(|item| *item < midpoint).next_multiple_of(2),
                );
                let (b_then, b_else) = b.split_at(partition_point);
                emit_binary_search(op, &mut else_emitter, b_then, b_else, midpoint)?;
                let else_stack = else_emitter.stack.clone();
                (else_emitter.into_emitted_block(span), else_stack)
            };

            if then_stack != else_stack {
                panic!(
                    "unexpected observable stack effect leaked from regions of {}

stack on exit from 'then': {then_stack:#?}
stack on exit from 'else': {else_stack:#?}
                ",
                    op.as_operation()
                );
            }

            emitter.stack = then_stack;

            emitter.emit_op(masm::Op::If {
                span,
                then_blk,
                else_blk,
            });

            Ok(())
        }
    }
}

impl HirLowering for scf::Yield {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.yield' is a no-op, as it is simply forwarding operands to another region,
        // and the semantics of that are handled by the lowering of the containing op
        Ok(())
    }
}

impl HirLowering for scf::Condition {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.condition' is a no-op, as it is simply forwarding operands to another
        // region, and the semantics of that are handled by the lowering of the containing op
        Ok(())
    }
}

impl HirLowering for arith::Constant {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let value = *self.value();

        emitter.inst_emitter(self.as_operation()).literal(value, self.span());

        Ok(())
    }
}

impl HirLowering for hir::Assert {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let code = *self.code();

        emitter.emitter().assert(Some(code), self.span());

        Ok(())
    }
}

impl HirLowering for hir::Assertz {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let code = *self.code();

        emitter.emitter().assertz(Some(code), self.span());

        Ok(())
    }
}

impl HirLowering for hir::AssertEq {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.emitter().assert_eq(self.span());

        Ok(())
    }
}

impl HirLowering for hir::AssertEqImm {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let rhs = *self.rhs();

        emitter.emitter().assert_eq_imm(rhs, self.span());

        Ok(())
    }
}

impl HirLowering for ub::Unreachable {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // This instruction, if reached, must cause the VM to trap, so we emit an assertion that
        // always fails to guarantee this, i.e. assert(false)
        let span = self.span();
        let mut op_emitter = emitter.emitter();
        op_emitter.emit(masm::Instruction::PushU32(0), span);
        op_emitter.emit(masm::Instruction::Assert, span);

        Ok(())
    }
}

impl HirLowering for ub::Poison {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        use midenc_hir2::Type;

        // This instruction represents a value that results from undefined behavior in a program.
        // The presence of it does not indicate that a program is invalid, but rather, the fact that
        // undefined behavior resulting from control flow to unreachable code produces effectively
        // any value in the domain of the type associated with the poison result.
        //
        // For our purposes, we choose a value that will appear obvious in a debugger, should it
        // ever appear as an operand to an instruction; and a value that we could emit debug asserts
        // for should we ever wish to do so. We could also catch the evaluation of poison under an
        // emulator for the IR itself.
        let span = self.span();
        let mut op_emitter = emitter.inst_emitter(self.as_operation());
        op_emitter.literal(
            {
                match self.value().as_immediate() {
                    Ok(imm) => imm,
                    Err(Type::U256) => {
                        return Err(self
                            .as_operation()
                            .context()
                            .diagnostics()
                            .diagnostic(Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(
                                span,
                                "the lowering for u256 immediates is not yet implemented",
                            )
                            .into_report());
                    }
                    Err(Type::F64) => {
                        return Err(self
                            .as_operation()
                            .context()
                            .diagnostics()
                            .diagnostic(Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(
                                span,
                                "the lowering for f64 immediates is not yet implemented",
                            )
                            .into_report());
                    }
                    Err(ty) => panic!("unexpected poison type: {ty}"),
                }
            },
            span,
        );

        Ok(())
    }
}

impl HirLowering for arith::Add {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).add(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::AddOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .add(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for arith::Sub {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).sub(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::SubOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .sub(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for arith::Mul {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).mul(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::MulOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .mul(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for arith::Exp {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).exp(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Div {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_div(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Sdiv {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed division lowering not implemented yet");
    }
}

impl HirLowering for arith::Mod {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_mod(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Smod {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed modular division lowering not implemented yet");
    }
}

impl HirLowering for arith::Divmod {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_divmod(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Sdivmod {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed division + modular division lowering not implemented yet");
    }
}

impl HirLowering for arith::And {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).and(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Or {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).or(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Xor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).xor(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Band {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).band(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Bor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bor(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Bxor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bxor(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Shl {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).shl(self.span());
        Ok(())
    }
}

impl HirLowering for arith::ShlImm {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let rhs = *self.shift();
        emitter.inst_emitter(self.as_operation()).shl_imm(rhs, self.span());
        Ok(())
    }
}

impl HirLowering for arith::Shr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).shr(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Ashr {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("arithmetic shift right not yet implemented");
    }
}

impl HirLowering for arith::Rotl {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).rotl(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Rotr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).rotr(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Eq {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).eq(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Neq {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).neq(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Gt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).gt(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Gte {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).gte(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Lt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).lt(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Lte {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).lte(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Min {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).min(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Max {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).max(self.span());
        Ok(())
    }
}

impl HirLowering for hir::PtrToInt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result_ty = self.result().ty().clone();
        emitter.stack.pop().expect("operand stack is empty");
        emitter.stack.push(result_ty);
        Ok(())
    }
}

impl HirLowering for hir::IntToPtr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).inttoptr(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Cast {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).cast(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Bitcast {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).bitcast(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::Trunc {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).trunc(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::Zext {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).zext(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for arith::Sext {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).sext(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Exec {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        use midenc_hir2::{CallOpInterface, CallableOpInterface};

        let callee = self.resolve().ok_or_else(|| {
            let context = self.as_operation().context();
            context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid call operation: unable to resolve callee")
                .with_primary_label(
                    self.span(),
                    "this symbol path is not resolvable from this operation",
                )
                .with_help(
                    "Make sure that all referenced symbols are reachable via the root symbol \
                     table, and use absolute paths to refer to symbols in ancestor/sibling modules",
                )
                .into_report()
        })?;
        let callee = callee.borrow();
        let callee_path = callee.path();
        let signature = match callee.as_symbol_operation().as_trait::<dyn CallableOpInterface>() {
            Some(callable) => callable.signature(),
            None => {
                let context = self.as_operation().context();
                return Err(context
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message("invalid call operation: callee is not a callable op")
                    .with_primary_label(
                        self.span(),
                        format!(
                            "this symbol resolved to a '{}' op, which does not implement Callable",
                            callee.as_symbol_operation().name()
                        ),
                    )
                    .into_report());
            }
        };

        // Convert the path components to an absolute procedure path
        let mut path = callee_path.to_library_path();
        let name = masm::ProcedureName::new_unchecked(
            path.pop().expect("expected at least two path components"),
        );
        let callee = masm::InvocationTarget::AbsoluteProcedurePath { name, path };

        emitter.inst_emitter(self.as_operation()).exec(callee, signature, self.span());

        Ok(())
    }
}

impl HirLowering for hir::Call {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        use midenc_hir2::{CallOpInterface, CallableOpInterface};

        let callee = self.resolve().ok_or_else(|| {
            let context = self.as_operation().context();
            context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid call operation: unable to resolve callee")
                .with_primary_label(
                    self.span(),
                    "this symbol path is not resolvable from this operation",
                )
                .with_help(
                    "Make sure that all referenced symbols are reachable via the root symbol \
                     table, and use absolute paths to refer to symbols in ancestor/sibling modules",
                )
                .into_report()
        })?;
        let callee = callee.borrow();
        let callee_path = callee.path();
        let signature = match callee.as_symbol_operation().as_trait::<dyn CallableOpInterface>() {
            Some(callable) => callable.signature(),
            None => {
                let context = self.as_operation().context();
                return Err(context
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message("invalid call operation: callee is not a callable op")
                    .with_primary_label(
                        self.span(),
                        format!(
                            "this symbol resolved to a '{}' op, which does not implement Callable",
                            callee.as_symbol_operation().name()
                        ),
                    )
                    .into_report());
            }
        };

        // Convert the path components to an absolute procedure path
        let mut path = callee_path.to_library_path();
        let name = masm::ProcedureName::new_unchecked(
            path.pop().expect("expected at least two path components"),
        );
        let callee = masm::InvocationTarget::AbsoluteProcedurePath { name, path };

        emitter.inst_emitter(self.as_operation()).call(callee, signature, self.span());

        Ok(())
    }
}

impl HirLowering for hir::Load {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).load(result.ty().clone(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Store {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.emitter().store(self.span());
        Ok(())
    }
}

impl HirLowering for hir::MemGrow {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).mem_grow(self.span());
        Ok(())
    }
}

impl HirLowering for hir::MemSize {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).mem_size(self.span());
        Ok(())
    }
}

impl HirLowering for hir::MemSet {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).memset(self.span());
        Ok(())
    }
}

impl HirLowering for hir::MemCpy {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).memcpy(self.span());
        Ok(())
    }
}

impl HirLowering for cf::Select {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).select(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Incr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).incr(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Neg {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).neg(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Inv {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).inv(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Ilog2 {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).ilog2(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Pow2 {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).pow2(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Not {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).not(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Bnot {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bnot(self.span());
        Ok(())
    }
}

impl HirLowering for arith::IsOdd {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).is_odd(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Popcnt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).popcnt(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Clz {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).clz(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Ctz {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).ctz(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Clo {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).clo(self.span());
        Ok(())
    }
}

impl HirLowering for arith::Cto {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).cto(self.span());
        Ok(())
    }
}

impl HirLowering for builtin::GlobalSymbol {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let context = self.as_operation().context();

        // 1. Resolve symbol to computed address in global layout
        let current_module = self
            .nearest_parent_op::<builtin::Module>()
            .expect("expected 'hir.global_symbol' op to have a module ancestor");
        let symbol = current_module.borrow().resolve(&self.symbol().path).ok_or_else(|| {
            context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid symbol reference")
                .with_primary_label(
                    self.span(),
                    "unable to resolve this symbol in the current module",
                )
                .into_report()
        })?;

        let global_variable = symbol
            .borrow()
            .downcast_ref::<builtin::GlobalVariable>()
            .map(|gv| unsafe { builtin::GlobalVariableRef::from_raw(gv) })
            .ok_or_else(|| {
                context
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message("invalid symbol reference")
                    .with_primary_label(
                        self.span(),
                        format!(
                            "this symbol resolves to a '{}', but a 'hir.global_variable' was \
                             expected",
                            symbol.borrow().as_symbol_operation().name()
                        ),
                    )
                    .into_report()
            })?;

        let computed_addr = emitter
            .link_info
            .globals_layout()
            .get_computed_addr(global_variable)
            .expect("link error: missing global variable in computed global layout");
        let addr = computed_addr.checked_add_signed(*self.offset()).ok_or_else(|| {
            context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid global symbol offset")
                .with_primary_label(
                    self.span(),
                    "the specified offset is invalid for the referenced symbol",
                )
                .with_help(
                    "the offset is invalid because the computed address under/overflows the \
                     address space",
                )
                .into_report()
        })?;

        // 2. Push computed address on the stack as the result
        emitter.emitter().push_u32(addr, self.span());

        Ok(())
    }
}

fn compute_loop_depth(op: &Operation) -> usize {
    let mut depth = 0;
    let mut next = op.parent_op();
    while let Some(parent) = next.take() {
        let parent = parent.borrow();
        if parent.is::<scf::While>() {
            depth += 1;
        } else if parent.is::<builtin::Function>() {
            break;
        }
        next = parent.parent_op();
    }
    depth
}
