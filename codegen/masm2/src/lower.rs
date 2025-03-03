mod native_ptr;

use alloc::sync::Arc;

use midenc_dialect_hir as hir;
use midenc_hir2::{
    dialects::builtin, pass::AnalysisManager, FunctionIdent, Immediate, Op, OpExt, Operation, Span,
    SymbolTable, Value, ValueRef,
};
use midenc_session::diagnostics::{Report, Severity, Spanned};

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

impl HirLowering for hir::Ret {
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

impl HirLowering for hir::RetImm {
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

impl HirLowering for hir::If {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let cond = self.condition().as_value_ref();

        // Ensure `cond` is on top of the stack, and remove it at the same time
        assert_eq!(
            emitter.stack.pop().unwrap().as_value(),
            Some(cond),
            "expected {} on top of the stack",
            cond
        );

        let then_body = self.then_body();
        let then_dest = then_body.entry();
        let else_dest = self.else_body().entry_block_ref();

        let then_blk = {
            let then_emitter = emitter.nest();
            then_emitter.emit(&then_dest)
        };

        let else_blk = match else_dest {
            None => masm::Block::new(span, Default::default()),
            Some(dest) => {
                let else_emitter = emitter.nest();
                else_emitter.emit(&dest.borrow())
            }
        };

        for result in self.results().all().iter().rev().copied() {
            emitter.stack.push(result as ValueRef);
        }

        emitter.emit_op(masm::Op::If {
            span,
            then_blk,
            else_blk,
        });

        Ok(())
    }
}

impl HirLowering for hir::While {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let inputs = self.operands().all();

        // Ensure all of the input operands are on the stack, without consuming them
        {
            let mut stack = emitter.stack.iter().rev();
            for (index, input) in inputs.iter().copied().enumerate() {
                let input = input.borrow().as_value_ref();
                assert_eq!(
                    stack.next().map(|operand| operand.as_value()),
                    Some(Some(input)),
                    "expected {} at stack depth {index}",
                    input,
                );
            }
        }

        // Save a snapshot of the operand stack at entry to the op, without any of the `hir::While`
        // operands, and with the results (if any) added. This will be compared against the state of
        // the operand stack on exit from the op, so that we can sanity check the operand stack
        // state.
        let mut stack = emitter.stack.clone();
        stack.dropn(inputs.len());
        for result in self.results().all().as_slice().iter().rev() {
            stack.push(*result as ValueRef);
        }

        // We map `hir::While` semantics to Miden's 'while.true' semantics as follows:
        //
        // TODO(pauls): We can simplify the lowering here when it is known that a loop is in
        // do-while form, by simply emitting a `true` to enter the loop the first time.
        //
        // * First, we must evaluate the "before" block unconditionally, to obtain the value of the
        //   condition that determines whether or not to enter the loop. This is done by inlining
        //   the body of the "before" block at the current position in the current block
        // * Next, we emit the 'while.true' op itself in the current block
        // * Then, we emit the body of the 'while.true' op. This begins by emitting the "after"
        //   block first, then emitting the "before" block after renaming the region arguments
        //   passed from "after" to "before".
        let before = self.before();
        let before_block = before.entry();

        // Rename the 'hir.while' operands to match the 'before' region's entry block args
        for index in 0..inputs.len() {
            let param = before_block.arguments()[index] as ValueRef;
            emitter.stack.rename(index, param);
        }

        // Emit the 'before' block, which represents the loop header
        emitter.emit_inline(&before_block);

        // Emit the 'while.true' body block to a new block
        let while_body = {
            let mut body_emitter = emitter.nest();

            // Rename the yielded 'before' operands on the operand stack
            let after = self.after();
            let after_block = after.entry();
            for (index, arg) in after_block.arguments().iter().enumerate() {
                body_emitter.stack.rename(index, *arg as ValueRef);
            }

            // Emit the "after" block
            body_emitter.emit_inline(&after_block);

            // At this point, control yields from "after" back to "before" to re-evaluate the loop
            // condition. The "before" block will be emitted inline, but we must ensure that the
            // yielded operands are renamed just as before
            for (index, arg) in before_block.arguments().iter().enumerate() {
                let arg = *arg as ValueRef;
                body_emitter.stack.rename(index, arg);
            }

            // Emit the "before" block
            body_emitter.emit(&before_block)
        };

        // The 'hir.condition' instruction we emitted in the unrolled first iteration will have
        // popped the condition operand, but we need to rename the forwarded operands to the results
        // produced by the 'hir.while' to represent the state after evaluation
        for (index, result) in self.results().all().iter().enumerate() {
            emitter.stack.rename(index, *result as ValueRef);
        }

        // Emit the 'while.true' loop instruction
        emitter.emit_op(masm::Op::While {
            span,
            body: while_body,
        });

