use super::*;

/// This tactic is for specifically optimising binary operators, especially those which are
/// commutative.  The best case scenario for commutative ops is no work needs to be done.
/// Otherwise binary ops may be solved with a single swap, move or dupe, and at worst two swaps,
/// moves or dupes.
///
/// The only criterion for success is an arity of exactly two.  Then the solution will always
/// succeed, adjusted only by whether commutativity is a factor.

#[derive(Default)]
pub struct TwoArgs;

impl Tactic for TwoArgs {
    fn apply(&mut self, builder: &mut SolutionBuilder) -> TacticResult {
        if builder.arity() != 2 {
            return Err(TacticError::PreconditionFailed);
        }

        let rhs = builder.unwrap_expected(0);
        let rhs_un = rhs.unaliased();
        let mut rhs_is_copy = builder.context.copies().has_copies(&rhs_un);

        let lhs = builder.unwrap_expected(1);
        let lhs_un = lhs.unaliased();
        let mut lhs_is_copy = builder.context.copies().has_copies(&lhs_un);

        let positions = if lhs_un != rhs_un {
            // Values are distinct; just find them individually.
            builder.get_current_position(&lhs_un).and_then(|lhs_pos| {
                builder.get_current_position(&rhs_un).map(|rhs_pos| (lhs_pos, rhs_pos))
            })
        } else if lhs_is_copy {
            // We are given the same value, but one or both is a copy.  Need to check which is an
            // alias.
            lhs_is_copy = lhs.is_alias();
            rhs_is_copy = rhs.is_alias();

            // Return the same position for both.
            builder.get_current_position(&lhs_un).map(|pos| (pos, pos))
        } else {
            // We are given the same value, but they're not copies, implying they have different
            // positions.  I.e., literally the same value twice in the input.
            builder.get_current_position(&lhs_un).and_then(|lhs_pos| {
                builder
                    .get_current_position_skip(lhs_pos + 1, &lhs_un)
                    .map(|rhs_pos| (lhs_pos, rhs_pos))
            })
        };

        if let Some((lhs_pos, rhs_pos)) = positions {
            match (lhs_is_copy, rhs_is_copy) {
                (true, true) => self.copy_copy(builder, lhs, lhs_pos, rhs, rhs_pos),
                (true, false) => self.copy_move(builder, lhs, lhs_pos, rhs, rhs_pos),
                (false, true) => self.move_copy(builder, lhs, lhs_pos, rhs, rhs_pos),
                (false, false) => self.move_move(builder, lhs, lhs_pos, rhs, rhs_pos),
            }
        } else {
            Err(TacticError::NotApplicable)
        }
    }
}

impl TwoArgs {
    fn copy_copy(
        &mut self,
        builder: &mut SolutionBuilder,
        lhs: ValueOrAlias,
        lhs_pos: u8,
        rhs: ValueOrAlias,
        rhs_pos: u8,
    ) -> TacticResult {
        if lhs_pos == rhs_pos {
            // Copy it twice.  The scheduler will be requesting a dupe of the dupe.
            builder.dup(lhs_pos, lhs.unwrap_alias());
            let first_copy_alias_id = builder.stack()[0].unwrap_alias();
            let next_copy_alias_id = first_copy_alias_id.checked_add(1).unwrap();
            builder.dup(0, next_copy_alias_id);
        } else {
            builder.dup(lhs_pos, lhs.unwrap_alias());
            builder.dup(rhs_pos + 1, rhs.unwrap_alias());
        }

        Ok(())
    }

    fn copy_move(
        &mut self,
        builder: &mut SolutionBuilder,
        lhs: ValueOrAlias,
        lhs_pos: u8,
        _rhs: ValueOrAlias,
        rhs_pos: u8,
    ) -> TacticResult {
        builder.dup(lhs_pos, lhs.unwrap_alias());

        // We don't need to move the RHS if it was on top already and either LHS is the same value
        // or we can leave the operands out of order.
        let dupe_of_top = lhs_pos == rhs_pos && lhs_pos == 0;
        let can_leave_rhs = builder.unordered_allowed() && rhs_pos == 0;

        if !can_leave_rhs && !dupe_of_top {
            builder.movup(rhs_pos + 1);
        }

        Ok(())
    }

