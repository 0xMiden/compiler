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

        let a = builder.unwrap_expected(0);
        let b = builder.unwrap_expected(1);

        let a_orig = a.unaliased();
        let b_orig = b.unaliased();
        let duplicates = a_orig == b_orig;

        let (a_index, b_index) = if duplicates {
            let a_index =
                builder.get_current_position(&a_orig).ok_or(TacticError::NotApplicable)?;
            if a.is_alias() || b.is_alias() {
                (a_index, a_index)
            } else {
                let b_index =
                    builder.get_current_position_skip(a_index + 1, &b_orig).unwrap_or(a_index);
                (a_index, b_index)
            }
        } else {
            let a_index =
                builder.get_current_position(&a_orig).ok_or(TacticError::NotApplicable)?;
            let b_index =
                builder.get_current_position(&b_orig).ok_or(TacticError::NotApplicable)?;
            (a_index, b_index)
        };
        match (a.is_alias(), b.is_alias()) {
            (true, true) => self.copy_copy(builder, a, a_index, b, b_index),
            (true, false) => self.copy_move(builder, a, a_index, b, b_index),
            (false, true) => self.move_copy(builder, a, a_index, b, b_index),
            (false, false) => self.move_move(builder, a, a_index, b, b_index),
        }
    }
}

impl TwoArgs {
    fn copy_copy(
        &mut self,
        builder: &mut SolutionBuilder,
        a: ValueOrAlias,
        a_index: u8,
        b: ValueOrAlias,
        b_index: u8,
    ) -> TacticResult {
        if a_index == b_index {
            // Materialize two new copies of the same value
            builder.dup(a_index, b.unwrap_alias());
            builder.dup(0, a.unwrap_alias());
        } else {
            // Materialize copies of each value
            builder.dup(b_index, b.unwrap_alias());
            builder.dup(a_index + 1, a.unwrap_alias());
        }

        Ok(())
    }

    fn copy_move(
        &mut self,
        builder: &mut SolutionBuilder,
        a: ValueOrAlias,
        a_index: u8,
        _b: ValueOrAlias,
        b_index: u8,
    ) -> TacticResult {
        // Note that for this type of solution, commutativity doesn't help us
        if a_index == b_index {
            // Reference to the same value, where a copy must be materialized before use.
            // Move the value first, then materialize a copy
            if b_index > 0 {
                builder.movup(b_index);
            }
            builder.dup(0, a.unwrap_alias());
        } else if b_index > a_index {
            // If b appears after a on the operand stack, then moving b to the top of the operand
            // stack will shift a down the stack.
            //
            // Note that this necessarily implies that b_index > 0.
            builder.movup(b_index);
            builder.dup(a_index + 1, a.unwrap_alias());
        } else if b_index > 0 {
            // If b is not already on top of the stack, it must be moved up, then we can make a
            // copy of a to the top.
            builder.movup(b_index);
            builder.dup(a_index, a.unwrap_alias());
        } else {
            // b_index == 0, and a_index != b_index, so we need only copy a to the top
            builder.dup(a_index, a.unwrap_alias());
        }

        Ok(())
    }

