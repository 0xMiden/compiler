use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify unconditional branches to a block from that block's sole predecessor
///
/// # Example
///
/// ```text,ignore
///   br ^bb1
/// ^bb1
///   br ^bbN(...)
/// ```
///
/// Becomes:
///
/// ```text,ignore
///   br ^bbN(...)
/// ```
pub struct SimplifyPassthroughBr {
    info: PatternInfo,
}

impl SimplifyPassthroughBr {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<Br>().expect("cf.br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-passthrough-br",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyPassthroughBr {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyPassthroughBr {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(br_op) = op.downcast_ref::<Br>() else {
            return Ok(false);
        };

        let successor = br_op.target();
        let dest = successor.successor();
        let mut dest_operands = successor
            .arguments
            .iter()
            .copied()
            .map(|o| o.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        // Try to collapse the successor if it points somewhere other than this block.
        if dest == op.parent().unwrap() {
            return Ok(false);
        }

        let span = op.span();
        drop(op);

        let Some(new_dest) = collapse_branch(operation, dest, &mut dest_operands) else {
            return Ok(false);
        };

        // Create a new branch with the collapsed successor.
        let new_br = rewriter.br(new_dest, dest_operands, span)?;
        rewriter.replace_op(operation, new_br.as_operation_ref());
        retire_unused_forwarding_block(dest, rewriter);

        Ok(true)
    }
}

/// Given a successor, try to collapse it to a new destination if it only contains a passthrough
/// unconditional branch. If the successor is collapsable, the function returns the new successor,
/// and `successor_operands` is updated with the remapped values.
pub fn collapse_branch(
    predecessor: OperationRef,
    successor: BlockRef,
    successor_operands: &mut SmallVec<[ValueRef; 4]>,
) -> Option<BlockRef> {
    // Check that the successor only contains a unconditional branch.
    let succ = successor.borrow();
    let terminator = succ.terminator()?;
    if succ.body().front().as_pointer() != Some(terminator) {
        return None;
    }

    // Check that the terminator is an unconditional branch.
    let terminator_op = terminator.borrow();
    let successor_br = terminator_op.downcast_ref::<Br>()?;

    // Check that the block arguments are only used by the terminator.
    for arg in succ.arguments().iter() {
        let arg = arg.borrow();
        for user in arg.iter_uses() {
            if user.owner != terminator {
                return None;
            }
        }
    }

    // Don't try to collapse branches to infinite loops.
    let target = successor_br.target();
    let successor_dest = target.successor();
    if successor_dest == successor {
        return None;
    }

    // A conditional bypass must replace exactly one incoming edge with exactly one edge.
    // The forwarding block can then be retired before splitting observes its stale outgoing edge.
    if predecessor.borrow().num_successors() > 1 {
        let mut incoming = succ.predecessors();
        let edge = incoming.next()?;
        if succ.is_entry_block()
            || edge.owner != predecessor
            || incoming.next().is_some()
            || successor_dest.borrow().get_single_predecessor() != Some(successor)
        {
            return None;
        }
    }

    // Update the operands to the successor. If the branch parent has no arguments, we can use the
    // branch operands directly.
    if !succ.has_arguments() {
        successor_operands.clear();
        successor_operands.extend(target.arguments.iter().map(|o| o.borrow().as_value_ref()));
        return Some(successor_dest);
    }

    // Otherwise, we need to remap any argument operands.
    let mut new_operands = SmallVec::default();
    for operand in target.arguments.iter() {
        let value = operand.borrow().as_value_ref();
        let operand = value.borrow();
        let block_arg = operand.downcast_ref::<BlockArgument>();
        match block_arg {
            Some(block_arg) if block_arg.owner() == successor => {
                new_operands.push(successor_operands[block_arg.index()]);
            }
            _ => {
                new_operands.push(value);
            }
        }
    }

    *successor_operands = new_operands;

    Some(successor_dest)
}

/// Retire only a forwarding block whose explicit uses have all been replaced. Entry activation
/// keeps an entry live independently of its explicit incoming edges.
pub(super) fn retire_unused_forwarding_block(block: BlockRef, rewriter: &mut dyn Rewriter) {
    let can_retire = {
        let block = block.borrow();
        !block.is_entry_block() && !block.is_used()
    };
    if can_retire {
        rewriter.erase_block(block);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec::Vec};
    use core::cell::Cell;

    use midenc_hir::{
        diagnostics::Uri,
        dialects::builtin::{Function, FunctionRef},
        parse::{self, ParserConfig},
        patterns::{RegionSimplificationLevel, RewriterImpl, RewriterListener},
    };

    use super::*;
    use crate::canonicalization::{
        SimplifyPassthroughCondBr, SplitCriticalEdges, testing::BorrowingListener,
    };

    struct RetirementListener {
        erased: Rc<Cell<usize>>,
    }

    impl Listener for RetirementListener {
        fn kind(&self) -> ListenerType {
            ListenerType::Rewriter
        }

        fn notify_operation_inserted(&self, op: OperationRef, prev: ProgramPoint) {
            BorrowingListener.notify_operation_inserted(op, prev);
        }
    }

    impl RewriterListener for RetirementListener {
        fn notify_operation_modified(&self, op: OperationRef) {
            BorrowingListener.notify_operation_modified(op);
        }

        fn notify_block_erased(&self, block: BlockRef) {
            let block = block.borrow();
            assert!(!block.is_entry_block());
            assert!(!block.is_used());
            for neighbor in block.parent().unwrap().borrow().body().iter() {
                for op in neighbor.body().iter() {
                    let _ = op.as_operation_ref().borrow();
                }
            }
            self.erased.set(self.erased.get() + 1);
        }
    }

    fn parse(context: &Rc<Context>, source: &str) -> Result<FunctionRef, Report> {
        parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("passthrough.hir"),
            source,
        )
    }

    fn rewrite(context: &Rc<Context>, branch: OperationRef) -> Result<bool, Report> {
        let region = branch.parent_region().unwrap();
        let before = region.borrow().body().iter().count();
        let erased = Rc::new(Cell::new(0));
        let rewriter: RewriterImpl = RewriterImpl::new(context.clone());
        let mut rewriter = rewriter.with_listener(RetirementListener {
            erased: erased.clone(),
        });
        rewriter.set_insertion_point_before(branch);
        let conditional = branch.borrow().is::<CondBr>();
        let changed = if conditional {
            SimplifyPassthroughCondBr::new(context.clone())
                .match_and_rewrite(branch, &mut rewriter)?
        } else {
            SimplifyPassthroughBr::new(context.clone()).match_and_rewrite(branch, &mut rewriter)?
        };
        assert_eq!(before - region.borrow().body().iter().count(), erased.get());
        Ok(changed)
    }

    #[test]
    fn passthrough_pair_converges_with_zero_and_ordered_mixed_arguments() -> Result<(), Report> {
        for mixed in [false, true] {
            let context = Rc::new(Context::default());
            let source = if mixed {
                r#"builtin.function public extern("C") @chain(%c: i1, %a: u32, %b: u64, %d: u32) -> (u32, u64, u32) {
                cf.cond_br %c ^exit, ^forward(%a, %b, %d : u32, u64, u32) : (i1);
            ^forward(%x: u32, %y: u64, %z: u32): cf.br ^join(%z, %y, %x : u32, u64, u32);
            ^exit: builtin.ret %a, %b, %d : (u32, u64, u32);
            ^join(%p: u32, %q: u64, %r: u32): builtin.ret %p, %q, %r : (u32, u64, u32);
            };"#
            } else {
                r#"builtin.function public extern("C") @chain(%c: i1, %a: u32, %b: u64) -> (u32, u64) {
                cf.cond_br %c ^exit, ^forward : (i1);
            ^forward: cf.br ^join;
            ^exit: builtin.ret %a, %b : (u32, u64);
            ^join: builtin.ret %a, %b : (u32, u64);
            };"#
            };
            let function = parse(&context, source)?;
            let entry = function.borrow().body().entry_block_ref().unwrap();
            let branch = entry.borrow().terminator().unwrap();
            let forward = branch.borrow().successor(1).successor();
            let target = forward.borrow().terminator().unwrap().borrow().successor(0).successor();
            let expected = if mixed {
                let args =
                    entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>();
                alloc::vec![args[3], args[2], args[1]]
            } else {
                Vec::new()
            };
            assert!(rewrite(&context, branch)?);
            assert!(
                forward.parent().is_none(),
                "the obsolete forwarding edge must be retired atomically"
            );
            let branch = entry.borrow().terminator().unwrap();
            assert_eq!(branch.borrow().successor(1).successor(), target);
            assert_eq!(
                branch
                    .borrow()
                    .successor(1)
                    .arguments
                    .iter()
                    .map(|o| o.borrow().as_value_ref())
                    .collect::<Vec<_>>(),
                expected
            );
            let rewriter: RewriterImpl = RewriterImpl::new(context.clone());
            let mut rewriter = rewriter.with_listener(BorrowingListener);
            rewriter.set_insertion_point_before(branch);
            let splitter = SplitCriticalEdges::for_op(context.clone(), branch.borrow().name());
            assert!(!splitter.match_and_rewrite(branch, &mut rewriter)?);
            let region = function.borrow().body().as_region_ref();
            let _ =
                Region::simplify_all(&[region], &mut rewriter, RegionSimplificationLevel::Normal);
            function.as_operation_ref().borrow().recursively_verify()?;
            let stable = format!("{}", function.as_operation_ref().borrow());
            assert!(!rewrite(&context, entry.borrow().terminator().unwrap())?);
            let branch = entry.borrow().terminator().unwrap();
            assert!(!splitter.match_and_rewrite(branch, &mut rewriter)?);
            assert_eq!(format!("{}", function.as_operation_ref().borrow()), stable);
        }
        Ok(())
    }

