use alloc::collections::BTreeSet;

use midenc_hir2::{
    dataflow::analyses::LivenessAnalysis,
    dialects::builtin,
    traits::{BinaryOp, Commutative},
    Block, Operation, ProgramPoint, ValueRef,
};
use midenc_session::diagnostics::{SourceSpan, Spanned};
use smallvec::SmallVec;

use crate::{
    emit::{InstOpEmitter, OpEmitter},
    linker::LinkInfo,
    masm,
    opt::{OperandMovementConstraintSolver, SolverError},
    Constraint, OperandStack,
};

pub(crate) struct BlockEmitter<'b> {
    pub function: &'b builtin::Function,
    pub liveness: &'b LivenessAnalysis,
    pub link_info: &'b LinkInfo,
    pub invoked: &'b mut BTreeSet<masm::Invoke>,
    pub target: Vec<masm::Op>,
    pub stack: OperandStack,
}

impl BlockEmitter<'_> {
    pub fn nest<'nested, 'current: 'nested>(&'current mut self) -> BlockEmitter<'nested> {
        BlockEmitter {
            function: self.function,
            liveness: self.liveness,
            link_info: self.link_info,
            invoked: self.invoked,
            target: Default::default(),
            stack: self.stack.clone(),
        }
    }

    pub fn emit(mut self, block: &Block) -> masm::Block {
        self.emit_inline(block);
        self.into_emitted_block(block.span())
    }

    pub fn emit_inline(&mut self, block: &Block) {
        // Continue normally, by emitting the contents of the block based on the given schedule
        for op in block.body() {
            self.emit_inst(&op);
            // TODO?: Drop unused results of the instruction immediately
        }
    }

    pub fn into_emitted_block(mut self, span: SourceSpan) -> masm::Block {
        let ops = core::mem::take(&mut self.target);
        masm::Block::new(span, ops)
    }

    fn emit_inst(&mut self, op: &Operation) {
        use crate::HirLowering;

        self.drop_unused_operands_at(op);

        // Move instruction operands into place, minimizing unnecessary stack manipulation ops
        //
        // NOTE: This does not include block arguments for control flow instructions, those are
        // handled separately within the specific handlers for those instructions
        let mut args = op
            .operands()
            .iter()
            .map(|operand| operand.borrow().as_value_ref())
            .collect::<SmallVec<[_; 2]>>();

        let mut constraints = op
            .operands()
            .iter()
            .enumerate()
            .map(|(index, operand)| {
                let value = operand.borrow().as_value_ref();
                if self.liveness.is_live_after_entry(value, op) {
                    Constraint::Copy
                } else {
                    // Check if this is the last use of `value` by this operation
                    let operands = op.operands().all();
                    let remaining = &operands.as_slice()[..index];
                    if remaining.iter().any(|o| o.borrow().as_value_ref() == value) {
                        Constraint::Copy
                    } else {
                        Constraint::Move
                    }
                }
            })
            .collect::<SmallVec<[_; 2]>>();

        // All of Miden's binary ops expect the right-hand operand on top of the stack, this
        // requires us to invert the expected order of operands from the standard ordering in the
        // IR
        //
        // TODO(pauls): We should probably assign a dedicated trait for this type of argument
        // ordering override, rather than assuming that all BinaryOp impls need it
        if op.implements::<dyn BinaryOp>() {
            args.swap(0, 1);
            constraints.swap(0, 1);
        }

        // If we're emitting a commutative binary op, and the operands are on top of the operand
        // stack, then we can skip any stack manipulation, so long as we can consume both of the
        // operands, and they are of the same type. This is a narrow optimization, but a useful one.
        let is_binary_commutative = args.len() == 2 && op.implements::<dyn Commutative>();
        let preserve_stack = if is_binary_commutative {
            let can_move = constraints.iter().all(|c| matches!(c, Constraint::Move));
            let operands_in_place = self.stack[0].as_value().is_none_or(|v| args.contains(&v));
            let operands_in_place =
                operands_in_place && self.stack[1].as_value().is_none_or(|v| args.contains(&v));
            can_move && operands_in_place
        } else {
            false
        };

        if !preserve_stack && !args.is_empty() {
            log::trace!(target: "codegen", "scheduling operands for {op}");
            for arg in args.iter() {
                log::trace!(target: "codegen", "{arg} is live after: {}", self.liveness.is_live_after(*arg, op));
            }
            log::trace!(target: "codegen", "starting with stack: {:#?}", &self.stack);
            self.schedule_operands(&args, &constraints, op.span()).unwrap_or_else(|err| {
                panic!(
                    "failed to schedule operands: {args:?}\nfor inst '{}'\nwith error: \
                     {err:?}\nconstraints: {constraints:?}\nstack: {:#?}",
                    op.name(),
                    self.stack,
                )
            });
            log::trace!(target: "codegen", "stack after scheduling: {:#?}", &self.stack);
        }

        let lowering = op.as_trait::<dyn HirLowering>().unwrap_or_else(|| {
            panic!("illegal operation: no lowering has been defined for '{}'", op.name())
        });
        lowering
            .emit(self)
            .expect("unexpected error occurred when lowering hir operation to masm");
    }

    /// Drop the operands on the stack which are no longer live upon entry into
    /// the current program point.
    ///
    /// This is intended to be called before scheduling `op`
    pub fn drop_unused_operands_at(&mut self, op: &Operation) {
        // We start by computing the set of unused operands on the stack at this point
        // in the program. We will use the resulting vectors to schedule instructions
        // that will move those operands to the top of the stack to be discarded
        let mut unused = SmallVec::<[ValueRef; 4]>::default();
        let mut constraints = SmallVec::<[Constraint; 4]>::default();
        for operand in self.stack.iter().rev() {
            let value = operand.as_value().expect("unexpected non-ssa value on stack");
            // If the given value is not live at or through this op, it should be dropped
            let is_live = self.liveness.is_live_before(value, op)
                || self.liveness.is_live_after_entry(value, op);
            if !is_live {
                log::trace!("should drop {value} at {}", ProgramPoint::before(op));
                unused.push(value);
                constraints.push(Constraint::Move);
            }
        }

        // Next, emit the optimal set of moves to get the unused operands to the top
        if !unused.is_empty() {
            // If the number of unused operands is greater than the number
            // of used operands, then we will schedule manually, since this
            // is a pathological use case for the operand scheduler.
            let num_used = self.stack.len() - unused.len();
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
                        emitter.dropn(batch_size, SourceSpan::default());
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
                                emitter.movdn(unused_chunk_size as u8, SourceSpan::default());
                                emitter.dropn(unused_chunk_size, SourceSpan::default());
                            } else {
                                emitter.swap(1, SourceSpan::default());
                                emitter.drop(SourceSpan::default());
                            }
                        }
                        // We've got multiple unused values together, so choose instead
                        // to move the unused value to the top and drop it
                        _ => {
                            let mut emitter = self.emitter();
                            emitter.movup(current_index as u8, SourceSpan::default());
                            emitter.drop(SourceSpan::default());
                        }
                    }
                    batch_size = 0;
                    current_index = 0;
                }
            } else {
                self.schedule_operands(&unused, &constraints, SourceSpan::default())
                    .unwrap_or_else(|err| {
                        panic!(
                            "failed to schedule unused operands for {}: {err:?}",
                            ProgramPoint::before(op)
                        )
                    });
                let mut emitter = self.emitter();
                emitter.dropn(unused.len(), SourceSpan::default());
            }
        }
    }

    fn schedule_operands(
        &mut self,
        expected: &[ValueRef],
        constraints: &[Constraint],
        span: SourceSpan,
    ) -> Result<(), SolverError> {
        match OperandMovementConstraintSolver::new(expected, constraints, &self.stack) {
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