    fn move_copy(
        &mut self,
        builder: &mut SolutionBuilder,
        _a: ValueOrAlias,
        a_index: u8,
        b: ValueOrAlias,
        b_index: u8,
    ) -> TacticResult {
        let commutative = !builder.requires_strict_solution();
        if a_index == b_index {
            // Reference to the same value, where a copy must be materialized before use.
            // Move the value first, then materialize a copy
            if b_index > 0 {
                builder.movup(b_index);
            }
            builder.dup(0, b.unwrap_alias());
            let b = builder.pending[0];
            let a = builder.pending[0].unaliased();
            builder.pending[1] = b;
            builder.pending[0] = a;
        } else if b_index > a_index {
            // If b appears after a on the operand stack, then copying b to the top of the operand
            // stack will shift a down the stack.
            //
            // Note that this necessarily implies that b_index > 0.

            // For commutative operations, we can elide some actions so long as a and b end up
            // on top in some order. For non-commutative operations, we may need to issue an
            // extra `swap` to get things in the strict order required
            if commutative && a_index > 0 {
                builder.movup(a_index);
                builder.dup(b_index, b.unwrap_alias());
            } else if commutative {
                builder.dup(b_index, b.unwrap_alias());
            } else if a_index > 0 {
                builder.dup(b_index, b.unwrap_alias());
                builder.movup(a_index + 1);
            } else {
                builder.dup(b_index, b.unwrap_alias());
                builder.swap(1);
            }
        } else if b_index > 0 && commutative {
            // a_index > b_index && b_index > 0
            builder.movup(a_index);
            builder.dup(b_index + 1, b.unwrap_alias());
        } else if b_index > 0 {
            // a_index > b_index && b_index > 0
            builder.dup(b_index, b.unwrap_alias());
            builder.movup(a_index + 1);
        } else {
            // b_index == 0 && a_index != b_index
            builder.dup(b_index, b.unwrap_alias());
            builder.movup(a_index + 1);
        }

        Ok(())
    }

