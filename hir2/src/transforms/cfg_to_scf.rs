//! This code is an implementation of the algorithm described in _Perfect Reconstructability of
//! Control Flow from Demand Dependence Graphs_, by Bahmann, Reismann, Jahre, and Meyer. 2015.
//! See https://doi.org/10.1145/2693261.
//!
//! It defines an algorithm to translate any control flow graph with a single entry and single exit
//! block into structured control flow operations consisting of regions of do-while loops and
//! operations conditionally dispatching to one out of multiple regions before continuing after the
//! operation. This includes control flow graphs containing irreducible control flow.
//!
//! The implementation here additionally supports the transformation on regions with multiple exit
//! blocks. This is implemented by first transforming all occurrences of return-like operations to
//! branch to a single exit block containing an instance of that return-like operation. If there are
//! multiple kinds of return-like operations, multiple exit blocks are created. In that case the
//! transformation leaves behind a conditional control flow graph operation that dispatches to the
//! given regions terminating with different kinds of return-like operations each.
//!
//! If the function only contains a single kind of return-like operations, it is guaranteed that all
//! control flow graph ops will be lifted to structured control flow, and that no more control flow
//! graph ops remain after the operation.
//!
//! The algorithm to lift CFGs consists of two transformations applied after each other on any
//! single-entry, single-exit region:
//!
//! 1. Lifting cycles to structured control flow loops
//! 2. Lifting conditional branches to structured control flow branches
//!
//! These are then applied recursively on any new single-entry single-exit regions created by the
//! transformation until no more CFG operations remain.
//!
//! The first part of cycle lifting is to detect any cycles in the CFG. This is done using an
//! algorithm for iterating over SCCs. Every SCC representing a cycle is then transformed into a
//! structured loop with a single entry block and a single latch containing the only back edge to
//! the entry block and the only edge to an exit block outside the loop. Rerouting control flow to
//! create single entry and exit blocks is achieved via a multiplexer construct that can be
//! visualized as follows:
//!
//! ```text,ignore
//! +-----+ +-----+   +-----+
//! | bb0 | | bb1 |...| bbN |
//! +--+--+ +--+--+   +-+---+
//!    |       |        |
//!    |       v        |
//!    |  +------+      |
//!    | ++      ++<----+
//!    | | Region |
//!    +>|        |<----+
//!      ++      ++     |
//!       +------+------+
//! ```
//!
//! The above transforms to:
//!
//! ```text,ignore
//! +-----+ +-----+   +-----+
//! | bb0 | | bb1 |...| bbN |
//! +-----+ +--|--+   ++----+
//!      |     v       |
//!      +->+-----+<---+
//!         | bbM |<-------+
//!         +---+-+        |
//!     +---+   | +----+   |
//!     |       v      |   |
//!     |   +------+   |   |
//!     |  ++      ++<-+   |
//!     +->| Region |      |
//!        ++      ++      |
//!         +------+-------+
//! ```
//!
//! bbM in the above is the multiplexer block, and any block previously branching to an entry block
//! of the region are redirected to it. This includes any branches from within the region. Using a
//! block argument, bbM then dispatches to the correct entry block of the region dependent on the
//! predecessor.
//!
//! A similar transformation is done to create the latch block with the single back edge and loop
//! exit edge.
//!
//! The above form has the advantage that bbM now acts as the loop header of the loop body. After
//! the transformation on the latch, this results in a structured loop that can then be lifted to
//! structured control flow. The conditional branches created in bbM are later lifted to conditional
//! branches.
//!
//! Lifting conditional branches is done by analyzing the *first* conditional branch encountered in
//! the entry region. The algorithm then identifies all blocks that are dominated by a specific
//! control flow edge and the region where control flow continues:
//!
//! ```text,ignore
//!                  +-----+
//!            +-----+ bb0 +----+
//!            v     +-----+    v
//! Region 1 +-+-+    ...     +-+-+ Region n
//!          +---+            +---+
//!           ...              ...
//!            |                |
//!            |      +---+     |
//!            +---->++   ++<---+
//!                  |     |
//!                  ++   ++ Region T
//!                   +---+
//! ```
//!
//! Every region following bb0 consists of 0 or more blocks that eventually branch to Region T. If
//! there are multiple entry blocks into Region T, a single entry block is created using a
//! multiplexer block as shown above. Region 1 to Region n are then lifted together with the
//! conditional control flow operation terminating bb0 into a structured conditional operation
//! followed by the operations of the entry block of Region T.
use alloc::rc::Rc;

use smallvec::{smallvec, SmallVec};

use crate::{
    adt::{SmallMap, SmallSet},
    dominance::{DominanceInfo, PreOrderDomTreeIter},
    traits::BranchOpInterface,
    Block, BlockRef, Builder, Context, EntityWithParent, FxHashMap, OpBuilder, OpOperand,
    Operation, OperationRef, Region, RegionRef, Report, SourceSpan, Spanned, Type, Usable, Value,
    ValueRef, WalkResult,
};

/// Interface that should be implemented by any caller of `transformCFGToSCF`.
/// The transformation requires the caller to 1) create switch-like control
/// flow operations for intermediate transformations and 2) to create
/// the desired structured control flow ops.
pub trait CFGToSCFInterface {
    /// Creates a structured control flow operation branching to one of `regions`.
    /// It replaces `controlFlowCondOp` and must have `resultTypes` as results.
    /// `regions` contains the list of branch regions corresponding to each
    /// successor of `controlFlowCondOp`. Their bodies must simply be taken and
    /// left as is.
    /// Returns failure if incapable of converting the control flow graph
    /// operation.
    fn create_structured_branch_region_op(
        &self,
        builder: &mut OpBuilder,
        control_flow_cond_op: OperationRef,
        result_types: &[Type],
        regions: &mut SmallVec<[RegionRef; 2]>,
    ) -> Result<OperationRef, Report>;

    /// Creates a return-like terminator for a branch region of the op returned
    /// by `createStructuredBranchRegionOp`. `branchRegionOp` is the operation
    /// returned by `createStructuredBranchRegionOp`.
    /// `replacedControlFlowOp` is the control flow op being replaced by the
    /// terminator or `None` if the terminator is not replacing any existing
    /// control flow op. `results` are the values that should be returned by the
    /// branch region.
    fn create_structured_branch_region_terminator_op(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        branch_region_op: OperationRef,
        replaced_control_flow_op: Option<OperationRef>,
        results: &[ValueRef],
    ) -> Result<(), Report>;

    /// Creates a structured control flow operation representing a do-while loop.
    /// The do-while loop is expected to have the exact same result types as the
    /// types of the iteration values.
    /// `loopBody` is the body of the loop. The implementation of this
    /// function must create a suitable terminator op at the end of the last block
    /// in `loopBody` which continues the loop if `condition` is 1 and exits the
    /// loop if 0. `loopValuesNextIter` are the values that have to be passed as
    /// the iteration values for the next iteration if continuing, or the result
    /// of the loop if exiting.
    /// `condition` is guaranteed to be of the same type as values returned by
    /// `getCFGSwitchValue` with either 0 or 1 as value.
    ///
    /// `loopValuesInit` are the values used to initialize the iteration
    /// values of the loop.
    /// Returns failure if incapable of creating a loop op.
    fn create_structured_do_while_loop_op(
        &self,
        builder: &mut OpBuilder,
        replaced_op: OperationRef,
        loop_values_init: &[ValueRef],
        condition: ValueRef,
        loop_values_next_iter: &[ValueRef],
        loop_body: RegionRef,
    ) -> Result<OperationRef, Report>;

    /// Creates a constant operation with a result representing `value` that is
    /// suitable as flag for `createCFGSwitchOp`.
    fn get_cfg_switch_value(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        value: u32,
    ) -> ValueRef;

    /// Creates a switch CFG branch operation branching to one of
    /// `caseDestinations` or `defaultDest`. This is used by the transformation
    /// for intermediate transformations before lifting to structured control
    /// flow. The switch op branches based on `flag` which is guaranteed to be of
    /// the same type as values returned by `getCFGSwitchValue`. The insertion
    /// block of the builder is guaranteed to have its predecessors already set
    /// to create an equivalent CFG after this operation.
    /// Note: `caseValues` and other related ranges may be empty to represent an
    /// unconditional branch.
    #[allow(clippy::too_many_arguments)]
    fn create_cfg_switch_op(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        flag: ValueRef,
        case_values: &[u32],
        case_destinations: &[BlockRef],
        case_arguments: &[&[ValueRef]],
        default_dest: BlockRef,
        default_args: &[ValueRef],
    ) -> Result<(), Report>;

    /// Creates a constant operation returning an undefined instance of `type`.
    /// This is required by the transformation as the lifting process might create
    /// control-flow paths where an SSA-value is undefined.
    fn get_undef_value(&self, span: SourceSpan, builder: &mut OpBuilder, ty: Type) -> ValueRef;

