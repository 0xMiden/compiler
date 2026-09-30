use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Remove loop invariant arguments from `before` block of a [While] operation.
///
/// A before block argument is considered loop invariant if:
///
/// 1. i-th yield operand is equal to the i-th while operand.
/// 2. i-th yield operand is the k-th after block argument of this loop AND the k-th forwarded
///    operand of the condition is equal to either the i-th before block argument or the i-th
///    while operand.
///
/// For the arguments which are removed, their uses inside [While] are replaced with their
/// corresponding initial value.
///
/// # Example
///
/// INPUT:
///
/// ```text,ignore
/// res = scf.while <...> iter_args(%arg0_before = %a, %arg1_before = %b,
///                                 %arg2_before = %c, ..., %argN_before = %N)
///   {
///        ...
///        scf.condition(%cond) %arg1_before, %arg0_before,
///                             %arg2_before, %arg0_before, ...
///   } do {
///     ^bb0(%arg1_after, %arg0_after_1, %arg2_after, %arg0_after_2,
///          ..., %argK_after):
///        ...
///        scf.yield %arg0_after_2, %b, %arg1_after, ..., %argK_after
///   }
/// ```
///
/// OUTPUT:
///
/// ```text,ignore
/// res = scf.while <...> iter_args(%arg2_before = %c, ..., %argN_before = %N)
///   {
///        ...
///        scf.condition(%cond) %b, %a, %arg2_before, %a, ...
///   } do {
///     ^bb0(%arg1_after, %arg0_after_1, %arg2_after, %arg0_after_2,
///          ..., %argK_after):
///        ...
///        scf.yield %arg1_after, ..., %argK_after
///   }
/// ```
///
/// EXPLANATION:
///
/// We iterate over each yield operand.
///
/// 1. Yield operand 0, %arg0_after_2, is the after block argument at index 3, and the forwarded
///    condition operand at index 3 is %arg0_before, the before block argument of the same column.
///    So we remove before block argument 0 and yield operand 0, and replace all uses of before
///    block argument 0 with its initial value %a.
/// 2. Yield operand 1, %b, is the initial value of column 1. So we remove before block argument 1
///    and yield operand 1, and replace all uses of before block argument 1 with %b.
/// 3. Yield operand 2, %arg1_after, is the after block argument at index 0, but the forwarded
///    condition operand at index 0 is %arg1_before, the before block argument of column 1, not
///    of column 2. So column 2 is not loop invariant and stays.
///
pub struct RemoveLoopInvariantArgsFromBeforeBlock {
    info: PatternInfo,
}

impl RemoveLoopInvariantArgsFromBeforeBlock {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "remove-loop-invariant-args-from-before-block",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for RemoveLoopInvariantArgsFromBeforeBlock {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for RemoveLoopInvariantArgsFromBeforeBlock {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        let before_block = while_op.before().entry_block_ref().unwrap();
        let after_block = while_op.after().entry_block_ref().unwrap();
        let before_args = before_block
            .borrow()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let cond_op = while_op.condition_op();
        let cond_op_args = cond_op
            .borrow()
            .forwarded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let yield_op = while_op.yield_op();
        let yield_op_args = yield_op
            .borrow()
            .yielded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let init_args = while_op
            .inits()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        let result_types = while_op
            .results()
            .iter()
            .map(|r| r.borrow().ty().clone())
            .collect::<SmallVec<[_; 4]>>();

        // Everything below indexes columns by position (inits, before arguments and yield
        // operands on one side; condition operands, after arguments and results on the other),
        // so a loop whose arities disagree is left to the verifier.
        let num_after_args = after_block.borrow().num_arguments();
        if before_args.len() != init_args.len()
            || yield_op_args.len() != init_args.len()
            || cond_op_args.len() != num_after_args
            || result_types.len() != num_after_args
        {
            return Ok(false);
        }

