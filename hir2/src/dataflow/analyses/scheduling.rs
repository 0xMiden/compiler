//mod depgraph;
//mod treegraph;

use super::LivenessAnalysis;
use crate::{dominance::DominanceTree, BlockArgument, OpOperandImpl, ValueRef};

/// The intent of the scheduling analysis is to provide the following:
///
/// 1. For a given instruction, for each of its operands, dictate whether that operand is the
///    last use of that operand in the program, or if there are further uses.
pub struct Scheduling {
    tree: DominanceTree,
    liveness: LivenessAnalysis,
}

impl Scheduling {
    pub fn new(tree: DominanceTree, liveness: LivenessAnalysis) -> Self {
        Self { tree, liveness }
    }

    /// Returns true if `operand` is the last use of the value it references in the current function
    ///
    /// The definition of "last use" is tied to the way in which codegen/scheduling is performed.
    /// In particular, operands of a given operation are materialized in reverse order, so that
    /// the first operand of an operation is on top of the operand stack when lowered to Miden.
    /// Thus, the "last use" of an operand that is used multiple times by the same operation, will
    /// be whichever one has the lowest index (i.e. appears first in the argument list).
    ///
    /// Beyond that, last use is closely related to dominance. Specifically, the last use of an
    /// operand must post-dominate all other uses which are reachable via the containing block.
    ///
    /// ## Example
    ///
    /// A conditional branch that escapes the function on each branch. Here, there are two last
    /// uses, because each post-dominates all other uses that reach the same exit. Further, this
    /// also demonstrates how two uses by the same operation are interpreted in terms of the
    /// order in which they are used:
    ///
    /// ```text
    /// a:
    ///   v0 = ..
    ///   v1 = ..
    ///   condbr v0, b, c
    ///
    /// b:
    ///   foo v1
    ///   v2 = bar v1, v1   # `bar <last use>, v1`
    ///   ret v2
    ///
    /// c:
    ///   ret v1            # last use
    /// ```text
    ///
    ///
    pub fn is_last_use(&self, operand: &OpOperandImpl) -> bool {
        let value = operand.value();
        let user = operand.owner();
        // If `value` has additional uses used by `user`
        if let Some(defined_by) = value.get_defining_op() {
            todo!()
        } else {
            let block_arg = value.downcast_ref::<BlockArgument>().unwrap();
            todo!()
        }
    }
}
