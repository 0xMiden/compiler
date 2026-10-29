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
        let Ok(mut yield_op) = op.try_downcast_op::<Yield>() else {
            // Returns and traps do not forward region results.
            return;
        };

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

    use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
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

    /// **A CASE REGION THAT DOES NOT YIELD IS SKIPPED, NOT UNWRAPPED**
    /// (S7-F-6). The pattern narrows a switch's results by rewriting each
    /// region's `scf.yield`; a region terminated by `ub.unreachable` has no
    /// yielded operands to narrow, and the pass used to `unwrap()` on it
    /// twice — once for the terminator and once for the downcast.
    ///
    /// It is not an exotic shape. An exhaustive enum match lowers its DEFAULT
    /// region to `ub.unreachable` by construction, so every exhaustive match
    /// with an unused switch result reached this. The Sigma reproduction is
    /// `let v = match o { Some(x) => x, None => 0u32 }; let _ = v;` in a
    /// unit-returning function — ordinary source, with no `abort` in it —
    /// and it panicked the compiler.
    #[test]
    fn an_unreachable_case_region_is_left_alone() -> Result<(), Report> {
        let mut test = Test::new("unreachable_case_region", &[Type::U32], &[Type::U32]);

        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let selector = entry.borrow().arguments()[0].upcast();

        let dead_case_value = builder.u32(1, span)?;
        let live_case_value = builder.u32(2, span)?;

        let switch = builder.index_switch(selector, [1], &[Type::U32, Type::U32], span)?;

        let case_region = switch.borrow().get_case_region(0);
        let case_block = builder.create_block_in_region(case_region);
        builder.switch_to_block(case_block);
        builder.r#yield([dead_case_value, live_case_value], span)?;

        // THE DEFAULT REGION DIVERGES — the shape an exhaustive match emits.
        let default_region = switch.borrow().default_region().as_region_ref();
        let default_block = builder.create_block_in_region(default_region);
        builder.switch_to_block(default_block);
        builder.unreachable(span);

        builder.switch_to_block(entry);
        let live_switch_result = switch.borrow().results()[1].upcast();
        builder.ret(Some(live_switch_result), span)?;

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
        assert!(changed, "the yielding case is still narrowed");

        test.function().as_operation_ref().borrow().recursively_verify()?;

        // The yielding region lost its dead operand; the diverging one is
        // untouched and still terminates the switch's second region.
        let rendered = normalize_hir(&format!("{}", test.function().as_operation_ref().borrow()));
        assert!(rendered.contains("ub.unreachable"), "{rendered}");
        assert_eq!(rendered.matches("scf.yield").count(), 1, "{rendered}");

        Ok(())
    }
}

#[cfg(test)]
mod terminal_region_tests {
    use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
    use midenc_hir::{
        dialects::builtin::BuiltinOpBuilder,
        patterns::{PatternRewriter, RewriterListener},
        testing::Test,
    };

    use super::*;

    struct BorrowingListener;

    impl Listener for BorrowingListener {
        fn kind(&self) -> ListenerType {
            ListenerType::Rewriter
        }

        fn notify_operation_inserted(&self, op: OperationRef, _: ProgramPoint) {
            if let Some(next) = op.next() {
                let _ = next.borrow();
            }
        }
    }

