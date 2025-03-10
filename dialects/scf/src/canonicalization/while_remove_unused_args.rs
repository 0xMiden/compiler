use alloc::rc::Rc;

use midenc_hir2::*;

use crate::*;

/// Remove unused init/yield args of a [While] loop.
pub struct WhileRemoveUnusedArgs {
    info: PatternInfo,
}

impl WhileRemoveUnusedArgs {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-remove-unused-args",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileRemoveUnusedArgs {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileRemoveUnusedArgs {
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

    fn match_and_rewrite(
        &self,
        mut operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        use bitvec::prelude::{BitVec, Lsb0};

        let mut op = operation.borrow_mut();
        let Some(while_op) = op.downcast_mut::<While>() else {
            return Ok(false);
        };

        if while_op.before().entry().arguments().iter().all(|arg| arg.borrow().is_used()) {
            // All the arguments are used (nothing to remove)
            return Ok(false);
        }

        // Collect results mapping, new terminator args, and new result types
        let yield_op = while_op.yield_op();
        let mut before_block = while_op.before().entry().as_block_ref();
        let after_block = while_op.after().entry().as_block_ref();
        let argc = while_op.before().entry().num_arguments();
        let mut new_yields = SmallVec::<[ValueRef; 4]>::with_capacity(argc);
        let mut new_inits = SmallVec::<[ValueRef; 4]>::with_capacity(argc);
        let mut args_to_erase = BitVec::<usize, Lsb0>::new();

        {
            let yield_op = yield_op.borrow();
            let before_entry = before_block.borrow();
            for (i, before_arg) in before_entry.arguments().iter().enumerate() {
                let before_arg = before_arg.borrow();
                let yield_value = yield_op.yielded()[i];
                let init_value = while_op.inits()[i];
                if before_arg.is_used() {
                    args_to_erase.push(false);
                    new_yields.push(yield_value.borrow().as_value_ref());
                    new_inits.push(init_value.borrow().as_value_ref());
                } else {
                    args_to_erase.push(true);
                }
            }
        }
        let yield_op = yield_op.as_operation_ref();

        before_block
            .borrow_mut()
            .erase_arguments(|arg| *args_to_erase.get(arg.index()).unwrap());

        let span = while_op.span();
        let new_while_op = {
            let results = while_op
                .results()
                .all()
                .iter()
                .map(|r| r.borrow().ty().clone())
                .collect::<SmallVec<[_; 2]>>();
            drop(op);
            rewriter.r#while(new_inits.iter().copied(), &results, span)?
        };

        let new_while = new_while_op.borrow();
        let new_before_block = { new_while.before().entry().as_block_ref() };
        let new_after_block = { new_while.after().entry().as_block_ref() };

        let mut guard = InsertionGuard::new(rewriter);
        guard.set_insertion_point_before(yield_op);
        let new_yield_op = guard.r#yield(new_yields, yield_op.span())?;
        let new_yield_op = new_yield_op.as_operation_ref();
        guard.replace_op(yield_op, new_yield_op);

        let new_before_args = new_before_block
            .borrow()
            .arguments()
            .iter()
            .map(|arg| Some(arg.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 2]>>();
        guard.merge_blocks(before_block, new_before_block, &new_before_args);

        let new_after_args = new_after_block
            .borrow()
            .arguments()
            .iter()
            .map(|arg| Some(arg.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 2]>>();
        guard.merge_blocks(after_block, new_after_block, &new_after_args);

        let results = new_while_op
            .borrow()
            .results()
            .all()
            .into_iter()
            .map(|r| Some(r.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 4]>>();
        guard.replace_op_with_values(operation, &results);

        Ok(true)
    }
}
