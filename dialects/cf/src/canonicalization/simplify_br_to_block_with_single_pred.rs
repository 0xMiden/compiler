use alloc::rc::Rc;

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Simplify a branch to a block that has a single predecessor. This effectively merges the two
/// blocks.
pub struct SimplifyBrToBlockWithSinglePred {
    info: PatternInfo,
}

impl SimplifyBrToBlockWithSinglePred {
    pub fn new(context: Rc<Context>) -> Self {
        let cf_dialect = context.get_or_register_dialect::<ControlFlowDialect>();
        let br_op = cf_dialect.registered_name::<Br>().expect("cf.br is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "simplify-br-to-block-with-single-predecessor",
                PatternKind::Operation(br_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyBrToBlockWithSinglePred {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyBrToBlockWithSinglePred {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = operation.borrow();
        let Some(br_op) = op.downcast_ref::<Br>() else {
            return Ok(false);
        };

        // Check that the successor block has a single predecessor.
        let target = br_op.target();
        let succ = target.successor();
        let parent = op.parent().unwrap();
        // Entry activation supplies arguments independently of explicit CFG predecessors.
        if succ == parent
            || succ.borrow().is_entry_block()
            || succ.borrow().get_single_predecessor().is_none()
        {
            return Ok(false);
        }

        // Merge the successor into the current block and erase the branch.
        let operands = target
            .arguments
            .iter()
            .map(|o| Some(o.borrow().as_value_ref()))
            .collect::<SmallVec<[_; 4]>>();

        drop(op);
        rewriter.erase_op(operation);
        rewriter.merge_blocks(succ, parent, &operands);

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use midenc_hir::{
        diagnostics::Uri,
        dialects::builtin::Function,
        parse::{self, ParserConfig},
        patterns::RewriterImpl,
    };

    use super::*;

    #[test]
    fn canonical_entry_single_predecessor_preserves_initial_arguments() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("entry.hir"),
            r#"builtin.function public extern("C") @cycle(%a: u32, %b: u64) {
            cf.br ^again;
        ^again: cf.br ^placeholder(%a, %b : u32, u64);
        ^placeholder(%x: u32, %y: u64): builtin.ret;
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>();
        let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
        let mut branch = placeholder
            .borrow()
            .get_single_predecessor()
            .unwrap()
            .borrow()
            .terminator()
            .unwrap();
        branch.borrow_mut().successor_mut(0).set(entry);
        placeholder.borrow_mut().erase();
        let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
        assert!(
            !SimplifyBrToBlockWithSinglePred::new(context)
                .match_and_rewrite(branch, &mut rewriter)?
        );
        assert!(entry.borrow().is_entry_block());
        assert_eq!(
            entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>(),
            args
        );
        Ok(())
    }

    #[test]
    fn canonical_entry_conditional_preserves_initial_arguments() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("entry.hir"),
            r#"builtin.function public extern("C") @cycle(%c: i1, %a: u32, %b: u64) {
            cf.br ^again;
        ^again: cf.cond_br %c ^placeholder(%c, %a, %b : i1, u32, u64), ^exit : (i1);
        ^exit: builtin.ret;
        ^placeholder(%x: i1, %y: u32, %z: u64): builtin.ret;
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>();
        let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
        let mut branch = placeholder
            .borrow()
            .get_single_predecessor()
            .unwrap()
            .borrow()
            .terminator()
            .unwrap();
        branch.borrow_mut().successor_mut(0).set(entry);
        placeholder.borrow_mut().erase();
        let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
        assert!(
            !crate::canonicalization::RemoveUnusedSinglePredBlockArgs::new(context)
                .match_and_rewrite(branch, &mut rewriter)?
        );
        assert_eq!(
            entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>(),
            args
        );
        Ok(())
    }

    #[test]
    fn canonical_entry_return_is_copied_without_erasing_entry() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("entry.hir"),
            r#"builtin.function public extern("C") @cycle(%a: u32, %b: u64) -> (u32, u64) {
            builtin.ret %a, %b : (u32, u64);
        ^again:
            %incoming_a = arith.constant 13 : u32;
            %incoming_b = arith.constant 29 : u64;
            cf.br ^placeholder(%incoming_a, %incoming_b : u32, u64);
        ^placeholder(%x: u32, %y: u64): builtin.ret %x, %y : (u32, u64);
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>();
        let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
        let mut branch = placeholder
            .borrow()
            .get_single_predecessor()
            .unwrap()
            .borrow()
            .terminator()
            .unwrap();
        let parent = branch.parent().unwrap();
        let incoming = branch
            .borrow()
            .successor(0)
            .arguments
            .iter()
            .map(|operand| operand.borrow().as_value_ref())
            .collect::<Vec<_>>();
        assert_ne!(incoming, args);
        branch.borrow_mut().successor_mut(0).set(entry);
        placeholder.borrow_mut().erase();
        let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
        rewriter.set_insertion_point_before(branch);
        assert!(
            crate::canonicalization::SimplifyBrToReturn::new(context)
                .match_and_rewrite(branch, &mut rewriter)?
        );
        assert!(entry.borrow().is_entry_block());
        assert_eq!(
            entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>(),
            args
        );
        let ret = parent.borrow().terminator().unwrap();
        assert!(ret.borrow().is::<midenc_hir::dialects::builtin::Ret>());
        assert_eq!(
            ret.borrow().operands().all().as_value_range().into_iter().collect::<Vec<_>>(),
            incoming
        );
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn canonical_entry_critical_edge_preserves_initial_interface() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = parse::parse::<Function>(
            ParserConfig::new(context.clone()),
            Uri::new("critical-entry.hir"),
            r#"builtin.function public extern("C") @cycle(%c: i1, %a: u32, %b: u64) -> (u32, u64) {
            cf.cond_br %c ^left, ^right : (i1);
        ^left: cf.cond_br %c ^placeholder(%c, %a, %b : i1, u32, u64), ^exit : (i1);
        ^right: cf.cond_br %c ^placeholder(%c, %a, %b : i1, u32, u64), ^exit : (i1);
        ^exit: builtin.ret %a, %b : (u32, u64);
        ^placeholder(%x: i1, %y: u32, %z: u64): builtin.ret %y, %z : (u32, u64);
        };"#,
        )?;
        let entry = function.borrow().body().entry_block_ref().unwrap();
        let args = entry.borrow().arguments().to_vec();
        let mut placeholder = function.borrow().body().body().back().as_pointer().unwrap();
        let mut branches = Vec::new();
        for block in function.borrow().body().body().iter() {
            let Some(mut op) = block.terminator() else {
                continue;
            };
            let num_successors = op.borrow().num_successors();
            for i in 0..num_successors {
                if op.borrow().successor(i).successor() == placeholder {
                    op.borrow_mut().successor_mut(i).set(entry);
                    branches.push(op);
                }
            }
        }
        placeholder.borrow_mut().erase();
        function.as_operation_ref().borrow().recursively_verify()?;
        let branch = branches[0];
        let original_successors = {
            let op = branch.borrow();
            (0..op.num_successors())
                .map(|index| {
                    let target = op.successor(index);
                    (
                        target.successor(),
                        target
                            .arguments
                            .iter()
                            .map(|operand| operand.borrow().as_value_ref())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
        rewriter.set_insertion_point_before(branch);
        let pattern =
            crate::canonicalization::SplitCriticalEdges::for_op(context, branch.borrow().name());
        assert!(pattern.match_and_rewrite(branch, &mut rewriter)?);
        for (index, (original_dest, original_args)) in original_successors.into_iter().enumerate() {
            let split = branch.borrow().successor(index).successor();
            assert_ne!(split, original_dest);
            assert!(branch.borrow().successor(index).arguments.is_empty());
            assert!(split.borrow().arguments().is_empty());
            assert_eq!(split.borrow().get_single_predecessor(), branch.parent());
            let forwarded = split.borrow().terminator().unwrap();
            let forwarded = forwarded.borrow();
            assert!(forwarded.is::<Br>());
            assert_eq!(forwarded.successor(0).successor(), original_dest);
            assert_eq!(
                forwarded
                    .successor(0)
                    .arguments
                    .iter()
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<Vec<_>>(),
                original_args
            );
        }
        assert_eq!(function.borrow().body().entry_block_ref(), Some(entry));
        assert_eq!(entry.borrow().arguments(), args.as_slice());
        function.as_operation_ref().borrow().recursively_verify()?;
        Ok(())
    }

    #[test]
    fn canonical_nonentry_branch_and_return_still_merge() -> Result<(), Report> {
        for return_pattern in [false, true] {
            let context = Rc::new(Context::default());
            let function = parse::parse::<Function>(
                ParserConfig::new(context.clone()),
                Uri::new("nonentry.hir"),
                r#"builtin.function public extern("C") @forward(%a: u32, %b: u64) -> (u32, u64) {
                cf.br ^exit(%a, %b : u32, u64);
            ^exit(%x: u32, %y: u64): builtin.ret %x, %y : (u32, u64);
            };"#,
            )?;
            let entry = function.borrow().body().entry_block_ref().unwrap();
            let args = entry.borrow().arguments().as_value_range().into_iter().collect::<Vec<_>>();
            let branch = entry.borrow().terminator().unwrap();
            let mut rewriter: RewriterImpl = RewriterImpl::new(context.clone());
            rewriter.set_insertion_point_before(branch);
            let changed = if return_pattern {
                crate::canonicalization::SimplifyBrToReturn::new(context)
                    .match_and_rewrite(branch, &mut rewriter)?
            } else {
                SimplifyBrToBlockWithSinglePred::new(context)
                    .match_and_rewrite(branch, &mut rewriter)?
            };
            assert!(changed);
            assert_eq!(function.borrow().body().body().iter().count(), 1);
            let ret = entry.borrow().terminator().unwrap();
            assert!(ret.borrow().is::<midenc_hir::dialects::builtin::Ret>());
            assert_eq!(
                ret.borrow().operands().all().as_value_range().into_iter().collect::<Vec<_>>(),
                args
            );
            function.as_operation_ref().borrow().recursively_verify()?;
        }
        Ok(())
    }
}