    /// Creates a return-like terminator indicating unreachable.
    /// This is required when the transformation encounters a statically known
    /// infinite loop. Since structured control flow ops are not terminators,
    /// after lifting an infinite loop, a terminator has to be placed after to
    /// possibly satisfy the terminator requirement of the region originally
    /// passed to `transformCFGToSCF`.
    ///
    /// `region` is guaranteed to be the region originally passed to
    /// `transformCFGToSCF` and the op is guaranteed to always be an op in a block
    /// directly nested under `region` after the transformation.
    ///
    /// Returns failure if incapable of creating an unreachable terminator.
    fn create_unreachable_terminator(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        region: RegionRef,
    ) -> Result<OperationRef, Report>;

    /// Helper function to create an unconditional branch using
    /// `createCFGSwitchOp`.
    fn create_single_destination_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        dummy_flag: ValueRef,
        destination: BlockRef,
        arguments: &[ValueRef],
    ) -> Result<(), Report> {
        self.create_cfg_switch_op(span, builder, dummy_flag, &[], &[], &[], destination, arguments)
    }

    /// Helper function to create a conditional branch using
    /// `createCFGSwitchOp`.
    #[allow(clippy::too_many_arguments)]
    fn create_conditional_branch(
        &self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        condition: ValueRef,
        true_dest: BlockRef,
        true_args: &[ValueRef],
        false_dest: BlockRef,
        false_args: &[ValueRef],
    ) -> Result<(), Report> {
        self.create_cfg_switch_op(
            span,
            builder,
            condition,
            &[0],
            &[false_dest],
            &[false_args],
            true_dest,
            true_args,
        )
    }
}

/// Appends all the block arguments from `other` to the block arguments of `block`, copying their
/// types and locations.
fn add_block_arguments_from_other(context: &Context, block: BlockRef, other: BlockRef) {
    let other = other.borrow();
    for arg in other.arguments() {
        let arg = arg.borrow();
        context.append_block_argument(block, arg.ty().clone(), arg.span());
    }
}

/// Type representing an edge in the CFG.
///
/// Consists of a from-block, a successor and corresponding successor operands passed to the block
/// arguments of the successor.
#[derive(Copy, Clone)]
struct Edge {
    from_block: BlockRef,
    successor_index: usize,
}

impl Edge {
    pub fn get_from_block(&self) -> BlockRef {
        self.from_block
    }

    pub fn get_successor(&self) -> BlockRef {
        let from_block = self.from_block.borrow();
        from_block.get_successor(self.successor_index)
    }

    /// Sets the successor of the edge, adjusting the terminator in the from-block.
    pub fn set_successor(&self, block: BlockRef) {
        let mut terminator = {
            let from_block = self.from_block.borrow();
            from_block.terminator().unwrap()
        };
        let mut terminator = terminator.borrow_mut();
        let mut succ = terminator.successor_mut(self.successor_index);
        succ.set(block);
    }
}

/// Structure containing the entry, exit and back edges of a cycle.
///
/// A cycle is a generalization of a loop that may have multiple entry edges. See also
/// https://llvm.org/docs/CycleTerminology.html.
#[derive(Default)]
struct CycleEdges {
    /// All edges from a block outside the cycle to a block inside the cycle.
    /// The targets of these edges are entry blocks.
    entry_edges: SmallVec<[Edge; 1]>,
    /// All edges from a block inside the cycle to a block outside the cycle.
    exit_edges: SmallVec<[Edge; 1]>,
    /// All edges from a block inside the cycle to an entry block.
    back_edges: SmallVec<[Edge; 1]>,
}

/// Typed used to orchestrate creation of so-called edge multiplexers.
///
/// This class creates a new basic block and routes all inputs edges to this basic block before
/// branching to their original target. The purpose of this transformation is to create single-entry,
/// single-exit regions.
struct EdgeMultiplexer<'multiplexer, 'context: 'multiplexer> {
    transform_ctx: &'multiplexer mut TransformationContext<'context>,
    /// Newly created multiplexer block.
    multiplexer_block: BlockRef,
    /// Mapping of the block arguments of an entry block to the corresponding block arguments in the
    /// multiplexer block. Block arguments of an entry block are simply appended ot the multiplexer
    /// block. This map simply contains the offset to the range in the multiplexer block.
    block_arg_mapping: SmallMap<BlockRef, usize, 4>,
    /// Discriminator value used in the multiplexer block to dispatch to the correct entry block.
    /// `None` if not required due to only having one entry block.
    discriminator: Option<ValueRef>,
}

impl<'multiplexer, 'context: 'multiplexer> EdgeMultiplexer<'multiplexer, 'context> {
    /// Creates a new edge multiplexer capable of redirecting all edges to one of the `entry_blocks`.
    ///
    /// This creates the multiplexer basic block with appropriate block arguments after the first
    /// entry block. `extra_args` contains the types of possible extra block arguments passed to the
    /// multiplexer block that are added to the successor operands of every outgoing edge.
    ///
    /// NOTE: This does not yet redirect edges to branch to the multiplexer block nor code
    /// dispatching from the multiplexer code to the original successors. See [Self::redirect_edge]
    /// and  [Self::create_switch].
    pub fn create(
        transform_ctx: &'multiplexer mut TransformationContext<'context>,
        span: SourceSpan,
        entry_blocks: &[BlockRef],
        extra_args: &[Type],
    ) -> Self {
        assert!(!entry_blocks.is_empty(), "require at least one entry block");

        let mut multiplexer_block = transform_ctx.context.create_block();
        {
            let mut mb = multiplexer_block.borrow_mut();
            mb.insert_after(entry_blocks[0]);
        }

        // To implement the multiplexer block, we have to add the block arguments of every distinct
        // successor block to the multiplexer block. When redirecting edges, block arguments
        // designated for blocks that aren't branched to will be assigned the `get_undef_value`. The
        // amount of block arguments and their offset is saved in the map for `redirect_edge` to
        // transform the edges.
        let mut block_arg_mapping = SmallMap::<BlockRef, usize, 4>::new();
        for entry_block in entry_blocks.iter().copied() {
            let argc = multiplexer_block.borrow().num_arguments();
            if block_arg_mapping.insert(entry_block, argc).is_none() {
                add_block_arguments_from_other(
                    &transform_ctx.context,
                    multiplexer_block,
                    entry_block,
                );
            }
        }

        // If we have more than one successor, we have to additionally add a discriminator value,
        // denoting which successor to jump to. When redirecting edges, an appropriate value will be
        // passed using `get_switch_value`.
        let discriminator = if block_arg_mapping.len() > 1 {
            let val = transform_ctx.get_switch_value(0);
            Some(transform_ctx.context.append_block_argument(
                multiplexer_block,
                val.borrow().ty().clone(),
                span,
            ))
        } else {
            None
        };

        if !extra_args.is_empty() {
            for ty in extra_args {
                transform_ctx.context.append_block_argument(multiplexer_block, ty.clone(), span);
            }
        }

        Self {
            transform_ctx,
            multiplexer_block,
            block_arg_mapping,
            discriminator,
        }
    }

    /// Returns the created multiplexer block.
    pub fn get_multiplexer_block(&self) -> BlockRef {
        self.multiplexer_block
    }

    /// Redirects `edge` to branch to the multiplexer block before continuing to its original
    /// target. The edges successor must have originally been part of the entry blocks array passed
    /// to the `create` function. `extraArgs` must be used to pass along any additional values
    /// corresponding to `extraArgs` in `create`.
    pub fn redirect_edge(&mut self, edge: &Edge, extra_args: &[ValueRef]) {
        let result = self
            .block_arg_mapping
            .get(&edge.get_successor())
            .copied()
            .expect("edge was not originally passed to `create`");

        let succ_block = edge.get_successor();
        let mut terminator_ref = {
            let succ_block = succ_block.borrow();
            succ_block.terminator().unwrap()
        };
        let mut terminator = terminator_ref.borrow_mut();
        let context = terminator.context_rc();
        let mut succ = terminator.successor_mut(edge.successor_index);
        let succ_operands = &mut succ.arguments;

        // Extra arguments are always appended at the end of the block arguments.
        let multiplexer_block = self.multiplexer_block.borrow();
        let multiplexer_argc = multiplexer_block.num_arguments();
        let extra_args_begin_index = multiplexer_argc - extra_args.len();
        // If a discriminator exists, it is right before the extra arguments.
        let discriminator_index = self.discriminator.map(|_| extra_args_begin_index - 1);

        // NOTE: Here, we're redirecting the edge from the entry block, to the multiplexer block.
        // This requires us to ensure the successor operand vector is large enough for all of the
        // required multiplexer block arguments, and then to redirect the original entry block
        // arguments to their corresponding index in the multiplexer block parameter list. The
        // remaining arguments will either be undef, the discriminator value, or extra arguments.
        let mut new_succ_operands = SmallVec::<[OpOperand; 4]>::with_capacity(multiplexer_argc);
        for arg in multiplexer_block.arguments().iter() {
            let arg = arg.borrow();
            let index = arg.index();
            if index >= result && index < result + succ_operands.len() {
                // Original block arguments to the entry block.
                let mut operand = succ_operands[index - result];
                // Update the operand index now
                {
                    let mut operand = operand.borrow_mut();
                    operand.index = index as u8;
                }
                new_succ_operands.push(operand);
                continue;
            }

            // Discriminator value if it exists.
            if discriminator_index.is_some_and(|di| di == index) {
                let succ_index =
                    self.block_arg_mapping.iter().position(|(k, _)| k == &succ_block).unwrap()
                        as u32;
                let value = self.transform_ctx.get_switch_value(succ_index);
                let operand = context.make_operand(value, terminator_ref, index as u8);
                new_succ_operands.push(operand);
                continue;
            }

            // Followed by the extra arguments.
            if index >= extra_args_begin_index {
                let extra_arg = extra_args[index - extra_args_begin_index];
                let operand = context.make_operand(extra_arg, terminator_ref, index as u8);
                new_succ_operands.push(operand);
                continue;
            }

            // Otherwise undef values for any unused block arguments used by other entry blocks.
            let undef_value = self.transform_ctx.get_undef_value(arg.ty());
            let operand = context.make_operand(undef_value, terminator_ref, index as u8);
            new_succ_operands.push(operand);
        }

        edge.set_successor(self.multiplexer_block);

        let num_operands = succ_operands.len();
        for (index, new_operand) in new_succ_operands.into_iter().enumerate() {
            if num_operands >= index {
                succ_operands.push(new_operand);
            }

            succ_operands[index] = new_operand;
        }
    }

    /// Creates a switch op using `builder` which dispatches to the original successors of the edges
    /// passed to `create` minus the ones in `excluded`. The builder's insertion point has to be in a
    /// block dominated by the multiplexer block. All edges to the multiplexer block must have already
    /// been redirected using `redirectEdge`.
    pub fn create_switch(
        &mut self,
        span: SourceSpan,
        builder: &mut OpBuilder,
        excluded: &[BlockRef],
    ) -> Result<(), Report> {
        let multiplexer_block = self.multiplexer_block.borrow();
        let multiplexer_block_args = SmallVec::<[ValueRef; 4]>::from_iter(
            multiplexer_block.arguments().iter().copied().map(|arg| arg as ValueRef),
        );

        // We create the switch by creating a case for all entries and then splitting of the last
        // entry as a default case.
        let mut case_arguments = SmallVec::<[_; 4]>::default();
        let mut case_values = SmallVec::<[u32; 4]>::default();
        let mut case_destinations = SmallVec::<[BlockRef; 4]>::default();

        for (index, (&succ, &offset)) in self.block_arg_mapping.iter().enumerate() {
            if excluded.contains(&succ) {
                continue;
            }

            case_values.push(index as u32);
            case_destinations.push(succ);
            let succ = succ.borrow();
            case_arguments.push(&multiplexer_block_args[offset..(offset + succ.num_arguments())]);
        }

        // If we don't have a discriminator due to only having one entry we have to create a dummy
        // flag for the switch.
        let real_discriminator = if self.discriminator.is_none_or(|_| case_arguments.len() == 1) {
            self.transform_ctx.get_switch_value(0)
        } else {
            self.discriminator.unwrap()
        };

        case_values.pop();
        let default_dest = case_destinations.pop().unwrap();
        let default_args = case_arguments.pop().unwrap();

        assert!(
            builder.insertion_block().is_some_and(|b| b.borrow().has_predecessors()),
            "edges need to be redirected prior to creating switch"
        );

        self.transform_ctx.interface.create_cfg_switch_op(
            span,
            builder,
            real_discriminator,
            &case_values,
            &case_destinations,
            &case_arguments,
            default_dest,
            default_args,
        )
    }
}

