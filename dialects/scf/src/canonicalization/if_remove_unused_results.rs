use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::result_rebuild::{keep_map, rebuild_if};
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
        let results = {
            let op = operation.borrow();
            if !op.is::<If>() {
                return Ok(false);
            }
            let Some(results) = used_results(&op) else {
                return Ok(false);
            };
            results
        };

        rebuild_if(rewriter, operation, &results)
    }
}

/// The result map (see [rebuild_if]) that keeps the results of `op` with real uses and drops the
/// others, or `None` if every result is used.
pub(super) fn used_results(op: &Operation) -> Option<SmallVec<[Option<usize>; 4]>> {
    let results = op.results();
    let map = keep_map(results.len(), |index| results[index].borrow().has_real_uses());
    map.iter().any(Option::is_none).then_some(map)
}

#[cfg(test)]
pub(super) mod tests {
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
    pub(crate) struct InsertionPointListener;

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
