use alloc::rc::Rc;

use midenc_hir2::*;

use crate::*;

/// Remove results of a [While] that are also unused in its 'after' block.
///
/// Before:
///
/// ```text,ignore
/// %0:2 = scf.while () : () -> (i32, i64) {
///     %condition = "test.condition"() : () -> i1
///     %v1 = "test.get_some_value"() : () -> i32
///     %v2 = "test.get_some_value"() : () -> i64
///     scf.condition(%condition) %v1, %v2 : i32, i64
/// } do {
///  ^bb0(%arg0: i32, %arg1: i64):
///     "test.use"(%arg0) : (i32) -> ()
///     scf.yield
/// }
/// scf.ret %0#0 : i32
///
/// After:
///
/// ```text,ignore
/// %0 = scf.while () : () -> (i32) {
///     %condition = "test.condition"() : () -> i1
///     %v1 = "test.get_some_value"() : () -> i32
///     %v2 = "test.get_some_value"() : () -> i64
///     scf.condition(%condition) %v1 : i32
/// } do {
/// ^bb0(%arg0: i32):
///     "test.use"(%arg0) : (i32) -> ()
///     scf.yield
/// }
/// scf.ret %0 : i32
/// ```
pub struct WhileUnusedResult {
    info: PatternInfo,
}

impl WhileUnusedResult {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-unused-result",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileUnusedResult {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileUnusedResult {
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let operation = op.borrow();
        let Some(while_op) = operation.downcast_ref::<While>() else {
            return Ok(false);
        };
        let condition_operation = while_op.condition_op();
        let span = while_op.span();

        let after_args = {
            while_op
                .after()
                .entry()
                .arguments()
                .iter()
                .map(|arg| arg.borrow().as_value_ref())
                .collect::<SmallVec<[_; 4]>>()
        };
        let forwarded = {
            condition_operation
                .borrow()
                .forwarded()
                .iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<SmallVec<[_; 4]>>()
        };

        // Collect results mapping, new terminator args, and new result types
        let mut new_results_indices = SmallVec::<[usize; 4]>::default();
        let mut new_result_types = SmallVec::<[Type; 4]>::default();
        let mut new_term_args = SmallVec::<[ValueRef; 4]>::default();
        let mut new_arg_spans = SmallVec::<[SourceSpan; 4]>::default();
        let mut need_update = false;

        for (i, result) in while_op.results().iter().enumerate() {
            let result = result.borrow();
            let after_arg = after_args[i];
            let term_arg = forwarded[i];

            if !result.is_used() && !after_arg.borrow().is_used() {
                need_update = true;
            } else {
                new_results_indices.push(i);
                new_term_args.push(term_arg);
                new_result_types.push(result.ty().clone());
                new_arg_spans.push(result.span());
            }
        }

        if !need_update {
            return Ok(false);
        }

        {
            let mut guard = InsertionGuard::new(rewriter);
            let (span, condition, condition_op) = {
                let cond_op = condition_operation.borrow();
                let condition = cond_op.condition().as_value_ref();
                (cond_op.span(), condition, cond_op.as_operation_ref())
            };
            guard.set_insertion_point_before(condition_op);
            let new_condition = guard.condition(condition, new_term_args, span)?;
            let new_condition_op = new_condition.as_operation_ref();
            guard.replace_op(condition_op, new_condition_op);
        }

        let new_while = {
            let inits = while_op.inits().into_iter().map(|o| o.borrow().as_value_ref());
            rewriter.r#while(inits, &new_result_types, span)?
        };

        let new_after_block = rewriter.create_block(
            new_while.borrow().after().as_region_ref(),
            None,
            &new_result_types,
        );

        // Build new results list and new after block args (unused entries will be None)
        let num_results = while_op.num_results();
        let mut new_results: SmallVec<[_; 4]> = smallvec![None; num_results];
        let mut new_after_block_args: SmallVec<[_; 4]> = smallvec![None; num_results];
        {
            let new_while_op = new_while.borrow();
            let new_after_block = new_after_block.borrow();
            for (i, new_result_index) in new_results_indices.iter().copied().enumerate() {
                new_results[new_result_index] =
                    Some(new_while_op.results()[i].borrow().as_value_ref());
                new_after_block_args[new_result_index] =
                    Some(new_after_block.arguments()[i].borrow().as_value_ref());
            }
        }

        let before_region = while_op.before().as_region_ref();
        let new_before_region = new_while.borrow().before().as_region_ref();
        let after_block = while_op.after().entry_block_ref().unwrap();

        rewriter.inline_region_before(before_region, new_before_region);
        rewriter.merge_blocks(after_block, new_after_block, &new_after_block_args);
        rewriter.replace_op_with_values(op, &new_results);

        Ok(true)
    }
}
