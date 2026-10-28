use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// A canonicalization pattern for [IndexSwitch] that folds away the operation if it has a constant
/// selector value.
pub struct FoldConstantIndexSwitch {
    info: PatternInfo,
}

impl FoldConstantIndexSwitch {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let switch_op = scf_dialect
            .registered_name::<IndexSwitch>()
            .expect("scf.index_switch is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "fold-constant-index-switch",
                PatternKind::Operation(switch_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl FoldConstantIndexSwitch {
    fn match_constant_selector(&self, op: OperationRef) -> Option<u32> {
        use midenc_hir::matchers::{self, Matcher};

        let op = op.borrow();
        if let Some(op) = op.downcast_ref::<IndexSwitch>() {
            let selector = op.selector().as_value_ref();
            selector
                .borrow()
                .get_defining_op()
                .and_then(|defined_by| {
                    let matcher = matchers::constant_of::<Immediate>();
                    matcher.matches(&*defined_by.borrow())
                })
                .and_then(|imm| imm.as_u32())
        } else {
            None
        }
    }
}

impl Pattern for FoldConstantIndexSwitch {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for FoldConstantIndexSwitch {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let Some(selector) = self.match_constant_selector(op) else {
            return Ok(false);
        };
        let case_region = {
            let switch_operation = op.borrow();
            let switch_op = switch_operation.downcast_ref::<IndexSwitch>().unwrap();
            let case_index = switch_op.get_case_index_for_selector(selector);
            case_index
                .map(|idx| switch_op.get_case_region(idx))
                .unwrap_or_else(|| switch_op.default_region().as_region_ref())
        };

        let source = case_region.borrow().entry_block_ref().expect("expected non-empty region");
        let terminator =
            source.borrow().terminator().expect("expected region to have a terminator");
        // Only a yield forwards the switch results and can be erased by this fold.
        if !terminator.borrow().is::<Yield>() {
            return Ok(false);
        }
        let results = terminator
            .borrow()
            .operands()
            .iter()
            .copied()
            .map(|o| Some(o.borrow().as_value_ref()))
            .collect::<SmallVec<[Option<ValueRef>; 2]>>();

        let dest = op.parent().unwrap();
        rewriter.inline_block_before(source, dest, Some(op), &[]);
        rewriter.erase_op(terminator);
        // Replace the operation with a potentially empty list of results.
        //
        // The fold mechanism doesn't support the case where the result list is empty
        let unreachable = op
            .borrow()
            .regions()
            .iter()
            .filter(|region| region.as_region_ref() != case_region)
            .flat_map(|region| {
                region.body().iter().map(|block| block.as_block_ref()).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        {
            let _debug_update = midenc_hir::dialects::debuginfo::DebugInfoMutation::for_blocks(
                &unreachable,
                midenc_hir::dialects::debuginfo::DebugInfoUpdate::DiscardUnreachable,
            );
            for block in unreachable {
                rewriter.erase_block(block);
            }
        }
        rewriter.replace_op_with_values_and_debug(
            op,
            &results,
            midenc_hir::dialects::debuginfo::DebugInfoUpdate::Equivalent,
        );

        Ok(true)
    }
}

#[cfg(test)]
mod terminal_region_tests {
    use midenc_dialect_arith::ArithOpBuilder;
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
        check_terminal_regions(&[1, 2], true)
    }

    #[test]
    fn yielding_regions_and_unselected_terminals_keep_folding() -> Result<(), Report> {
        check_terminal_regions(&[0, 1], false)
    }

    fn check_terminal_regions(
        terminal_counts: &[usize],
        select_terminal: bool,
    ) -> Result<(), Report> {
        for returns in [false, true] {
            for terminal_first in [false, true] {
                for terminal_count in terminal_counts.iter().copied() {
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

                        let values = [args[1], args[2], args[3]];
                        let types = if shape == ResultShape::Zero {
                            SmallVec::<[Type; 3]>::new()
                        } else {
                            [Type::U32, Type::U64, Type::U32].into_iter().collect()
                        };
                        let selector = builder.u32(
                            if terminal_first == select_terminal {
                                1
                            } else {
                                0
                            },
                            span,
                        );
                        let structured = builder.index_switch(selector, [1], &types, span)?;
                        let regions = [
                            structured.borrow().get_case_region(0),
                            structured.borrow().default_region().as_region_ref(),
                        ];
                        let mut terminals = SmallVec::<[(usize, OperationRef); 2]>::new();
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
                                terminals.push((index, terminal));
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
                        let changed = FoldConstantIndexSwitch::new(test.context_rc())
                            .match_and_rewrite(op, &mut *rewriter)?;
                        assert_eq!(
                            changed,
                            terminal_count == 0 || (terminal_count == 1 && !select_terminal)
                        );
                        if !changed {
                            assert_eq!(before, alloc::format!("{}", function.borrow()));
                        }
                        if changed && matches!(shape, ResultShape::One | ResultShape::Two) {
                            assert_eq!(
                                user.borrow().operands()[0].borrow().as_value_ref(),
                                args[3]
                            );
                            if shape == ResultShape::Two {
                                assert_eq!(
                                    user.borrow().operands()[1].borrow().as_value_ref(),
                                    args[2]
                                );
                            }
                        }
                        let returned_values = [args[3], args[2]];
                        for (region_index, terminal) in terminals.into_iter().filter(|_| !changed) {
                            assert_eq!(
                                terminal.parent(),
                                regions[region_index].borrow().entry_block_ref()
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
