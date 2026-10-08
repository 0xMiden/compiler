use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Removed unused results of an [If] instruction
pub struct IfRemoveUnusedResults {
    info: PatternInfo,
}

impl IfRemoveUnusedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let if_op = scf_dialect.registered_name::<If>().expect("scf.if is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "if-remove-unused-results",
                PatternKind::Operation(if_op),
                PatternBenefit::MAX,
            ),
        }
    }

    fn transfer_body(
        &self,
        src: BlockRef,
        dest: BlockRef,
        used_results: &[OpResultRef],
        rewriter: &mut dyn Rewriter,
    ) {
        // Move all operations to the destination block
        rewriter.merge_blocks(src, dest, &[]);

        // Replace the yield op with one that returns only the used values.
        let op = { dest.borrow().terminator().unwrap() };
        let mut yield_op = op.try_downcast_op::<Yield>().unwrap();

        let mut used_operands = SmallVec::<[ValueRef; 4]>::with_capacity(used_results.len());
        {
            let yield_ = yield_op.borrow();
            for used_result in used_results {
                let operand = yield_.operands()[used_result.borrow().index()];
                used_operands.push(operand.borrow().as_value_ref());
            }
        }

        let _guard = rewriter.modify_op_in_place(op);
        let mut yield_ = yield_op.borrow_mut();
        let context = yield_.as_operation().context_rc();
        yield_.yielded_mut().set_operands(used_operands, op, &context);
    }
}

impl Pattern for IfRemoveUnusedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for IfRemoveUnusedResults {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        // Everything the rewrite needs from the original op is read up front, so that no borrow
        // of `operation` is alive while the rewriter runs: a rewriter listener may inspect the
        // ops around each insertion made below, and `operation` is next to all of them.
        let (used_results, num_results, condition, new_types, then_entry, else_entry, span) = {
            let op = operation.borrow();
            let Some(if_op) = op.downcast_ref::<If>() else {
                return Ok(false);
            };

            // Compute the list of used results.
            let used_results = op
                .results()
                .iter()
                .copied()
                .filter(|result| result.borrow().has_real_uses())
                .collect::<SmallVec<[_; 4]>>();

            // Replace the operation if only a subset of its results have uses.
            let num_results = op.num_results();
            if used_results.len() == num_results {
                return Ok(false);
            }

            // Compute the result types of the replacement operation.
            let new_types = used_results
                .iter()
                .map(|result| result.borrow().ty().clone())
                .collect::<SmallVec<[_; 4]>>();

            (
                used_results,
                num_results,
                if_op.condition().as_value_ref(),
                new_types,
                if_op.then_body().entry_block_ref().unwrap(),
                if_op.else_body().entry_block_ref().unwrap(),
                if_op.span(),
            )
        };

        // Create a replacement operation with empty then and else regions.
        let new_if = rewriter.r#if(condition, &new_types, span)?;
        let (new_then_region, new_else_region) = {
            let new_if_op = new_if.borrow();
            (new_if_op.then_body().as_region_ref(), new_if_op.else_body().as_region_ref())
        };
        let new_then_block = rewriter.create_block(new_then_region, None, &[]);
        let new_else_block = rewriter.create_block(new_else_region, None, &[]);

        // Move the bodies and replace the terminators (note there is a then and an else region
        // since the operation returns results).
        self.transfer_body(then_entry, new_then_block, &used_results, rewriter);
        self.transfer_body(else_entry, new_else_block, &used_results, rewriter);

        // Replace the operation by the new one.
        let mut replaced_results = SmallVec::<[_; 4]>::with_capacity(num_results);
        replaced_results.resize(num_results, None);
        {
            let new_if_op = new_if.borrow();
            for (index, result) in used_results.into_iter().enumerate() {
                replaced_results[result.borrow().index()] =
                    Some(new_if_op.results()[index] as ValueRef);
            }
        }
        rewriter.replace_op_with_values(operation, &replaced_results);

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};

    use midenc_expect_test::expect_file;
    use midenc_hir::{
        Listener, ListenerType, Op, OperationRef, ProgramPoint, Report, SourceSpan, Type,
        dialects::{builtin::BuiltinOpBuilder, test::TestOpBuilder},
        patterns::{
            self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePatternSet, RewriterListener,
        },
        testing::Test,
    };

    use super::*;

    /// Strips trailing whitespace from every line and ends the text with a single newline
    fn normalize_hir(input: &str) -> String {
        let mut normalized = input.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
        normalized.push('\n');
        normalized
    }

    /// Borrows the op following every inserted op while the pattern that made the insertion is
    /// still running, the way a listener inspecting the neighbourhood of an insertion does. The
    /// pattern inserts before the op it rewrites, so that op is the neighbour: the rewrite fails
    /// here if the pattern still holds it (the shape of the `AliasingViolationError` in #1421).
    struct InsertionPointListener;

    impl Listener for InsertionPointListener {
        fn kind(&self) -> ListenerType {
            ListenerType::Rewriter
        }

        fn notify_operation_inserted(&self, op: OperationRef, _prev: ProgramPoint) {
            if let Some(next) = op.next() {
                let _ = next.borrow();
            }
        }
    }

    impl RewriterListener for InsertionPointListener {}

    /// An `scf.if` with one dead and one live result is narrowed to the live one, with a
    /// listener that borrows the neighbour of every op the pattern creates.
    #[test]
    fn if_remove_unused_results() -> Result<(), Report> {
        let mut test = Test::new("if_remove_unused_results", &[Type::I1], &[Type::U32]);

        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let condition = entry.borrow().arguments()[0].upcast();

        let dead_then_value = builder.u32(1, span)?;
        let live_then_value = builder.u32(2, span)?;
        let dead_else_value = builder.u32(3, span)?;
        let live_else_value = builder.u32(4, span)?;

        let if_op = builder.r#if(condition, &[Type::U32, Type::U32], span)?;

        let then_region = if_op.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        builder.r#yield([dead_then_value, live_then_value], span)?;

        let else_region = if_op.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        builder.r#yield([dead_else_value, live_else_value], span)?;

        builder.switch_to_block(entry);
        let live_if_result = if_op.borrow().results()[1].upcast();
        builder.ret(Some(live_if_result), span)?;

        let input = normalize_hir(&format!("{}", test.function().as_operation_ref().borrow()));
        expect_file!["expected/if_remove_unused_results_before.hir"].assert_eq(&input);

        let context = test.context_rc();
        let pattern: Box<dyn RewritePattern> =
            Box::new(IfRemoveUnusedResults::new(context.clone()));
        let pattern_set = RewritePatternSet::from_iter(context.clone(), [pattern]);
        let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));
        let changed = patterns::apply_patterns_and_fold_greedily(
            test.function().as_operation_ref(),
            rewrites,
            GreedyRewriteConfig::new_with_listener(InsertionPointListener),
        )
        .expect("expected canonicalizer to converge");
        assert!(changed, "expected if to be rewritten");

        test.function().as_operation_ref().borrow().recursively_verify()?;

        let output = normalize_hir(&format!("{}", test.function().as_operation_ref().borrow()));
        expect_file!["expected/if_remove_unused_results_after.hir"].assert_eq(&output);

        Ok(())
    }
}
