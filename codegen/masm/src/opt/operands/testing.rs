use alloc::rc::Rc;
use core::fmt;

use midenc_hir::{self as hir, Type};
use proptest::{prelude::*, test_runner::TestRunner};
use smallvec::SmallVec;

use super::*;
use crate::Constraint;

pub fn logger_setup() {
    use log::LevelFilter;
    let _ = env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .format_timestamp(None)
        .is_test(true)
        .try_init();
}

// Strategy:
//
// 1. Generate a set of 1..16 operands to form a stack (called `stack`), with no more than 2
//    pairs of duplicate operands
// 2. Generate a set of up to 8 constraints (called `constraints`) by sampling `stack` twice,
//    and treating duplicate samples as copies
// 3. Generate the set of expected operands by mapping `constraints` to values
pub struct ProblemInputs {
    #[allow(dead_code)]
    pub context: Rc<hir::Context>,
    pub block: hir::BlockRef,
    pub stack: crate::OperandStack,
    pub expected: SmallVec<[hir::ValueRef; 8]>,
    pub constraints: SmallVec<[Constraint; 8]>,
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

pub fn shuffled_value_stack(size: usize) -> proptest::strategy::Shuffle<Just<Vec<usize>>> {
    let mut next_id = 0;
    let mut raw_stack = Vec::with_capacity(size);
    raw_stack.resize_with(size, || {
        let id = next_id;
        next_id += 1;
        id
    });
    Just(raw_stack).prop_shuffle()
}

pub fn copy_all(arity: usize) -> impl Strategy<Value = usize> {
    Just((1usize << arity) - 1)
}

pub fn copy_some(range: core::ops::RangeInclusive<usize>) -> impl Strategy<Value = usize> {
    let max = *range.end();
    proptest::bits::usize::sampled(0..max, range)
}

pub fn copy_any(arity: usize) -> impl Strategy<Value = usize> {
    let min = core::cmp::min(1, arity);
    prop_oneof![
        CopyStrategy::all(arity),
        CopyStrategy::none(arity),
        CopyStrategy::some(min..=arity),
    ]
}

#[derive(Debug, Clone)]
pub struct CopyStrategy {
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

pub struct CopyStrategyValueTree {
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

pub fn make_problem_inputs(raw_stack: Vec<usize>, arity: usize, copies: usize) -> ProblemInputs {
    use proptest::bits::BitSetLike;

    let context = Rc::new(hir::Context::default());
    let block = context.create_block_with_params(core::iter::repeat_n(Type::I32, raw_stack.len()));
    let block_borrowed = block.borrow();
    let block_args = block_borrowed.arguments();
    let raw_stack = raw_stack
        .into_iter()
        .map(|index| block_args[index] as hir::ValueRef)
        .collect::<Vec<hir::ValueRef>>();
    let mut stack = crate::OperandStack::default();
    let mut expected = SmallVec::with_capacity(arity);
    let mut constraints = SmallVec::with_capacity(arity);
    for value in raw_stack.into_iter().rev() {
        stack.push(value);
    }
    for (id, value) in block_args.iter().copied().enumerate().take(arity) {
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
    fn generate_copy_count(num_expected: usize)(num_copies in 0..(num_expected + 1))(copies in copy_any(num_copies)) -> usize {
        copies
    }
}

prop_compose! {
    pub fn generate_stack_subset_copy_any_problem(stack_size: usize)
        (raw_stack in shuffled_value_stack(stack_size), num_expected in 0..(stack_size + 1))
            (stack in Just(raw_stack), expected in Just(num_expected), copies in generate_copy_count(num_expected)) -> ProblemInputs {
        make_problem_inputs(stack, expected, copies)
    }
}

prop_compose! {
    pub fn generate_copy_any_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                            (copies in copy_any(arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
        make_problem_inputs(raw_stack, arity, copies)
    }
}

prop_compose! {
    pub fn generate_copy_none_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                            (raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
        make_problem_inputs(raw_stack, arity, 0)
    }
}

prop_compose! {
    pub fn generate_copy_all_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 0..=stack_size)))
                            (copies in copy_all(arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
        make_problem_inputs(raw_stack, arity, copies)
    }
}

prop_compose! {
    pub fn generate_copy_some_problem()((raw_stack, arity) in (1..8usize).prop_flat_map(|stack_size| (shuffled_value_stack(stack_size), 1..=stack_size)))
                            (copies in copy_some(1..=arity), raw_stack in Just(raw_stack), arity in Just(arity)) -> ProblemInputs {
        make_problem_inputs(raw_stack, arity, copies)
    }
}

pub fn solve_problem(problem: ProblemInputs) -> Result<(), TestCaseError> {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE").is_test(true).try_init();
    let block = problem.block.borrow();
    let block_args = block.arguments();
    match OperandMovementConstraintSolver::new_with_options(
        &problem.expected,
        &problem.constraints,
        &problem.stack,
        SolverOptions {
            fuel: 10,
            ..Default::default()
        },
    ) {
        Ok(solver) => {
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
                reject_out_of_scope(action)?;
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

pub fn solve_problem_with_tactic<T: tactics::Tactic + Default>(
    problem: ProblemInputs,
) -> Result<(), TestCaseError> {
    let block = problem.block.borrow();
    let block_args = block.arguments();
    match OperandMovementConstraintSolver::new_with_options(
        &problem.expected,
        &problem.constraints,
        &problem.stack,
        SolverOptions {
            fuel: 10,
            ..Default::default()
        },
    ) {
        Ok(solver) => {
            let result = solver.solve_with_tactic::<T>();
            // We are expecting solutions for all inputs
            prop_assert!(
                result.is_ok(),
                "solver returned error {result:?} for problem: {problem:#?}"
            );
            let actions = result.unwrap();
            prop_assert!(
                actions.is_some(),
                "solver indicated tactic produced only a partial solution for problem: \
                 {problem:#?}"
            );
            let actions = actions.unwrap();
            // We are expecting that applying `actions` to the input stack will produce a stack
            // that has all of the expected operands on top of the stack,
            // ordered by id, e.g. [v1, v2, ..vN]
            let mut stack = problem.stack.clone();
            for action in actions.into_iter() {
                reject_out_of_scope(action)?;
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

fn reject_out_of_scope(action: Action) -> Result<(), TestCaseError> {
    // Reject test cases that are impossible to solve with operand stack management alone, i.e.
    // more than 8 operands in random order all of which are copy constrained. This can easily
    // push the problem into unsolvable territory without spills to memory. As such, we check for
    // such cases and reject them.
    let index = match action {
        Action::Copy(index)
        | Action::Swap(index)
        | Action::MoveUp(index)
        | Action::MoveDown(index) => index,
    };
    if index >= 16 {
        Err(TestCaseError::Reject("problem is out of scope for operand stack solver".into()))
    } else {
        Ok(())
    }
}
