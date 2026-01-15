use midenc_hir::adt::SmallSet;
use petgraph::prelude::{DiGraphMap, Direction};

use super::*;

/// Returns the deepest addressable source index for materializing a copy of `value`.
///
/// MASM stack manipulation instructions can only directly access the first 16 field elements on
/// the operand stack. Since a single operand may consist of multiple field elements, we must
/// ensure the copy source is within that addressable window.
fn find_deepest_addressable_copy_source(stack: &Stack, value: &ValueOrAlias) -> Option<u8> {
    let target = value.unaliased();
    let mut required_depth = 0usize;
    let mut source = None;
    for (pos, operand) in stack.iter().rev().enumerate() {
        required_depth += operand.stack_size();
        if required_depth > MASM_STACK_WINDOW_FELTS {
            break;
        }
        if operand.unaliased() == target {
            source = Some(pos as u8);
        }
    }
    source
}

/// Moves move-constrained expected operands towards the top if copy materialization would push them
/// beyond the MASM addressable window.
///
/// Returns `true` if the stack state was modified.
fn preemptively_move_endangered_operands_to_top(builder: &mut SolutionBuilder) -> bool {
    let missing_copy_felts: usize = builder
        .context()
        .expected()
        .iter()
        .filter(|value| value.is_alias() && builder.get_current_position(value).is_none())
        .map(|value| value.stack_size())
        .sum();
    if missing_copy_felts == 0 {
        return false;
    }

    let mut changed = false;

    // Repeatedly move the deepest move-constrained operand that would fall out of the addressable
    // window to the top. This avoids generating solutions that require unsupported stack access
    // when copies are materialized.
    loop {
        let mut worst: Option<(u8, usize)> = None;
        for value in builder.context().expected().iter() {
            if value.is_alias() {
                continue;
            }
            let Some(pos) = builder.get_current_position(value) else {
                continue;
            };
            let current_depth: usize = builder
                .stack()
                .iter()
                .rev()
                .take(pos as usize + 1)
                .map(|v| v.stack_size())
                .sum();
            let projected_depth = current_depth + missing_copy_felts;
            if projected_depth > MASM_STACK_WINDOW_FELTS {
                match worst {
                    None => worst = Some((pos, current_depth)),
                    Some((_, best_depth)) if current_depth > best_depth => {
                        worst = Some((pos, current_depth))
                    }
                    _ => {}
                }
            }
        }

        let Some((pos, _)) = worst else {
            break;
        };
        if pos == 0 {
            break;
        }
        builder.movup(pos);
        changed = true;
    }

    changed
}

