use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_hir::{
    Op, OpExt, Span, SymbolTable, Value, ValueRange, ValueRef,
    dialects::builtin,
    traits::{BinaryOp, Commutative},
};
use midenc_session::diagnostics::{Report, Severity, Spanned};
use smallvec::{SmallVec, smallvec};

use super::*;
use crate::{Constraint, emitter::BlockEmitter, masm, opt::operands::SolverOptions};

/// This trait is registered with all ops, of all dialects, which are legal for lowering to MASM.
///
/// The [BlockEmitter] is responsible for then invoking the methods of this trait to facilitate
/// the lowering to Miden Assembly of whole components.
pub trait HirLowering: Op {
    /// This method is invoked once operands have been scheduled for this operation.
    ///
    /// Implementations are expected to:
    ///
    /// * Emit Miden Assembly that matches the semantics of the HIR instruction
    /// * Ensure that the operand stack reflects any effects the operation has (both when consuming
    ///   its operands, and producing results). However, it is permitted to elide stack effects
    ///   that are transient during execution of the operation, so long as those effects are not
    ///   visible outside the instruction (i.e. it should not be possible for other instructions
    ///   to witness such transient effects).
    /// * Ensure that the operand stack state is consistent at control flow join points, i.e. it
    ///   is not valid to allow the stack to diverge based on conditional control flow, in terms
    ///   of where each SSA value is found, and in terms of stack depth.
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report>;

    /// This method is invoked in order to emit any necessary operand stack manipulation sequences,
    /// such that the instruction operands are in their place.
    ///
    /// By default, this uses our operand stack constraint solver to generate a solution for
    /// moving the instruction operands into place in the order they are expected, using liveness
    /// information to compute constraints.
    ///
    /// For operations that can support more efficient schedules due to their semantics, such as
    /// the commutativity property, this method can be overridden to incorporate that information,
    /// and provide a custom schedule.
    fn schedule_operands(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let op = self.as_operation();

        // Move instruction operands into place, minimizing unnecessary stack manipulation ops
        //
        // NOTE: This does not include block arguments for control flow instructions, those are
        // handled separately within the specific handlers for those instructions
        let args = self.required_operands();
        if args.is_empty() {
            return Ok(());
        }

        let mut constraints = emitter.constraints_for(op, &args);
        let mut args = args.into_smallvec();

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

        log::trace!(target: "codegen", "scheduling operands for {op}");
        for arg in args.iter() {
            log::trace!(target: "codegen", "{arg} is live at/after entry: {}", emitter.liveness.is_live_after_entry(*arg, op));
        }
        log::trace!(target: "codegen", "starting with stack: {:#?}", &emitter.stack);
        emitter
            .schedule_operands(
                &args,
                &constraints,
                op.span(),
                SolverOptions {
                    strict: !op.implements::<dyn Commutative>(),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|err| {
                panic!(
                    "failed to schedule operands: {args:?}\nfor inst '{}'\nwith error: \
                     {err:?}\nconstraints: {constraints:?}\nstack: {:#?}",
                    op.name(),
                    &emitter.stack,
                )
            });
        log::trace!(target: "codegen", "stack after scheduling: {:#?}", &emitter.stack);

        Ok(())
    }

    /// Returns the set of operands that must be scheduled on entry to this operation.
    ///
    /// Typically, this excludes successor operands, but it depends on the specific operation. In
    /// order to abstract over these details, this function can be used to customize just this
    /// detail of operand scheduling.
    fn required_operands(&self) -> ValueRange<'_, 4> {
        ValueRange::from(self.operands().all())
    }
}

impl HirLowering for builtin::Ret {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let argc = self.num_operands();

        // Upon return, the operand stack should only contain the function result(s),
        // so empty the stack before proceeding.
        emitter.emitter().truncate_stack(argc, span);

        Ok(())
    }
}

impl HirLowering for builtin::RetImm {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let mut emitter = emitter.emitter();

        // Upon return, the operand stack should only contain the function result(s),
        // so empty the stack before proceeding.
        emitter.truncate_stack(0, span);