    fn move_copy(
        &mut self,
        builder: &mut SolutionBuilder,
        _lhs: ValueOrAlias,
        lhs_pos: u8,
        rhs: ValueOrAlias,
        rhs_pos: u8,
    ) -> TacticResult {
        if lhs_pos == 0 {
            builder.dup(rhs_pos, rhs.unwrap_alias());
        } else {
            builder.movup(lhs_pos);
            if lhs_pos < rhs_pos {
                builder.dup(rhs_pos, rhs.unwrap_alias());
            } else if lhs_pos == rhs_pos {
                builder.dup(0, rhs.unwrap_alias());
            } else {
                builder.dup(rhs_pos + 1, rhs.unwrap_alias());
            }
        }

        Ok(())
    }

    fn move_move(
        &mut self,
        builder: &mut SolutionBuilder,
        _lhs: ValueOrAlias,
        lhs_pos: u8,
        _rhs: ValueOrAlias,
        rhs_pos: u8,
    ) -> TacticResult {
        assert!(lhs_pos != rhs_pos);

        if lhs_pos == 0 {
            // Just move the RHS to the top, if needed.
            if !(builder.unordered_allowed() && rhs_pos == 1) {
                builder.movup(rhs_pos);
            }
        } else if rhs_pos == 0 && builder.unordered_allowed() {
            // Just move the LHS to the top.
            builder.movup(lhs_pos);
        } else if rhs_pos >= 2 && lhs_pos == 1 {
            // Swap the RHS up to the top.
            builder.swap(rhs_pos);
        } else if (rhs_pos == 1 && lhs_pos == 2)
            || (builder.unordered_allowed() && lhs_pos == 1 && rhs_pos == 2)
        {
            // Can just move the top value out of the way.
            builder.movdn(2);
        } else {
            // Default solution of moving them both.
            builder.movup(lhs_pos);
            if lhs_pos < rhs_pos {
                builder.movup(rhs_pos);
            } else {
                builder.movup(rhs_pos + 1);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;

    // These are actually RHS/LHS pairs.
    const ALL_CONSTRAINTS: [[crate::Constraint; 2]; 4] = [
        [crate::Constraint::Move, crate::Constraint::Move],
        [crate::Constraint::Move, crate::Constraint::Copy],
        [crate::Constraint::Copy, crate::Constraint::Move],
        [crate::Constraint::Copy, crate::Constraint::Copy],
    ];

    fn generate_valrefs(k: usize) -> Vec<midenc_hir::ValueRef> {
        // The easiest? way to create a bunch of ValueRefs is to create a block with args and use them.
        let hir_ctx = std::rc::Rc::new(midenc_hir::Context::default());

        let block = hir_ctx
            .create_block_with_params(core::iter::repeat_n(midenc_hir::Type::I32, k))
            .borrow();

        block
            .arguments()
            .iter()
            .map(|block_arg| *block_arg as midenc_hir::ValueRef)
            .collect()
    }

    // Generate permutations of k values and run the two_args tactic on them all.  Return the total
    // number of actions required to solve ALL problems.
    //
    // Each solution must use a prescribed maximum number of actions and be valid.
    fn permute_stacks(
        val_refs: &[midenc_hir::ValueRef],
        max_actions: usize,
        allow_unordered: bool,
    ) -> usize {
        // Use just v0 and v1 at the top.  The input is permuted so always using these is OK.
        let expected = vec![val_refs[0], val_refs[1]];

        permute_stacks_advanced(val_refs, &expected, &ALL_CONSTRAINTS, max_actions, allow_unordered)
    }

    fn permute_stacks_advanced(
        val_refs: &[midenc_hir::ValueRef],
        expected: &[midenc_hir::ValueRef],
        constraints: &[[crate::Constraint; 2]],
        max_actions: usize,
        allow_unordered: bool,
    ) -> usize {
        let mut total_actions = 0;

        // Permute every possible input stack variation and solve for each.
        for val_refs_perm in val_refs.iter().permutations(val_refs.len()).unique() {
            let mut pending = crate::OperandStack::default();
            for value in val_refs_perm {
                pending.push(*value);
            }

            for constraint_pair in constraints {
                let context =
                    SolverContext::new(expected, allow_unordered, constraint_pair, &pending);

                match context {
                    Ok(context) => {
                        let mut builder = SolutionBuilder::new(&context);

                        let mut tactic = TwoArgs;
                        let res = tactic.apply(&mut builder);

                        assert!(res.is_ok(), "Tactic should always succeed: {:?}.", res.err());
                        assert!(
                            builder.is_valid(),
                            "Invalid solution:\nlhs constraint: {:?}, rhs constraint: \
                             {:?}\ninput: {:?}\nexpected: {:?}\noutput: {:?}",
                            constraint_pair[1],
                            constraint_pair[0],
                            &pending,
                            &context.expected(),
                            &builder.stack()
                        );

                        let num_actions = builder.take().len();
                        assert!(num_actions <= max_actions);
                        total_actions += num_actions;
                    }

                    Err(crate::opt::SolverError::AlreadySolved) => {}
                    Err(_) => panic!("Unexpected error while building the solver context."),
                }
            }
        }

        total_actions
    }

    #[test]
    fn ordered_stack() {
        // Take every permutation of a 5 element stack and each permutation of two operand
        // constraints and confirm that at most 2 actions are required to solve.
        let val_refs = generate_valrefs(5);
        let total_actions = permute_stacks(&val_refs, 2, false);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["876"].assert_eq(&total_actions.to_string());
    }

    #[test]
    fn unordered_stack() {
        // Take every permutation of a 5 element stack and each permutation of two operand
        // constraints and confirm that at most 2 actions are required for an unordered solution.
        let val_refs = generate_valrefs(5);
        let total_actions = permute_stacks(&val_refs, 2, true);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["828"].assert_eq(&total_actions.to_string());
    }

    #[test]
    fn unordered_3_stack() {
        // Take every permutation of a 3 element stack and confirm that at most 1 action is
        // required for an unordered solution with move/move constraints.
        let val_refs = generate_valrefs(3);
        let expected = vec![val_refs[0], val_refs[1]];
        let constraints = [[crate::Constraint::Move, crate::Constraint::Move]];

        let total_actions = permute_stacks_advanced(&val_refs, &expected, &constraints, 1, true);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["4"].assert_eq(&total_actions.to_string());
    }

    fn duplicated_stack_single_util(allow_unordered: bool) -> usize {
        // Take every permutation of a 4 element stack etc. where the two operands are the very
        // same value.  In this case it doesn't make sense for a Move/Move constraint to be used.
        //
        // The expected output is v0, v0.
        let val_refs = generate_valrefs(4);
        let expected = vec![val_refs[0], val_refs[0]];
        let constraints = [
            [crate::Constraint::Move, crate::Constraint::Copy],
            [crate::Constraint::Copy, crate::Constraint::Move],
            [crate::Constraint::Copy, crate::Constraint::Copy],
        ];

        permute_stacks_advanced(&val_refs, &expected, &constraints, 2, allow_unordered)
    }

    #[test]
    fn duplicated_stack_single() {
        let total_actions = duplicated_stack_single_util(false);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["132"].assert_eq(&total_actions.to_string());
    }

    #[test]
    fn duplicated_stack_single_unordered() {
        let total_actions = duplicated_stack_single_util(true);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["132"].assert_eq(&total_actions.to_string());
    }

    fn duplicated_stack_double_util(allow_unordered: bool) -> usize {
        // Take every permutation of a 5 element stack etc. where the two operands are the same value
        // but represented twice in the input.

        // Generate 4 val refs but append a copy of v0.
        let mut val_refs = generate_valrefs(4);
        let v0 = val_refs[0];
        val_refs.push(v0);

        let expected = vec![v0, v0];

        permute_stacks_advanced(&val_refs, &expected, &ALL_CONSTRAINTS, 2, allow_unordered)
    }

    #[test]
    fn duplicated_stack_double() {
        let total_actions = duplicated_stack_double_util(false);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["384"].assert_eq(&total_actions.to_string());
    }

    #[test]
    fn duplicated_stack_double_unordered() {
        let total_actions = duplicated_stack_double_util(true);

        // This number should only ever go down as we add optimisations.
        midenc_expect_test::expect!["384"].assert_eq(&total_actions.to_string());
    }
}
