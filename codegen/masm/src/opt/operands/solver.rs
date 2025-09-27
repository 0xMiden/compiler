use midenc_hir::{self as hir, SourceSpan};
use smallvec::SmallVec;

use super::{tactics::Tactic, *};
use crate::Constraint;

/// This error type is produced by the [OperandMovementConstraintSolver]
#[derive(Debug)]
pub enum SolverError {
    /// The current operand stack represents a valid solution already
    AlreadySolved,
    /// All of the tactics we tried failed
    NoSolution,
}

/// Configures the behavior of the [OperandMovementConstraintSolver].
#[derive(Debug, Copy, Clone)]
pub struct SolverOptions {
    /// This flag conveys to the solver that the solution(s) it computes must adhere strictly to
    /// the order of the expected operands provided to the solver.
    ///
    /// When `false`, the solver is only required to ensure that the solution(s) it computes place
    /// the expected operands provided to it on top of the operand stack; however, the order of
    /// operands in the solution does not matter.
    ///
    /// This flag defaults to true.
    ///
    /// WARNING: Setting this to `false` is only useful when producing an operand schedule for
    /// commutative instructions, where we don't care/need to know the order of the operands, as
    /// the instruction semantics are equivalent in any permutation. Using non-strict operation in
    /// any other use case is likely to lead to miscompilation.
    pub strict: bool,
    /// An integer representing the amount of optimization fuel we have available
    ///
    /// The more fuel, the more effort will be spent attempting to find an optimal solution.
    pub fuel: usize,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            strict: true,
            fuel: 25,
        }
    }
}

/// The [OperandMovementConstraintSolver] is used to produce a solution to the following problem:
///
/// An instruction is being emitted which requires some specific set of operands, in a particular
/// order. These operands are known to be on the operand stack, but their usage is constrained by a
/// rule that determines whether a specific use of an operand can consume the operand, or must copy
/// it and consume the copy. Furthermore, the operands on the stack are not guaranteed to be in the
/// desired order, so we must also move operands into position while operating within the bounds of
/// the move/copy constraints.
///
/// Complicating matters further, a naive approach to solving this problem will produce a lot of
/// unnecessary stack manipulation instructions in the emitted code. We would like the code we emit
/// to match what a human might write if facing the same set of constraints. As a result, we are
/// looking for a solution to this problem that is also the "smallest" solution, i.e. the least
/// expensive solution in terms of cycle count.
///
/// ## Implementation
///
/// With that context in mind, what we have here is a non-trivial optimization problem. If we could
/// treat the operand stack as an array, and didn't have to worry about copies, we could solve this
/// using a standard minimum-swap solution, but neither of those are true here. The copy constraint,
/// when present, means that even if the stack is in the exact order we need, we must still find a
/// way to copy the operands we are required to copy, move the ones we are required to consume, and
/// do so in such a way that getting them into the required order on top of the stack takes the
/// minimum number of steps.
///
/// Even this would be relatively straightforward, but an additional problem is that the MASM
/// instruction set does not provide us a way to swap two operands at arbitrary positions on the
/// stack. We are forced to move operands to the top of the stack before we can move them elsewhere
/// (either by swapping them with the current operand on top of the stack, or by moving the operand
/// up to the top, shifting all the remaining operands on the stack down by one). However, moving a
/// value up/down the stack also has the effect of shifting other values on the stack, which may
/// shift them in to, or out of, position.
///
/// Long story short, all of this must be taken into consideration at once, which is extremely
/// difficult to express in a way that is readable/maintainable, but also debuggable if something
/// goes wrong.
///
/// To address these concerns, the [OperandMovementConstraintSolver] is architected as follows:
///
/// We expect to receive as input to the solver:
///
/// * The set of expected operand values
/// * The set of move/copy constraints corresponding to each of the expected operands
/// * The current state of the operand stack at this point in the program
///
/// The solver produces one of three possible outcomes:
///
/// * `Ok(solution)`, where `solution` is a vector of actions the code generator must take to get
///   the operands into place correctly
/// * `Err(AlreadySolved)`, indicating that the solver is not needed, and the stack is usable as-is
/// * `Err(_)`, indicating an unrecoverable error that prevented the solver from finding a solution
///   with the given inputs
///
/// When the solver is constructed, it performs the following steps:
///
/// 1. Identify and rename aliased values to make them unique (i.e. multiple uses of the same value
///    will be uniqued)
/// 2. Determine if any expected operands require copying (if so, then the solver is always
///    required)
/// 3. Determine if the solver is required for the given inputs, and if not, return
///    `Err(AlreadySolved)`
///
/// When the solver is run, it attempts to find an optimal solution using the following algorithm:
///
/// 1. Pick a tactic to try and produce a solution for the given set of constraints.
/// 2. If the tactic failed, go back to step 1.
/// 3. If the tactic succeeded, take the best solution between the one we just produced, and the
///    last one produced (if applicable).
/// 4. If we have optimization fuel remaining, go back to step 1 and see if we can find a better
///    solution.
/// 5. If we have a solution, and either run out of optimization fuel, or tactics to try, then that
///    solution is returned.
/// 6. If we haven't found a solution, then return an error
pub struct OperandMovementConstraintSolver {
    context: SolverContext,
    tactics: SmallVec<[Box<dyn Tactic>; 4]>,
    fuel: usize,
}
impl OperandMovementConstraintSolver {
    /// Construct a new solver for the given expected operands, constraints, and operand stack
    /// state.
    pub fn new(
        expected: &[hir::ValueRef],
        constraints: &[Constraint],
        stack: &crate::OperandStack,
    ) -> Result<Self, SolverError> {
        Self::new_with_options(expected, constraints, stack, Default::default())
    }