        // We need to push the return value on the stack at this point.
        emitter.literal(*self.value(), span);

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
            "expected {cond} on top of the stack"
        );

        let then_body = self.then_body();
        let else_body = self.else_body();

        utils::emit_if(emitter, self.as_operation(), &then_body, &else_body)
    }
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
            // instructions to do so, as this will be handled by the 'if.true' instruction
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
                body_emitter.emitter().push_immediate(true.into(), span);

                body_emitter.into_emitted_block(span)
            };

            let exit_loop_body = {
                let mut body_emitter = body_emitter.nest();

                // Exit the loop
                body_emitter.emitter().push_immediate(false.into(), span);

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
        emitter.emit_op(masm::Op::Inst(Span::new(
            span,
            masm::Instruction::Push(masm::Immediate::Value(Span::new(span, 1u8.into()))),
        )));
        emitter.emit_op(masm::Op::While {
            span,
            body: loop_body,
        });

        Ok(())
    }

    fn required_operands(&self) -> ValueRange<'_, 4> {
        ValueRange::from(self.inits())
    }
}

impl HirLowering for scf::IndexSwitch {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.index_switch' is done by lowering to a sequence of if/else ops, comparing
        // the selector against each non-default case to determine whether control should enter
        // that block. The final else contains the default case.
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
            return utils::emit_binary_search(self, emitter, &[], &cases, 0, 1);
        }

        // Emit binary-search-optimized 'hir.if' sequence
        //
        // Partition such that the condition for the `then` branch guarantees that no fallback
        // branch is needed, i.e. an even number of cases must be in the first partition
        let num_cases = cases.len();
        let midpoint = cases[0].midpoint(cases[cases.len() - 1]);
        let partition_point = core::cmp::min(
            cases.len(),
            cases.partition_point(|item| *item < midpoint).next_multiple_of(2),
        );
        let (a, b) = cases.split_at(partition_point);
        utils::emit_binary_search(self, emitter, a, b, midpoint, num_cases)
    }

    fn required_operands(&self) -> ValueRange<'_, 4> {
        ValueRange::from(self.operands().group(0))
    }
}

impl HirLowering for scf::Yield {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.yield' is a no-op, as it is simply forwarding operands to another region,
        // and the semantics of that are handled by the lowering of the containing op
        log::trace!(target: "codegen", "yielding {:#?}", &_emitter.stack);
        Ok(())
    }
}

impl HirLowering for scf::Condition {
    fn emit(&self, _emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // Lowering 'hir.condition' is a no-op, as it is simply forwarding operands to another
        // region, and the semantics of that are handled by the lowering of the containing op
        log::trace!(target: "codegen", "conditionally yielding {:#?}", &_emitter.stack);
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

impl HirLowering for hir::Breakpoint {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.emit_op(masm::Op::Inst(Span::new(self.span(), masm::Instruction::Breakpoint)));

        Ok(())
    }
}

impl HirLowering for ub::Unreachable {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        // This instruction, if reached, must cause the VM to trap, so we emit an assertion that
        // always fails to guarantee this, i.e. assert(false)
        let span = self.span();
        let mut op_emitter = emitter.emitter();
        op_emitter.emit_push(0u32, span);
        op_emitter.emit(masm::Instruction::Assert, span);

        Ok(())
    }
}

impl HirLowering for ub::Poison {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        use midenc_hir::Type;

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
            .add(midenc_hir::Overflow::Overflowing, self.span());
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
            .sub(midenc_hir::Overflow::Overflowing, self.span());
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
            .mul(midenc_hir::Overflow::Overflowing, self.span());
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
        let mut inst_emitter = emitter.inst_emitter(self.as_operation());
        inst_emitter.pop().expect("operand stack is empty");
        inst_emitter.push(result_ty);
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
        use midenc_hir::{CallOpInterface, CallableOpInterface};

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
        let name = masm::ProcedureName::from_raw_parts(
            path.pop().expect("expected at least two path components"),
        );
        let callee = masm::InvocationTarget::AbsoluteProcedurePath { name, path };

        emitter.inst_emitter(self.as_operation()).exec(callee, signature, self.span());

        Ok(())
    }
}

impl HirLowering for hir::Call {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        use midenc_hir::{CallOpInterface, CallableOpInterface};

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
        let name = masm::ProcedureName::from_raw_parts(
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

impl HirLowering for hir::LoadLocal {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.inst_emitter(self.as_operation()).load_local(self.local(), self.span());
        Ok(())
    }
}

impl HirLowering for hir::Store {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.emitter().store(self.span());
        Ok(())
    }
}

impl HirLowering for hir::StoreLocal {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        emitter.emitter().store_local(self.local(), self.span());
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

impl HirLowering for cf::CondBr {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let then_dest = self.then_dest().successor();
        let else_dest = self.else_dest().successor();