/// Alternative implementation of Eq/Hash for Operation, using the operation equivalence infra to
/// check whether two 'return-like' operations are equivalent in the context of this transformation.
///
/// This means that both operations are of the same kind, have the same amount of operands and types
/// and the same attributes and properties. The operands themselves don't have to be equivalent.
#[derive(Copy, Clone)]
struct ReturnLikeOpKey(OperationRef);
impl Eq for ReturnLikeOpKey {}
impl PartialEq for ReturnLikeOpKey {
    fn eq(&self, other: &Self) -> bool {
        use crate::equivalence::{ignore_value_equivalence, OperationEquivalenceFlags};
        let a = self.0.borrow();
        a.is_equivalent_with_options(
            &other.0.borrow(),
            OperationEquivalenceFlags::IGNORE_LOCATIONS,
            ignore_value_equivalence,
        )
    }
}
impl core::hash::Hash for ReturnLikeOpKey {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use crate::equivalence::{IgnoreValueEquivalenceOperationHasher, OperationHasher};

        const HASHER: IgnoreValueEquivalenceOperationHasher = IgnoreValueEquivalenceOperationHasher;

        HASHER.hash_operation(&self.0.borrow(), state);
    }
}

/// Utility-class for transforming a region to only have one single block for every return-like
/// operation.
/// Iterates over a range of all edges from `block` to each of its successors.
struct SuccessorEdges<'a> {
    block: &'a Block,
    num_successors: usize,
}

impl<'a> SuccessorEdges<'a> {
    pub fn new(block: &'a Block) -> Self {
        let num_successors = block.num_successors();
        Self {
            block,
            num_successors,
        }
    }
}

impl Iterator for SuccessorEdges<'_> {
    type Item = Edge;

    fn next(&mut self) -> Option<Self::Item> {
        let successor_index = self.num_successors.checked_sub(1)?;
        self.num_successors = successor_index;
        Some(Edge {
            from_block: self.block.as_block_ref(),
            successor_index,
        })
    }
}

/// Calculates entry, exit and back edges of the given cycle.
fn calculate_cycle_edges(cycles: &[BlockRef]) -> CycleEdges {
    let mut result = CycleEdges::default();
    let mut entry_blocks = SmallSet::<BlockRef, 8>::default();

    // First identify all exit and entry edges by checking whether any successors or predecessors
    // are from outside the cycles.
    for block_ref in cycles.iter().copied() {
        let block = block_ref.borrow();
        for pred in block.predecessors() {
            if cycles.contains(&pred.block) {
                continue;
            }

            result.entry_edges.push(Edge {
                from_block: pred.block,
                successor_index: pred.index as usize,
            });
            entry_blocks.insert(block_ref);
        }

        let terminator = block.terminator().unwrap();
        let terminator = terminator.borrow();
        for succ in terminator.successor_iter() {
            let succ = succ.dest.borrow();
            if cycles.contains(&succ.block) {
                continue;
            }

            result.exit_edges.push(Edge {
                from_block: block_ref,
                successor_index: succ.index as usize,
            });
        }
    }

    // With the entry blocks identified, find all the back edges.
    for block_ref in cycles.iter().copied() {
        let block = block_ref.borrow();
        let terminator = block.terminator().unwrap();
        let terminator = terminator.borrow();
        for succ in terminator.successor_iter() {
            let succ = succ.dest.borrow();
            if !entry_blocks.contains(&succ.block) {
                continue;
            }

            result.back_edges.push(Edge {
                from_block: block_ref,
                successor_index: succ.index as usize,
            });
        }
    }

    result
}

/// Special loop properties of a structured loop.
///
/// A structured loop is a loop satisfying all of the following:
///
/// * Has at most one entry, one exit and one back edge.
/// * The back edge originates from the same block as the exit edge.
struct StructuredLoopProperties {
    /// Block containing both the single exit edge and the single back edge.
    latch: BlockRef,
    /// Loop condition of type equal to a value returned by `getSwitchValue`.
    condition: ValueRef,
    /// Exit block which is the only successor of the loop.
    exit_block: BlockRef,
}

/// Returns true if this block is an exit block of the region.
fn is_region_exit_block(block: &Block) -> bool {
    block.num_successors() == 0
}

struct TransformationContext<'a> {
    span: SourceSpan,
    region: RegionRef,
    entry: BlockRef,
    context: Rc<Context>,
    interface: &'a mut dyn CFGToSCFInterface,
    dominance_info: &'a mut DominanceInfo,
    typed_undef_cache: FxHashMap<Type, ValueRef>,
    // The transformation only creates all values in the range of 0 to max(num_successors).
    // Therefore using a vector instead of a map.
    switch_value_cache: SmallVec<[Option<ValueRef>; 2]>,
    return_like_to_combined_exit: FxHashMap<ReturnLikeOpKey, BlockRef>,
}

impl<'a> TransformationContext<'a> {
    pub fn new(
        region: RegionRef,
        interface: &'a mut dyn CFGToSCFInterface,
        dominance_info: &'a mut DominanceInfo,
    ) -> Result<Self, Report> {
        let (parent, entry) = {
            let region = region.borrow();
            let parent = region.parent().unwrap();
            let entry = region.entry_block_ref().unwrap();
            (parent, entry)
        };
        let op = parent.borrow();

        let mut this = Self {
            span: op.span(),
            region,
            entry,
            context: op.context_rc(),
            interface,
            dominance_info,
            typed_undef_cache: Default::default(),
            switch_value_cache: Default::default(),
            return_like_to_combined_exit: Default::default(),
        };

        this.create_single_exit_blocks_for_return_like()?;

        Ok(this)
    }