    /// Same as [Self::new], but takes [SolverOptions] to customize the behavior of the solver.
    pub fn new_with_options(
        expected: &[hir::ValueRef],
        constraints: &[Constraint],
        stack: &crate::OperandStack,
        options: SolverOptions,
    ) -> Result<Self, SolverError> {
        assert_eq!(expected.len(), constraints.len());

        let fuel = options.fuel;
        let context = SolverContext::new(expected, constraints, stack, options)?;

        Ok(Self {
            context,
            tactics: Default::default(),
            fuel,
        })
    }

    /// Compute a solution that can be used to get the stack into the correct state
    pub fn solve(mut self) -> Result<Vec<Action>, SolverError> {
        use super::tactics::*;

        // We use a few heuristics to guide which tactics we try:
        //
        // * If all operands are copies, we only apply copy-all
        // * If copies are needed, we only apply tactics which support copies, or a mix of copies
        //   and moves.
        // * If no copies are needed, we start with the various move up/down + swap patterns, as
        //   many common patterns are solved in two moves or less with them. If no tactics are
        //   successful, move-all is used as the fallback.
        // * If we have no optimization fuel, we do not attempt to look for better solutions once
        //   we've found one.
        // * If we have optimization fuel, we will try additional tactics looking for a solution
        //   until we have exhausted the fuel, assuming the solution we do have can be minimized.
        //   For example, a solution which requires less than two actions is by definition optimal
        //   already, so we never waste time on optimization in such cases.

        // The tactics are pushed in reverse order
        if self.tactics.is_empty() {
            let is_binary = self.context.arity() == 2;
            if is_binary {
                self.tactics.push(Box::new(TwoArgs));
            } else if self.context.copies().is_empty() {
                self.tactics.push(Box::new(Linear));
                self.tactics.push(Box::new(SwapAndMoveUp));
                self.tactics.push(Box::new(MoveUpAndSwap));
                self.tactics.push(Box::new(MoveDownAndSwap));
            } else {
                self.tactics.push(Box::new(Linear));
                self.tactics.push(Box::new(CopyAll));
            }
        }

        // Now that we know what constraints are in place, we can derive
        // a strategy to solve for those constraints. The overall strategy
        // is a restricted backtracking search based on a number of predefined
        // tactics for permuting the stack. The search is restricted because
        // we do not try every possible combination of tactics, and instead
        // follow a shrinking strategy that always subdivides the problem if
        // a larger tactic doesn't succeed first. The search proceeds until
        // a solution is derived, or we cannot proceed any further, in which
        // case we fall back to the most naive approach possible - copying
        // items to the top of the stack one after another until all arguments
        // are in place.
        //
        // Some tactics are derived simply by the number of elements involved,
        // others based on the fact that all copies are required, or all moves.
        // Many solutions are trivially derived from a given set of constraints,
        // we aim simply to recognize common patterns recognized by a human and
        // apply those solutions in such a way that we produce code like we would
        // by hand when preparing instruction operands
        let mut best_solution: Option<Vec<Action>> = None;
        let mut builder = SolutionBuilder::new(&self.context);
        while let Some(mut tactic) = self.tactics.pop() {
            match tactic.apply(&mut builder) {
                // The tactic was applied successfully
                Ok(_) => {
                    if builder.is_valid() {
                        let solution = builder.take();
                        let solution_size = solution.len();
                        let best_size = best_solution.as_ref().map(|best| best.len());
                        match best_size {
                            Some(best_size) if best_size > solution_size => {
                                best_solution = Some(solution);
                                log::trace!(
                                    "a better solution ({solution_size} vs {best_size}) was found \
                                     using tactic {}",
                                    tactic.name()
                                );
                            }
                            Some(best_size) => {
                                log::trace!(
                                    "a solution of size {solution_size} was found using tactic \
                                     {}, but it is no better than the best found so far \
                                     ({best_size})",
                                    tactic.name()
                                );
                            }
                            None => {
                                best_solution = Some(solution);
                                log::trace!(
                                    "an initial solution of size {solution_size} was found using \
                                     tactic {}",
                                    tactic.name()
                                );
                            }
                        }
                    } else {
                        log::trace!(
                            "a partial solution was found using tactic {}, but is not sufficient \
                             on its own",
                            tactic.name()
                        );
                        builder.discard();
                    }
                }
                Err(_) => {
                    log::trace!("tactic {} could not be applied", tactic.name());
                    builder.discard();
                }
            }
            let remaining_fuel = self.fuel.saturating_sub(tactic.cost(&self.context));
            if remaining_fuel == 0 {
                log::trace!("no more optimization fuel, using the best solution found so far");
                break;
            }
            self.fuel = remaining_fuel;
        }

        best_solution.take().ok_or(SolverError::NoSolution)
    }

