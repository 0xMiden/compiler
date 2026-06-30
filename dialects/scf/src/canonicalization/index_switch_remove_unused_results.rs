use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

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

    fn transfer_body(
        &self,
        src: BlockRef,
        dest: BlockRef,
        used_results: &[OpResultRef],
        rewriter: &mut dyn Rewriter,
    ) {
        // Move all operations to the destination block.
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
        let (used_results, num_results, selector, cases, result_types, old_regions, span) = {
            let op = operation.borrow();
            let Some(switch_op) = op.downcast_ref::<IndexSwitch>() else {
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

            let result_types = used_results
                .iter()
                .map(|result| result.borrow().ty().clone())
                .collect::<SmallVec<[_; 4]>>();

            (
                used_results,
                num_results,
                switch_op.selector().as_value_ref(),
                switch_op.cases().iter().copied().collect::<Vec<_>>(),
                result_types,
                switch_op
                    .regions()
                    .iter()
                    .map(|region| region.as_region_ref())
                    .collect::<Vec<_>>(),
                switch_op.span(),
            )
        };

        // Create a replacement operation with empty regions.
        let new_switch = rewriter.index_switch(selector, cases, &result_types, span)?;
        let new_switch_op = new_switch.as_operation_ref();
        let new_regions = {
            let switch_op = new_switch.borrow();
            switch_op
                .regions()
                .iter()
                .map(|region| region.as_region_ref())
                .collect::<Vec<_>>()
        };

        for (old_region, new_region) in old_regions.into_iter().zip(new_regions) {
            let old_entry = old_region.borrow().entry_block_ref().unwrap();
            let new_entry = rewriter.create_block(new_region, None, &[]);
            self.transfer_body(old_entry, new_entry, &used_results, rewriter);
        }

        // Replace the operation by the new one.
        let mut replaced_results = SmallVec::<[_; 4]>::with_capacity(num_results);
        replaced_results.resize(num_results, None);
        {
            let new_switch_op = new_switch_op.borrow();
            for (index, result) in used_results.into_iter().enumerate() {
                replaced_results[result.borrow().index()] =
                    Some(new_switch_op.results()[index] as ValueRef);
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