    pub fn get_undef_value(&mut self, ty: &Type) -> ValueRef {
        use hashbrown::hash_map::Entry;

        match self.typed_undef_cache.entry(ty.clone()) {
            Entry::Vacant(entry) => {
                let mut constant_builder = OpBuilder::new(self.context.clone());
                constant_builder.set_insertion_point_to_start(self.entry);
                let value =
                    self.interface.get_undef_value(self.span, &mut constant_builder, ty.clone());
                entry.insert(value);
                value
            }
            Entry::Occupied(entry) => *entry.get(),
        }
    }

    pub fn get_switch_value(&mut self, discriminant: u32) -> ValueRef {
        let index = discriminant as usize;

        if let Some(val) = self.switch_value_cache.get(index).copied().flatten() {
            return val;
        }

        // Make sure the cache is large enough
        let new_cache_size = core::cmp::max(self.switch_value_cache.len(), index + 1);
        self.switch_value_cache.resize(new_cache_size, None);

        let mut constant_builder = OpBuilder::new(self.context.clone());
        constant_builder.set_insertion_point_to_start(self.entry);
        let result =
            self.interface
                .get_cfg_switch_value(self.span, &mut constant_builder, discriminant);
        self.switch_value_cache[index] = Some(result);
        result
    }

    /// Transforms the region to only have a single block for every kind of return-like operation that
    /// all previous occurrences of the return-like op branch to.
    ///
    /// If the region only contains a single kind of return-like operation, it creates a single-entry
    /// and single-exit region.
    fn create_single_exit_blocks_for_return_like(&mut self) -> Result<(), Report> {
        // Do not borrow the region while visiting its blocks, as some parts of the transformation
        // may need to mutably borrow the region to add new blocks. Here, we only borrow it long
        // enough to get the next block in the list
        let mut next = {
            let region = self.region.borrow();
            region.body().front().as_pointer()
        };

        while let Some(block_ref) = next.take() {
            let block = block_ref.borrow();
            if block.num_successors() == 0 {
                let terminator = block.terminator().unwrap();
                self.combine_exit(terminator)?;
            }

            let region = self.region.borrow();
            let mut cursor = unsafe { region.body().cursor_from_ptr(block_ref) };
            cursor.move_next();
            next = cursor.as_pointer();
        }

        // Invalidate any dominance tree on the region as the exit combiner has added new blocks and
        // edges.
        self.dominance_info.info_mut().invalidate_region(self.region);

        Ok(())
    }

    /// Transforms `returnLikeOp` to a branch to the only block in the region with an instance of
    /// `return_like_op`s kind.
    fn combine_exit(&mut self, mut return_like_op_ref: OperationRef) -> Result<(), Report> {
        use hashbrown::hash_map::Entry;

        let key = ReturnLikeOpKey(return_like_op_ref);
        let mut return_like_op = return_like_op_ref.borrow_mut();

        match self.return_like_to_combined_exit.entry(key) {
            Entry::Occupied(entry) => {
                if OperationRef::ptr_eq(&entry.key().0, &return_like_op_ref) {
                    return Ok(());
                }

                let exit_block = *entry.get();
                let mut builder = OpBuilder::new(self.context.clone());
                builder.set_insertion_point_to_end(return_like_op.parent().unwrap());
                let dummy_value = self.get_switch_value(0);
                let operands = return_like_op.operands().all();
                let operands = SmallVec::<[ValueRef; 2]>::from_iter(
                    operands.iter().copied().map(|o| o.borrow().as_value_ref()),
                );
                self.interface.create_single_destination_branch(
                    return_like_op.span(),
                    &mut builder,
                    dummy_value,
                    exit_block,
                    &operands,
                )?;

                return_like_op.erase();
            }
            Entry::Vacant(entry) => {
                let operands = return_like_op.operands().all();
                let args =
                    SmallVec::<[Type; 2]>::from_iter(operands.iter().map(|o| o.borrow().ty()));
                let operands = SmallVec::<[ValueRef; 2]>::from_iter(
                    operands.iter().copied().map(|o| o.borrow().as_value_ref()),
                );

                let mut builder = OpBuilder::new(self.context.clone());
                let exit_block = builder.create_block(self.region, None, &args);
                entry.insert(exit_block);

                builder.set_insertion_point_to_end(return_like_op.parent().unwrap());
                let dummy_value = self.get_switch_value(0);
                self.interface.create_single_destination_branch(
                    return_like_op.span(),
                    &mut builder,
                    dummy_value,
                    exit_block,
                    &operands,
                )?;

                let exit_block = exit_block.borrow();
                let exit_terminator = exit_block.back().unwrap();
                return_like_op.move_to(crate::ProgramPoint::before(exit_terminator));
                return_like_op.set_operands(
                    exit_block.arguments().iter().copied().map(|arg| arg as ValueRef),
                );
            }
        }

        Ok(())
    }

    /// Transforms all outer-most cycles in the region with the region entry block `region_entry` into
    /// structured loops.
    ///
    /// Returns the entry blocks of any newly created regions potentially requiring further
    /// transformations.
    pub fn transform_cycles_to_scf_loops(
        &mut self,
        region_entry: BlockRef,
    ) -> Result<SmallVec<[BlockRef; 4]>, Report> {
        use crate::{cfg::StronglyConnectedComponents, EntityWithParent};

        let mut new_sub_regions = SmallVec::<[BlockRef; 4]>::default();

        let region_entry_block = region_entry.borrow();

        let scc_iter = StronglyConnectedComponents::new(&*region_entry_block);

        for scc in scc_iter {
            if !scc.has_cycle() {
                continue;
            }

            // Save the set and increment the SCC iterator early to avoid our modifications breaking
            // the SCC iterator.
            let edges = calculate_cycle_edges(scc.as_slice());
            let mut cycle_block_set = SmallSet::<BlockRef, 4>::from_iter(scc);
            let mut loop_header = edges.entry_edges[0].get_successor();

            // First turn the cycle into a loop by creating a single entry block if needed.
            if edges.entry_edges.len() > 1 {
                let mut edges_to_entry_blocks = SmallVec::<[Edge; 4]>::default();
                edges_to_entry_blocks.extend_from_slice(&edges.entry_edges);
                edges_to_entry_blocks.extend_from_slice(&edges.back_edges);

                let loop_header_term = loop_header.borrow().terminator().unwrap();
                let span = loop_header_term.borrow().span();
                let multiplexer = self.create_single_entry_block(span, &edges_to_entry_blocks)?;
                loop_header = multiplexer.get_multiplexer_block();
            }
            cycle_block_set.insert(loop_header);

            // Then turn it into a structured loop by creating a single latch.
            let from_block = edges.back_edges[0].get_from_block();
            let from_block_term = from_block.borrow().terminator().unwrap();
            let span = from_block_term.borrow().span();
            let loop_properties =
                self.create_single_exiting_latch(span, &edges.back_edges, &edges.exit_edges)?;

            let latch_block_ref = loop_properties.latch;
            let mut exit_block_ref = loop_properties.exit_block;
            cycle_block_set.insert(latch_block_ref);
            cycle_block_set.insert(loop_header);

            // Finally, turn it into reduce form.
            let iteration_values = self.transform_to_reduce_loop(
                loop_header,
                exit_block_ref,
                cycle_block_set.as_slice(),
            );

            // Create a block acting as replacement for the loop header and insert the structured
            // loop into it.
            let mut new_loop_parent_block_ref = self.context.create_block();
            add_block_arguments_from_other(&self.context, new_loop_parent_block_ref, loop_header);

            let mut region_ref = region_entry_block.parent().unwrap();
            let mut region = region_ref.borrow_mut();

            let blocks = region.body_mut();

            let mut loop_body_ref = self.context.alloc_tracked(Region::default());
            let mut loop_body = loop_body_ref.borrow_mut();

            // Make sure the loop header is the entry block.
            loop_body.push_back(unsafe {
                let mut cursor = blocks.cursor_mut_from_ptr(loop_header);
                cursor.remove().unwrap()
            });
            EntityWithParent::on_inserted_into_parent(loop_header, region_ref);

            for block in cycle_block_set {
                if !BlockRef::ptr_eq(&block, &latch_block_ref)
                    && !BlockRef::ptr_eq(&block, &loop_header)
                {
                    loop_body.push_back(unsafe {
                        let mut cursor = blocks.cursor_mut_from_ptr(block);
                        cursor.remove().unwrap()
                    });
                    EntityWithParent::on_inserted_into_parent(block, region_ref);
                }
            }

            // And the latch is the last block.
            loop_body.push_back(unsafe {
                let mut cursor = blocks.cursor_mut_from_ptr(latch_block_ref);
                cursor.remove().unwrap()
            });
            EntityWithParent::on_inserted_into_parent(latch_block_ref, region_ref);

            let mut old_terminator = latch_block_ref.borrow().terminator().unwrap();
            old_terminator.borrow_mut().remove();

            let mut builder = OpBuilder::new(self.context.clone());
            builder.set_insertion_point_to_end(new_loop_parent_block_ref);

            let loop_values_init = {
                let new_loop_parent_block = new_loop_parent_block_ref.borrow();
                SmallVec::<[ValueRef; 4]>::from_iter(
                    new_loop_parent_block.arguments().iter().map(|arg| arg.borrow().as_value_ref()),
                )
            };
            let structured_loop_op = self.interface.create_structured_do_while_loop_op(
                &mut builder,
                old_terminator,
                &loop_values_init,
                loop_properties.condition,
                &iteration_values,
                loop_body_ref,
            )?;

            new_sub_regions.push(loop_header);

            let structured_loop = structured_loop_op.borrow();
            let loop_results = structured_loop.results().all();
            let mut exit_block = exit_block_ref.borrow_mut();
            for (mut old_value, new_value) in
                exit_block.arguments().iter().copied().zip(loop_results)
            {
                let new_value = new_value.borrow().as_value_ref();
                old_value.borrow_mut().replace_all_uses_with(new_value);
            }

            loop_header.borrow_mut().replace_all_uses_with(new_loop_parent_block_ref);

            // Merge the exit block right after the loop operation.
            let mut new_loop_parent_block = new_loop_parent_block_ref.borrow_mut();
            let ops = new_loop_parent_block.body_mut();
            let mut spliced_ops = exit_block.body_mut().take();
            {
                let mut cursor = spliced_ops.front_mut();
                while let Some(op) = cursor.as_pointer() {
                    cursor.move_next();
                    Operation::on_inserted_into_parent(op, new_loop_parent_block_ref);
                }
            }
            ops.back_mut().splice_after(spliced_ops);

            exit_block.erase();
        }

        Ok(new_sub_regions)
    }