    impl RewriterListener for BorrowingListener {
        fn notify_operation_modified(&self, op: OperationRef) {
            let _ = op.borrow();
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ResultShape {
        Zero,
        Unused,
        One,
        Two,
    }

    #[test]
    fn terminal_regions_preserve_identity_and_result_order() -> Result<(), Report> {
        for returns in [false, true] {
            for terminal_first in [false, true] {
                for terminal_count in [0usize, 1, 2] {
                    for shape in
                        [ResultShape::Zero, ResultShape::Unused, ResultShape::One, ResultShape::Two]
                    {
                        let outputs = if shape == ResultShape::Two {
                            &[Type::U32, Type::U64][..]
                        } else {
                            &[Type::U32][..]
                        };
                        let mut test = Test::new(
                            "terminal_regions",
                            &[Type::U32, Type::U32, Type::U64, Type::U32],
                            outputs,
                        );
                        let span = SourceSpan::default();
                        let mut builder = test.function_builder();
                        let entry = builder.entry_block();
                        let args = entry
                            .borrow()
                            .arguments()
                            .iter()
                            .map(|arg| arg.upcast())
                            .collect::<SmallVec<[ValueRef; 4]>>();
                        let condition = args[0];
                        let values = [args[1], args[2], args[3]];
                        let types = if shape == ResultShape::Zero {
                            SmallVec::<[Type; 3]>::new()
                        } else {
                            [Type::U32, Type::U64, Type::U32].into_iter().collect()
                        };
                        let structured = builder.index_switch(condition, [1], &types, span)?;
                        let regions = [
                            structured.borrow().get_case_region(0),
                            structured.borrow().default_region().as_region_ref(),
                        ];
                        let mut terminals = SmallVec::<[(usize, OperationRef); 2]>::new();
                        let mut yields = SmallVec::<[(usize, OperationRef); 2]>::new();
                        for (index, region) in regions.into_iter().enumerate() {
                            let block = builder.create_block_in_region(region);
                            builder.switch_to_block(block);
                            if terminal_count == 2
                                || (terminal_count == 1 && (index == 0) == terminal_first)
                            {
                                let terminal = if returns {
                                    builder
                                        .ret(
                                            if shape == ResultShape::Two {
                                                alloc::vec![args[3], args[2]]
                                            } else {
                                                alloc::vec![args[3]]
                                            },
                                            span,
                                        )?
                                        .as_operation_ref()
                                } else {
                                    builder.unreachable(span).as_operation_ref()
                                };
                                terminals.push((index, terminal));
                            } else {
                                let yielded = if types.is_empty() {
                                    &values[..0]
                                } else {
                                    &values[..]
                                };
                                yields.push((
                                    index,
                                    builder
                                        .r#yield(yielded.iter().copied(), span)?
                                        .as_operation_ref(),
                                ));
                            }
                        }
                        builder.switch_to_block(entry);
                        let result = if matches!(shape, ResultShape::One | ResultShape::Two) {
                            structured.borrow().results()[2].upcast()
                        } else {
                            args[1]
                        };
                        let returned = if shape == ResultShape::Two {
                            alloc::vec![result, structured.borrow().results()[1].upcast()]
                        } else {
                            alloc::vec![result]
                        };
                        let user = builder.ret(returned, span)?.as_operation_ref();
                        let op = structured.as_operation_ref();
                        let function = test.function().as_operation_ref();
                        function.borrow().recursively_verify()?;
                        let before = alloc::format!("{}", function.borrow());
                        let mut rewriter = PatternRewriter::new_with_listener(
                            test.context_rc(),
                            BorrowingListener,
                        );
                        rewriter.set_insertion_point_before(op);
                        let changed = IndexSwitchRemoveUnusedResults::new(test.context_rc())
                            .match_and_rewrite(op, &mut *rewriter)?;
                        assert_eq!(changed, shape != ResultShape::Zero);
                        let replacement_op = if changed {
                            user.prev().expect("replacement precedes user")
                        } else {
                            op
                        };
                        assert!(replacement_op.borrow().is::<IndexSwitch>());
                        let expected_types: &[Type] = match shape {
                            ResultShape::Zero | ResultShape::Unused => &[],
                            ResultShape::One => &[Type::U32],
                            ResultShape::Two => &[Type::U64, Type::U32],
                        };
                        let actual_types = replacement_op
                            .borrow()
                            .results()
                            .iter()
                            .map(|result| result.borrow().ty().clone())
                            .collect::<SmallVec<[Type; 3]>>();
                        assert_eq!(actual_types.as_slice(), expected_types);
                        if changed {
                            if matches!(shape, ResultShape::One | ResultShape::Two) {
                                let result_index = if shape == ResultShape::Two { 1 } else { 0 };
                                assert_eq!(
                                    user.borrow().operands()[0].borrow().as_value_ref(),
                                    replacement_op.borrow().results()[result_index].upcast()
                                );
                                if shape == ResultShape::Two {
                                    assert_eq!(
                                        user.borrow().operands()[1].borrow().as_value_ref(),
                                        replacement_op.borrow().results()[0].upcast()
                                    );
                                }
                            }
                            let replacement = user.borrow().operands()[0].borrow().as_value_ref();
                            if matches!(shape, ResultShape::One | ResultShape::Two) {
                                assert_eq!(replacement.borrow().ty(), &Type::U32);
                                for (_, yielded) in &yields {
                                    let expected = if shape == ResultShape::Two {
                                        &values[1..3]
                                    } else {
                                        &values[2..3]
                                    };
                                    let actual = yielded
                                        .borrow()
                                        .operands()
                                        .iter()
                                        .map(|operand| operand.borrow().as_value_ref())
                                        .collect::<SmallVec<[ValueRef; 3]>>();
                                    assert_eq!(actual.as_slice(), expected);
                                }
                            } else {
                                for (_, yielded) in &yields {
                                    assert_eq!(yielded.borrow().num_operands(), 0);
                                }
                            }
                        } else {
                            assert_eq!(before, alloc::format!("{}", function.borrow()));
                        }
                        let returned_values = [args[3], args[2]];
                        let replacement_regions = {
                            let replacement = replacement_op.borrow();
                            let replacement = replacement.downcast_ref::<IndexSwitch>().unwrap();
                            [
                                replacement.get_case_region(0),
                                replacement.default_region().as_region_ref(),
                            ]
                        };
                        for (region_index, yielded) in yields {
                            let expected_block = replacement_regions[region_index]
                                .borrow()
                                .entry_block_ref()
                                .unwrap();
                            assert_eq!(yielded.parent(), Some(expected_block));
                            assert_eq!(expected_block.borrow().terminator(), Some(yielded));
                        }
                        for (region_index, terminal) in terminals {
                            assert_eq!(
                                terminal.parent(),
                                replacement_regions[region_index].borrow().entry_block_ref()
                            );
                            assert_eq!(
                                replacement_regions[region_index].borrow().entry().terminator(),
                                Some(terminal)
                            );
                            let operands = terminal
                                .borrow()
                                .operands()
                                .iter()
                                .map(|o| o.borrow().as_value_ref())
                                .collect::<SmallVec<[ValueRef; 2]>>();
                            assert_eq!(
                                operands.as_slice(),
                                if returns {
                                    if shape == ResultShape::Two {
                                        &returned_values[..]
                                    } else {
                                        &args[3..4]
                                    }
                                } else {
                                    &args[0..0]
                                }
                            );
                        }
                        function.borrow().recursively_verify()?;
                        test.apply_boxed_pass(
                            alloc::boxed::Box::new(midenc_hir_transform::Canonicalizer::new(
                                midenc_hir::patterns::GreedyRewriteConfig::default(),
                                true,
                            )),
                            true,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}