    #[test]
    fn passthrough_conditional_refuses_shared_duplicate_and_converging_forwarders()
    -> Result<(), Report> {
        let sources = [
            r#"builtin.function public extern("C") @shared(%c: i1, %a: u32, %b: u64, %d: u32) -> (u32, u64) {
                cf.cond_br %c ^forward(%a, %b : u32, u64), ^other : (i1);
            ^other: cf.br ^forward(%d, %b : u32, u64);
            ^forward(%x: u32, %y: u64): cf.br ^join(%x, %y : u32, u64);
            ^join(%p: u32, %q: u64): builtin.ret %p, %q : (u32, u64);
            };"#,
            r#"builtin.function public extern("C") @duplicate(%c: i1, %a: u32, %b: u64, %d: u32) -> (u32, u64) {
                cf.cond_br %c ^forward(%a, %b : u32, u64), ^forward(%d, %b : u32, u64) : (i1);
            ^forward(%x: u32, %y: u64): cf.br ^join(%x, %y : u32, u64);
            ^join(%p: u32, %q: u64): builtin.ret %p, %q : (u32, u64);
            };"#,
            r#"builtin.function public extern("C") @converging(%c: i1, %a: u32, %b: u64, %d: u32) -> (u32, u64) {
                cf.cond_br %c ^left, ^right : (i1);
            ^left: cf.br ^join(%a, %b : u32, u64);
            ^right: cf.br ^join(%d, %b : u32, u64);
            ^join(%p: u32, %q: u64): builtin.ret %p, %q : (u32, u64);
            };"#,
        ];
        for source in sources {
            let context = Rc::new(Context::default());
            let function = parse(&context, source)?;
            let entry = function.borrow().body().entry_block_ref().unwrap();
            let branch = entry.borrow().terminator().unwrap();
            let original = format!("{}", function.as_operation_ref().borrow());
            let destinations = (0..2)
                .map(|i| {
                    let branch = branch.borrow();
                    let target = branch.successor(i);
                    let dest = target.successor();
                    let args = dest.borrow().arguments().to_vec();
                    let values = target
                        .arguments
                        .iter()
                        .map(|o| o.borrow().as_value_ref())
                        .collect::<Vec<_>>();
                    (dest, args, values)
                })
                .collect::<Vec<_>>();
            assert!(!rewrite(&context, branch)?);
            assert_eq!(format!("{}", function.as_operation_ref().borrow()), original);
            for (i, (dest, args, values)) in destinations.into_iter().enumerate() {
                assert_eq!(branch.borrow().successor(i).successor(), dest);
                assert!(dest.parent().is_some());
                assert_eq!(dest.borrow().arguments(), args.as_slice());
                assert_eq!(
                    branch
                        .borrow()
                        .successor(i)
                        .arguments
                        .iter()
                        .map(|o| o.borrow().as_value_ref())
                        .collect::<Vec<_>>(),
                    values
                );
            }
            function.as_operation_ref().borrow().recursively_verify()?;
        }
        Ok(())
    }

    #[test]
    fn passthrough_unconditional_preserves_shared_forwarder_and_ordered_values()
    -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse(
            &context,
            r#"builtin.function public extern("C") @shared(%c: i1, %a: u32, %b: u64, %d: u32) -> (u64, u32) {
            cf.cond_br %c ^left, ^right : (i1);
        ^left: cf.br ^forward(%a, %b : u32, u64);
        ^right: cf.br ^forward(%d, %b : u32, u64);
        ^forward(%x: u32, %y: u64): cf.br ^join(%y, %x : u64, u32);
        ^join(%p: u64, %q: u32): builtin.ret %p, %q : (u64, u32);
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let entry_branch = entry.borrow().terminator().unwrap();
        let left = entry_branch.borrow().successor(0).successor();
        let right = entry_branch.borrow().successor(1).successor();
        let branch = left.borrow().terminator().unwrap();
        let right_branch = right.borrow().terminator().unwrap();
        let forward = branch.borrow().successor(0).successor();
        let original_args = forward.borrow().arguments().to_vec();
        let right_operands = right_branch
            .borrow()
            .successor(0)
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<Vec<_>>();
        let target = forward.borrow().terminator().unwrap().borrow().successor(0).successor();
        let expected = branch
            .borrow()
            .successor(0)
            .arguments
            .iter()
            .rev()
            .map(|o| o.borrow().as_value_ref())
            .collect::<Vec<_>>();
        assert!(rewrite(&context, branch)?);
        let rewritten = left.borrow().terminator().unwrap();
        assert_eq!(rewritten.borrow().successor(0).successor(), target);
        assert_eq!(
            rewritten
                .borrow()
                .successor(0)
                .arguments
                .iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(forward.parent().is_some());
        assert_eq!(forward.borrow().arguments(), original_args.as_slice());
        assert_eq!(right_branch.borrow().successor(0).successor(), forward);
        assert_eq!(
            right_branch
                .borrow()
                .successor(0)
                .arguments
                .iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<Vec<_>>(),
            right_operands
        );
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn passthrough_unconditional_retires_exclusive_forwarder() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse(
            &context,
            r#"builtin.function public extern("C") @chain(%a: u32, %b: u64) -> (u64, u32) {
            cf.br ^forward(%a, %b : u32, u64);
        ^forward(%x: u32, %y: u64): cf.br ^join(%y, %x : u64, u32);
        ^join(%p: u64, %q: u32): builtin.ret %p, %q : (u64, u32);
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let branch = entry.borrow().terminator().unwrap();
        let forward = branch.borrow().successor(0).successor();
        assert!(rewrite(&context, branch)?);
        assert!(forward.parent().is_none());
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn passthrough_never_retires_initial_entry() -> Result<(), Report> {
        for conditional in [false, true] {
            let context = Rc::new(Context::default());
            let source = r#"builtin.function public extern("C") @entry(%a: u32, %b: u64) -> (u32, u64) {
                cf.br ^join(%a, %b : u32, u64);
            ^join(%x: u32, %y: u64): builtin.ret %x, %y : (u32, u64);
            ^source:
                %cond = arith.constant 1 : i1;
                %a2 = arith.constant 13 : u32;
                %b2 = arith.constant 29 : u64;
                cf.br ^placeholder(%a2, %b2 : u32, u64);
            ^placeholder(%p: u32, %q: u64): builtin.ret %p, %q : (u32, u64);
            };"#;
            let source = if conditional {
                source.replace(
                    "cf.br ^placeholder(%a2, %b2 : u32, u64);",
                    "cf.cond_br %cond ^placeholder(%a2, %b2 : u32, u64), ^exit : (i1);
^exit: builtin.ret %a2, %b2 : (u32, u64);",
                )
            } else {
                source.into()
            };
            let function = parse(&context, &source)?;
            let entry = function.borrow().body().entry_block_ref().unwrap();
            let args = entry.borrow().arguments().to_vec();
            let target = entry.borrow().terminator().unwrap().borrow().successor(0).successor();
            let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
            let source = placeholder.borrow().get_single_predecessor().unwrap();
            let mut branch = source.borrow().terminator().unwrap();
            let incoming = branch
                .borrow()
                .successor(0)
                .arguments
                .iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<Vec<_>>();
            branch.borrow_mut().successor_mut(0).set(entry);
            placeholder.borrow_mut().erase();
            function.as_operation_ref().borrow().recursively_verify()?;
            assert_eq!(rewrite(&context, branch)?, !conditional);
            assert_eq!(entry.borrow().is_used(), conditional);
            assert_eq!(function.borrow().body().entry_block_ref(), Some(entry));
            assert_eq!(entry.borrow().arguments(), args.as_slice());
            let branch = source.borrow().terminator().unwrap();
            assert_eq!(
                branch.borrow().successor(0).successor(),
                if conditional { entry } else { target }
            );
            assert_eq!(
                branch
                    .borrow()
                    .successor(0)
                    .arguments
                    .iter()
                    .map(|o| o.borrow().as_value_ref())
                    .collect::<Vec<_>>(),
                incoming
            );
            function.as_operation_ref().borrow().recursively_verify()?;
        }
        Ok(())
    }
}