/// This tactic produces a solution for the given constraints by traversing
/// the stack top-to-bottom, copying/evicting/swapping as needed to put
/// the expected value for the current working index in place.
///
/// This tactic does make an effort to avoid needless moves by searching
/// for swap opportunities that will place multiple expected operands in
/// place at once using the optimal number of swaps. In cases where this
/// cannot be done however, it will perform as few swaps as it can while
/// still making progress.
#[derive(Default)]
pub struct Linear;
impl Tactic for Linear {
    fn cost(&self, context: &SolverContext) -> usize {
        core::cmp::max(context.copies().len(), 1)
    }

    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        let mut changed = true;
        while changed {
            changed = false;

            let mut graph = DiGraphMap::<Operand, ()>::new();

            changed |= preemptively_move_endangered_operands_to_top(builder);

            // Materialize copies
            let mut materialized = SmallSet::<ValueOrAlias, 4>::default();
            // First, mark all expected operands already present on the stack as materialized.
            for expected in builder.context().expected().iter() {
                if builder.get_current_position(expected).is_some() {
                    materialized.insert(*expected);
                }
            }
            // Materialize missing copies deepest-first so that we don't push an as-yet-unmaterialized
            // copy source past the 16-field-element addressing window.
            loop {
                let mut next_copy: Option<(u8, ValueOrAlias)> = None;
                for expected in builder.context().expected().iter() {
                    if builder.get_current_position(expected).is_some() {
                        continue;
                    }
                    // `expected` isn't on the stack because it is a copy we haven't materialized yet
                    assert!(expected.is_alias());
                    let source_at = find_deepest_addressable_copy_source(builder.stack(), expected)
                        .ok_or(TacticError::NotApplicable)?;
                    match next_copy {
                        None => next_copy = Some((source_at, *expected)),
                        Some((best_at, _)) if source_at > best_at => {
                            next_copy = Some((source_at, *expected))
                        }
                        _ => {}
                    }
                }

                let Some((source_at, expected)) = next_copy else {
                    break;
                };

                log::trace!(
                    "materializing copy of {expected:?} from index {source_at} to top of stack",
                );

                // When the stack contains exactly 16 field elements, we cannot emit stack
                // manipulation instructions that directly address index 16+.
                //
                // If the only missing piece is a copy of the operand already on top of the stack,
                // we can safely materialize it by first pushing the original below the 16-field-element
                // addressing window and then duplicating it back to the top.
                //
                // This prevents generating solutions that would require an invalid `swap 16` or
                // `movup 16` during cycle resolution.
                if builder.arity() == MASM_STACK_WINDOW_FELTS
                    && builder
                        .stack()
                        .iter()
                        .map(|operand| operand.stack_size())
                        .sum::<usize>()
                        == MASM_STACK_WINDOW_FELTS
                    && builder.unwrap_expected_position(&expected) == 0
                    && source_at == 0
                    && (1..builder.arity()).all(|i| builder.is_expected(i as u8))
                {
                    // Rotate the original copied operand below the 16 expected operands, then
                    // duplicate it back to the top as the required alias.
                    builder.movdn(15);
                    builder.dup(15, expected.unwrap_alias());
                } else {
                    builder.dup(source_at, expected.unwrap_alias());
                }

                materialized.insert(expected);
                changed = true;
            }

            // Visit each materialized operand and, if out of place, add it to the graph
            // along with the node occupying its expected location on the stack. The occupying
            // node is then considered materialized and visited as well.
            let mut current_index = 0;
            let mut materialized = materialized.into_vec();
            loop {
                if current_index >= materialized.len() {
                    break;
                }
                let value = materialized[current_index];
                let currently_at = builder.unwrap_current_position(&value);
                if let Some(expected_at) = builder.get_expected_position(&value) {
                    if currently_at == expected_at {
                        log::trace!(
                            "{value:?} at index {currently_at} is expected there, no movement \
                             needed"
                        );
                        current_index += 1;
                        continue;
                    }
                    let occupied_by = builder.unwrap_current(expected_at);
                    log::trace!(
                        "{value:?} at index {currently_at}, is expected at index {expected_at}, \
                         which is currently occupied by {occupied_by:?}"
                    );
                    let from = graph.add_node(Operand {
                        pos: currently_at,
                        value,
                    });
                    let to = graph.add_node(Operand {
                        pos: expected_at,
                        value: occupied_by,
                    });
                    graph.add_edge(from, to, ());
                    if !materialized.contains(&occupied_by) {
                        materialized.push(occupied_by);
                    }
                } else {
                    // `value` is not an expected operand, but is occupying a spot
                    // on the stack needed by one of the expected operands. We can
                    // create a connected component with `value` by finding the root
                    // of the path which leads to `value` from an expected operand,
                    // then adding an edge from `value` back to that operand. This
                    // forms a cycle which will allow all expected operands to be
                    // swapped into place, and the unused operand evicted, without
                    // requiring excess moves.
                    let operand = Operand {
                        pos: currently_at,
                        value,
                    };
                    let mut parent = graph.neighbors_directed(operand, Direction::Incoming).next();
                    // There must have been an immediate parent to `value`, or it would
                    // have an expected position on the stack, and only expected operands
                    // are materialized initially.
                    let mut root = parent.unwrap();
                    log::trace!(
                        "{value:?} at index {currently_at}, is not an expected operand; but must \
                         be moved to make space for {:?}",
                        root.value
                    );
                    let mut seen = alloc::collections::BTreeSet::default();
                    seen.insert(root);
                    while let Some(parent_operand) = parent {
                        root = parent_operand;
                        parent =
                            graph.neighbors_directed(parent_operand, Direction::Incoming).next();
                    }
                    log::trace!(
                        "forming component with {value:?} by adding edge to {:?}, the start of \
                         the path which led to it",
                        root.value
                    );
                    graph.add_edge(operand, root, ());
                }
                current_index += 1;
            }

            // Compute the strongly connected components of the graph we've constructed,
            // and use that to drive our decisions about moving operands into place.
            let components = petgraph::algo::kosaraju_scc(&graph);
            if components.is_empty() {
                break;
            }
            log::trace!(
                "found the following connected components when analyzing required operand moves: \
                 {components:?}"
            );
            for component in components.into_iter() {
                // A component of two or more elements indicates a cycle of operands.
                //
                // To determine the order in which swaps must be performed, we first look
                // to see if any of the elements are on top of the stack. If so, we swap
                // it with its parent in the graph, and so on until we reach the edge that
                // completes the cycle (i.e. brings us back to the operand we started with).
                //
                // If we didn't have an operand on top of the stack yet, we pick the operand
                // that is closest to the top of the stack to move to the top, so as not to
                // disturb the positions of the other operands. We then proceed as described
                // above. The only additional step required comes at the end, where we move
                // whatever operand ended up on top of the stack to the original position of
                // the operand we started with.
                //
                // # Examples
                //
                // Consider a component of 3 operands: B -> A -> C -> B
                //
                // We can put all three operands in position by first swapping B with A,
                // putting B into position; and then A with C, putting A into position,
                // and leaving C in position as a result.
                //
                // Let's extend it one operand further: B -> A -> C -> D -> B
                //
                // The premise is the same, B with A, A with C, then C with D, the result
                // is that they all end up in position at the end.
                //
                // Here's a diagram of how the state changes as we perform the swaps
                //
                // 0    1    2    3
                // C -> D -> B -> A -> C
                //
                // 0    1    2    3
                // D    C    B    A
                //
                // 0    1    2    3
                // B    C    D    A
                //
                // 0    1    2    3
                // A    C    D    B
                //
                if component.len() > 1 {
                    // Find the operand at the shallowest depth on the stack to move.
                    let start = component.iter().min_by(|a, b| a.pos.cmp(&b.pos)).copied().unwrap();
                    log::trace!(
                        "resolving component {component:?} by starting from {:?} at index {}",
                        start.value,
                        start.pos
                    );

                    // If necessary, move the starting operand to the top of the stack
                    let start_position = start.pos;
                    if start_position > 0 {
                        changed = true;
                        builder.movup(start_position);
                    }

                    // Do the initial swap to set up our state for the remaining swaps
                    let mut child =
                        graph.neighbors_directed(start, Direction::Outgoing).next().unwrap();
                    // Swap each child with its parent until we reach the edge that forms a cycle
                    while child != start {
                        log::trace!(
                            "swapping {:?} with {:?} at index {}",
                            builder.unwrap_current(0),
                            child.value,
                            child.pos
                        );
                        builder.swap(child.pos);
                        changed = true;
                        if let Some(next_child) =
                            graph.neighbors_directed(child, Direction::Outgoing).next()
                        {
                            child = next_child;
                        } else {
                            // This edge case occurs when the component is of size 2, and the
                            // start and end nodes are being swapped to get them both into position.
                            //
                            // We verify that here to ensure we catch any exceptions we are not yet
                            // aware of.
                            assert_eq!(component.len(), 2);
                            break;
                        }
                    }

                    // If necessary, move the final operand to the original starting position
                    if start_position > 0 {
                        builder.movdn(start_position);
                        changed = true;
                    }
                }
            }

            if builder.is_valid() {
                break;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::opt::operands::{
        OperandMovementConstraintSolver, SolverContext, SolverOptions,
        tactics::Linear,
        testing::{self, ProblemInputs},
    };

    prop_compose! {
        fn generate_linear_problem()
        (stack_size in 0usize..16)
        (problem in testing::generate_stack_subset_copy_any_problem(stack_size)) -> ProblemInputs {
            problem
        }
    }

    #[test]
    fn linear_tactic_regression_case_does_not_require_unsupported_stack_access() {
        // Regression test: when the stack grows beyond 16 operands due to copy materialization,
        // the tactic must still produce a solution that never requires addressing index 16+.
        let problem = testing::make_problem_inputs(
            vec![0, 6, 9, 3, 2, 11, 5, 12, 10, 4, 1, 7, 8, 13],
            8,
            0b1111_1000,
        );

        let context = SolverContext::new(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
            SolverOptions {
                fuel: 10,
                ..Default::default()
            },
        )
        .expect("expected solver context to be valid");

        for _ in 0..64 {
            let actions = OperandMovementConstraintSolver::new_with_options(
                &problem.expected,
                &problem.constraints,
                &problem.stack,
                SolverOptions {
                    fuel: 10,
                    ..Default::default()
                },
            )
            .expect("expected solver context to be valid")
            .solve_with_tactic::<Linear>()
            .expect("expected tactic to be applicable");
            assert!(
                actions.is_some(),
                "linear tactic produced a partial solution for regression case: {problem:#?}"
            );
            let actions = actions.unwrap();
            assert!(
                !OperandMovementConstraintSolver::solution_requires_unsupported_stack_access(
                    &actions,
                    context.stack(),
                ),
                "linear tactic produced a solution requiring unsupported stack access: \
                 {problem:#?}"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn operand_tactics_linear_proptest(problem in generate_linear_problem()) {
            testing::solve_problem_with_tactic::<Linear>(problem)?
        }
    }
}