        // This lowering is only legal if it represents a choice between multiple exits
        assert_eq!(
            then_dest.borrow().num_successors(),
            0,
            "illegal cf.cond_br: only exit blocks are supported"
        );
        assert_eq!(
            else_dest.borrow().num_successors(),
            0,
            "illegal cf.cond_br: only exit blocks are supported"
        );

        // Drop the condition if no longer live
        if !emitter
            .liveness
            .is_live_after(self.condition().as_value_ref(), self.as_operation())
        {
            emitter.stack.drop();
        }

        let span = self.span();
        let then_blk = {
            let mut emitter = emitter.nest();

            // At this point is when we need to schedule the successor operands for this block
            let then_operand = self.then_dest();
            let successor_operands = ValueRange::from(then_operand.arguments);
            let constraints = emitter.constraints_for(self.as_operation(), &successor_operands);
            let successor_operands = successor_operands.into_smallvec();
            emitter
                .schedule_operands(&successor_operands, &constraints, span, Default::default())
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to schedule operands: {successor_operands:?}\nfor inst '{}'\nwith \
                         error: {err:?}\nconstraints: {constraints:?}\nstack: {:#?}",
                        self.as_operation().name(),
                        &emitter.stack,
                    )
                });

            // Rename any uses of the block arguments of `then_dest` to the values given as
            // successor operands.
            let then_block = then_dest.borrow();
            for (index, block_argument) in then_block.arguments().iter().copied().enumerate() {
                emitter.stack.rename(index, block_argument as ValueRef);
            }

            emitter.emit(&then_dest.borrow())
        };

        let else_blk = {
            let mut emitter = emitter.nest();

            // At this point is when we need to schedule the successor operands for this block
            let else_operand = self.else_dest();
            let successor_operands = ValueRange::from(else_operand.arguments);
            let constraints = emitter.constraints_for(self.as_operation(), &successor_operands);
            let successor_operands = successor_operands.into_smallvec();
            emitter
                .schedule_operands(&successor_operands, &constraints, span, Default::default())
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to schedule operands: {successor_operands:?}\nfor inst '{}'\nwith \
                         error: {err:?}\nconstraints: {constraints:?}\nstack: {:#?}",
                        self.as_operation().name(),
                        &emitter.stack,
                    )
                });

            // Rename any uses of the block arguments of `else_dest` to the values given as
            // successor operands.
            let else_block = else_dest.borrow();
            for (index, block_argument) in else_block.arguments().iter().copied().enumerate() {
                emitter.stack.rename(index, block_argument as ValueRef);
            }

            emitter.emit(&else_dest.borrow())
        };

        let span = self.span();
        emitter.emit_op(masm::Op::If {
            span,
            then_blk,
            else_blk,
        });

        Ok(())
    }

    fn required_operands(&self) -> ValueRange<'_, 4> {
        ValueRange::from(smallvec![self.condition().as_value_ref()])
    }

    // We only schedule the condition here
    fn schedule_operands(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let op = self.as_operation();

        let condition = self.condition().as_value_ref();
        let constraints = emitter.constraints_for(op, &ValueRange::Borrowed(&[condition]));

        let span = op.span();
        let top = emitter.stack[0].as_value();
        if top == Some(condition) {
            if matches!(constraints[0], Constraint::Copy) {
                emitter.emitter().dup(0, span);
            }
            return Ok(());
        } else {
            let index = emitter.stack.find(&condition).unwrap() as u8;
            match constraints[0] {
                Constraint::Copy => {
                    emitter.emitter().dup(index, span);
                }
                Constraint::Move => {
                    if index == 1 {
                        emitter.emitter().swap(1, span);
                    } else {
                        emitter.emitter().movup(index, span);
                    }
                }
            }
        }

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

impl HirLowering for arith::Join {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let mut inst_emitter = emitter.inst_emitter(self.as_operation());
        inst_emitter.pop().expect("operand stack is empty");
        inst_emitter.pop().expect("operand stack is empty");
        inst_emitter.push(self.result().as_value_ref());
        Ok(())
    }
}

impl HirLowering for arith::Split {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let mut inst_emitter = emitter.inst_emitter(self.as_operation());
        inst_emitter.pop().expect("operand stack is empty");
        inst_emitter.push(self.result_low().as_value_ref());
        inst_emitter.push(self.result_high().as_value_ref());
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
        emitter.inst_emitter(self.as_operation()).literal(addr, self.span());

        Ok(())
    }
}
