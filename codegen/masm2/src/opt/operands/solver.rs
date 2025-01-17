use midenc_hir2::{self as hir, SourceSpan};
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
    /// An integer representing the amount of optimization fuel we have available
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
        assert_eq!(expected.len(), constraints.len());

        let context = SolverContext::new(expected, constraints, stack)?;

        Ok(Self {
            context,
            tactics: Default::default(),
            fuel: 25,
        })
    }

    /// Set the quantity of optimization fuel the solver has to work with
    #[allow(unused)]
    pub fn set_optimization_fuel(&mut self, fuel: usize) {
        self.fuel = fuel;
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
            if self.context.copies().is_empty() {
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

    #[track_caller]
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
                if let Some(current_position) = self.context.stack().position(&expected.value) {
                    if current_position > 0 {
                        emitter.move_operand_to_position(current_position, 0, false, span);
                    }
                } else {
                    assert!(
                        self.context.copies().has_copies(&expected.value),
                        "{:?} was not found on the operand stack",
                        expected.value
                    );
                    let current_position =
                        self.context.stack().position(&expected.value.unaliased()).unwrap_or_else(
                            || {
                                panic!(
                                    "{:?} was not found on the operand stack",
                                    expected.value.unaliased()
                                )
                            },
                        );
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
    use core::fmt;

    use midenc_hir2::{self as hir, Type};
    use proptest::{prelude::*, test_runner::TestRunner};

    use super::*;

    #[allow(unused)]
    fn setup() {
        use log::LevelFilter;
        let _ = env_logger::builder()
            .filter_level(LevelFilter::Trace)
            .format_timestamp(None)
            .is_test(true)
            .try_init();
    }

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

    /// This test reproduces https://github.com/0xPolygonMiden/compiler/issues/200
    /// where v7 value should be duplicated on the stack
    #[test]
    fn operand_movement_constraint_solver_duplicate() {
        use hir::Context;

        setup();

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

    // Strategy:
    //
    // 1. Generate a set of 1..16 operands to form a stack (called `stack`), with no more than 2
    //    pairs of duplicate operands
    // 2. Generate a set of up to 8 constraints (called `constraints`) by sampling `stack` twice,
    //    and treating duplicate samples as copies
    // 3. Generate the set of expected operands by mapping `constraints` to values
    struct ProblemInputs {
        #[allow(dead_code)]
        context: Rc<hir::Context>,
        block: hir::BlockRef,
        stack: crate::OperandStack,
        expected: Vec<hir::ValueRef>,
        constraints: Vec<Constraint>,
    }
    impl fmt::Debug for ProblemInputs {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("ProblemInputs")
                .field("stack", &self.stack)
                .field_with("expected", |f| {
                    let mut builder = f.debug_list();
                    for value in self.expected.iter() {
                        builder.entry(&format_args!("{value}"));
                    }
                    builder.finish()
                })
                .field("constraints", &self.constraints)
                .finish()
        }
    }

    fn shuffled_value_stack(size: usize) -> proptest::strategy::Shuffle<Just<Vec<usize>>> {
        let mut next_id = 0;
        let mut raw_stack = Vec::with_capacity(size);
        raw_stack.resize_with(size, || {
            let id = next_id;
            next_id += 1;
            id
        });
        Just(raw_stack).prop_shuffle()
    }

    fn copy_all(arity: usize) -> impl Strategy<Value = usize> {
        Just((1usize << arity) - 1)
    }

    fn copy_some(range: core::ops::RangeInclusive<usize>) -> impl Strategy<Value = usize> {
        let max = *range.end();
        proptest::bits::usize::sampled(0..max, range)
    }

    fn copy_any(arity: usize) -> impl Strategy<Value = usize> {
        let min = core::cmp::min(1, arity);
        prop_oneof![
            CopyStrategy::all(arity),
            CopyStrategy::none(arity),
            CopyStrategy::some(min..=arity),
        ]
    }

    #[derive(Debug, Clone)]
    struct CopyStrategy {
        strategy: proptest::strategy::BoxedStrategy<usize>,
        arity: u8,
        min: u8,
        max: u8,
    }
    impl CopyStrategy {
        /// The simplest strategy, always solvable by copying
        pub fn all(arity: usize) -> Self {
            assert!(arity <= 16);
            let max = arity as u8;
            let strategy = if arity == 0 {
                Just(0usize).boxed()
            } else if arity == 1 {
                Just(1usize).boxed()
            } else {
                proptest::bits::usize::sampled(1..arity, 0..arity).boxed()
            };
            Self {
                strategy,
                arity: max,
                min: max,
                max,
            }
        }

        /// The next simplest strategy, avoids complicating strategies with copies
        pub fn none(arity: usize) -> Self {
            assert!(arity <= 16);
            let max = arity as u8;
            let strategy = if arity == 0 {
                Just(0usize).boxed()
            } else if arity == 1 {
                Just(1usize).boxed()
            } else {
                proptest::bits::usize::sampled(1..arity, 0..arity).boxed()
            };
            Self {
                strategy,
                arity: max,
                min: 0,
                max: 0,
            }
        }

        /// The most complicated strategy,
        pub fn some(range: core::ops::RangeInclusive<usize>) -> Self {
            let min = *range.start();
            let max = *range.end();
            assert!(max <= 16);
            let strategy = if max == 0 {
                Just(0usize).boxed()
            } else if max == 1 {
                Just(1usize).boxed()
            } else {
                proptest::bits::usize::sampled(0..max, range).boxed()
            };
            let arity = max as u8;
            Self {
                strategy,
                arity,
                min: min as u8,
                max: arity,
            }
        }
    }
    impl Strategy for CopyStrategy {
        type Tree = CopyStrategyValueTree;
        type Value = usize;

        fn new_tree(&self, runner: &mut TestRunner) -> proptest::strategy::NewTree<Self> {
            let tree = self.strategy.new_tree(runner)?;
            Ok(CopyStrategyValueTree {
                tree,
                arity: self.arity,
                min: self.min,
                max: self.max,
                prev: (self.min, self.max),
                hi: (0, self.max),
            })
        }
    }

    struct CopyStrategyValueTree {
        tree: Box<dyn proptest::strategy::ValueTree<Value = usize>>,
        arity: u8,
        min: u8,
        max: u8,
        prev: (u8, u8),
        hi: (u8, u8),
    }
    impl proptest::strategy::ValueTree for CopyStrategyValueTree {
        type Value = usize;

        fn current(&self) -> Self::Value {
            match (self.min, self.max) {
                (0, 0) => 0,
                (min, max) if min == max => (1 << max as usize) - 1,
                _ => self.tree.current(),
            }
        }

        fn simplify(&mut self) -> bool {
            match (self.min, self.max) {
                (0, 0) => {
                    self.hi = (0, 0);
                    self.min = self.arity;
                    self.max = self.arity;
                    true
                }
                (min, max) if min == max => {
                    self.hi = (min, max);
                    false
                }
                current => {
                    self.hi = current;
                    if !self.tree.simplify() {
                        self.min = 0;
                        self.max = 0;
                    }
                    true
                }
            }
        }

        fn complicate(&mut self) -> bool {
            match (self.min, self.max) {
                current if current == self.hi => false,
                (0, 0) => {
                    self.min = self.prev.0;
                    self.max = self.prev.1;
                    true
                }
                (min, max) if min == max => {
                    self.min = 0;
                    self.max = 0;
                    true
                }
                _ => self.tree.complicate(),
            }
        }
    }

    fn make_problem_inputs(raw_stack: Vec<usize>, arity: usize, copies: usize) -> ProblemInputs {
        use proptest::bits::BitSetLike;

        let context = Rc::new(hir::Context::default());
        let block = context.create_block_with_params(vec![Type::I32; raw_stack.len()]);
        let block_borrowed = block.borrow();
        let block_args = block_borrowed.arguments();
        let raw_stack = raw_stack
            .into_iter()
            .map(|index| block_args[index] as hir::ValueRef)
            .collect::<Vec<hir::ValueRef>>();
        let mut stack = crate::OperandStack::default();
        let mut expected = Vec::with_capacity(arity);
        let mut constraints = Vec::with_capacity(arity);
        for value in raw_stack.into_iter().rev() {
            stack.push(value);
        }
        for (id, value) in block_args.iter().copied().enumerate() {
            expected.push(value as hir::ValueRef);
            if copies.test(id) {
                constraints.push(Constraint::Copy);
            } else {
                constraints.push(Constraint::Move);
            }
        }
        ProblemInputs {
            context,
            block,
            stack,
            expected,
            constraints,
        }
    }

    prop_compose! {
        fn generate_copy_any_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                             (copies in copy_any(arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
            make_problem_inputs(raw_stack, arity, copies)
        }
    }

    prop_compose! {
        fn generate_copy_none_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                             (raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
            make_problem_inputs(raw_stack, arity, 0)
        }
    }

    prop_compose! {
        fn generate_copy_all_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                             (copies in copy_all(arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
            make_problem_inputs(raw_stack, arity, copies)
        }
    }

    prop_compose! {
        fn generate_copy_some_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 1..=stack_size)))
                             (copies in copy_some(1..=arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
            make_problem_inputs(raw_stack, arity, copies)
        }
    }

    fn run_solver(problem: ProblemInputs) -> Result<(), TestCaseError> {
        let block = problem.block.borrow();
        let block_args = block.arguments();
        match OperandMovementConstraintSolver::new(
            &problem.expected,
            &problem.constraints,
            &problem.stack,
        ) {
            Ok(mut solver) => {
                solver.set_optimization_fuel(10);
                let result = solver.solve();
                // We are expecting solutions for all inputs
                prop_assert!(
                    result.is_ok(),
                    "solver returned error {result:?} for problem: {problem:#?}"
                );
                let actions = result.unwrap();
                // We are expecting that if all operands are copies, that the number of actions is
                // equal to the number of copies
                if problem.constraints.iter().all(|c| matches!(c, Constraint::Copy)) {
                    prop_assert_eq!(actions.len(), problem.expected.len());
                }
                // We are expecting that applying `actions` to the input stack will produce a stack
                // that has all of the expected operands on top of the stack,
                // ordered by id, e.g. [v1, v2, ..vN]
                let mut stack = problem.stack.clone();
                for action in actions.into_iter() {
                    match action {
                        Action::Copy(index) => {
                            stack.dup(index as usize);
                        }
                        Action::Swap(index) => {
                            stack.swap(index as usize);
                        }
                        Action::MoveUp(index) => {
                            stack.movup(index as usize);
                        }
                        Action::MoveDown(index) => {
                            stack.movdn(index as usize);
                        }
                    }
                }
                for index in 0..problem.expected.len() {
                    let expected = block_args[index] as hir::ValueRef;
                    prop_assert_eq!(
                        &stack[index],
                        &expected,
                        "solution did not place {} at the correct location on the stack",
                        expected
                    );
                }

                Ok(())
            }
            Err(SolverError::AlreadySolved) => Ok(()),
            Err(err) => panic!("invalid solver context: {err:?}"),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn operand_movement_constraint_solver_copy_any(problem in generate_copy_any_problem()) {
            run_solver(problem)?
        }

        #[test]
        fn operand_movement_constraint_solver_copy_none(problem in generate_copy_none_problem()) {
            run_solver(problem)?;
        }

        #[test]
        fn operand_movement_constraint_solver_copy_all(problem in generate_copy_all_problem()) {
            run_solver(problem)?;
        }

        #[test]
        fn operand_movement_constraint_solver_copy_some(problem in generate_copy_some_problem()) {
            run_solver(problem)?;
        }
    }
}