    fn move_move(
        &mut self,
        builder: &mut SolutionBuilder,
        _a: ValueOrAlias,
        a_index: u8,
        _b: ValueOrAlias,
        b_index: u8,
    ) -> TacticResult {
        debug_assert_ne!(a_index, b_index);

        let commutative = !builder.requires_strict_solution();
        match (a_index, b_index) {
            (0, 1) => (),
            (0, b_index) => {
                builder.movup(b_index);
                if !commutative {
                    builder.swap(1);
                }
            }
            (1, 0) if !commutative => {
                builder.swap(1);
            }
            (a_index, 0) => {
                if !commutative || a_index > 1 {
                    builder.movup(a_index);
                }
            }
            (a_index, 1) => {
                builder.swap(a_index);
            }
            (1, 2) => {
                // Shift the operands to the top by moving the top element down
                builder.movdn(2);
            }
            (a_index, b_index) if a_index > b_index => {
                builder.movup(b_index);
                builder.movup(a_index);
            }
            (a_index, b_index) => {
                builder.movup(b_index);
                builder.movup(a_index + 1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use itertools::Itertools;
    use midenc_hir::Context;

    use super::*;
    use crate::{
        Constraint, OperandStack,
        opt::{SolverError, operands::SolverOptions},
    };

    // These are actually RHS/LHS pairs.
    const ALL_CONSTRAINTS: [[Constraint; 2]; 4] = [
        [Constraint::Move, Constraint::Move],
        [Constraint::Move, Constraint::Copy],
        [Constraint::Copy, Constraint::Move],
        [Constraint::Copy, Constraint::Copy],
    ];

    #[test]
    fn solves_with_strict_operand_order() {
        let hir_ctx = Rc::new(Context::default());

        // Take every permutation of a 5 element stack and each permutation of two operand
        // constraints and confirm that at most 2 actions are required to solve.
        let val_refs = generate_valrefs(&hir_ctx, 5);
        let total_actions = permute_stacks(&val_refs, 2, true);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 876,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_for_any_commutative_permutation() {
        let hir_ctx = Rc::new(Context::default());

        // Take every permutation of a 5 element stack and each permutation of two operand
        // constraints and confirm that at most 2 actions are required for an unordered solution.
        let val_refs = generate_valrefs(&hir_ctx, 5);
        let total_actions = permute_stacks(&val_refs, 2, false);

        // This number should only ever go down as we add optimisations.
        //
        // This value should always be smaller than that of `solves_with_strict_operand_order`
        assert!(
            total_actions <= 828,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_optimally_for_move_move_commutative_permutation() {
        let hir_ctx = Rc::new(Context::default());

        // Take every permutation of a 3 element stack and confirm that at most 1 action is
        // required for an unordered solution with move/move constraints.
        let val_refs = generate_valrefs(&hir_ctx, 3);
        let expected = [val_refs[0], val_refs[1]];
        let constraints = [[Constraint::Move, Constraint::Move]];

        let total_actions = permute_stacks_advanced(&val_refs, expected, &constraints, 1, false);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 4,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_with_materialized_copy_strict() {
        let hir_ctx = Rc::new(Context::default());
        let total_actions = duplicated_stack_single_util(&hir_ctx, true);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 132,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_with_materialized_copy_commutative() {
        let hir_ctx = Rc::new(Context::default());
        let total_actions = duplicated_stack_single_util(&hir_ctx, false);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 132,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_with_existing_copy_strict() {
        let hir_ctx = Rc::new(Context::default());
        let total_actions = duplicated_stack_double_util(&hir_ctx, true);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 414,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    #[test]
    fn solves_with_existing_copy_commutative() {
        let hir_ctx = Rc::new(Context::default());
        let total_actions = duplicated_stack_double_util(&hir_ctx, false);

        // This number should only ever go down as we add optimisations.
        assert!(
            total_actions <= 396,
            "optimization regression, observed an unexpected increase in number of stack ops \
             needed to solve to {total_actions}"
        );
    }

    fn duplicated_stack_single_util(context: &Context, strict: bool) -> usize {
        // Take every permutation of a 4 element stack etc. where the two operands are the very
        // same value.  In this case it doesn't make sense for a Move/Move constraint to be used.
        //
        // The expected output is v0, v0.
        let val_refs = generate_valrefs(context, 4);
        let expected = [val_refs[0], val_refs[0]];
        let constraints = [
            [Constraint::Move, Constraint::Copy],
            [Constraint::Copy, Constraint::Move],
            [Constraint::Copy, Constraint::Copy],
        ];

        permute_stacks_advanced(&val_refs, expected, &constraints, 2, strict)
    }

    fn duplicated_stack_double_util(context: &Context, strict: bool) -> usize {
        // Take every permutation of a 5 element stack etc. where the two operands are the same value
        // but represented twice in the input.

        // Generate 4 val refs but append a copy of v0.
        let mut val_refs = generate_valrefs(context, 4);
        let v0 = val_refs[0];
        val_refs.push(v0);

        let expected = [v0, v0];

        permute_stacks_advanced(&val_refs, expected, &ALL_CONSTRAINTS, 2, strict)
    }

    fn generate_valrefs(context: &Context, k: usize) -> Vec<midenc_hir::ValueRef> {
        // The easiest? way to create a bunch of ValueRefs is to create a block with args and use them.
        let block = context
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
        strict: bool,
    ) -> usize {
        // Use just v0 and v1 at the top.  The input is permuted so always using these is OK.
        let expected = [val_refs[0], val_refs[1]];

        permute_stacks_advanced(val_refs, expected, &ALL_CONSTRAINTS, max_actions, strict)
    }

    fn permute_stacks_advanced(
        val_refs: &[midenc_hir::ValueRef],
        expected: [midenc_hir::ValueRef; 2],
        constraints: &[[Constraint; 2]],
        max_actions: usize,
        strict: bool,
    ) -> usize {
        let mut total_actions = 0;

        // Permute every possible input stack variation and solve for each.
        for val_refs_perm in val_refs.iter().permutations(val_refs.len()).unique() {
            let mut pending = OperandStack::default();
            for value in val_refs_perm.into_iter().rev() {
                pending.push(*value);
            }

            for constraint_pair in constraints {
                let context = SolverContext::new(
                    &expected,
                    constraint_pair,
                    &pending,
                    SolverOptions {
                        strict,
                        ..Default::default()
                    },
                );

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
                        assert!(
                            num_actions <= max_actions,
                            "expected solution to take no more than {max_actions} actions, got \
                             {num_actions}"
                        );
                        total_actions += num_actions;
                    }

                    Err(SolverError::AlreadySolved) => {}
                    Err(_) => panic!("Unexpected error while building the solver context."),
                }
            }
        }

        total_actions
    }
}
