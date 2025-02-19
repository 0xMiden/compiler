use alloc::rc::Rc;

use smallvec::{smallvec, SmallVec};

use super::{
    edges::{Edge, EdgeMultiplexer, SuccessorEdges},
    *,
};
use crate::{
    adt::{SmallMap, SmallSet},
    dominance::{DominanceInfo, PreOrderDomTreeIter},
    Block, BlockRef, Builder, Context, EntityWithParent, FxHashMap, OpBuilder, OperationRef,
    Region, RegionRef, Report, SourceSpan, Spanned, Type, Usable, Value, ValueRef,
};

/// This type represents the necessary context required for performing the control flow lifting
/// transformation over some region.
///
/// It is not meant to be used directly, but as an internal implementation detail of
/// [transform_cfg_to_scf].
pub struct TransformationContext<'a> {
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

    #[inline]
    pub const fn entry(&self) -> BlockRef {
        self.entry
    }

    #[inline]
    pub fn create_block(&self) -> BlockRef {
        self.context.create_block()
    }

    #[inline]
    pub fn append_block_argument(&self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.context.append_block_argument(block, ty, span)
    }

    /// Appends all the block arguments from `other` to the block arguments of `block`, copying their
    /// types and locations.
    pub fn add_block_arguments_from_other(&self, block: BlockRef, other: BlockRef) {
        let other = other.borrow();
        for arg in other.arguments() {
            let arg = arg.borrow();
            self.context.append_block_argument(block, arg.ty().clone(), arg.span());
        }
    }

    pub fn invalidate_dominance_info_for_region(&mut self, region: RegionRef) {
        self.dominance_info.info_mut().invalidate_region(region);
    }

    #[inline(always)]
    pub fn interface_mut(&mut self) -> &mut dyn CFGToSCFInterface {
        self.interface
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

    pub fn garbage_collect(&mut self) {
        // If any of the temporary switch values we created are unused, remove them now
        for value in self.switch_value_cache.drain(..).flatten() {
            let mut defining_op = {
                let val = value.borrow();
                if val.is_used() {
                    continue;
                }
                val.get_defining_op().unwrap()
            };

            defining_op.borrow_mut().erase();
        }

        for (_, value) in self.typed_undef_cache.drain() {
            let mut defining_op = {
                let val = value.borrow();
                if val.is_used() {
                    continue;
                }
                val.get_defining_op().unwrap()
            };

            defining_op.borrow_mut().erase();
        }
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
                drop(block);
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
        match self.return_like_to_combined_exit.entry(key) {
            Entry::Occupied(entry) => {
                if OperationRef::ptr_eq(&entry.key().0, &return_like_op_ref) {
                    return Ok(());
                }

                let mut return_like_op = return_like_op_ref.borrow_mut();

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
                let mut return_like_op = return_like_op_ref.borrow_mut();
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

                return_like_op.move_to(crate::ProgramPoint::at_end_of(exit_block));
                let exit_block = exit_block.borrow();
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

        let scc_iter = StronglyConnectedComponents::new(&region_entry);

        for scc in scc_iter {
            if !scc.has_cycle() {
                continue;
            }

            // Save the set and increment the SCC iterator early to avoid our modifications breaking
            // the SCC iterator.
            let edges = edges::calculate_cycle_edges(scc.as_slice());
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

            // Finally, turn it into reduce form.
            let iteration_values = self.transform_to_reduce_loop(
                loop_header,
                exit_block_ref,
                cycle_block_set.as_slice(),
            );

            // Create a block acting as replacement for the loop header and insert the structured
            // loop into it.
            let mut new_loop_parent_block_ref = self.context.create_block();
            new_loop_parent_block_ref.borrow_mut().insert_before(loop_header);
            self.add_block_arguments_from_other(new_loop_parent_block_ref, loop_header);

            let mut region_ref = region_entry.borrow().parent().unwrap();

            let mut loop_body_ref = self.context.alloc_tracked(Region::default());
            {
                let mut region = region_ref.borrow_mut();
                let blocks = region.body_mut();
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
            }

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
            new_loop_parent_block_ref.borrow_mut().splice_block(&mut exit_block);

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
        let num_successors = region_entry.borrow().num_successors();

        // Trivial region.
        if num_successors == 0 {
            return Ok(Default::default());
        }

        // Single successor we can just splice on to the entry block.
        if num_successors == 1 {
            let region_entry_block = region_entry.borrow();
            let mut successor = region_entry_block.get_successor(0);
            let succ = successor.borrow();
            // Replace all uses of the successor block arguments (if any) with the operands of the
            // block terminator
            let mut entry_terminator = region_entry_block.terminator().unwrap();
            let mut terminator = entry_terminator.borrow_mut();
            let terminator_succ = terminator.successor(0);
            for (mut old_value, new_value) in
                succ.arguments().iter().copied().zip(terminator_succ.arguments)
            {
                let mut old_value = old_value.borrow_mut();
                old_value.replace_all_uses_with(new_value.borrow().as_value_ref());
            }

            // Erase the original region entry block terminator, as it will be replaced with the
            // contents of the successor block once spliced
            //
            // NOTE: In order to erase the terminator, we must not be borrowing its parent block
            drop(region_entry_block);
            drop(succ);
            terminator.drop_all_references();
            terminator.erase();

            let mut succ = successor.borrow_mut();

            // Splice the operations of `succ` to `region_entry`
            region_entry.borrow_mut().splice_block(&mut succ);

            // Erase the successor block now that we have emptied it
            succ.erase();

            return Ok(smallvec![region_entry]);
        }

        // Split the CFG into "#num_successors + 1" regions.
        //
        // For every edge to a successor, the blocks it solely dominates are determined and become
        // the region following that edge. The last region is the continuation that follows the
        // branch regions.
        let mut not_continuation = SmallSet::<BlockRef, 8>::default();
        not_continuation.insert(region_entry);

        let mut successor_branch_regions = SmallVec::<[SmallVec<[BlockRef; 2]>; 2]>::default();
        successor_branch_regions.resize_with(num_successors, Default::default);

        let terminator = region_entry.borrow().terminator().unwrap();
        {
            let terminator = terminator.borrow();
            for (block_list, succ) in
                successor_branch_regions.iter_mut().zip(terminator.successor_iter())
            {
                let dest = succ.dest.borrow().successor();

                // If the region entry is not the only predecessor, then the edge does not dominate the
                // block it leads to.
                if dest.borrow().get_single_predecessor().is_none() {
                    continue;
                }

                // Otherwise get all blocks it dominates in DFS/pre-order.
                let node = self.dominance_info.info().node(dest).unwrap();
                for curr in PreOrderDomTreeIter::new(node) {
                    if let Some(block) = curr.block() {
                        block_list.push(block);
                        not_continuation.insert(block);
                    }
                }
            }
        }

        // Finds all relevant edges and checks the shape of the control flow graph at this point.
        //
        // Branch regions may either:
        //
        // * Be post-dominated by the continuation
        // * Be post-dominated by a return-like op
        // * Dominate a return-like op and have an edge to the continuation.
        //
        // The control flow graph may then be one of three cases:
        //
        // 1) All branch regions are post-dominated by the continuation. This is the usual case. If
        //    there are multiple entry blocks into the continuation a single entry block has to be
        //    created. A structured control flow op can then be created from the branch regions.
        //
        // 2) No branch region has an edge to a continuation:
        //
        //                                 +-----+
        //                           +-----+ bb0 +----+
        //                           v     +-----+    v
        //                Region 1 +-+--+    ...     +-+--+ Region n
        //                         |ret1|            |ret2|
        //                         +----+            +----+
        //
        //   This can only occur if every region ends with a different kind of return-like op. In
        //   that case the control flow operation must stay as we are unable to create a single
        //   exit-block. We can nevertheless process all its successors as they single-entry,
        //   single-exit regions.
        //
        // 3) Only some branch regions are post-dominated by the continuation. The other branch
        //    regions may either be post-dominated by a return-like op or lead to either the
        //    continuation or return-like op. In this case we also create a single entry block like
        //    in Case 1 that also includes all edges to the return-like op:
        //
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
        // bb0 to bbM is now a single-entry, single-exit region that applies to Case 1. The control
        // flow op at the end of bbM will trigger Case 2.
        let mut continuation_edges = SmallVec::<[Edge; 2]>::default();
        let mut continuation_post_dominates_all_regions = true;
        let mut no_successor_has_continuation_edge = true;

        for (entry_edge, branch_region) in
            SuccessorEdges::new(region_entry).zip(successor_branch_regions.iter_mut())
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
                            from_block: pred.predecessor(),
                            successor_index: pred.index as usize,
                        });
                    }
                    continue;
                }

                for edge in SuccessorEdges::new(*block_ref) {
                    if not_continuation.contains(&edge.get_successor()) {
                        continue;
                    }

                    continuation_edges.push(edge);
                    no_successor_has_continuation_edge = false;
                }
            }
        }

        // Case 2: Keep the control flow op but process its successors further.
        if no_successor_has_continuation_edge {
            let term = region_entry.borrow().terminator().unwrap();
            let term = term.borrow();
            return Ok(term.successor_iter().map(|s| s.dest.borrow().successor()).collect());
        }

        // Collapse to a single continuation block, or None
        let mut continuation = continuation_edges.iter().fold(None, |acc, e| match acc {
            None => Some(e.get_successor()),
            Some(prev) => {
                if BlockRef::ptr_eq(&prev, &e.get_successor()) {
                    Some(prev)
                } else {
                    None
                }
            }
        });

        // In Case 3, or if not all continuation edges have the same entry block, create a single
        // entry block as continuation for all branch regions.
        if continuation.is_none() || !continuation_post_dominates_all_regions {
            let term = continuation_edges[0].get_from_block().borrow().terminator().unwrap();
            let span = term.borrow().span();
            let multiplexer = self.create_single_entry_block(span, &continuation_edges)?;
            continuation = Some(multiplexer.get_multiplexer_block());
        }

        // Trigger reprocessing of Case 3 after creating the single entry block.
        if !continuation_post_dominates_all_regions {
            // Unlike in the general case, we are explicitly revisiting the same region entry again
            // after having changed its control flow edges and dominance. We have to therefore
            // explicitly invalidate the dominance tree.
            let region = region_entry.borrow().parent().unwrap();
            self.dominance_info.info_mut().invalidate_region(region);
            return Ok(smallvec![region_entry]);
        }

        let mut continuation = continuation.unwrap();
        let mut new_sub_regions = SmallVec::<[BlockRef; 4]>::default();

        // Empty blocks with the values they return to the parent op.
        let mut created_empty_blocks =
            SmallVec::<[(BlockRef, SmallVec<[ValueRef; 4]>); 2]>::default();

        // Create the branch regions.
        let mut conditional_regions = SmallVec::<[RegionRef; 2]>::default();
        for (branch_region, entry_edge) in
            successor_branch_regions.iter_mut().zip(SuccessorEdges::new(region_entry))
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

            let arg_types = {
                let cont = continuation.borrow();
                cont.arguments()
                    .iter()
                    .map(|arg| arg.borrow().ty().clone())
                    .collect::<SmallVec<[_; 2]>>()
            };
            let mut terminator = region_entry.borrow().terminator().unwrap();
            let op = self.interface.create_structured_branch_region_op(
                &mut builder,
                terminator,
                &arg_types,
                &mut conditional_regions,
            )?;
            let mut term = terminator.borrow_mut();
            term.drop_all_references();
            term.erase();
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
        for (mut old_value, new_value) in cont
            .arguments()
            .iter()
            .copied()
            .zip(structured_cond.results().iter().map(|r| r.borrow().as_value_ref()))
        {
            old_value.borrow_mut().replace_all_uses_with(new_value);
        }

        // Splice together the continuations operations with the region entry.
        region_entry.borrow_mut().splice_block(&mut cont);

        // Remove the empty continuation block
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
        let latch_term = latch_terminator.borrow();
        // Take a snapshot of the loop header successor operands as we cannot hold a reference to
        // them and mutate them at the same time
        let mut loop_header_successor_operands = latch_term
            .successor(loop_header_index)
            .arguments
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        drop(latch_term);

        // Add all values used in the next iteration to the exit block. Replace any uses that are
        // outside the loop with the newly created exit block.
        for mut arg in loop_header_successor_operands.iter().copied() {
            let argument = arg.borrow();
            let exit_arg = self.context.append_block_argument(
                exit_block,
                argument.ty().clone(),
                argument.span(),
            );
            drop(argument);

            let operand = self.context.make_operand(arg, latch_terminator, 0);
            {
                let mut latch_term = latch_terminator.borrow_mut();
                latch_term.successor_mut(exit_block_index).arguments.push(operand);
            }
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
                let mut block_argument = None;
                let mut next_use = { value.borrow().uses().front().as_pointer() };
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
                        let (value_ty, span, value_block) = {
                            let val = value.borrow();
                            (val.ty().clone(), val.span(), val.parent_block().unwrap())
                        };
                        block_argument = Some(ctx.context.append_block_argument(
                            exit_block,
                            value_ty.clone(),
                            span,
                        ));
                        ctx.context.append_block_argument(loop_header, value_ty.clone(), span);

                        // `value` might be defined in a block that does not dominate `latch` but
                        // previously dominated an exit block with a use. In this case, add a block
                        // argument to the latch and go through all predecessors. If the value
                        // dominates the predecessor, pass the value as a successor operand,
                        // otherwise pass undef. The above is unnecessary if the value is a block
                        // argument of the latch or if `value` dominates all predecessors.
                        let mut argument = value;
                        if value_block != latch
                            && latch_block.predecessors().any(|pred| {
                                !loop_block_dominates(
                                    pred.owner.borrow().parent().unwrap(),
                                    ctx.dominance_info,
                                )
                            })
                        {
                            argument =
                                ctx.context.append_block_argument(latch, value_ty.clone(), span);
                            for pred in latch_block.predecessors() {
                                let mut succ_operand = value;
                                if !loop_block_dominates(
                                    pred.owner.borrow().parent().unwrap(),
                                    ctx.dominance_info,
                                ) {
                                    succ_operand = ctx.get_undef_value(&value_ty);
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
                        for edge in SuccessorEdges::new(latch) {
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
                for arg in latch_block_arguments_prior.iter().map(|arg| arg.borrow().as_value_ref())
                {
                    check_value(self, arg);
                }
            } else if BlockRef::ptr_eq(loop_block_ref, &loop_header) {
                for arg in loop_header_arguments_prior.iter().map(|arg| arg.borrow().as_value_ref())
                {
                    check_value(self, arg);
                }
            } else {
                let loop_block = loop_block_ref.borrow();
                for arg in loop_block.arguments().iter().map(|arg| arg.borrow().as_value_ref()) {
                    check_value(self, arg);
                }
            }

            let loop_block = loop_block_ref.borrow();
            for op in loop_block.body() {
                for result in op.results().iter().map(|res| res.borrow().as_value_ref()) {
                    check_value(self, result);
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
        let context = self.context.clone();
        let mut multiplexer = EdgeMultiplexer::create(self, span, &entry_blocks, &[]);

        // Redirect the edges prior to creating the switch op. We guarantee that predecessors are up
        // to date.
        for edge in entry_edges {
            multiplexer.redirect_edge(edge, &[]);
        }

        let mut builder = OpBuilder::new(context);
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
            for edge in SuccessorEdges::new(block_ref) {
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
                    self.add_block_arguments_from_other(seb, continuation);
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

            let mut brp = branch_region_parent.borrow_mut();
            unsafe {
                let mut cursor = brp.body_mut().cursor_mut_from_ptr(block_ref);
                cursor.remove();
                Block::on_removed_from_parent(block_ref, branch_region_parent);
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
        let context = self.context.clone();
        let mut multiplexer = EdgeMultiplexer::create(self, span, &successors, &extra_args);

        let latch_block = multiplexer.get_multiplexer_block();

        // Create a separate exit block that comes right after the latch.
        let mut exit_block = multiplexer.transform().create_block();
        exit_block.borrow_mut().insert_after(latch_block);

        // Since this is a loop, all back edges point to the same loop header.
        let loop_header = back_edges[0].get_successor();

        // Redirect the edges prior to creating the switch op. We guarantee that predecessors are up
        // to date.

        // Redirecting back edges with `should_repeat` as 1.
        for edge in back_edges {
            let extra_args = [multiplexer.transform().get_switch_value(1)];
            multiplexer.redirect_edge(edge, &extra_args);
        }

        // Redirecting exits edges with `should_repeat` as 0.
        for edge in exit_edges {
            let extra_args = [multiplexer.transform().get_switch_value(0)];
            multiplexer.redirect_edge(edge, &extra_args);
        }

        // Create the new only back edge to the loop header. Branch to the exit block otherwise.
        let should_repeat = latch_block.borrow().arguments().last().copied().unwrap();
        let should_repeat = should_repeat.borrow().as_value_ref();
        {
            let mut builder = OpBuilder::new(context.clone());
            builder.set_insertion_point_to_end(latch_block);

            let num_args = loop_header.borrow().num_arguments();
            let latch_args = {
                let latch_block = latch_block.borrow();
                latch_block
                    .arguments()
                    .iter()
                    .take(num_args)
                    .map(|arg| arg.borrow().as_value_ref())
                    .collect::<SmallVec<[ValueRef; 4]>>()
            };
            multiplexer.transform().interface_mut().create_conditional_branch(
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
            let mut builder = OpBuilder::new(context);
            builder.set_insertion_point_to_end(exit_block);

            if exit_edges.is_empty() {
                // A loop without an exit edge is a statically known infinite loop.
                // Since structured control flow ops are not terminator ops, the caller has to
                // create a fitting return-like unreachable terminator operation.
                let region = latch_block.borrow().parent().unwrap();
                let terminator = multiplexer
                    .transform()
                    .interface_mut()
                    .create_unreachable_terminator(span, &mut builder, region)?;
                // Transform the just created transform operation in the case that an occurrence of
                // it existed in input IR.
                multiplexer.transform().combine_exit(terminator)?;
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

/// Special loop properties of a structured loop.
///
/// A structured loop is a loop satisfying all of the following:
///
/// * Has at most one entry, one exit and one back edge.
/// * The back edge originates from the same block as the exit edge.
#[derive(Debug)]
struct StructuredLoopProperties {
    /// Block containing both the single exit edge and the single back edge.
    latch: BlockRef,
    /// Loop condition of type equal to a value returned by `getSwitchValue`.
    condition: ValueRef,
    /// Exit block which is the only successor of the loop.
    exit_block: BlockRef,
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

/// Returns true if this block is an exit block of the region.
fn is_region_exit_block(block: &Block) -> bool {
    block.num_successors() == 0
}
