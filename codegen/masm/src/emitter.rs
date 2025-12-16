use alloc::collections::BTreeSet;

use miden_assembly::diagnostics::WrapErr;
use midenc_hir::{Block, Operation, ProgramPoint, ValueRange, ValueRef};
use midenc_hir_analysis::analyses::LivenessAnalysis;
use midenc_session::diagnostics::{SourceSpan, Spanned};
use smallvec::SmallVec;

use crate::{
    Constraint, OperandStack,
    emit::{InstOpEmitter, OpEmitter},
    linker::LinkInfo,
    masm,
    opt::{OperandMovementConstraintSolver, SolverError, operands::SolverOptions},
};

pub(crate) struct BlockEmitter<'b> {
    pub liveness: &'b LivenessAnalysis,
    pub link_info: &'b LinkInfo,
    pub invoked: &'b mut BTreeSet<masm::Invoke>,
    pub target: Vec<masm::Op>,
    pub stack: OperandStack,
}

impl BlockEmitter<'_> {
    pub fn nest<'nested, 'current: 'nested>(&'current mut self) -> BlockEmitter<'nested> {
        BlockEmitter {
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
        for op in block.body() {
            self.emit_inst(&op);

            // Drop any dead instruction results immediately
            if op.has_results() {
                let span = op.span();
                index = 0;
                let results = ValueRange::<2>::from(op.results().all());
                for next_result in results {
                    if self.liveness.is_live_after(next_result, &op) {
                        index += 1;
                        continue;
                    }

                    log::trace!(target: "codegen", "dropping dead instruction result {next_result} at index {index}");

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
        let ops = core::mem::take(&mut self.target);
        masm::Block::new(span, ops)
    }

    fn emit_inst(&mut self, op: &Operation) {
        use crate::HirLowering;

        // If any values on the operand stack are no longer live, drop them now to avoid wasting
        // operand stack space on operands that will never be used.
        //self.drop_unused_operands_at(op);

        let lowering = op.as_trait::<dyn HirLowering>().unwrap_or_else(|| {
            panic!("illegal operation: no lowering has been defined for '{}'", op.name())
        });

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

    /// Drop the operands on the stack which are no longer live upon entry into
    /// the current program point.
    ///
    /// This is intended to be called before scheduling `op`
    pub fn drop_unused_operands_at<F>(&mut self, op: &Operation, is_live: F)
    where
        F: Fn(ValueRef) -> bool,
    {
        log::trace!(target: "codegen", "dropping unused operands at: {op}");
        // We start by computing the set of unused operands on the stack at this point
        // in the program. We will use the resulting vectors to schedule instructions
        // that will move those operands to the top of the stack to be discarded
        let mut unused = SmallVec::<[ValueRef; 4]>::default();
        let mut constraints = SmallVec::<[Constraint; 4]>::default();
        for operand in self.stack.iter().rev() {
            let value = operand.as_value().expect("unexpected non-ssa value on stack");
            if !is_live(value) {
                log::trace!(target: "codegen", "should drop {value} at {}", ProgramPoint::before(op));
                unused.push(value);
                constraints.push(Constraint::Move);
            }
        }

        log::trace!(target: "codegen", "found unused operands {unused:?} with constraints {constraints:?}");

        // Next, emit the optimal set of moves to get the unused operands to the top
        if !unused.is_empty() {
            // If the number of unused operands is greater than the number
            // of used operands, then we will schedule manually, since this
            // is a pathological use case for the operand scheduler.
            let num_used = self.stack.len() - unused.len();
            log::trace!(target: "codegen", "there are {num_used} used operands out of {}", self.stack.len());
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
                    log::trace!(target: "codegen", "dropping {batch_size} operands from {:?}", &self.stack);
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