        // Returns true if the `index`-th before block argument is loop invariant, i.e. if the
        // value fed back to it over the back edge is always its initial value.
        let is_loop_invariant = |index: usize| -> bool {
            let init_value = init_args[index];
            let yield_arg = yield_op_args[index];
            // If i-th yield operand is equal to the i-th operand of the `scf.while`, the i-th
            // before block argument is loop invariant
            if yield_arg == init_value {
                return true;
            }

            // If the i-th yield operand is the k-th after block argument, then we check if the
            // k-th forwarded operand of the condition op is equal to either the i-th before block
            // argument or the initial value of the i-th before block argument. If the comparison
            // results `true`, the i-th before block argument is loop invariant.
            //
            // Only after block arguments are mirrored by the condition operands; a block argument
            // of any other block (e.g. the function entry block) says nothing about the back edge.
            let Ok(yield_op_block_arg) = yield_arg.try_downcast_value::<BlockArgument>() else {
                return false;
            };
            let yield_op_block_arg = yield_op_block_arg.borrow();
            if yield_op_block_arg.owner() != after_block {
                return false;
            }
            let cond_op_arg = cond_op_args[yield_op_block_arg.index()];
            cond_op_arg == before_args[index] || cond_op_arg == init_value
        };
        let invariant =
            (0..init_args.len()).map(is_loop_invariant).collect::<SmallVec<[bool; 8]>>();
        if !invariant.contains(&true) {
            return Ok(false);
        }

        let mut new_init_args = SmallVec::<[ValueRef; 4]>::default();
        let mut new_yield_args = SmallVec::<[ValueRef; 4]>::default();
        for (index, invariant) in invariant.iter().copied().enumerate() {
            if !invariant {
                new_init_args.push(init_args[index]);
                new_yield_args.push(yield_op_args[index]);
            }
        }

        // Creating the new op is the only fallible step; nothing has been modified before it.
        let new_while =
            rewriter.r#while(new_init_args.iter().copied(), &result_types, while_op.span())?;

        // The builder populates both regions of the new op with an entry block whose arguments
        // match the retained iter args (before) and the results (after), so the original blocks
        // are merged into them. Only block refs are kept from here on: a borrow of an op or a
        // region must not be alive while the rewriter moves blocks between them, which is why
        // the borrow of the original op is dropped before the merges below.
        let (new_before_block, new_after_block) = {
            let new_while = new_while.borrow();
            (
                new_while.before().entry_block_ref().unwrap(),
                new_while.after().entry_block_ref().unwrap(),
            )
        };

        // Each before block argument is replaced with its initial value if it is loop invariant,
        // and with the next argument of the new before block otherwise.
        let new_before_block_args = {
            let new_before_block = new_before_block.borrow();
            let mut new_args = new_before_block.arguments().iter();
            invariant
                .iter()
                .zip(init_args.iter())
                .map(|(invariant, init_value)| {
                    Some(if *invariant {
                        *init_value
                    } else {
                        *new_args.next().expect("missing argument in new before block") as ValueRef
                    })
                })
                .collect::<SmallVec<[Option<ValueRef>; 4]>>()
        };

        // The new after block keeps the original after block arguments one-to-one.
        let new_after_block_args = new_after_block
            .borrow()
            .arguments()
            .iter()
            .map(|arg| Some(*arg as ValueRef))
            .collect::<SmallVec<[Option<ValueRef>; 4]>>();

        // Narrow the yield to the kept columns in place.
        {
            let mut yield_op = yield_op.as_operation_ref();
            let _guard = rewriter.modify_op_in_place(yield_op);
            yield_op.borrow_mut().set_operands(new_yield_args.iter().copied());
        }

        drop(op);

        rewriter.merge_blocks(before_block, new_before_block, &new_before_block_args);
        rewriter.merge_blocks(after_block, new_after_block, &new_after_block_args);

        let replacements = new_while
            .borrow()
            .results()
            .all()
            .into_iter()
            .map(|r| Some(*r as ValueRef))
            .collect::<SmallVec<[_; 4]>>();
        rewriter.replace_op_with_values(operation, &replacements);

        Ok(true)
    }
}
