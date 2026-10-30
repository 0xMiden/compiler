//! Arithmetic costs derived from the backend's own MASM lowerings.
//!
//! These are minimum-path VM-operation estimates, not whole-program cycle predictions.
//! Final stack scheduling, control-flow clocks, and padding are checked by execution benchmarks.

use alloc::{collections::BTreeSet, rc::Rc, sync::Arc, vec::Vec};

use miden_assembly::{Assembler, Linkage, utils::Idx};
use miden_core::mast::{MastForest, MastNode, MastNodeId};
use miden_mast_package::Package;
use midenc_hir::{Context, Overflow, SourceSpan, Type, formatter::PrettyPrint};
use midenc_session::diagnostics::Report;

use crate::{OperandStack, emit::OpEmitter, masm};

#[derive(Clone, Copy)]
pub(crate) struct MasmArithmeticCostModel {
    signed_wrapping_mul: usize,
    unsigned_wrapping_mul: usize,
}

impl MasmArithmeticCostModel {
    pub fn load(context: Rc<Context>) -> Result<Option<Self>, Report> {
        let intrinsics = crate::intrinsics::load(context.session())?;
        let signed = wrapping_mul_cost(context.clone(), Type::I32, intrinsics.clone())?;
        let unsigned = wrapping_mul_cost(context, Type::U32, intrinsics)?;
        let (Some(signed_wrapping_mul), Some(unsigned_wrapping_mul)) = (signed, unsigned) else {
            return Ok(None);
        };
        Ok(Some(Self {
            signed_wrapping_mul,
            unsigned_wrapping_mul,
        }))
    }

    pub fn wrapping_mul_savings(&self) -> Option<usize> {
        self.signed_wrapping_mul
            .checked_sub(self.unsigned_wrapping_mul)
            .filter(|savings| *savings > 0)
    }
}

fn wrapping_mul_cost(
    context: Rc<Context>,
    ty: Type,
    intrinsics: Arc<Package>,
) -> Result<Option<usize>, Report> {
    let mut stack = OperandStack::new(context.clone());
    let mut invoked = BTreeSet::new();
    let mut instructions = Vec::new();
    {
        let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
        emitter.push(ty.clone());
        emitter.push(ty);
        emitter.mul(Overflow::Wrapping, SourceSpan::UNKNOWN);
    }
    let body = masm::Block::new(SourceSpan::UNKNOWN, instructions).to_pretty_string();
    let mut assembler = Assembler::default();
    assembler.link_package(intrinsics, Linkage::Static)?;
    let package = assembler.assemble_program("arithmetic_cost", format!("begin{body}\nend"))?;
    let program = package.try_into_program()?;
    Ok(minimum_operations(program.mast_forest(), program.entrypoint()))
}

/// Count minimum-path VM operations in finalized child-before-parent MAST order.
fn minimum_operations(forest: &MastForest, root: MastNodeId) -> Option<usize> {
    let mut costs: Vec<Option<usize>> = Vec::with_capacity(forest.nodes().len());
    for node in forest.nodes() {
        let child_cost = |id: MastNodeId| costs.get(id.to_usize()).copied().flatten();
        let cost = match node {
            MastNode::Block(block) => Some(block.raw_operations().count()),
            MastNode::Join(node) => child_cost(node.first())
                .zip(child_cost(node.second()))
                .and_then(|(first, second)| first.checked_add(second)),
            MastNode::Split(node) => child_cost(node.on_true())
                .zip(child_cost(node.on_false()))
                .map(|(on_true, on_false)| on_true.min(on_false)),
            MastNode::Call(node) => child_cost(node.callee()),
            MastNode::Loop(_) => Some(0),
            MastNode::Dyn(_) | MastNode::External(_) => None,
        };
        costs.push(cost);
    }
    costs.get(root.to_usize()).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_profitable_costs_do_not_authorize_rewrites() {
        for costs in [(3, 3), (2, 3)] {
            let model = MasmArithmeticCostModel {
                signed_wrapping_mul: costs.0,
                unsigned_wrapping_mul: costs.1,
            };
            assert_eq!(model.wrapping_mul_savings(), None);
        }
        let model = MasmArithmeticCostModel {
            signed_wrapping_mul: 10,
            unsigned_wrapping_mul: 3,
        };
        assert_eq!(model.wrapping_mul_savings(), Some(7));
    }

    #[test]
    fn costs_follow_the_taken_path_and_count_shared_uses() {
        use miden_core::{
            mast::{
                BasicBlockNodeBuilder, DenseMastForestBuilder, JoinNodeBuilder, SplitNodeBuilder,
            },
            operations::Operation,
        };

        let mut builder = DenseMastForestBuilder::new();
        let shared = builder
            .push_node(BasicBlockNodeBuilder::new(alloc::vec![Operation::Incr, Operation::Neg]))
            .unwrap();
        let twice = builder.push_node(JoinNodeBuilder::new([shared, shared])).unwrap();
        let split = builder.push_node(SplitNodeBuilder::new([shared, twice])).unwrap();
        let root = builder.push_node(JoinNodeBuilder::new([shared, split])).unwrap();
        builder.mark_root(root);
        let (forest, remapping) = builder.build_with_id_map().unwrap();
        assert_eq!(minimum_operations(&forest, remapping.get(root).unwrap()), Some(4));
    }

    #[test]
    fn dynamic_costs_are_unknown_instead_of_assumed_free() -> Result<(), Report> {
        let package = Assembler::default().assemble_program("dynamic", "begin dynexec end")?;
        let program = package.try_into_program()?;
        assert_eq!(minimum_operations(program.mast_forest(), program.entrypoint()), None);
        Ok(())
    }
}