    #[cfg(test)]
    pub fn solve_with_tactic<T: Tactic + Default>(
        self,
    ) -> Result<Option<Vec<Action>>, SolverError> {
        use super::tactics::*;

        let mut builder = SolutionBuilder::new(&self.context);
        let mut tactic = <T as Default>::default();
        match tactic.apply(&mut builder) {
            // The tactic was applied successfully
            Ok(_) => {
                if builder.is_valid() {
                    Ok(Some(builder.take()))
                } else {
                    log::trace!(
                        "a partial solution was found using tactic {}, but is not sufficient on \
                         its own",
                        tactic.name()
                    );
                    Ok(None)
                }
            }
            Err(_) => {
                log::trace!("tactic {} could not be applied", tactic.name());
                Err(SolverError::NoSolution)
            }
        }
    }

    pub fn solve_and_apply(
        self,
        emitter: &mut crate::emit::OpEmitter<'_>,
        span: SourceSpan,
    ) -> Result<(), SolverError> {
        match self.context.arity() {
            // No arguments, nothing to solve
            0 => Ok(()),
            // Only one argument, solution is trivial
            1 => {
                let expected = self.context.expected()[0];
                if let Some(current_position) = self.context.stack().position(&expected) {
                    if current_position > 0 {
                        emitter.move_operand_to_position(current_position, 0, false, span);
                    }
                } else {
                    assert!(
                        self.context.copies().has_copies(&expected.unaliased()),
                        "{:?} was not found on the operand stack copies",
                        expected.unaliased()
                    );
                    let current_position =
                        self.context.stack().position(&expected.unaliased()).unwrap_or_else(|| {
                            panic!("{:?} was not found on the operand stack", expected.unaliased())
                        });
                    emitter.copy_operand_to_position(current_position, 0, false, span);
                }

                Ok(())
            }
            // Run the solver for more than 1 argument
            _ => {
                let actions = self.solve()?;
                for action in actions.into_iter() {
                    match action {
                        Action::Copy(index) => {
                            emitter.copy_operand_to_position(index as usize, 0, false, span);
                        }
                        Action::Swap(index) => {
                            emitter.swap(index, span);
                        }
                        Action::MoveUp(index) => {
                            emitter.movup(index, span);
                        }
                        Action::MoveDown(index) => {
                            emitter.movdn(index, span);
                        }
                    }
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{self as hir, Type};
    use proptest::prelude::*;

    use super::{super::testing, *};

    #[test]
    fn operand_movement_constraint_solver_example() {
        use hir::Context;

        let context = Rc::new(Context::default());

        let block = context.create_block_with_params(vec![Type::I32; 6]);
        let block = block.borrow();
        let block_args = block.arguments();
        let v1 = block_args[0] as hir::ValueRef;
        let v2 = block_args[1] as hir::ValueRef;
        let v3 = block_args[2] as hir::ValueRef;
        let v4 = block_args[3] as hir::ValueRef;
        let v5 = block_args[4] as hir::ValueRef;
        let v6 = block_args[5] as hir::ValueRef;

        let tests = [[v2, v1, v3, v4, v5, v6], [v2, v4, v3, v1, v5, v6]];

        for test in tests.into_iter() {
            let mut stack = crate::OperandStack::default();
            for value in test.into_iter().rev() {
                stack.push(value);
            }
            let expected = [v1, v2, v3, v4, v5];
            let constraints = [Constraint::Move; 5];

            match OperandMovementConstraintSolver::new(&expected, &constraints, &stack) {
                Ok(solver) => {
                    let result = solver.solve().expect("no solution found");
                    assert!(result.len() <= 3, "expected solution of 3 moves or less");
                }
                Err(SolverError::AlreadySolved) => panic!("already solved"),
                Err(err) => panic!("invalid solver context: {err:?}"),
            }
        }
    }

    #[test]
    fn operand_movement_constraint_solver_two_moves() {
        use hir::Context;

        let context = Rc::new(Context::default());

        let block = context.create_block_with_params(vec![Type::I32; 6]);
        let block = block.borrow();
        let block_args = block.arguments();
        let v1 = block_args[0] as hir::ValueRef;
        let v2 = block_args[1] as hir::ValueRef;
        let v3 = block_args[2] as hir::ValueRef;
        let v4 = block_args[3] as hir::ValueRef;
        let v5 = block_args[4] as hir::ValueRef;
        let v6 = block_args[5] as hir::ValueRef;

        // Should take two moves
        let tests = [
            [v5, v4, v2, v3, v1, v6],
            [v4, v5, v1, v2, v3, v6],
            [v5, v2, v1, v3, v4, v6],
            [v1, v3, v2, v4, v5, v6],
            [v5, v2, v1, v4, v3, v6],
            [v1, v3, v4, v2, v5, v6],
            [v4, v3, v2, v1, v5, v6],
            [v4, v3, v2, v1, v5, v6],
        ];

        for test in tests.into_iter() {
            let mut stack = crate::OperandStack::default();
            for value in test.into_iter().rev() {
                stack.push(value);
            }
            let expected = [v1, v2, v3, v4, v5];
            let constraints = [Constraint::Move; 5];

            match OperandMovementConstraintSolver::new(&expected, &constraints, &stack) {
                Ok(solver) => {
                    let result = solver.solve().expect("no solution found");
                    assert!(
                        result.len() <= 2,
                        "expected solution of 2 moves or less, got {result:?}"
                    );
                }
                Err(SolverError::AlreadySolved) => panic!("already solved"),
                Err(err) => panic!("invalid solver context: {err:?}"),
            }
        }
    }

    #[test]
    fn operand_movement_constraint_solver_one_move() {
        use hir::Context;

        let context = Rc::new(Context::default());

        let block = context.create_block_with_params(vec![Type::I32; 6]);
        let block = block.borrow();
        let block_args = block.arguments();
        let v1 = block_args[0] as hir::ValueRef;
        let v2 = block_args[1] as hir::ValueRef;
        let v3 = block_args[2] as hir::ValueRef;
        let v4 = block_args[3] as hir::ValueRef;
        let v5 = block_args[4] as hir::ValueRef;
        let v6 = block_args[5] as hir::ValueRef;

        // Should take one move
        let tests = [
            [v2, v3, v1, v4, v5, v6],
            [v4, v1, v2, v3, v5, v6],
            [v4, v2, v3, v1, v5, v6],
            [v2, v1, v3, v4, v5, v6],
        ];

        for test in tests.into_iter() {
            let mut stack = crate::OperandStack::default();
            for value in test.into_iter().rev() {
                stack.push(value);
            }
            let expected = [v1, v2, v3, v4, v5];
            let constraints = [Constraint::Move; 5];

            match OperandMovementConstraintSolver::new(&expected, &constraints, &stack) {
                Ok(solver) => {
                    let result = solver.solve().expect("no solution found");
                    assert!(
                        result.len() <= 1,
                        "expected solution of 1 move or less, got {result:?}"
                    );
                }
                Err(SolverError::AlreadySolved) => panic!("already solved"),
                Err(err) => panic!("invalid solver context: {err:?}"),
            }
        }
    }

    /// This test reproduces https://github.com/0xMiden/compiler/issues/200
    /// where v7 value should be duplicated on the stack
    #[test]
    fn operand_movement_constraint_solver_duplicate() {
        use hir::Context;

        testing::logger_setup();

        let context = Rc::new(Context::default());

        let block = context.create_block_with_params(vec![Type::I32; 6]);
        let block = block.borrow();
        let block_args = block.arguments();
        let v7 = block_args[0] as hir::ValueRef;
        let v16 = block_args[1] as hir::ValueRef;
        let v32 = block_args[2] as hir::ValueRef;
        let v0 = block_args[3] as hir::ValueRef;

        let tests = [[v32, v7, v16, v0]];

        for test in tests.into_iter() {
            let mut stack = crate::OperandStack::default();
            for value in test.into_iter().rev() {
                stack.push(value);
            }
            // The v7 is expected to be twice on stack
            let expected = [v7, v7, v32, v16];
            let constraints = [Constraint::Copy; 4];

            match OperandMovementConstraintSolver::new(&expected, &constraints, &stack) {
                Ok(solver) => {
                    let _result = solver.solve().expect("no solution found");
                }
                Err(SolverError::AlreadySolved) => panic!("already solved"),
                Err(err) => panic!("invalid solver context: {err:?}"),
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn operand_movement_constraint_solver_copy_any(problem in testing::generate_copy_any_problem()) {
            testing::solve_problem(problem)?
        }

        #[test]
        fn operand_movement_constraint_solver_copy_none(problem in testing::generate_copy_none_problem()) {
            testing::solve_problem(problem)?
        }

        #[test]
        fn operand_movement_constraint_solver_copy_all(problem in testing::generate_copy_all_problem()) {
            testing::solve_problem(problem)?
        }

        #[test]
        fn operand_movement_constraint_solver_copy_some(problem in testing::generate_copy_some_problem()) {
            testing::solve_problem(problem)?
        }
    }
}