    /// Transforms the first occurrence of conditional control flow in `regionEntry`
    /// into conditionally executed regions. Returns the entry block of the created
    /// regions and the region after the conditional control flow.
    pub fn transform_to_structured_cf_branches(
        &mut self,
        mut region_entry: BlockRef,
    ) -> Result<SmallVec<[BlockRef; 4]>, Report> {
        let mut region_entry_block = region_entry.borrow_mut();
        let num_successors = region_entry_block.num_arguments();

        // Trivial region.
        if num_successors == 0 {
            return Ok(Default::default());
        }

        if num_successors == 1 {
            // Single successor we can just splice together.
            let mut successor = region_entry_block.get_successor(0);
            let mut succ = successor.borrow_mut();
            let mut entry_terminator = region_entry_block.terminator().unwrap();
            let mut terminator = entry_terminator.borrow_mut();
            let terminator_succ = terminator.successor(0);
            for (mut old_value, new_value) in
                succ.arguments().iter().copied().zip(terminator_succ.arguments)
            {
                let mut old_value = old_value.borrow_mut();
                old_value.replace_all_uses_with(new_value.borrow().as_value_ref());
            }
            terminator.erase();

            let region_entry_body = region_entry_block.body_mut();
            let spliced_ops = {
                let mut ops = succ.body_mut().take();
                let mut cursor = ops.front_mut();
                while let Some(op) = cursor.as_pointer() {
                    cursor.move_next();
                    Operation::on_inserted_into_parent(op, region_entry);
                }
                ops
            };
            region_entry_body.back_mut().splice_after(spliced_ops);

            succ.erase();

            return Ok(smallvec![region_entry]);
        }

        // Split the CFG into "#numSuccessor + 1" regions.
        //
        // For every edge to a successor, the blocks it solely dominates are determined and become
        // the region following that edge. The last region is the continuation that follows the
        // branch regions.
        let mut not_continuation = SmallSet::<BlockRef, 8>::default();
        not_continuation.insert(region_entry);

        let mut successor_branch_regions = SmallVec::<[SmallVec<[BlockRef; 2]>; 2]>::default();
        successor_branch_regions.resize_with(num_successors, Default::default);

        let terminator = region_entry_block.terminator().unwrap();
        let terminator = terminator.borrow();
        for (block_list, succ) in
            successor_branch_regions.iter_mut().zip(terminator.successor_iter())
        {
            // If the region entry is not the only predecessor, then the edge does not dominate the
            // block it leads to.
            let succ_operand = succ.dest.borrow();
            let dest = succ_operand.block.borrow();
            if dest
                .get_single_predecessor()
                .is_none_or(|pred| !BlockRef::ptr_eq(&region_entry, &pred))
            {
                continue;
            }

            // Otherwise get all blocks it dominates in DFS/pre-order.
            let node = self.dominance_info.info().node(succ_operand.block).unwrap();
            for curr in PreOrderDomTreeIter::new(node) {
                if let Some(block) = curr.block() {
                    block_list.push(block);
                    not_continuation.insert(block);
                }
            }
        }

        // Finds all relevant edges and checks the shape of the control flow graph at
        // this point.
        // Branch regions may either:
        // * Be post-dominated by the continuation
        // * Be post-dominated by a return-like op
        // * Dominate a return-like op and have an edge to the continuation.
        //
        // The control flow graph may then be one of three cases:
        // 1) All branch regions are post-dominated by the continuation. This is the
        // usual case. If there are multiple entry blocks into the continuation a
        // single entry block has to be created. A structured control flow op
        // can then be created from the branch regions.
        //
        // 2) No branch region has an edge to a continuation:
        //                                 +-----+
        //                           +-----+ bb0 +----+
        //                           v     +-----+    v
        //                Region 1 +-+--+    ...     +-+--+ Region n
        //                         |ret1|            |ret2|
        //                         +----+            +----+
        //
        // This can only occur if every region ends with a different kind of
        // return-like op. In that case the control flow operation must stay as we are
        // unable to create a single exit-block. We can nevertheless process all its
        // successors as they single-entry, single-exit regions.
        //
        // 3) Only some branch regions are post-dominated by the continuation.
        // The other branch regions may either be post-dominated by a return-like op
        // or lead to either the continuation or return-like op.
        // In this case we also create a single entry block like in 1) that also
        // includes all edges to the return-like op:
        //                                 +-----+
        //                           +-----+ bb0 +----+
        //                           v     +-----+    v
        //             Region 1    +-+-+    ...     +-+-+ Region n
        //                         +---+            +---+
        //                  +---+  |...              ...
        //                  |ret|<-+ |                |
        //                  +---+    |      +---+     |
        //                           +---->++   ++<---+
        //                                 |     |
        //                                 ++   ++ Region T
        //                                  +---+
        // This transforms to:
        //                                 +-----+
        //                           +-----+ bb0 +----+
        //                           v     +-----+    v
        //                Region 1 +-+-+    ...     +-+-+ Region n
        //                         +---+            +---+
        //                          ...    +-----+   ...
        //                           +---->+ bbM +<---+
        //                                 +-----+
        //                           +-----+  |
        //                           |        v
        //                  +---+    |      +---+
        //                  |ret+<---+     ++   ++
        //                  +---+          |     |
        //                                 ++   ++ Region T
        //                                  +---+
        //
        // bb0 to bbM is now a single-entry, single-exit region that applies to case
        // 1). The control flow op at the end of bbM will trigger case 2.

        let mut continuation_edges = SmallVec::<[Edge; 2]>::default();
        let mut continuation_post_dominates_all_regions = true;
        let mut no_successor_has_continuation_edge = true;

        for (entry_edge, branch_region) in
            SuccessorEdges::new(&region_entry_block).zip(successor_branch_regions.iter_mut())
        {
            // If the branch region is empty then the branch target itself is part of the
            // continuation.
            if branch_region.is_empty() {
                continuation_edges.push(entry_edge);
                no_successor_has_continuation_edge = false;
                continue;
            }

            for block_ref in branch_region.iter() {
                let block = block_ref.borrow();
                if is_region_exit_block(&block) {
                    // If a return-like op is part of the branch region then the continuation no
                    // longer post-dominates the branch region. Add all its incoming edges to edge
                    // list to create the single-exit block for all branch regions.
                    continuation_post_dominates_all_regions = false;
                    for pred in block.predecessors() {
                        continuation_edges.push(Edge {
                            from_block: pred.block,
                            successor_index: pred.index as usize,
                        });
                    }
                    continue;
                }

                for edge in SuccessorEdges::new(&block) {
                    if not_continuation.contains(&edge.get_successor()) {
                        continue;
                    }

                    continuation_edges.push(edge);
                    no_successor_has_continuation_edge = false;
                }
            }
        }

        // case 2) Keep the control flow op but process its successors further.
        if no_successor_has_continuation_edge {
            let term = region_entry_block.terminator().unwrap();
            let term = term.borrow();
            return Ok(term.successor_iter().map(|s| s.dest.borrow().block).collect());
        }

        let mut continuation = continuation_edges.first().map(|e| e.get_successor());

        // In case 3) or if not all continuation edges have the same entry block, create a single
        // entry block as continuation for all branch regions.
        if continuation.is_none() || !continuation_post_dominates_all_regions {
            let term = continuation_edges[0].get_from_block().borrow().terminator().unwrap();
            let span = term.borrow().span();
            let multiplexer = self.create_single_entry_block(span, &continuation_edges)?;
            continuation = Some(multiplexer.get_multiplexer_block());
        }

        // Trigger reprocess of case 3) after creating the single entry block.
        if !continuation_post_dominates_all_regions {
            // Unlike in the general case, we are explicitly revisiting the same region entry again
            // after having changed its control flow edges and dominance. We have to therefore
            // explicitly invalidate the dominance tree.
            self.dominance_info
                .info_mut()
                .invalidate_region(region_entry_block.parent().unwrap());
            return Ok(smallvec![region_entry]);
        }

        let mut continuation = continuation.unwrap();
        let mut new_sub_regions = SmallVec::<[BlockRef; 4]>::default();

        // Empty blocks with the values they return to the parent op.
        let mut created_empty_blocks =
            SmallVec::<[(BlockRef, SmallVec<[ValueRef; 4]>); 2]>::default();

        // Create the branch regions.
        let mut conditional_regions = SmallVec::<[RegionRef; 2]>::default();
        for (branch_region, entry_edge) in successor_branch_regions
            .iter_mut()
            .zip(SuccessorEdges::new(&region_entry_block))
        {
            let mut conditional_region = self.context.alloc_tracked(Region::default());
            conditional_regions.push(conditional_region);

            if branch_region.is_empty() {
                // If no block is part of the branch region, we create a dummy block to place the
                // region terminator into.
                let mut empty_block = self.context.create_block();
                let pred = entry_edge.from_block.borrow().terminator().unwrap();
                let pred = pred.borrow();
                let succ = pred.successor(entry_edge.successor_index);
                let succ_operands =
                    succ.arguments.iter().map(|o| o.borrow().as_value_ref()).collect();
                created_empty_blocks.push((empty_block, succ_operands));
                empty_block.borrow_mut().insert_at_end(conditional_region);
                continue;
            }

            self.create_single_exit_branch_region(
                branch_region,
                continuation,
                &mut created_empty_blocks,
                conditional_region,
            );

            // The entries of the branch regions may only have redundant block arguments since the
            // edge to the branch region is always dominating.
            let mut cond_region = conditional_region.borrow_mut();
            let mut sub_region_entry_block = cond_region.entry_mut();
            let pred = entry_edge.from_block.borrow().terminator().unwrap();
            let pred = pred.borrow();
            let succ = pred.successor(entry_edge.successor_index);
            for (mut old_value, new_value) in sub_region_entry_block
                .arguments()
                .iter()
                .copied()
                .zip(succ.arguments.as_slice())
            {
                old_value.borrow_mut().replace_all_uses_with(new_value.borrow().as_value_ref());
            }

            sub_region_entry_block.erase_arguments(|_| true);

            new_sub_regions.push(sub_region_entry_block.as_block_ref());
        }

        let structured_cond_op = {
            let mut builder = OpBuilder::new(self.context.clone());
            builder.set_insertion_point_to_end(region_entry);

            let cont = continuation.borrow();
            let arg_types = cont
                .arguments()
                .iter()
                .map(|arg| arg.borrow().ty().clone())
                .collect::<SmallVec<[_; 2]>>();
            let mut terminator = region_entry_block.terminator().unwrap();
            let op = self.interface.create_structured_branch_region_op(
                &mut builder,
                terminator,
                &arg_types,
                &mut conditional_regions,
            )?;
            terminator.borrow_mut().erase();
            op
        };

        for (block, value_range) in created_empty_blocks {
            let mut builder = OpBuilder::new(self.context.clone());
            builder.set_insertion_point_to_end(block);

            let span = structured_cond_op.span();
            self.interface.create_structured_branch_region_terminator_op(
                span,
                &mut builder,
                structured_cond_op,
                None,
                &value_range,
            )?;
        }

        // Any leftover users of the continuation must be from unconditional branches in a branch
        // region. There can only be at most one per branch region as all branch regions have been
        // made single-entry single-exit above. Replace them with the region terminator.
        let mut cont = continuation.borrow_mut();
        let uses = cont.uses_mut();
        let mut current_user = uses.front_mut();
        while let Some(mut user) = current_user.as_pointer() {
            let mut user = user.borrow_mut();

            assert_eq!(user.owner.borrow().num_successors(), 1);

            let mut builder = OpBuilder::new(self.context.clone());
            builder.set_insertion_point_after(user.owner);

            let args = {
                let pred = user.owner.borrow();
                pred.successor(0)
                    .arguments
                    .iter()
                    .map(|arg| arg.borrow().as_value_ref())
                    .collect::<SmallVec<[ValueRef; 2]>>()
            };
            self.interface.create_structured_branch_region_terminator_op(
                user.owner.span(),
                &mut builder,
                structured_cond_op,
                Some(user.owner),
                &args,
            )?;

            current_user.remove();
            user.owner.borrow_mut().erase();
        }

        let structured_cond = structured_cond_op.borrow();
        for (mut old_value, new_value) in
            cont.arguments().iter().copied().zip(structured_cond.results().iter())
        {
            old_value.borrow_mut().replace_all_uses_with(new_value.borrow().as_value_ref());
        }

        // Splice together the continuations operations with the region entry.
        let region_body = region_entry_block.body_mut();
        let mut spliced_ops = cont.body_mut().take();
        {
            let mut cursor = spliced_ops.front_mut();
            while let Some(op) = cursor.as_pointer() {
                cursor.move_next();
                Operation::on_inserted_into_parent(op, region_entry);
            }
        }
        region_body.back_mut().splice_after(spliced_ops);

        cont.erase();

        // After splicing the continuation, the region has to be reprocessed as it has new
        // successors.
        new_sub_regions.push(region_entry);

        Ok(new_sub_regions)
    }

