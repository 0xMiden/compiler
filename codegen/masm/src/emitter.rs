use alloc::collections::BTreeSet;

use miden_assembly::diagnostics::WrapErr;
use midenc_hir::{
    Block, Operation, OperationRef, ProgramPoint, TraceTarget, ValueRange, ValueRef,
    dialects::{
        builtin::{Function, attributes::LocalVariable},
        debuginfo::attributes::{INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChainAttr},
    },
    traits::TransparentCast,
};
use midenc_hir_analysis::analyses::LivenessAnalysis;
use midenc_session::diagnostics::{SourceSpan, Spanned};
use smallvec::SmallVec;

use crate::{
    Constraint, OperandStack,
    emit::{InstOpEmitter, OpEmitter},
    linker::LinkInfo,
    masm,
    opt::{OperandMovementConstraintSolver, SolverError, operands::SolverOptions, peephole},
};

/// The layout of a procedure's locals frame, in field elements.
///
/// Debug locations refer to locals by index, while the frame is addressed by element offset, so
/// both are needed to place a local relative to the frame pointer.
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameLayout<'a> {
    /// The offset of each local from the start of the frame, indexed by local.
    ///
    /// A local wider than one element moves every local declared after it, so the index of a local
    /// is not its offset.
    pub local_offsets: &'a [u32],
    /// The size of the frame, rounded up the way the assembler rounds it. `locaddr.N` addresses
    /// `FMP - aligned_size + N`, so offsets relative to the frame pointer are derived from it.
    pub aligned_size: u32,
}

impl<'a> FrameLayout<'a> {
    /// Builds the layout of a frame holding `num_locals` elements, with `local_offsets` from
    /// [`Function::local_offsets`](midenc_hir::dialects::builtin::Function::local_offsets).
    pub fn new(local_offsets: &'a [u32], num_locals: u16) -> Self {
        Self {
            local_offsets,
            aligned_size: u32::from(num_locals).next_multiple_of(miden_core::WORD_SIZE as u32),
        }
    }

    /// The element offset of `local` from the start of the frame: the operand of the `locaddr`
    /// that addresses it.
    ///
    /// # Panics
    ///
    /// Panics if `local` is not a local of the procedure this frame belongs to.
    pub fn locaddr(&self, local: &LocalVariable) -> u16 {
        let offset =
            self.element_offset(local.as_usize()).expect("local is not part of this frame");
        u16::try_from(offset).expect("local offset exceeds the procedure frame limit")
    }

    /// The element offset shared by executable local accesses and debug expressions.
    pub fn element_offset(&self, index: usize) -> Option<u32> {
        self.local_offsets.get(index).copied()
    }
}

/// Collects the local offsets of `function` for a [`FrameLayout`].
pub(crate) fn local_offsets(function: &Function) -> Vec<u32> {
    function
        .local_offsets()
        .map(|offset| {
            u32::try_from(offset).expect("local offset exceeds the procedure frame limit")
        })
        .collect()
}

pub(crate) fn has_inline_call_chain(operation: &Operation) -> bool {
    let mut has_inline_call_chain = false;
    operation.prewalk_all(|op| {
        has_inline_call_chain |= op.has_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
    });
    has_inline_call_chain
}

pub(crate) struct BlockEmitter<'b> {
    pub module_owner: Option<OperationRef>,
    pub liveness: &'b LivenessAnalysis,
    pub emit_inline_calls: bool,
    /// Layout of the current procedure's locals frame.
    pub frame: FrameLayout<'b>,
    pub link_info: &'b LinkInfo,
    pub invoked: &'b mut BTreeSet<masm::Invoke>,
    pub target: Vec<masm::Op>,
    pub stack: OperandStack,
    pub trace_target: TraceTarget,
}

