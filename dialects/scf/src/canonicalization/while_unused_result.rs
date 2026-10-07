use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::while_rebuild::{IterArg, rebuild_while};
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
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        // A result is removed when neither it nor the after block argument at its position has
        // real uses.
        let mut num_kept = 0;
        let results = {
            let after_region = while_op.after();
            let after_block = after_region.entry();
            while_op
                .results()
                .iter()
                .zip(after_block.arguments().iter())
                .map(|(result, after_arg)| {
                    if result.borrow().has_real_uses() || after_arg.borrow().has_real_uses() {
                        num_kept += 1;
                        Some(num_kept - 1)
                    } else {
                        None
                    }
                })
                .collect::<SmallVec<[Option<usize>; 4]>>()
        };
        if num_kept == results.len() {
            return Ok(false);
        }
        let iter_args = SmallVec::<[_; 4]>::from_elem(IterArg::Keep, while_op.inits().len());
        drop(op);

        rebuild_while(rewriter, operation, &iter_args, &results)
    }
}