    /// Transforms a structured loop into a loop in reduce form.
    ///
    /// Reduce form is defined as a structured loop where:
    ///
    /// 1. No values defined within the loop body are used outside the loop body.
    /// 2. The block arguments and successor operands of the exit block are equal to the block arguments
    ///    of the loop header and the successor operands of the back edge.
    ///
    /// This is required for many structured control flow ops as they tend to not have separate "loop
    /// result arguments" and "loop iteration arguments" at the end of the block. Rather, the "loop
    /// iteration arguments" from the last iteration are the result of the loop.
    ///
    /// Note that the requirement of 1 is shared with LCSSA form in LLVM. However, due to this being a
    /// structured loop instead of a general loop, we do not require complicated dominance algorithms
    /// nor SSA updating making this implementation easier than creating a generic LCSSA transformation
    /// pass.
    pub fn transform_to_reduce_loop(
        &mut self,
        loop_header: BlockRef,
        exit_block: BlockRef,
        loop_blocks: &[BlockRef],
    ) -> SmallVec<[ValueRef; 4]> {
        let latch = {
            let exit_block = exit_block.borrow();
            let latch = exit_block
                .get_single_predecessor()
                .expect("exit block must have only latch as predecessor at this point");
            assert_eq!(
                exit_block.arguments().len(),
                0,
                "exit block musn't have any block arguments at this point"
            );
            latch
        };

        let latch_block = latch.borrow();

        let mut loop_header_index = 0;
        let mut exit_block_index = 1;
        if !BlockRef::ptr_eq(&latch_block.get_successor(loop_header_index), &loop_header) {
            core::mem::swap(&mut loop_header_index, &mut exit_block_index);
        }

        assert!(BlockRef::ptr_eq(&latch_block.get_successor(loop_header_index), &loop_header));
        assert!(BlockRef::ptr_eq(&latch_block.get_successor(exit_block_index), &exit_block));

        let mut latch_terminator = latch_block.terminator().unwrap();
        let mut latch_term = latch_terminator.borrow_mut();
        // Take a snapshot of the loop header successor operands as we cannot hold a reference to
        // them and mutate them at the same time
        let mut loop_header_successor_operands = latch_term
            .successor(loop_header_index)
            .arguments
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        let mut exit_block_successor_operands = latch_term.successor_mut(exit_block_index);

        // Add all values used in the next iteration to the exit block. Replace any uses that are
        // outside the loop with the newly created exit block.
        for mut arg in loop_header_successor_operands.iter().copied() {
            let argument = arg.borrow();
            let exit_arg = self.context.append_block_argument(
                exit_block,
                argument.ty().clone(),
                argument.span(),
            );
            let operand = self.context.make_operand(arg, latch_terminator, 0);
            exit_block_successor_operands.arguments.push(operand);
            arg.borrow_mut().replace_uses_with_if(exit_arg, |user| {
                !loop_blocks.contains(&user.owner().parent().unwrap())
            });
        }

        // Loop below might add block arguments to the latch and loop header. Save the block
        // arguments prior to the loop to not process these.
        let latch_block_arguments_prior =
            latch_block.arguments().iter().copied().collect::<SmallVec<[_; 2]>>();
        let loop_header_arguments_prior =
            loop_header.borrow().arguments().iter().copied().collect::<SmallVec<[_; 2]>>();

        // Go over all values defined within the loop body. If any of them are used outside the loop
        // body, create a block argument on the exit block and loop header and replace the outside
        // uses with the exit block argument. The loop header block argument is added to satisfy
        // requirement (1) in the reduce form condition.
        for loop_block_ref in loop_blocks.iter() {
            // Cache dominance queries for loop_block_ref.
            // There are likely to be many duplicate queries as there can be many value definitions
            // within a block.
            let mut dominance_cache = SmallMap::<BlockRef, bool>::default();
            // Returns true if `loop_block_ref` dominates `block`.
            let mut loop_block_dominates = |block: BlockRef, dominance_info: &DominanceInfo| {
                use crate::adt::smallmap::Entry;
                match dominance_cache.entry(block) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        let dominates = dominance_info.dominates(loop_block_ref, &block);
                        entry.insert(dominates);
                        dominates
                    }
                }
            };

