use alloc::rc::Rc;

use midenc_hir::{adt::SmallSet, *};

use crate::{
    builders::{DefaultInstBuilder, InstBuilder},
    ops::While,
    Condition, Constant, HirDialect, If,
};

/// Identifies [While] loops in do-while form where the body of the `while` block simply forwards
/// operands to the `do` block, which consists solely of an `hir.if`, any operations that feed into
/// its condition, and an `hir.condition` where the condition is a result of the `hir.if` that is
/// constant. See below for an example.
///
/// Such loops are produced by the control flow lifting transformation before these patterns can
/// be recognized, and the code generated for them is noisier and less efficient than when they
/// are in canonical form, which is what this canonicalization pattern is designed to do.
///
/// Before:
///
/// ```text,ignore
/// %input1 = ... : u32
/// %inputN = ... : u32
/// %true = hir.constant 1 : i1
/// %false = hir.constant 0 : i1
/// %out1, %outN = hir.while %input1, %inputN : u32, u32 -> .. {
/// ^bb0(%in1: u32, %in2: u32):
///    %condition = call @evaluate_condition(%in1, %in2) : (u32, u32) -> i1
///    %should_continue, %result1, %resultN = hir.if %condition : -> i1, .. {
///        %v1 = ... : u32
///        %vN = ... : u32
///        hir.yield %true, %v1, %vN
///    } else {
///        hir.yield %false, %in1, %in2
///    }
///    hir.condition %should_continue, %result1, %resultN : i1, ...
/// } do {
/// ^bb1(%arg1: u32, %argN: u32):
///    hir.yield %arg1, %argN
///    ...
/// ```
///
/// After:
///
/// ```text,ignore
/// %input1 = ... : u32
/// %inputN = ... : u32
/// %out1, %outN = hir.while %input1, %inputN : u32, u32 -> .. {
/// ^bb0(%in1: u32, %in2: u32):
///    %condition = call @evaluate_condition(%in1, %in2) : (u32, u32) -> i1
///    hir.condition %condition, %in1, %in2
/// } do {
/// ^bb1(%arg1: u32, %argN: u32):
///    %v1 = ... : u32
///    %vN = ... : u32
///    hir.yield %v1, %vN
/// }
/// ```
///
/// The process looks like so:
///
/// 1. We determine that the `after` block of the loop consists of just an `hir.yield` that forwards
///    some or all of the block arguments. This tells us that the loop is almost certainly in
///    do-while form.
///
/// 2. We then look at the `hir.condition` of the `before` block.
///    a. Is the condition operand the result of an `hir.if`?
///    b. If so, is that result assigned a constant value in each branch?
///    c. If so, then we have a possible match for this transformation
///
/// 3. Next, we must ensure that the `before` block only consists of operations that are used to
///    compute the `hir.if` condition used as input to the `hir.condition` op that terminates the
///    block. We do this by starting from the `hir.condition`, and then adding the defining ops of
///    any of its operands to a set, doing so recursively. We then walk the block, and if any ops
///    are encountered which are not in that set, and those ops have side effects, then it is not
///    safe for us to perform this transformation. If they do not have side effects, and they are
///    used within the body of the `hir.if`, then they can be moved as needed (or erased if dead).
///
/// In the above example, we can see that our input IR matches all three criteria, so the transform
/// can proceed.
pub struct ConvertDoWhileToWhileTrue {
    info: PatternInfo,
}