impl BlockEmitter<'_> {
    pub fn nest<'nested, 'current: 'nested>(&'current mut self) -> BlockEmitter<'nested> {
        BlockEmitter {
            module_owner: self.module_owner,
            liveness: self.liveness,
            emit_inline_calls: self.emit_inline_calls,
            frame: self.frame,
            link_info: self.link_info,
            invoked: self.invoked,
            target: Default::default(),
            stack: self.stack.clone(),
            trace_target: self.trace_target.clone(),
        }
    }

    pub fn emit(mut self, block: &Block) -> masm::Block {
        self.emit_inline(block);
        self.into_emitted_block(block.span())
    }

    pub fn emit_inline(&mut self, block: &Block) {
        // Drop any unused block arguments on block entry
        let block_ref = block.as_block_ref();
        let mut index = 0;
        let unused_params = ValueRange::<2>::from(block.arguments());
        for next_param in unused_params {
            if self.liveness.is_live_at_start(next_param, block_ref) {
                index += 1;
                continue;
            }

            self.emitter().drop_operand_at_position(index, next_param.span());
        }

        // Drop any operands that may have been inherited from a predecessor where they are live,
        // but they are dead on entry to this block. We do this now, rather than later, so that
        // we keep the operand stack clean.
        {
            if let Some(next_op) = block.body().front().get() {
                self.drop_unused_operands_at(&next_op, |value| {
                    // If the given value is not live at this op, it should be dropped
                    self.liveness.is_live_before(value, &next_op)
                });
            }
        }

        // Continue normally, by emitting the contents of the block based on the given schedule
        let scheduling_target = self.trace_target.clone().with_topic("operand-scheduling");
        for op in block.body() {
            self.emit_inst(&op);

            // Drop any dead instruction results immediately
            if op.has_results() {
                let span = op.span();
                let results = ValueRange::<2>::from(op.results().all());
                for next_result in results {
                    if self.liveness.is_live_after(next_result, &op) {
                        continue;
                    }

                    // Results are pushed on top of the stack, except the result of a transparent
                    // cast, which takes its operand's place
                    let index = self
                        .stack
                        .find(&next_result)
                        .expect("an instruction result is not on the operand stack");
                    log::trace!(
                        target: &scheduling_target,
                        symbol = self.trace_target.relevant_symbol();
                        "dropping dead instruction result {next_result} at index {index}"
                    );

                    self.emitter().drop_operand_at_position(index, span);
                }
            }

            // Drop any operands on the stack that did not live across this operation
            if let Some(next_op) = op.as_operation_ref().next() {
                let next_op = next_op.borrow();
                self.drop_unused_operands_at(&next_op, |value| {
                    // If the given value is not live at this op, it should be dropped
                    self.liveness.is_live_before(value, &next_op)
                });
            }
        }
    }

    pub fn into_emitted_block(mut self, span: SourceSpan) -> masm::Block {
        let mut ops = core::mem::take(&mut self.target);
        peephole::simplify(&mut ops);
        masm::Block::new(span, ops)
    }

    fn emit_inst(&mut self, op: &Operation) {
        use crate::HirLowering;

        if self.emit_inline_calls {
            self.emit_inline_call_chain(op);
        }

        // If any values on the operand stack are no longer live, drop them now to avoid wasting
        // operand stack space on operands that will never be used.
        //self.drop_unused_operands_at(op);

        // A transparent cast has no `HirLowering`, so that it cannot be lowered any other way
        if op.implements::<dyn TransparentCast>() {
            self.emit_transparent_cast(op);
            return;
        }

        let Some(lowering) = op.as_trait::<dyn HirLowering>() else {
            panic!("illegal operation: no lowering has been defined for '{}'", op.name());
        };

        // Schedule operands for this instruction
        lowering
            .schedule_operands(self)
            .wrap_err("failed during operand scheduling")
            .unwrap_or_else(|err| panic!("{err}"));

        // Emit the Miden Assembly for this instruction to the current block
        lowering
            .emit(self)
            .wrap_err("failed while emitting instruction lowering")
            .unwrap_or_else(|err| panic!("{err}"));
    }

    /// Lower a [TransparentCast], whose result is its operand under another type.
    ///
    /// Nothing is emitted, and the operand stays where it is: its stack slot becomes the result.
    /// Only when the operand is still live after the cast is it copied first, as for any other
    /// instruction, and the copy becomes the result.
    fn emit_transparent_cast(&mut self, op: &Operation) {
        let operands = ValueRange::<4>::from(op.operands().all());
        let operand = operands.iter().next().expect("a transparent cast has one operand");
        let result = op.results()[0].borrow().as_value_ref();
        let index = match self.constraints_for(op, &operands)[0] {
            Constraint::Move => self
                .stack
                .find(&operand)
                .expect("the operand of a transparent cast is not on the operand stack"),
            Constraint::Copy => {
                self.schedule_operands(
                    &[operand],
                    &[Constraint::Copy],
                    op.span(),
                    SolverOptions::default(),
                )
                .unwrap_or_else(|err| {
                    panic!("failed to copy the operand of '{}': {err:?}", op.name())
                });
                0
            }
        };
        self.stack.retype(index, result);
    }

    fn emit_inline_call_chain(&mut self, op: &Operation) {
        use miden_assembly::{
            ast::DebugInlineCallInfo,
            debuginfo::{ColumnNumber, FileLineCol, LineNumber, Uri},
        };

        self.target.push(masm::Op::Inst(midenc_hir::Span::new(
            op.span(),
            masm::Instruction::DebugInlineCallClear,
        )));

        let Some(attr) = op
            .get_attribute(INLINE_CALL_CHAIN_ATTR_NAME)
            .and_then(|attr| attr.try_downcast_attr::<InlineCallChainAttr>().ok())
        else {
            return;
        };
        let attr = attr.borrow();
        for frame in &attr.frames {
            // TODO: Emit unknown call-site lines once the assembler can represent them.
            // Keep the original zero in HIR rather than inventing a call at line 1.
            let Some(call_line) = LineNumber::new(frame.call_line) else {
                continue;
            };
            let declaration = FileLineCol::new(
                Uri::new(frame.file.as_str()),
                LineNumber::new(frame.line).unwrap_or_default(),
                ColumnNumber::new(frame.column).unwrap_or_default(),
            );
            let call_site = FileLineCol::new(
                Uri::new(frame.call_file.as_str()),
                call_line,
                ColumnNumber::new(frame.call_column).unwrap_or_default(),
            );
            let inline_call = DebugInlineCallInfo::new(frame.name.as_str(), declaration, call_site);
            let inline_call = match frame.linkage_name {
                Some(linkage_name) => inline_call.with_linkage_name(linkage_name.as_str()),
                None => inline_call,
            };
            self.target.push(masm::Op::Inst(midenc_hir::Span::new(
                op.span(),
                masm::Instruction::DebugInlineCall(inline_call),
            )));
        }
    }

    /// Drop the operands on the stack which are no longer live upon entry into
    /// the current program point.
    ///
    /// This is intended to be called before scheduling `op`
    pub fn drop_unused_operands_at<F>(&mut self, op: &Operation, is_live: F)
    where
        F: Fn(ValueRef) -> bool,
    {
        let trace_target = self.trace_target.clone().with_topic("operand-scheduling");
        log::trace!(
            target: &trace_target,
            symbol = self.trace_target.relevant_symbol();
            "dropping unused operands at: {op}"
        );
        // We start by computing the set of unused operands on the stack at this point
        // in the program. We will use the resulting vectors to schedule instructions
        // that will move those operands to the top of the stack to be discarded
        let mut unused = SmallVec::<[ValueRef; 4]>::default();
        let mut constraints = SmallVec::<[Constraint; 4]>::default();
        for operand in self.stack.iter().rev() {
            let value = operand.as_value().expect("unexpected non-ssa value on stack");
            if !is_live(value) {
                log::trace!(
                    target: &trace_target,
                    symbol = self.trace_target.relevant_symbol();
                    "should drop {value} at {}",
                    ProgramPoint::before(op)
                );
                unused.push(value);
                constraints.push(Constraint::Move);
            }
        }

        log::trace!(
            target: &trace_target,
            symbol = self.trace_target.relevant_symbol();
            "found unused operands {unused:?} with constraints {constraints:?}"
        );

        // Next, emit the optimal set of moves to get the unused operands to the top
        if !unused.is_empty() {
            // If the number of unused operands is greater than the number
            // of used operands, then we will schedule manually, since this
            // is a pathological use case for the operand scheduler.
            let num_used = self.stack.len() - unused.len();
            log::trace!(
                target: &trace_target,
                symbol = self.trace_target.relevant_symbol();
                "there are {num_used} used operands out of {}", self.stack.len()
            );
            if unused.len() > num_used {
                // In this case, we emit code starting from the top
                // of the stack, i.e. if we encounter an unused value
                // on top, then we increment a counter and check the
                // next value, and so on, until we reach a used value
                // or the end of the stack. At that point, we emit drops
                // for the unused batch, and reset the counter.
                //
                // If we encounter a used value on top, or we have dropped
                // an unused batch and left a used value on top, we look
                // to see if the next value is used/unused:
                //
                // * If used, we increment the counter until we reach an
                // unused value or the end of the stack. We then move any
                // unused value found to the top and drop it, subtract 1
                // from the counter, and resume where we left off
                //
                // * If unused, we check if it is just a single unused value,
                // or if there is a string of unused values starting there.
                // In the former case, we swap it to the top of the stack and
                // drop it, and start over. In the latter case, we move the
                // used value on top of the stack down past the last unused
                // value, and then drop the unused batch.
                let mut batch_size = 0;
                let mut current_index = 0;
                let mut unused_batch = false;
                while self.stack.len() > current_index {
                    let value = self.stack[current_index].as_value().unwrap();
                    let is_unused = unused.contains(&value);
                    // If we're looking at the top operand, start
                    // a new batch of either used or unused operands
                    if current_index == 0 {
                        unused_batch = is_unused;
                        current_index += 1;
                        batch_size += 1;
                        continue;
                    }

                    // If we're putting together a batch of unused values,
                    // and the current value is unused, extend the batch
                    if unused_batch && is_unused {
                        batch_size += 1;
                        current_index += 1;
                        continue;
                    }

                    // If we're putting together a batch of unused values,
                    // and the current value is used, drop the unused values
                    // we've found so far, and then reset our cursor to the top
                    if unused_batch {
                        let mut emitter = self.emitter();
                        emitter.dropn(batch_size, op.span());
                        batch_size = 0;
                        current_index = 0;
                        continue;
                    }

                    // If we're putting together a batch of used values,
                    // and the current value is used, extend the batch
                    if !is_unused {
                        batch_size += 1;
                        current_index += 1;
                        continue;
                    }

                    // Otherwise, we have found more unused value(s) behind
                    // a batch of used value(s), so we need to determine the
                    // best course of action
                    match batch_size {
                        // If we've only found a single used value so far,
                        // and there is more than two unused values behind it,
                        // then move the used value down the stack and drop the unused.
                        1 => {
                            let unused_chunk_size = self
                                .stack
                                .iter()
                                .rev()
                                .skip(1)
                                .take_while(|o| unused.contains(&o.as_value().unwrap()))
                                .count();
                            let mut emitter = self.emitter();
                            if unused_chunk_size > 1 {
                                emitter.movdn(unused_chunk_size as u8, op.span());
                                emitter.dropn(unused_chunk_size, op.span());
                            } else {
                                emitter.swap(1, op.span());
                                emitter.drop(op.span());
                            }
                        }
                        // We've got multiple unused values together, so choose instead
                        // to move the unused value to the top and drop it
                        _ => {
                            let mut emitter = self.emitter();
                            emitter.movup(current_index as u8, op.span());
                            emitter.drop(op.span());
                        }
                    }
                    batch_size = 0;
                    current_index = 0;
                }

                // We may have accumulated a batch comprising the rest of the stack, handle that
                // here.
                if unused_batch && batch_size > 0 {
                    log::trace!(
                        target: &trace_target,
                        symbol = self.trace_target.relevant_symbol();
                        "dropping {batch_size} operands from {:?}",
                        self.stack
                    );
                    // It should only be possible to hit this point if the entire stack is unused
                    assert_eq!(batch_size, self.stack.len());
                    match batch_size {
                        1 => {
                            self.emitter().drop(op.span());
                        }
                        _ => {
                            self.emitter().dropn(batch_size, op.span());
                        }
                    }
                }
            } else {
                self.schedule_operands(&unused, &constraints, op.span(), Default::default())
                    .unwrap_or_else(|err| {
                        panic!(
                            "failed to schedule unused operands for {}: {err:?}",
                            ProgramPoint::before(op)
                        )
                    });
                let mut emitter = self.emitter();
                emitter.dropn(unused.len(), op.span());
            }
        }
    }

    pub fn schedule_operands(
        &mut self,
        expected: &[ValueRef],
        constraints: &[Constraint],
        span: SourceSpan,
        options: SolverOptions,
    ) -> Result<(), SolverError> {
        match OperandMovementConstraintSolver::new_with_options(
            expected,
            constraints,
            &self.stack,
            options,
        ) {
            Ok(solver) => {
                let mut emitter = self.emitter();
                solver.solve_and_apply(&mut emitter, span)
            }
            Err(SolverError::AlreadySolved) => Ok(()),
            Err(err) => {
                panic!("unexpected error constructing operand movement constraint solver: {err:?}")
            }
        }
    }

    /// Obtain the constraints that apply to this operation's operands, based on the provided
    /// liveness analysis.
    pub fn constraints_for(
        &self,
        op: &Operation,
        operands: &ValueRange<'_, 4>,
    ) -> SmallVec<[Constraint; 4]> {
        operands
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if self.liveness.is_live_after_entry(value, op) {
                    Constraint::Copy
                } else {
                    // Check if this is the last use of `value` by this operation
                    let remaining = operands.slice(..index);
                    if remaining.contains(value) {
                        Constraint::Copy
                    } else {
                        Constraint::Move
                    }
                }
            })
            .collect()
    }

    #[inline]
    pub fn emit_op(&mut self, op: masm::Op) {
        self.target.push(op);
    }

    #[inline(always)]
    pub fn inst_emitter<'short, 'long: 'short>(
        &'long mut self,
        inst: &'long Operation,
    ) -> InstOpEmitter<'short> {
        InstOpEmitter::new(inst, self.invoked, &mut self.target, &mut self.stack)
    }

    #[inline(always)]
    pub fn emitter<'short, 'long: 'short>(&'long mut self) -> OpEmitter<'short> {
        OpEmitter::new(self.invoked, &mut self.target, &mut self.stack)
    }
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_expect_test::{Expect, expect};
    use midenc_hir::{
        AddressSpace, PointerType, SourceSpan, Type, ValueRef,
        dialects::builtin::{self, BuiltinOpBuilder},
        formatter::PrettyPrint,
        pass::AnalysisManager,
        testing::Test,
        version::Version,
    };

    use super::*;

    /// Lower the entry block of `test`'s function, its arguments on the operand stack with the
    /// first on top, and pin the MASM as emitted, before the peephole: what the rename saves must
    /// show without it, since the peephole would also delete a `swap.1 swap.1` the scheduled path
    /// left.
    fn assert_lowers_to(test: &Test, masm: Expect) {
        let function_ref = test.function();
        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>().unwrap();
        let link_info = LinkInfo::new(Some(builtin::ComponentId {
            namespace: "root".into(),
            name: "root".into(),
            version: Version::new(1, 0, 0),
        }));

        let function = function_ref.borrow();
        let entry = function.entry_block();
        let mut stack = OperandStack::new(test.context_rc());
        for arg in entry.borrow().arguments().iter().rev() {
            stack.push(*arg as ValueRef);
        }

        let mut invoked = Default::default();
        let mut emitter = BlockEmitter {
            module_owner: None,
            frame: Default::default(),
            liveness: &liveness,
            emit_inline_calls: false,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen"),
        };
        emitter.emit_inline(&entry.borrow());
        // The block prints one level in, below an empty line
        let printed = masm::Block::new(SourceSpan::UNKNOWN, emitter.target).to_pretty_string();
        let mut lines = String::new();
        for line in printed.trim_start_matches('\n').lines() {
            lines.push_str(line.strip_prefix("    ").unwrap_or(line));
            lines.push('\n');
        }
        masm.assert_eq(&lines);
    }

    /// The arguments of `test`'s function.
    fn arguments(test: &mut Test) -> Vec<ValueRef> {
        let builder = test.function_builder();
        let entry = builder.entry_block();
        entry.borrow().arguments().iter().map(|arg| *arg as ValueRef).collect()
    }

    /// A transparent cast of an operand used nowhere else moves nothing: the `add` finds its
    /// operands where they were.
    #[test]
    fn a_transparent_cast_of_a_dead_operand_moves_nothing() {
        let ptr = Type::from(PointerType::new(Type::U32));
        let mut test = Test::new("transparent_ptrtoint", &[Type::U32, ptr], &[Type::U32]);
        let [x, p] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.ptrtoint(p, Type::U32, span).unwrap();
            let r = builder.add(a, x, span).unwrap();
            builder.ret([r], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                add
                u32assert
            "#]],
        );
    }

    /// `inttoptr` of an address, stored as a pointer where the store wants it, below the address
    /// of the slot it is stored in.
    #[test]
    fn a_transparent_inttoptr_moves_nothing() {
        let ptr = Type::from(PointerType::new_with_address_space(Type::U32, AddressSpace::Element));
        let slot =
            Type::from(PointerType::new_with_address_space(ptr.clone(), AddressSpace::Element));
        let mut test = Test::new("transparent_inttoptr", &[slot, Type::U32], &[]);
        let [slot, addr] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let p = builder.inttoptr(addr, ptr, span).unwrap();
            builder.store(slot, p, span).unwrap();
            builder.ret(None, span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                mem_store
            "#]],
        );
    }

    /// A sign-only `bitcast` of a 32-bit integer.
    #[test]
    fn a_transparent_32_bit_bitcast_moves_nothing() {
        let mut test = Test::new("transparent_bitcast_u32", &[Type::I32, Type::U32], &[Type::I32]);
        let [x, y] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.bitcast(y, Type::I32, span).unwrap();
            let r = builder.add_wrapping(a, x, span).unwrap();
            builder.ret([r], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                u32wrapping_add
            "#]],
        );
    }

    /// A sign-only `bitcast` of a 64-bit integer: two elements, one operand.
    #[test]
    fn a_transparent_64_bit_bitcast_moves_nothing() {
        let mut test = Test::new("transparent_bitcast_u64", &[Type::I64, Type::U64], &[Type::I64]);
        let [x, y] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.bitcast(y, Type::I64, span).unwrap();
            let r = builder.add_wrapping(a, x, span).unwrap();
            builder.ret([r], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u64::wrapping_add
                push.6229491882474008289 emit drop
            "#]],
        );
    }

    /// A `bitcast` of a felt to `i32`: the felt carrier's reinterpretation, which checks nothing.
    #[test]
    fn a_transparent_felt_bitcast_moves_nothing() {
        let mut test =
            Test::new("transparent_bitcast_felt", &[Type::I32, Type::Felt], &[Type::I32]);
        let [x, y] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.bitcast(y, Type::I32, span).unwrap();
            let r = builder.add_wrapping(a, x, span).unwrap();
            builder.ret([r], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                u32wrapping_add
            "#]],
        );
    }

    /// An operand still live after the cast is copied, and the copy becomes the result.
    #[test]
    fn a_transparent_cast_of_a_live_operand_copies_it() {
        let ptr = Type::from(PointerType::new(Type::U32));
        let mut test =
            Test::new("transparent_ptrtoint_copy", &[Type::U32, ptr.clone()], &[Type::U32, ptr]);
        let [x, p] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.ptrtoint(p, Type::U32, span).unwrap();
            let r = builder.add(a, x, span).unwrap();
            builder.ret([r, p], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                dup.1
                add
                u32assert
            "#]],
        );
    }

    /// A 64-bit operand still live after the cast is copied whole, and the copy becomes the
    /// result.
    #[test]
    fn a_transparent_64_bit_cast_of_a_live_operand_copies_it() {
        let mut test = Test::new(
            "transparent_bitcast_u64_copy",
            &[Type::I64, Type::U64],
            &[Type::I64, Type::U64],
        );
        let [x, y] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let a = builder.bitcast(y, Type::I64, span).unwrap();
            let r = builder.add_wrapping(a, x, span).unwrap();
            builder.ret([r, y], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                dup.3
                dup.3
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u64::wrapping_add
                push.6229491882474008289 emit drop
            "#]],
        );
    }

    /// `arith.split` names its limbs most-significant first, and an integer keeps its
    /// least-significant limb on top of the operand stack. So returning the high limb of a
    /// 64-bit integer drops the top element, the low limb.
    #[test]
    fn split_of_a_64_bit_integer_names_the_high_limb_first() {
        let mut test = Test::new("split_2x32", &[Type::U64], &[Type::U32]);
        let [x] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let (hi, _lo) = builder.split2(x, Type::U32, span).unwrap();
            builder.ret([hi], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                drop
            "#]],
        );
    }

    /// Returning the high 64-bit limb of a 128-bit integer drops the low one, the top two elements.
    #[test]
    fn split_of_a_128_bit_integer_into_64_bit_limbs_names_the_high_limb_first() {
        let mut test = Test::new("split_2x64", &[Type::U128], &[Type::U64]);
        let [x] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let (hi, _lo) = builder.split2(x, Type::U64, span).unwrap();
            builder.ret([hi], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
            drop
            drop
        "#]],
        );
    }

    /// Returning the most significant 32-bit limb of a 128-bit integer drops the other three, the
    /// top three elements.
    #[test]
    fn split_of_a_128_bit_integer_into_32_bit_limbs_names_the_high_limb_first() {
        let mut test = Test::new("split_4x32", &[Type::U128], &[Type::U32]);
        let [x] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let [hi, ..] = builder.split4(x, Type::U32, span).unwrap();
            builder.ret([hi], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                movup.2
                drop
                swap.1
                drop
                drop
            "#]],
        );
    }

    /// `arith.join` takes its limbs most-significant first and leaves the least-significant on
    /// top: joining the arguments `(hi, lo)`, which arrive with `hi` on top, swaps them.
    #[test]
    fn join_into_a_64_bit_integer_puts_the_low_limb_on_top() {
        let mut test = Test::new("join_2x32", &[Type::U32, Type::U32], &[Type::U64]);
        let [hi, lo] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let x = builder.join2(hi, lo, Type::U64, span).unwrap();
            builder.ret([x], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                swap.1
            "#]],
        );
    }

    /// Joining two 64-bit limbs `(hi, lo)`, which arrive with `hi` on top, moves `lo` above it.
    #[test]
    fn join_of_64_bit_limbs_puts_the_low_limb_on_top() {
        let mut test = Test::new("join_2x64", &[Type::U64, Type::U64], &[Type::U128]);
        let [hi, lo] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let x = builder.join2(hi, lo, Type::U128, span).unwrap();
            builder.ret([x], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                movdn.3
                movdn.3
            "#]],
        );
    }

    /// Joining four 32-bit limbs, most significant first, which arrive with the most significant
    /// on top, reverses them.
    #[test]
    fn join_of_32_bit_limbs_into_a_128_bit_integer_puts_the_low_limb_on_top() {
        let mut test =
            Test::new("join_4x32", &[Type::U32, Type::U32, Type::U32, Type::U32], &[Type::U128]);
        let limbs = arguments(&mut test);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let limbs = [limbs[0], limbs[1], limbs[2], limbs[3]];
            let x = builder.join4(limbs, Type::U128, span).unwrap();
            builder.ret([x], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                movdn.3
                swap.2
            "#]],
        );
    }

    /// The result of a cast nothing uses is dropped from where the operand was, not from the top.
    #[test]
    fn a_transparent_cast_with_a_dead_result_drops_it_in_place() {
        let mut test = Test::new("transparent_dead_result", &[Type::U32, Type::U32], &[Type::U32]);
        let [x, y] = arguments(&mut test)[..] else {
            unreachable!()
        };
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            builder.bitcast(y, Type::I32, span).unwrap();
            builder.ret([x], span).unwrap();
        }
        assert_lowers_to(
            &test,
            expect![[r#"
                swap.1
                drop
            "#]],
        );
    }
}