            let mut check_value = |ctx: &mut TransformationContext<'_>, value: ValueRef| {
                let val = value.borrow();
                let mut block_argument = None;
                let mut next_use = val.uses().front().as_pointer();
                while let Some(mut user) = next_use.take() {
                    next_use = user.next();

                    // Go through all the parent blocks and find the one part of the region of the
                    // loop. If the block is part of the loop, then the value does not escape the
                    // loop through this use.
                    let mut curr_block = user.borrow().owner().parent();
                    while let Some(cb) = curr_block {
                        if cb.borrow().parent().is_none_or(|p| {
                            !RegionRef::ptr_eq(&loop_header.borrow().parent().unwrap(), &p)
                        }) {
                            curr_block = cb.borrow().parent_block();
                            continue;
                        }

                        break;
                    }

                    let curr_block = curr_block.unwrap();
                    if loop_blocks.contains(&curr_block) {
                        continue;
                    }

                    // Block argument is only created the first time it is required.
                    if block_argument.is_none() {
                        block_argument = Some(ctx.context.append_block_argument(
                            exit_block,
                            val.ty().clone(),
                            val.span(),
                        ));
                        ctx.context.append_block_argument(
                            loop_header,
                            val.ty().clone(),
                            val.span(),
                        );

                        // `value` might be defined in a block that does not dominate `latch` but
                        // previously dominated an exit block with a use. In this case, add a block
                        // argument to the latch and go through all predecessors. If the value
                        // dominates the predecessor, pass the value as a successor operand,
                        // otherwise pass undef. The above is unnecessary if the value is a block
                        // argument of the latch or if `value` dominates all predecessors.
                        let mut argument = value;
                        if val.parent_block().unwrap() != latch
                            && latch_block.predecessors().any(|pred| {
                                !loop_block_dominates(
                                    pred.owner.borrow().parent().unwrap(),
                                    ctx.dominance_info,
                                )
                            })
                        {
                            argument = ctx.context.append_block_argument(
                                latch,
                                val.ty().clone(),
                                val.span(),
                            );
                            for pred in latch_block.predecessors() {
                                let mut succ_operand = value;
                                if !loop_block_dominates(
                                    pred.owner.borrow().parent().unwrap(),
                                    ctx.dominance_info,
                                ) {
                                    succ_operand = ctx.get_undef_value(val.ty());
                                }

                                let succ_operand =
                                    ctx.context.make_operand(succ_operand, pred.owner, 0);

                                let mut pred_op = pred.owner;
                                let mut pred_op = pred_op.borrow_mut();
                                let mut succ = pred_op.successor_mut(pred.index as usize);
                                succ.arguments.push(succ_operand);
                            }
                        }

                        loop_header_successor_operands.push(argument);
                        for edge in SuccessorEdges::new(&latch_block) {
                            let mut pred = edge.from_block.borrow().terminator().unwrap();
                            let operand = ctx.context.make_operand(argument, pred, 0);
                            let mut pred = pred.borrow_mut();
                            let mut succ = pred.successor_mut(edge.successor_index);
                            succ.arguments.push(operand);
                        }
                    }

                    user.borrow_mut().set(block_argument.unwrap());
                }
            };

            if BlockRef::ptr_eq(loop_block_ref, &latch) {
                for arg in latch_block_arguments_prior.iter() {
                    check_value(self, arg.borrow().as_value_ref());
                }
            } else if BlockRef::ptr_eq(loop_block_ref, &loop_header) {
                for arg in loop_header_arguments_prior.iter() {
                    check_value(self, arg.borrow().as_value_ref());
                }
            } else {
                let loop_block = loop_block_ref.borrow();
                for arg in loop_block.arguments() {
                    check_value(self, arg.borrow().as_value_ref());
                }
            }

            let loop_block = loop_block_ref.borrow();
            for op in loop_block.body() {
                for result in op.results().iter() {
                    check_value(self, result.borrow().as_value_ref());
                }
            }
        }

        // New block arguments may have been added to the loop header. Adjust the entry edges to
        // pass undef values to these.
        let loop_header = loop_header.borrow();
        for pred in loop_header.predecessors() {
            // Latch successor arguments have already been handled.
            if BlockRef::ptr_eq(&pred.predecessor(), &latch) {
                continue;
            }

            let mut op = pred.owner;
            let mut op = op.borrow_mut();
            let mut succ = op.successor_mut(pred.index as usize);
            succ.arguments
                .extend(loop_header.arguments().iter().skip(succ.arguments.len()).map(|arg| {
                    let val = self.get_undef_value(arg.borrow().ty());
                    self.context.make_operand(val, pred.owner, 0)
                }));
        }

        loop_header_successor_operands
    }

    /// Creates a single entry block out of multiple entry edges using an edge multiplexer and returns
    /// it.
    fn create_single_entry_block(
        &mut self,
        span: SourceSpan,
        entry_edges: &[Edge],
    ) -> Result<EdgeMultiplexer<'_, 'a>, Report> {
        let entry_blocks = SmallVec::<[BlockRef; 2]>::from_iter(
            entry_edges.iter().map(|edge| edge.get_successor()),
        );
        let mut multiplexer = EdgeMultiplexer::create(self, span, &entry_blocks, &[]);

        // Redirect the edges prior to creating the switch op. We guarantee that predecessors are up
        // to date.
        for edge in entry_edges {
            multiplexer.redirect_edge(edge, &[]);
        }

        let mut builder = OpBuilder::new(multiplexer.transform_ctx.context.clone());
        builder.set_insertion_point_to_end(multiplexer.get_multiplexer_block());
        multiplexer.create_switch(span, &mut builder, &[])?;

        Ok(multiplexer)
    }

    /// Makes sure the branch region only has a single exit.
    ///
    /// This is required by the recursive part of the algorithm, as it expects the CFG to be single-
    /// entry and single-exit. This is done by simply creating an empty block if there is more than one
    /// block with an edge to the continuation block. All blocks with edges to the continuation are then
    /// redirected to this block. A region terminator is later placed into the block.
    #[allow(clippy::type_complexity)]
    fn create_single_exit_branch_region(
        &mut self,
        branch_region: &[BlockRef],
        continuation: BlockRef,
        created_empty_blocks: &mut SmallVec<[(BlockRef, SmallVec<[ValueRef; 4]>); 2]>,
        conditional_region: RegionRef,
    ) {
        let mut single_exit_block = None;
        let mut previous_edge_to_continuation = None;
        let mut branch_region_parent = branch_region[0].borrow().parent().unwrap();

        for mut block_ref in branch_region.iter().copied() {
            let block = block_ref.borrow();
            for edge in SuccessorEdges::new(&block) {
                if !BlockRef::ptr_eq(&edge.get_successor(), &continuation) {
                    continue;
                }

                if previous_edge_to_continuation.is_none() {
                    previous_edge_to_continuation = Some(edge);
                    continue;
                }

                // If this is not the first edge to the continuation we create the single exit block
                // and redirect the edges.
                if single_exit_block.is_none() {
                    let seb = self.context.create_block();
                    single_exit_block = Some(seb);
                    add_block_arguments_from_other(&self.context, seb, continuation);
                    previous_edge_to_continuation.as_mut().unwrap().set_successor(seb);
                    let seb_block = seb.borrow();
                    let seb_args = seb_block
                        .arguments()
                        .iter()
                        .map(|arg| arg.borrow().as_value_ref())
                        .collect();
                    created_empty_blocks.push((seb, seb_args));
                }

                edge.set_successor(single_exit_block.unwrap());
            }

            let mut branch_region_parent = branch_region_parent.borrow_mut();
            unsafe {
                let mut cursor = branch_region_parent.body_mut().cursor_mut_from_ptr(block_ref);
                cursor.remove();
            }

            block_ref.borrow_mut().insert_at_end(conditional_region);
        }

        if let Some(mut single_exit_block) = single_exit_block {
            let mut single_exit_block = single_exit_block.borrow_mut();
            single_exit_block.insert_at_end(conditional_region);
        }
    }

    /// Transforms a loop into a structured loop with only a single back edge and
    /// exiting edge, originating from the same block.
    fn create_single_exiting_latch(
        &mut self,
        span: SourceSpan,
        back_edges: &[Edge],
        exit_edges: &[Edge],
    ) -> Result<StructuredLoopProperties, Report> {
        assert!(
            all_same_block(back_edges, |edge| edge.get_successor()),
            "all repetition edges must lead to the single loop header"
        );

        // First create the multiplexer block, which will be our latch, for all back edges and exit
        // edges. We pass an additional argument to the multiplexer block which indicates whether
        // the latch was reached from what was originally a back edge or an exit block. This is
        // later used to branch using the new only back edge.
        let mut successors = SmallVec::<[BlockRef; 4]>::default();
        successors.extend(back_edges.iter().map(|edge| edge.get_successor()));
        successors.extend(exit_edges.iter().map(|edge| edge.get_successor()));

        let extra_args = [self.get_switch_value(0).borrow().ty().clone()];
        let mut multiplexer = EdgeMultiplexer::create(self, span, &successors, &extra_args);

        let latch_block = multiplexer.get_multiplexer_block();

        // Create a separate exit block that comes right after the latch.
        let mut exit_block = multiplexer.transform_ctx.context.create_block();
        exit_block.borrow_mut().insert_after(latch_block);

        // Since this is a loop, all back edges point to the same loop header.
        let loop_header = back_edges[0].get_successor();

        // Redirect the edges prior to creating the switch op. We guarantee that predecessors are up
        // to date.

        // Redirecting back edges with `should_repeat` as 1.
        for edge in back_edges {
            let extra_args = [multiplexer.transform_ctx.get_switch_value(1)];
            multiplexer.redirect_edge(edge, &extra_args);
        }

        // Redirecting exits edges with `should_repeat` as 0.
        for edge in exit_edges {
            let extra_args = [multiplexer.transform_ctx.get_switch_value(0)];
            multiplexer.redirect_edge(edge, &extra_args);
        }

        // Create the new only back edge to the loop header. Branch to the exit block otherwise.
        let should_repeat = latch_block.borrow().arguments().last().copied().unwrap();
        let should_repeat = should_repeat.borrow().as_value_ref();
        {
            let mut builder = OpBuilder::new(multiplexer.transform_ctx.context.clone());
            builder.set_insertion_point_to_start(latch_block);

            let num_args = loop_header.borrow().num_arguments();
            let latch_block = latch_block.borrow();
            let latch_args = latch_block
                .arguments()
                .iter()
                .take(num_args)
                .map(|arg| arg.borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 4]>>();
            multiplexer.transform_ctx.interface.create_conditional_branch(
                span,
                &mut builder,
                should_repeat,
                loop_header,
                &latch_args,
                exit_block,
                &[],
            )?;
        }

        {
            let mut builder = OpBuilder::new(multiplexer.transform_ctx.context.clone());
            builder.set_insertion_point_to_start(exit_block);

            if exit_edges.is_empty() {
                // A loop without an exit edge is a statically known infinite loop.
                // Since structured control flow ops are not terminator ops, the caller has to
                // create a fitting return-like unreachable terminator operation.
                let region = latch_block.borrow().parent().unwrap();
                let terminator = multiplexer
                    .transform_ctx
                    .interface
                    .create_unreachable_terminator(span, &mut builder, region)?;
                // Transform the just created transform operation in the case that an occurrence of
                // it existed in input IR.
                multiplexer.transform_ctx.combine_exit(terminator)?;
            } else {
                // Create the switch dispatching to what were originally the multiple exit blocks.
                // The loop header has to explicitly be excluded in the below switch as we would
                // otherwise be creating a new loop again. All back edges leading to the loop header
                // have already been handled in the switch above. The remaining edges can only jump
                // to blocks outside the loop.
                multiplexer.create_switch(span, &mut builder, &[loop_header])?;
            }
        }

        Ok(StructuredLoopProperties {
            latch: latch_block,
            condition: should_repeat,
            exit_block,
        })
    }
}

