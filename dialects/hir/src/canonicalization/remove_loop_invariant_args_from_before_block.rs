use alloc::rc::Rc;

use midenc_hir2::*;

use crate::{
    builders::{DefaultInstBuilder, InstBuilder},
    ops::While,
    HirDialect,
};

/// Remove loop invariant arguments from `before` block of a [While] operation.
///
/// A before block argument is considered loop invariant if:
///
/// 1. i-th yield operand is equal to the i-th while operand.
/// 2. i-th yield operand is k-th after block argument which is (k+1)-th condition operand AND this
///    (k+1)-th condition operand is equal to i-th iter argument/while operand.
///
/// For the arguments which are removed, their uses inside [While] are replaced with their
/// corresponding initial value.
///
/// # Example
///
/// INPUT:
///
/// ```text,ignore
/// res = scf.while <...> iter_args(%arg0_before = %a, %arg1_before = %b, ..., %argN_before = %N)
///   {
///        ...
///        scf.condition(%cond) %arg1_before, %arg0_before,
///                             %arg2_before, %arg0_before, ...
///   } do {
///     ^bb0(%arg1_after, %arg0_after_1, %arg2_after, %arg0_after_2,
///          ..., %argK_after):
///        ...
///        scf.yield %arg0_after_2, %b, %arg1_after, ..., %argN
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
///        scf.yield %arg1_after, ..., %argN
///   }
/// ```
///
/// EXPLANATION:
///
/// We iterate over each yield operand.
///
/// 1. 0-th yield operand %arg0_after_2 is 4-th condition operand %arg0_before, which in turn is the
///    0-th iter argument. So we remove 0-th before block argument and yield operand, and replace
///    all uses of the 0-th before block argument with its initial value %a.
/// 2. 1-th yield operand %b is equal to the 1-th iter arg's initial value. So we remove this
///    operand and the corresponding before block argument and replace all uses of 1-th before block
///    argument
///
pub struct RemoveLoopInvariantArgsFromBeforeBlock {
    info: PatternInfo,
}

impl RemoveLoopInvariantArgsFromBeforeBlock {
    pub fn new(context: Rc<Context>) -> Self {
        let hir_dialect = context.get_or_register_dialect::<HirDialect>();
        let while_op = hir_dialect.registered_name::<While>().expect("hir.while is not registered");
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
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

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

        let mut can_simplify = false;
        for (index, (init_value, yield_arg)) in while_op
            .inits()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .zip(yield_op_args.iter().copied())
            .enumerate()
        {
            // If i-th yield operand is equal to the i-th operand of the `hir.while`, the i-th
            // before block argument is loop invariant
            if yield_arg == init_value {
                can_simplify = true;
                break;
            }

            // If the i-th yield operand is k-th after block argument, then we check if the (k+1)-th
            // condition op operand is equal to either the i-th before block argument or the initial
            // value of i-th before block argument. If the comparison results `true`, i-th before
            // block argument is loop invariant.
            if let Ok(yield_op_block_arg) = yield_arg.try_downcast::<BlockArgument, dyn Value>() {
                let cond_op_arg = cond_op_args[yield_op_block_arg.borrow().index()];
                if cond_op_arg == before_args[index] || cond_op_arg == init_value {
                    can_simplify = true;
                    break;
                }
            }
        }

        if !can_simplify {
            return Ok(false);
        }

        let mut new_init_args = SmallVec::<[ValueRef; 4]>::default();
        let mut new_yield_args = SmallVec::<[ValueRef; 4]>::default();
        let mut before_block_init_val_map = SmallVec::<[Option<ValueRef>; 8]>::default();
        before_block_init_val_map.resize(yield_op_args.len(), None);
        for (index, (init_value, yield_arg)) in while_op
            .inits()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .zip(yield_op_args.iter().copied())
            .enumerate()
        {
            if yield_arg == init_value {
                before_block_init_val_map[index] = Some(init_value);
                continue;
            }

            if let Ok(yield_op_block_arg) = yield_arg.try_downcast::<BlockArgument, dyn Value>() {
                let cond_op_arg = cond_op_args[yield_op_block_arg.borrow().index()];
                if cond_op_arg == before_args[index] || cond_op_arg == init_value {
                    before_block_init_val_map[index] = Some(init_value);
                    continue;
                }
            }

            new_init_args.push(init_value);
            new_yield_args.push(yield_arg);
        }

        {
            let mut guard = InsertionGuard::new(rewriter);
            let yield_op = yield_op.as_operation_ref();
            guard.set_insertion_point_before(yield_op);
            let new_yield = DefaultInstBuilder::new(&mut guard)
                .r#yield(new_yield_args.iter().copied(), yield_op.span())?;
            guard.replace_op(yield_op, new_yield.as_operation_ref());
        }

        let mut result_types = while_op
            .results()
            .iter()
            .map(|r| r.borrow().ty().clone())
            .collect::<SmallVec<[_; 4]>>();
        let new_while = DefaultInstBuilder::new(rewriter).r#while(
            new_init_args.iter().copied(),
            &result_types,
            while_op.span(),
        )?;

        let new_before_region = new_while.borrow().before().as_region_ref();
        result_types.clear();
        result_types.extend(new_yield_args.iter().map(|arg| arg.borrow().ty().clone()));
        let new_before_block = rewriter.create_block(new_before_region, None, &result_types);
        let num_before_block_args = before_block.borrow().num_arguments();
        let mut new_before_block_args = SmallVec::<[_; 4]>::with_capacity(num_before_block_args);
        new_before_block_args.resize(num_before_block_args, None);
        // For each i-th before block argument we find it's replacement value as:
        //
        // 1. If i-th before block argument is a loop invariant, we fetch it's initial value from
        //    `before_block_init_val_map` by querying for key `i`.
        // 2. Else we fetch j-th new before block argument as the replacement value of i-th before
        //    block argument.
        {
            let mut next_new_before_block_argument = 0;
            let new_before_block = new_before_block.borrow();
            for i in 0..num_before_block_args {
                // If the index 'i' argument was a loop invariant we fetch it's initial value from
                // `before_block_init_val_map`.
                if let Some(val) = before_block_init_val_map[i] {
                    new_before_block_args[i] = Some(val);
                } else {
                    new_before_block_args[i] = Some(
                        new_before_block.arguments()[next_new_before_block_argument] as ValueRef,
                    );
                    next_new_before_block_argument += 1;
                }
            }
        }

        let after_region = while_op.after().as_region_ref();
        drop(op);

        rewriter.merge_blocks(before_block, new_before_block, &new_before_block_args);
        rewriter.inline_region_before(after_region, new_while.borrow().after().as_region_ref());

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