        // Validate that the expected operand stack state and the actual state match. We are
        // expecting that there are no observable stack effects outside of the `hir::While`,
        // except that the inputs were consumed, and replaced with results (if any).
        assert_eq!(
            emitter.stack, stack,
            "unexpected observable stack effect leaked from 'hir.while'"
        );

        Ok(())
    }
}

impl HirLowering for hir::IndexSwitch {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.index_switch' is done by lowering to a sequence of if/else ops, comparing
        // the selector against each non-default case to determine whether control should enter
        // that block. The final else contains the default case.
        todo!()
    }
}

impl HirLowering for hir::Yield {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.yield' is a no-op, as it is simply forwarding operands to another region,
        // and the semantics of that are handled by the lowering of the containing op
        Ok(())
    }
}

impl HirLowering for hir::Condition {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.condition' is a no-op, as it is simply forwarding operands to another
        // region, and the semantics of that are handled by the lowering of the containing op

        // Evaluating this is equivalent to consuming the condition operand
        emitter.stack.drop();

        Ok(())
    }
}

impl HirLowering for hir::Constant {
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

impl HirLowering for hir::Unreachable {
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

impl HirLowering for hir::Poison {
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
                match self.ty() {
                    Type::I1 => Immediate::I1(false),
                    Type::U8 => Immediate::U8(0xde),
                    Type::I8 => Immediate::I8(0xdeu8 as i8),
                    Type::U16 => Immediate::U16(0xdead),
                    Type::I16 => Immediate::I16(0xdeadu16 as i16),
                    Type::U32 => Immediate::U32(0xdeadc0de),
                    Type::I32 => Immediate::I32(0xdeadc0deu32 as i32),
                    Type::U64 => Immediate::U64(0xdeadc0dedeadc0de),
                    Type::I64 => Immediate::I64(0xdeadc0dedeadc0deu64 as i64),
                    Type::Felt => Immediate::Felt(miden_core::Felt::new(0xdeadc0de)),
                    Type::U128 => Immediate::U128(0xdeadc0dedeadc0dedeadc0dedeadc0de),
                    Type::I128 => Immediate::I128(0xdeadc0dedeadc0dedeadc0dedeadc0deu128 as i128),
                    Type::U256 => {
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
                    Type::F64 => {
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
                    // We emit a pointer that can never refer to a valid object in memory
                    Type::Ptr(_) => Immediate::U32(u32::MAX),
                    ty => panic!("unexpected poison type: {ty}"),
                }
            },
            span,
        );

        Ok(())
    }
}

impl HirLowering for hir::Add {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).add(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::AddOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .add(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for hir::Sub {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).sub(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::SubOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .sub(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for hir::Mul {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).mul(*self.overflow(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::MulOverflowing {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter
            .inst_emitter(self.as_operation())
            .mul(midenc_hir2::Overflow::Overflowing, self.span());
        Ok(())
    }
}

impl HirLowering for hir::Exp {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).exp(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Div {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_div(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Sdiv {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed division lowering not implemented yet");
    }
}

impl HirLowering for hir::Mod {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_mod(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Smod {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed modular division lowering not implemented yet");
    }
}

impl HirLowering for hir::Divmod {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).checked_divmod(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Sdivmod {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("signed division + modular division lowering not implemented yet");
    }
}

impl HirLowering for hir::And {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).and(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Or {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).or(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Xor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).xor(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Band {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).band(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Bor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bor(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Bxor {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bxor(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Shl {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).shl(self.span());
        Ok(())
    }
}

impl HirLowering for hir::ShlImm {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let rhs = *self.shift();
        emitter.inst_emitter(self.as_operation()).shl_imm(rhs, self.span());
        Ok(())
    }
}

impl HirLowering for hir::Shr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).shr(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Ashr {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        todo!("arithmetic shift right not yet implemented");
    }
}

impl HirLowering for hir::Rotl {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).rotl(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Rotr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).rotr(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Eq {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).eq(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Neq {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).neq(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Gt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).gt(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Gte {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).gte(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Lt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).lt(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Lte {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).lte(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Min {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).min(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Max {
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

impl HirLowering for hir::Trunc {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).trunc(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Zext {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let result = self.result();
        emitter.inst_emitter(self.as_operation()).zext(result.ty(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Sext {
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

impl HirLowering for hir::Select {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).select(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Incr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).incr(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Neg {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).neg(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Inv {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).inv(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Ilog2 {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).ilog2(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Pow2 {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).pow2(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Not {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).not(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Bnot {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).bnot(self.span());
        Ok(())
    }
}

impl HirLowering for hir::IsOdd {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).is_odd(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Popcnt {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).popcnt(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Clz {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).clz(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Ctz {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).ctz(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Clo {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).clo(self.span());
        Ok(())
    }
}

impl HirLowering for hir::Cto {
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
        if parent.is::<hir::While>() {
            depth += 1;
        } else if parent.is::<builtin::Function>() {
            break;
        }
        next = parent.parent_op();
    }
    depth
}
