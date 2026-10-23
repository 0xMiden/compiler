use alloc::rc::Rc;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    adt::SmallDenseMap,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    traits::{AnyInteger, TypeConstraint},
    *,
};

use crate::*;

/// Hoist any yielded results whose operands are defined outside an [If], to a `cf.select`.
///
/// A result yielding the same value from both branches is forwarded as is. Otherwise, only results
/// of a type `cf.select` accepts are hoisted; the others (e.g. pointers) stay in the [If], together
/// with the results computed inside its regions.
pub struct ConvertTrivialIfToSelect {
    info: PatternInfo,
}

impl ConvertTrivialIfToSelect {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let if_op = scf_dialect.registered_name::<If>().expect("scf.if is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "convert-trivial-if-to-select",
                PatternKind::Operation(if_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for ConvertTrivialIfToSelect {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for ConvertTrivialIfToSelect {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let num_results = op.num_results();
        if num_results == 0 {
            return Ok(false);
        }

        let Some(if_op) = op.downcast_ref::<If>() else {
            return Ok(false);
        };

        let span = if_op.span();
        let cond = if_op.condition().as_value_ref();
        let then_region = if_op.then_body().as_region_ref();
        let else_region = if_op.else_body().as_region_ref();
        let then_terminator = if_op.then_body().entry().terminator().unwrap();
        let else_terminator = if_op.else_body().entry().terminator().unwrap();
        let Ok(then_yield) = then_terminator.try_downcast_op::<Yield>() else {
            return Ok(false);
        };
        let Ok(else_yield) = else_terminator.try_downcast_op::<Yield>() else {
            return Ok(false);
        };
        let then_yield_args = then_yield
            .borrow()
            .yielded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let else_yield_args = else_yield
            .borrow()
            .yielded()
            .into_iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        drop(op);

        let mut non_hoistable = SmallVec::<[_; 4]>::default();
        let mut hoistable = SmallVec::<[bool; 4]>::with_capacity(num_results);
        for (true_value, false_value) in
            then_yield_args.iter().copied().zip(else_yield_args.iter().copied())
        {
            // A value yielded by both branches is forwarded as is, whatever its type (as
            // `FoldRedundantYields` would), so that it never ends up in the new `scf.if`.
            let true_value_ref = true_value.borrow();
            let is_hoistable = true_value_ref.parent_region().unwrap() != then_region
                && false_value.borrow().parent_region().unwrap() != else_region
                && (true_value == false_value || AnyInteger::get().matches(true_value_ref.ty()));
            hoistable.push(is_hoistable);
            if !is_hoistable {
                non_hoistable.push(true_value_ref.ty().clone());
            }
        }

        // Early exit if there aren't any yielded results we can hoist
        if non_hoistable.len() == num_results {
            return Ok(false);
        }

        let _debug_update = midenc_hir::dialects::debuginfo::DebugInfoMutation::new(
            operation,
            midenc_hir::dialects::debuginfo::DebugInfoUpdate::Relocate,
        );

        // Create a new `scf.if` for the non-hoistable results (erased below if it ends up doing
        // nothing), and use it as the anchor for inserting the hoisted `cf.select`s.
        let anchor = {
            // Create a new `scf.if` with the non-hoistable results
            let mut new_if = rewriter.r#if(cond, &non_hoistable, span)?;
            let mut new_if_op = new_if.borrow_mut();
            new_if_op.then_body_mut().take_body(then_region);
            new_if_op.else_body_mut().take_body(else_region);
            new_if
        };

        // Insert `cf.select` ops for each hoisted result
        let mut results = SmallVec::<[_; 4]>::with_capacity(num_results);
        assert_eq!(then_yield.borrow().num_operands(), num_results);
        assert_eq!(else_yield.borrow().num_operands(), num_results);
        let mut true_yields = SmallVec::<[ValueRef; 4]>::default();
        let mut false_yields = SmallVec::<[ValueRef; 4]>::default();
        let mut deduplicated_selections =
            SmallDenseMap::<(ValueRef, ValueRef), ValueRef, 4>::default();
        let anchor_op = anchor.borrow();
        let new_then_region = anchor_op.then_body().as_region_ref();
        let new_else_region = anchor_op.else_body().as_region_ref();
        rewriter.set_insertion_point_before(anchor.as_operation_ref());
        for ((true_value, false_value), is_hoistable) in then_yield_args
            .iter()
            .copied()
            .zip(else_yield_args.iter().copied())
            .zip(hoistable)
        {
            if !is_hoistable {
                results.push(Some(anchor_op.results()[true_yields.len()] as ValueRef));
                true_yields.push(true_value);
                false_yields.push(false_value);
            } else if true_value == false_value {
                results.push(Some(true_value));
            } else if let Some(duplicate) = deduplicated_selections.get(&(true_value, false_value))
            {
                results.push(Some(*duplicate));
            } else {
                let selected = rewriter.select(cond, true_value, false_value, span)?;
                results.push(Some(selected));
                deduplicated_selections.insert((true_value, false_value), selected);
            }
        }

        // Rewrite the `scf.yield` ops in the new `scf.if`
        let new_then_yield = anchor_op.then_yield();
        let new_else_yield = anchor_op.else_yield();

        rewriter.set_insertion_point_to_end(new_then_region.borrow().entry_block_ref().unwrap());
        let replacement_then_yield = rewriter.r#yield(true_yields, span)?.as_operation_ref();
        rewriter.replace_op(new_then_yield.as_operation_ref(), replacement_then_yield);

        rewriter.set_insertion_point_to_end(new_else_region.borrow().entry_block_ref().unwrap());
        let replacement_else_yield = rewriter.r#yield(false_yields, span)?;
        rewriter.replace_op(
            new_else_yield.as_operation_ref(),
            replacement_else_yield.as_operation_ref(),
        );

        // Determine if the new `scf.if` is redundant, i.e. returns no results and does no work
        let is_noop = non_hoistable.is_empty()
            && anchor_op.then_body().entry().body().len() == 1
            && anchor_op.else_body().entry().body().len() == 1;

        // Drop our reference to the new `scf.if`
        drop(anchor_op);

        // If the new `scf.if` is a no-op, remove it
        if is_noop {
            rewriter.erase_op(anchor.as_operation_ref());
        }

        // Replace the original results with the new ones
        rewriter.replace_op_with_values(operation, &results);

        Ok(true)
    }
}

#[cfg(test)]
mod terminal_region_tests {
    use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
    use midenc_hir::{
        dialects::builtin::BuiltinOpBuilder, patterns::PatternRewriter, testing::Test,
    };

    use super::*;

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
                            &[Type::I1, Type::U32, Type::U64, Type::U32],
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
                        let structured = builder.r#if(condition, &types, span)?;
                        let regions = [
                            structured.borrow().then_body().as_region_ref(),
                            structured.borrow().else_body().as_region_ref(),
                        ];
                        let mut terminals = SmallVec::<[OperationRef; 2]>::new();
                        let mut yields = SmallVec::<[OperationRef; 2]>::new();
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
                                terminals.push(terminal);
                            } else {
                                let yielded = if types.is_empty() {
                                    &values[..0]
                                } else {
                                    &values[..]
                                };
                                yields.push(
                                    builder
                                        .r#yield(yielded.iter().copied(), span)?
                                        .as_operation_ref(),
                                );
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
                        let mut rewriter = PatternRewriter::new(test.context_rc());
                        rewriter.set_insertion_point_before(op);
                        let changed = ConvertTrivialIfToSelect::new(test.context_rc())
                            .match_and_rewrite(op, &mut *rewriter)?;
                        assert_eq!(changed, terminal_count == 0 && shape != ResultShape::Zero);
                        if !changed {
                            assert_eq!(before, alloc::format!("{}", function.borrow()));
                        }
                        if changed && matches!(shape, ResultShape::One | ResultShape::Two) {
                            assert_eq!(
                                user.borrow().operands()[0].borrow().as_value_ref(),
                                args[3]
                            );
                        }
                        let returned_values = [args[3], args[2]];
                        for terminal in terminals {
                            assert!(terminal.parent().is_some());
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
