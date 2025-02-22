use smallvec::SmallVec;

use super::*;
use crate::{adt::SmallMap, BlockRef, OpBuilder, Report, SourceSpan, Type, ValueRef};

/// Type representing an edge in the CFG.
///
/// Consists of a from-block, a successor and corresponding successor operands passed to the block
/// arguments of the successor.
#[derive(Debug, Copy, Clone)]
pub struct Edge {
    pub from_block: BlockRef,
    pub successor_index: usize,
}

impl Edge {
    pub fn get_from_block(&self) -> BlockRef {
        self.from_block
    }

    pub fn get_successor(&self) -> BlockRef {
        let from_block = self.from_block.borrow();
        from_block.get_successor(self.successor_index)
    }

    pub fn get_predecessor(&self) -> OperationRef {
        let from_block = self.from_block.borrow();
        from_block.terminator().unwrap()
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

/// Utility-class for transforming a region to only have one single block for every return-like
/// operation.
/// Iterates over a range of all edges from `block` to each of its successors.
pub struct SuccessorEdges {
    block: BlockRef,
    num_successors: usize,
    current: usize,
}

impl SuccessorEdges {
    pub fn new(block: BlockRef) -> Self {
        let num_successors = block.borrow().num_successors();
        Self {
            block,
            num_successors,
            current: 0,
        }
    }
}

impl Iterator for SuccessorEdges {
    type Item = Edge;

    fn next(&mut self) -> Option<Self::Item> {
        let successor_index = self.current;
        if successor_index >= self.num_successors {
            return None;
        }
        self.current += 1;
        Some(Edge {
            from_block: self.block,
            successor_index,
        })
    }
}

/// Structure containing the entry, exit and back edges of a cycle.
///
/// A cycle is a generalization of a loop that may have multiple entry edges. See also
/// https://llvm.org/docs/CycleTerminology.html.
#[derive(Debug, Default)]
pub struct CycleEdges {
    /// All edges from a block outside the cycle to a block inside the cycle.
    /// The targets of these edges are entry blocks.
    pub entry_edges: SmallVec<[Edge; 1]>,
    /// All edges from a block inside the cycle to a block outside the cycle.
    pub exit_edges: SmallVec<[Edge; 1]>,
    /// All edges from a block inside the cycle to an entry block.
    pub back_edges: SmallVec<[Edge; 1]>,
}

/// Calculates entry, exit and back edges of the given cycle.
pub fn calculate_cycle_edges(cycles: &[BlockRef]) -> CycleEdges {
    let mut result = CycleEdges::default();
    let mut entry_blocks = SmallSet::<BlockRef, 8>::default();

    // First identify all exit and entry edges by checking whether any successors or predecessors
    // are from outside the cycles.
    for block_ref in cycles.iter().copied() {
        let block = block_ref.borrow();
        for pred in block.predecessors() {
            let from_block = pred.predecessor();
            if cycles.contains(&from_block) {
                continue;
            }

            result.entry_edges.push(Edge {
                from_block,
                successor_index: pred.index as usize,
            });
            entry_blocks.insert(block_ref);
        }

        let terminator = block.terminator().unwrap();
        let terminator = terminator.borrow();
        for succ in terminator.successor_iter() {
            let succ_operand = succ.dest.borrow();
            if cycles.contains(&succ_operand.successor()) {
                continue;
            }

            result.exit_edges.push(Edge {
                from_block: block_ref,
                successor_index: succ_operand.index as usize,
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
            if !entry_blocks.contains(&succ.successor()) {
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

/// Typed used to orchestrate creation of so-called edge multiplexers.
///
/// This class creates a new basic block and routes all inputs edges to this basic block before
/// branching to their original target. The purpose of this transformation is to create single-entry,
/// single-exit regions.
pub struct EdgeMultiplexer<'multiplexer, 'context: 'multiplexer> {
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

        let mut multiplexer_block = transform_ctx.create_block();
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
                transform_ctx.add_block_arguments_from_other(multiplexer_block, entry_block);
            }
        }

        // If we have more than one successor, we have to additionally add a discriminator value,
        // denoting which successor to jump to. When redirecting edges, an appropriate value will be
        // passed using `get_switch_value`.
        let discriminator = if block_arg_mapping.len() > 1 {
            let val = transform_ctx.get_switch_value(0);
            Some(transform_ctx.append_block_argument(
                multiplexer_block,
                val.borrow().ty().clone(),
                span,
            ))
        } else {
            None
        };

        if !extra_args.is_empty() {
            for ty in extra_args {
                transform_ctx.append_block_argument(multiplexer_block, ty.clone(), span);
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

    #[inline(always)]
    pub fn transform(&mut self) -> &mut TransformationContext<'context> {
        self.transform_ctx
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
        let mut terminator_ref = edge.get_predecessor();
        let mut terminator = terminator_ref.borrow_mut();
        let context = terminator.context_rc();
        let mut succ = terminator.successor_mut(edge.successor_index);

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
            if index >= result && index < result + succ.arguments.len() {
                // Original block arguments to the entry block.
                let mut operand = succ.arguments[index - result];
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

        drop(multiplexer_block);

        succ.set(self.multiplexer_block);

        let num_operands = succ.arguments.len();
        for (index, new_operand) in new_succ_operands.into_iter().enumerate() {
            if index < num_operands {
                succ.arguments[index] = new_operand;
                continue;
            }

            succ.arguments.push(new_operand);
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
        let multiplexer_block_args = {
            let multiplexer_block = self.multiplexer_block.borrow();
            SmallVec::<[ValueRef; 4]>::from_iter(
                multiplexer_block.arguments().iter().copied().map(|arg| arg as ValueRef),
            )
        };

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

        self.transform_ctx.interface_mut().create_cfg_switch_op(
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