impl ConvertDoWhileToWhileTrue {
    pub fn new(context: Rc<Context>) -> Self {
        let hir_dialect = context.get_or_register_dialect::<HirDialect>();
        let while_op = hir_dialect.registered_name::<While>().expect("hir.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "convert-do-while-to-while-true",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for ConvertDoWhileToWhileTrue {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for ConvertDoWhileToWhileTrue {
    fn matches(&self, _op: OperationRef) -> Result<bool, Report> {
        panic!("call match_and_rewrite")
    }

    fn rewrite(&self, _op: OperationRef, _rewriter: &mut dyn Rewriter) {
        panic!("call match_and_rewrite")
    }

    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        let before_block = while_op.before().entry_block_ref().unwrap();
        let after_block = while_op.after().entry_block_ref().unwrap();

        // Criteria #1
        let after = after_block.borrow();
        let after_term = after.terminator().unwrap();
        let after_only_yields = after_term.prev().is_none();

        // Criteria #2
        let condition = while_op.condition_op();
        let condition_op = condition.borrow();
        // Condition must be an operation result
        let condition_value = condition_op.condition().as_value_ref();
        let Some(condition_owner) = condition_value.borrow().get_defining_op() else {
            return Ok(false);
        };
        let condition_owner_op = condition_owner.borrow();
        // Condition owner must be an hir.if
        let Some(if_op) = condition_owner_op.downcast_ref::<If>() else {
            return Ok(false);
        };
        // Condition value must be a result of an hir.if that has a constant value along all paths
        // which produce that result
        let Some(condition_constant) = eval_condition(condition_value, if_op) else {
            return Ok(false);
        };

        // Criteria #3
        if !transformation_is_safe(&condition_op) {
            return Ok(false);
        }

        let span = while_op.span();
        let result_types = while_op
            .results()
            .iter()
            .map(|r| r.borrow().ty().clone())
            .collect::<SmallVec<[_; 4]>>();

        // Create a new hir.while to replace the original
        rewriter.set_insertion_point_before(operation);
        let loop_inits = while_op.inits().into_iter().map(|o| o.borrow().as_value_ref());
        let new_while =
            DefaultInstBuilder::new(rewriter).r#while(loop_inits, &result_types, span)?;

        let before_args = while_op
            .before()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let after_args = while_op
            .before()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        todo!();

        Ok(true)
    }
}

fn transformation_is_safe(condition: &Condition) -> bool {
    // Construct the set of allowed operations
    let parent_block = condition.parent().unwrap();
    let mut allowed = SmallSet::<OperationRef, 8>::default();
    allowed.insert(condition.as_operation_ref());
    let mut worklist = SmallVec::<[_; 4]>::from_iter(condition.operands().iter().copied());
    while let Some(operand) = worklist.pop() {
        if let Some(defining_op) = operand.borrow().value().get_defining_op() {
            if defining_op.parent().unwrap() == parent_block {
                allowed.insert(defining_op);
                worklist.extend(defining_op.borrow().operands().iter().copied());
            }
        }
    }

    // Determine if any of the operations in `parent_block` are not allowed
    let mut next_op = parent_block.borrow().body().back().as_pointer();
    while let Some(op) = next_op.take() {
        next_op = op.prev();

        if !allowed.contains(&op) {
            return false;
        }
    }

    true
}

fn eval_condition(value: ValueRef, if_op: &If) -> Option<bool> {
    let value = value.borrow();
    let result = value.downcast_ref::<OpResult>().unwrap();
    let result_index = result.index();

    let then_yield = if_op.then_yield();
    let then_yielded = then_yield.borrow().yielded()[result_index].borrow().as_value_ref();
    let definition = then_yielded.borrow().get_defining_op()?;
    let definition = definition.borrow();
    let definition = definition.downcast_ref::<Constant>()?;
    let then_value = definition.value().as_bool()?;

    let else_yield = if_op.else_yield();
    let else_yielded = else_yield.borrow().yielded()[result_index].borrow().as_value_ref();
    let definition = else_yielded.borrow().get_defining_op()?;
    let definition = definition.borrow();
    let definition = definition.downcast_ref::<Constant>()?;
    let else_value = definition.value().as_bool()?;

    // If the condition is the same in both branches, the transformation isn't safe, as the
    // loop is non-standard (i.e. it is either infinite, or something else less clear).
    if then_value == else_value {
        None
    } else {
        // Otherwise, we want to know what the value of the loop condition is when the hir.if
        // condition is true. This tells us whether the hir.if is effectively inverting the
        // condition or not
        Some(then_value)
    }
}