fn all_same_block<F>(edges: &[Edge], callback: F) -> bool
where
    F: Fn(&Edge) -> BlockRef,
{
    let Some((first, rest)) = edges.split_first() else {
        return true;
    };

    let expected = callback(first);
    rest.iter().all(|edge| callback(edge) == expected)
}

/// Transformation lifting any dialect implementing control flow graph
/// operations to a dialect implementing structured control flow operations.
/// `region` is the region that should be transformed.
/// The implementation of `interface` is responsible for the conversion of the
/// control flow operations to the structured control flow operations.
///
/// If the region contains only a single kind of return-like operation, all
/// control flow graph operations will be converted successfully.
/// Otherwise a single control flow graph operation branching to one block
/// per return-like operation kind remains.
///
/// The transformation currently requires that all control flow graph operations
/// have no side effects, implement the BranchOpInterface and does not have any
/// operation produced successor operands.
/// Returns failure if any of the preconditions are violated or if any of the
/// methods of `interface` failed. The IR is left in an unspecified state.
///
/// Otherwise, returns true or false if any changes to the IR have been made.
#[allow(unused)]
pub fn transform_cfg_to_scf(
    region: RegionRef,
    interface: &mut dyn CFGToSCFInterface,
    dominance_info: &mut DominanceInfo,
) -> Result<bool, Report> {
    {
        let region = region.borrow();
        if region.is_empty() || region.has_one_block() {
            return Ok(false);
        }

        check_transformation_preconditions(&region)?;
    }

    let mut transform_ctx = TransformationContext::new(region, interface, dominance_info)?;

    let mut worklist = SmallVec::<[BlockRef; 4]>::from_slice(&[transform_ctx.entry]);
    while let Some(current) = worklist.pop() {
        // Turn all top-level cycles in the CFG to structured control flow first.
        // After this transformation, the remaining CFG ops form a DAG.
        let mut new_regions = transform_ctx.transform_cycles_to_scf_loops(current)?;

        // Add the newly created subregions to the worklist. These are the bodies of the loops.
        worklist.extend(new_regions.iter().copied());
        // Invalidate the dominance tree as blocks have been moved, created and added during the
        // cycle to structured loop transformation.
        if !new_regions.is_empty() {
            let current = current.borrow();
            let parent_region = current.parent().unwrap();
            transform_ctx.dominance_info.info_mut().invalidate_region(parent_region);
        }
        new_regions = transform_ctx.transform_to_structured_cf_branches(current)?;
        // Invalidating the dominance tree is generally not required by the transformation above as
        // the new region entries correspond to unaffected subtrees in the dominator tree. Only its
        // parent nodes have changed but won't be visited again.
        worklist.extend(new_regions);
    }

    Ok(true)
}

/// Checks all preconditions of the transformation prior to any transformations.
///
/// Returns failure if any precondition is violated.
fn check_transformation_preconditions(region: &Region) -> Result<(), Report> {
    use crate::{SuccessorOperands, Walk};

    for block in region.body() {
        if !block.has_predecessors() && !block.is_entry_block() {
            return Err(Report::msg("transformation does not support unreachable blocks"));
        }
    }

    let walk_result = region.prewalk_interruptible(|op: &Operation| {
        if !op.has_successors() {
            return WalkResult::Skip;
        }

        // This transformation requires all ops with successors to implement the branch op interface.
        // It is impossible to adjust their block arguments otherwise.
        let branch_op_interface = match op.as_trait::<dyn BranchOpInterface>().ok_or_else(|| {
            Report::msg(
                "transformation does not support terminators with successors not implementing \
                 BranchOpInterface",
            )
        }) {
            Ok(boi) => boi,
            Err(err) => return WalkResult::Break(err),
        };

        // Branch operations must have no side effects. Replacing them would not be valid otherwise.
        if !op.is_memory_effect_free() {
            return WalkResult::Break(Report::msg(
                "transformation does not support terminators with side effects",
            ));
        }

        for index in 0..op.num_successors() {
            let succ_ops = branch_op_interface.get_successor_operands(index);

            // We cannot support operations with operation-produced successor operands as it is
            // currently not possible to pass them to any block arguments other than the first. This
            // breaks creating multiplexer blocks and would likely need special handling elsewhere
            // too.
            if succ_ops.num_produced() == 0 {
                continue;
            }

            return WalkResult::Break(Report::msg(
                "transformation does not support operations with operation-produced successor \
                 operands",
            ));
        }

        WalkResult::Continue(())
    });

    walk_result.into_result()
}
