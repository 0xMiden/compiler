use alloc::rc::Rc;

use midenc_hir::{
    adt::{SmallDenseMap, SmallSet},
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Remove duplicated [crate::ops::Condition] args in a [While] loop.
pub struct WhileRemoveDuplicatedResults {
    info: PatternInfo,
}

impl WhileRemoveDuplicatedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-remove-duplicated-results",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileRemoveDuplicatedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileRemoveDuplicatedResults {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        let cond_op = while_op.condition_op();
        let cond_op_args = cond_op
            .borrow()
            .forwarded()
            .iter()
            .map(|v| v.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        let mut args_set = SmallSet::<ValueRef, 4>::default();
        for arg in cond_op_args.iter().copied() {
            args_set.insert(arg);
        }

        if args_set.len() == cond_op_args.len() {
            // No results to remove
            return Ok(false);
        }

        let mut args_map = SmallDenseMap::<_, _, 4>::with_capacity(cond_op_args.len());
        let mut new_args = SmallVec::<[ValueRef; 4]>::with_capacity(cond_op_args.len());

        for arg in cond_op_args.iter().copied() {
            if !args_map.contains_key(&arg) {
                args_map.insert(arg, args_map.len());
                new_args.push(arg);
            }
        }

        let span = op.span();
        let results = new_args
            .iter()
            .map(|arg| arg.borrow().ty().clone())
            .collect::<SmallVec<[_; 4]>>();
        let new_while_op = rewriter.r#while(
            while_op.inits().into_iter().map(|o| o.borrow().as_value_ref()),
            &results,
            span,
        )?;

        let new_while = new_while_op.borrow();
        let new_before_block = new_while.before().entry().as_block_ref();
        let new_after_block = new_while.after().entry().as_block_ref();
        let before_block = while_op.before().entry().as_block_ref();
        let after_block = while_op.after().entry().as_block_ref();
        drop(op);

        let mut after_args_mapping = SmallVec::<[_; 4]>::default();
        let mut results_mapping = SmallVec::<[_; 4]>::default();
        for arg in cond_op_args.iter() {
            let pos = args_map.get(arg).copied().unwrap();
            after_args_mapping
                .push(Some(new_after_block.borrow().get_argument(pos).borrow().as_value_ref()));
            results_mapping.push(Some(new_while.results()[pos].borrow().as_value_ref()));
        }

        let mut guard = InsertionGuard::new(rewriter);
        guard.set_insertion_point_before(cond_op.as_operation_ref());

        let new_cond_op = guard.condition(
            cond_op.borrow().condition().as_value_ref(),
            new_args.iter().copied(),
            span,
        )?;
        let new_cond_op = new_cond_op.as_operation_ref();
        let cond_op = cond_op.as_operation_ref();
        guard.replace_op(cond_op, new_cond_op);

        let new_before_block_args = new_before_block
            .borrow()
            .arguments()
            .iter()
            .map(|v| Some(v.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 4]>>();
        guard.merge_blocks(before_block, new_before_block, &new_before_block_args);
        guard.merge_blocks(after_block, new_after_block, &after_args_mapping);
        guard.replace_op_with_values(operation, &results_mapping);

        Ok(true)
    }
}
