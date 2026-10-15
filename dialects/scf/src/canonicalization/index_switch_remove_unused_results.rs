use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use super::{if_remove_unused_results::used_results, result_rebuild::rebuild_index_switch};
use crate::*;

/// Remove unused results of an [IndexSwitch] instruction.
pub struct IndexSwitchRemoveUnusedResults {
    info: PatternInfo,
}

impl IndexSwitchRemoveUnusedResults {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let switch_op = scf_dialect
            .registered_name::<IndexSwitch>()
            .expect("scf.index_switch is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "index-switch-remove-unused-results",
                PatternKind::Operation(switch_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for IndexSwitchRemoveUnusedResults {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for IndexSwitchRemoveUnusedResults {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let results = {
            let op = operation.borrow();
            if !op.is::<IndexSwitch>() {
                return Ok(false);
            }
            let Some(results) = used_results(&op) else {
                return Ok(false);
            };
            results
        };

        rebuild_index_switch(rewriter, operation, &results)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};

    use midenc_expect_test::expect_file;
    use midenc_hir::{
        Op, Report, SourceSpan, Type,
        dialects::{builtin::BuiltinOpBuilder, test::TestOpBuilder},
        patterns::{self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePatternSet},
        testing::Test,
    };

    use super::*;

    fn normalize_hir(input: &str) -> String {
        let mut normalized = input.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
        normalized.push('\n');
        normalized
    }

    #[test]
    fn index_switch_remove_unused_results() -> Result<(), Report> {
        let mut test = Test::new("index_switch_remove_unused_results", &[Type::U32], &[Type::U32]);

        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0].upcast();

        let dead_case_value = builder.u32(1, span)?;
        let live_case_value = builder.u32(2, span)?;
        let dead_default_value = builder.u32(3, span)?;
        let live_default_value = builder.u32(4, span)?;

        let switch = builder.index_switch(selector, [1], &[Type::U32, Type::U32], span)?;

        let case_region = switch.borrow().get_case_region(0);
        let case_block = builder.create_block_in_region(case_region);
        builder.switch_to_block(case_block);
        builder.r#yield([dead_case_value, live_case_value], span)?;

        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);
        builder.r#yield([dead_default_value, live_default_value], span)?;

        builder.switch_to_block(entry);
        let live_switch_result = switch.borrow().results()[1].upcast();
        builder.ret(Some(live_switch_result), span)?;

        let input = normalize_hir(&format!("{}", test.function().as_operation_ref().borrow()));
        expect_file!["expected/index_switch_remove_unused_results_before.hir"].assert_eq(&input);

        let context = test.context_rc();
        let pattern: Box<dyn RewritePattern> =
            Box::new(IndexSwitchRemoveUnusedResults::new(context.clone()));
        let pattern_set = RewritePatternSet::from_iter(context.clone(), [pattern]);
        let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));
        let changed = patterns::apply_patterns_and_fold_greedily(
            test.function().as_operation_ref(),
            rewrites,
            GreedyRewriteConfig::default(),
        )
        .expect("expected canonicalizer to converge");
        assert!(changed, "expected index_switch to be rewritten");

        test.function().as_operation_ref().borrow().recursively_verify()?;

        let output = normalize_hir(&format!("{}", test.function().as_operation_ref().borrow()));
        expect_file!["expected/index_switch_remove_unused_results_after.hir"].assert_eq(&output);

        Ok(())
    }
}
