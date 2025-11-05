use alloc::{collections::BTreeMap, format, rc::Rc};
use core::{
    cell::{Cell, Ref, RefCell, RefMut},
    fmt,
};

use smallvec::SmallVec;

use super::{
    dominance::{DominanceInfo, DominanceTree, PostOrderDomTreeIter},
    RegionKindInterface, RegionRef,
};
use crate::{
    adt::{SmallDenseMap, SmallSet},
    cfg::{Graph, Inverse, InvertibleGraph},
    pass::Analysis,
    BlockRef, Operation, OperationRef, PostOrderBlockIter, Report,
};

/// Represents the results of analyzing an [Operation] and computing the [LoopForest] for each of
/// the op's regions.
///
/// This type implements [Analysis], so can be used in conjunction with other passes.
#[derive(Default)]
pub struct LoopInfo {
    per_region: SmallVec<[IntraRegionLoopInfo; 2]>,
}

/// Represents loop information for a intra-region CFG contained in `region`
struct IntraRegionLoopInfo {
    /// The region which contains the CFG
    pub region: RegionRef,
    /// The loop forest for the CFG in `region`
    pub forest: LoopForest,
}

impl Analysis for LoopInfo {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "loops"
    }

    fn analyze(
        &mut self,
        op: &Self::Target,
        analysis_manager: crate::pass::AnalysisManager,
    ) -> Result<(), Report> {
        // If the op has no regions, or it does, but they are graph regions, do not compute the
        // forest, as it cannot succeed.
        if !op.has_regions()
            || op
                .as_trait::<dyn RegionKindInterface>()
                .is_some_and(|rki| rki.has_graph_regions())
        {
            return Ok(());
        }

        // First, obtain the dominance info for this op
        let dominfo = analysis_manager.get_analysis::<DominanceInfo>()?;
        // Then compute the forests for each region of the op
        for region in op.regions() {
            // If this region has a single block, the loop forest is empty
            if region.has_one_block() {
                self.per_region.push(IntraRegionLoopInfo {
                    region: region.as_region_ref(),
                    forest: LoopForest::default(),
                });
                continue;
            }

            // Otherwise, compute it for this region
            let region = region.as_region_ref();
            let forest = LoopForest::new(&dominfo.info().dominance(region));
            self.per_region.push(IntraRegionLoopInfo { region, forest });
        }

        Ok(())
    }

    fn invalidate(&self, preserved_analyses: &mut crate::pass::PreservedAnalyses) -> bool {
        // Don't invalidate the LoopForest analysis unless the dominance tree was invalidated
        !preserved_analyses.is_preserved::<DominanceInfo>()
    }
}

impl LoopInfo {
    /// Returns true if the op this info was derived from contains any loops
    pub fn has_loops(&self) -> bool {
        !self.per_region.is_empty() && !self.per_region.iter().any(|info| !info.forest.is_empty())
    }

    /// Returns true if `region` has loops according to this loop info
    pub fn region_has_loops(&self, region: &RegionRef) -> bool {
        self.per_region
            .iter()
            .find_map(|info| {
                if &info.region == region {
                    Some(!info.forest.is_empty())
                } else {
                    None
                }
            })
            .unwrap_or(false)
    }

    /// Get the [LoopForest] for `region`
    pub fn get(&self, region: &RegionRef) -> Option<&LoopForest> {
        self.per_region.iter().find_map(|info| {
            if &info.region == region {
                Some(&info.forest)
            } else {
                None
            }
        })
    }
}

/// [LoopForest] represents all of the top-level loop structures in a specified region.
///
/// The [LoopForest] analysis is used to identify natural loops and determine the loop depth of
/// various nodes in a generic graph of blocks.  A natural loop has exactly one entry-point, which
/// is called the header. Note that natural loops may actually be several loops that share the same
/// header node.
///
/// This analysis calculates the nesting structure of loops in a function.  For each natural loop
/// identified, this analysis identifies natural loops contained entirely within the loop and the
/// basic blocks that make up the loop.
///
/// It can calculate on the fly various bits of information, for example:
///
/// * Whether there is a preheader for the loop
/// * The number of back edges to the header
/// * Whether or not a particular block branches out of the loop
/// * The successor blocks of the loop
/// * The loop depth
/// * etc...
///
/// Note that this analysis specifically identifies _loops_ not cycles or SCCs in the graph.  There
/// can be strongly connected components in the graph which this analysis will not recognize and
/// that will not be represented by a loop instance.  In particular, a loop might be inside such a
/// non-loop SCC, or a non-loop SCC might contain a sub-SCC which is a loop.
///
/// For an overview of terminology used in this API (and thus all related loop analyses or
/// transforms), see [Loop Terminology](https://llvm.org/docs/LoopTerminology.html).
#[derive(Default)]
pub struct LoopForest {
    /// The set of top-level loops in the forest
    top_level_loops: SmallVec<[Rc<Loop>; 4]>,
    /// Mapping of basic blocks to the inner most loop they occur in
    block_map: BTreeMap<BlockRef, Rc<Loop>>,
}

impl LoopForest {
    /// Compute a new [LoopForest] from the given dominator tree.
    pub fn new(tree: &DominanceTree) -> Self {
        let mut forest = Self::default();
        forest.analyze(tree);
        forest
    }

    /// Returns true if there are no loops in the forest
    pub fn is_empty(&self) -> bool {
        self.top_level_loops.is_empty()
    }

    /// Returns the number of loops in the forest
    pub fn len(&self) -> usize {
        self.top_level_loops.len()
    }

    /// Returns true if `block` is in this loop forest
    #[inline]
    pub fn contains_block(&self, block: BlockRef) -> bool {
        self.block_map.contains_key(&block)
    }

    /// Get the set of top-level/outermost loops in the forest
    pub fn top_level_loops(&self) -> &[Rc<Loop>] {
        &self.top_level_loops
    }

    /// Return all of the loops in the function in preorder across the loop nests, with siblings in
    /// forward program order.
    ///
    /// Note that because loops form a forest of trees, preorder is equivalent to reverse postorder.
    pub fn loops_in_preorder(&self) -> SmallVec<[Rc<Loop>; 4]> {
        // The outer-most loop actually goes into the result in the same relative order as we walk
        // it. But LoopForest stores the top level loops in reverse program order so for here we
        // reverse it to get forward program order.
        //
        // FIXME: If we change the order of LoopForest we will want to remove the reverse here.
        let mut preorder_loops = SmallVec::<[Rc<Loop>; 4]>::default();
        for l in self.top_level_loops.iter().cloned().rev() {
            let mut loops_in_preorder = l.loops_in_preorder();
            preorder_loops.append(&mut loops_in_preorder);
        }
        preorder_loops
    }

    /// Return all of the loops in the function in preorder across the loop nests, with siblings in
    /// _reverse_ program order.
    ///
    /// Note that because loops form a forest of trees, preorder is equivalent to reverse postorder.
    ///
    /// Also note that this is _not_ a reverse preorder. Only the siblings are in reverse program
    /// order.
    pub fn loops_in_reverse_sibling_preorder(&self) -> SmallVec<[Rc<Loop>; 4]> {
        // The outer-most loop actually goes into the result in the same relative order as we walk
        // it. LoopForest stores the top level loops in reverse program order so we walk in order
        // here.
        //
        // FIXME: If we change the order of LoopInfo we will want to add a reverse here.
        let mut preorder_loops = SmallVec::<[Rc<Loop>; 4]>::default();
        let mut preorder_worklist = SmallVec::<[Rc<Loop>; 4]>::default();
        for l in self.top_level_loops.iter().cloned() {
            assert!(preorder_worklist.is_empty());
            preorder_worklist.push(l);
            while let Some(l) = preorder_worklist.pop() {
                // Sub-loops are stored in forward program order, but will process the worklist
                // backwards so we can just append them in order.
                preorder_worklist.extend(l.nested().iter().cloned());
                preorder_loops.push(l);
            }
        }

        preorder_loops
    }

    /// Return the inner most loop that `block` lives in.
    ///
    /// If a basic block is in no loop (for example the entry node), `None` is returned.
    pub fn loop_for(&self, block: BlockRef) -> Option<Rc<Loop>> {
        self.block_map.get(&block).cloned()
    }

    /// Return the loop nesting level of the specified block.
    ///
    /// A depth of 0 means the block is not inside any loop.
    pub fn loop_depth(&self, block: BlockRef) -> usize {
        self.loop_for(block).map(|l| l.depth()).unwrap_or(0)
    }

    /// Returns true if the block is a loop header
    pub fn is_loop_header(&self, block: BlockRef) -> bool {
        self.loop_for(block).map(|l| l.header() == block).unwrap_or(false)
    }

    /// This removes the specified top-level loop from this loop info object.
    ///
    /// The loop is not deleted, as it will presumably be inserted into another loop.
    ///
    /// # Panics
    ///
    /// This function will panic if the given loop is not a top-level loop
    pub fn remove_loop(&mut self, l: &Loop) -> Option<Rc<Loop>> {
        assert!(l.is_outermost(), "`l` is not an outermost loop");
        let index = self.top_level_loops.iter().position(|tll| core::ptr::addr_eq(&**tll, l))?;
        Some(self.top_level_loops.swap_remove(index))
    }

    /// Change the top-level loop that contains `block` to the specified loop.
    ///
    /// This should be used by transformations that restructure the loop hierarchy tree.
    pub fn change_loop_for(&mut self, block: BlockRef, l: Option<Rc<Loop>>) {
        if let Some(l) = l {
            self.block_map.insert(block, l);
        } else {
            self.block_map.remove(&block);
        }
    }

    /// Replace the specified loop in the top-level loops list with the indicated loop.
    pub fn change_top_level_loop(&mut self, old: Rc<Loop>, new: Rc<Loop>) {
        assert!(
            new.parent_loop().is_none() && old.parent_loop().is_none(),
            "loops already embedded into a subloop"
        );
        let index = self
            .top_level_loops
            .iter()
            .position(|tll| Rc::ptr_eq(tll, &old))
            .expect("`old` loop is not a top-level loop");
        self.top_level_loops[index] = new;
    }

    /// This adds the specified loop to the collection of top-level loops.
    pub fn add_top_level_loop(&mut self, l: Rc<Loop>) {
        assert!(l.is_outermost(), "loop already in subloop");
        self.top_level_loops.push(l);
    }

    /// This method completely removes `block` from all data structures, including all of the loop
    /// objects it is nested in and our mapping from basic blocks to loops.
    pub fn remove_block(&mut self, block: BlockRef) {
        if let Some(l) = self.block_map.remove(&block) {
            let mut next_l = Some(l);
            while let Some(l) = next_l.take() {
                next_l = l.parent_loop();
                l.remove_block_from_loop(block);
            }
        }
    }

    pub fn is_not_already_contained_in(sub_loop: Option<&Loop>, parent: Option<&Loop>) -> bool {
        let Some(sub_loop) = sub_loop else {
            return true;
        };
        if parent.is_some_and(|parent| parent == sub_loop) {
            return false;
        }
        Self::is_not_already_contained_in(sub_loop.parent_loop().as_deref(), parent)
    }

    /// Analyze the given dominance tree to discover loops.
    ///
    /// The analysis discovers loops during a post-order traversal of the given dominator tree,
    /// interleaved with backward CFG traversals within each subloop
    /// (see `discover_and_map_subloop`). The backward traversal skips inner subloops, so this part
    /// of the algorithm is linear in the number of CFG edges. Subloop and block vectors are then
    /// populated during a single forward CFG traversal.
    ///
    /// During the two CFG traversals each block is seen three times:
    ///
    /// 1. Discovered and mapped by a reverse CFG traversal.
    /// 2. Visited during a forward DFS CFG traversal.
    /// 3. Reverse-inserted in the loop in postorder following forward DFS.
    ///
    /// The block vectors are inclusive, so step 3 requires loop-depth number of insertions per
    /// block.
    pub fn analyze(&mut self, tree: &DominanceTree) {
        // Postorder traversal of the dominator tree.
        let Some(root) = tree.root_node() else {
            return;
        };
        for node in PostOrderDomTreeIter::new(root.clone()) {
            let header = node.block().expect("expected header block");
            let mut backedges = SmallVec::<[BlockRef; 4]>::default();

            // Check each predecessor of the potential loop header.
            for backedge in BlockRef::inverse_children(header) {
                // If `header` dominates `pred`, this is a new loop. Collect the backedges.
                let backedge_node = tree.get(Some(backedge));
                if backedge_node.is_some() && tree.dominates_node(Some(node.clone()), backedge_node)
                {
                    backedges.push(backedge);
                }
            }

            // Perform a backward CFG traversal to discover and map blocks in this loop.
            if !backedges.is_empty() {
                let l = Rc::new(Loop::new(header));
                self.discover_and_map_sub_loop(l, backedges, tree);
            }
        }

        // Perform a single forward CFG traversal to populate blocks and subloops for all loops.
        for block in PostOrderBlockIter::new(root.block().unwrap()) {
            self.insert_into_loop(block);
        }
    }

    /// Discover a subloop with the specified backedges such that:
    ///
    /// * All blocks within this loop are mapped to this loop or a subloop.
    /// * All subloops within this loop have their parent loop set to this loop or a subloop.
    fn discover_and_map_sub_loop(
        &mut self,
        l: Rc<Loop>,
        backedges: SmallVec<[BlockRef; 4]>,
        tree: &DominanceTree,
    ) {
        let mut num_blocks = 0usize;
        let mut num_subloops = 0usize;

        // Perform a backward CFG traversal using a worklist.
        let mut reverse_cfg_worklist = backedges;
        while let Some(pred) = reverse_cfg_worklist.pop() {
            match self.loop_for(pred) {
                None if !tree.is_reachable_from_entry(pred) => continue,
                None => {
                    // This is an undiscovered block. Map it to the current loop.
                    self.change_loop_for(pred, Some(l.clone()));
                    num_blocks += 1;
                    if pred == l.header() {
                        continue;
                    }

                    // Push all block predecessors on the worklist
                    reverse_cfg_worklist.extend(Inverse::<BlockRef>::children(pred));
                }
                Some(subloop) => {
                    // This is a discovered block. Find its outermost discovered loop.
                    let subloop = subloop.outermost_loop();

                    // If it is already discovered to be a subloop of this loop, continue.
                    if subloop == l {
                        continue;
                    }

                    // Discover a subloop of this loop.
                    subloop.set_parent_loop(Some(l.clone()));
                    num_subloops += 1;
                    num_blocks += subloop.num_blocks();

                    // Continue traversal along predecessors that are not loop-back edges from
                    // within this subloop tree itself. Note that a predecessor may directly reach
                    // another subloop that is not yet discovered to be a subloop of this loop,
                    // which we must traverse.
                    for pred in BlockRef::inverse_children(subloop.header()) {
                        if self.loop_for(pred).is_none_or(|l| l != subloop) {
                            reverse_cfg_worklist.push(pred);
                        }
                    }
                }
            }
        }

        l.nested.borrow_mut().reserve(num_subloops);
        l.reserve(num_blocks);
    }

    /// Add a single block to its ancestor loops in post-order.
    ///
    /// If the block is a subloop header, add the subloop to its parent in post-order, then reverse
    /// the block and subloop vectors of the now complete subloop to achieve RPO.
    fn insert_into_loop(&mut self, block: BlockRef) {
        let mut subloop = self.loop_for(block);
        if let Some(sl) = subloop.clone().filter(|sl| sl.header() == block) {
            let parent = sl.parent_loop();
            // We reach this point once per subloop after processing all the blocks in the subloop.
            if sl.is_outermost() {
                self.add_top_level_loop(sl.clone());
            } else {
                parent.as_ref().unwrap().nested.borrow_mut().push(sl.clone());
            }

            // For convenience, blocks and subloops are inserted in postorder. Reverse the lists,
            // except for the loop header, which is always at the beginning.
            sl.reverse_blocks(1);
            sl.nested.borrow_mut().reverse();
            subloop = parent;
        }

        while let Some(sl) = subloop.take() {
            sl.add_block_entry(block);
            subloop = sl.parent_loop();
        }
    }

    /// Verify the loop forest structure using the provided [DominanceTree]
    pub fn verify(&self, tree: &DominanceTree) -> Result<(), Report> {
        let mut loops = SmallSet::<Rc<Loop>, 2>::default();
        for l in self.top_level_loops.iter().cloned() {
            if !l.is_outermost() {
                return Err(Report::msg("top-level loop has a parent"));
            }
            l.verify_loop_nest(&mut loops)?;
        }

        if cfg!(debug_assertions) {
            // Verify that blocks are mapped to valid loops.
            for (block, block_loop) in self.block_map.iter() {
                let block = *block;
                if !loops.contains(block_loop) {
                    return Err(Report::msg("orphaned loop"));
                }
                if !block_loop.contains_block(block) {
                    return Err(Report::msg("orphaned block"));
                }
                for child_loop in block_loop.nested().iter() {
                    if child_loop.contains_block(block) {
                        return Err(Report::msg(
                            "expected block map to reflect the innermost loop containing `block`",
                        ));
                    }
                }
            }

            // Recompute forest to verify loops structure.
            let other = LoopForest::new(tree);

            // Build a map we can use to move from our forest to the newly computed one. This allows
            // us to ignore the particular order in any layer of the loop forest while still
            // comparing the structure.
            let mut other_headers = SmallDenseMap::<BlockRef, Rc<Loop>, 8>::default();

            fn add_inner_loops_to_headers_map(
                headers: &mut SmallDenseMap<BlockRef, Rc<Loop>, 8>,
                l: &Rc<Loop>,
            ) {
                let header = l.header();
                headers.insert(header, Rc::clone(l));
                for sl in l.nested().iter() {
                    add_inner_loops_to_headers_map(headers, sl);
                }
            }

            for l in other.top_level_loops() {
                add_inner_loops_to_headers_map(&mut other_headers, l);
            }

            // Walk the top level loops and ensure there is a corresponding top-level loop in the
            // computed version and then recursively compare those loop nests.
            for l in self.top_level_loops() {
                let header = l.header();
                let other_l = other_headers.remove(&header);
                match other_l {
                    None => {
                        return Err(Report::msg(
                            "top level loop is missing in computed loop forest",
                        ))
                    }
                    Some(other_l) => {
                        // Recursively compare the loops
                        Self::compare_loops(l.clone(), other_l, &mut other_headers)?;
                    }
                }
            }

            // Any remaining entries in the map are loops which were found when computing a fresh
            // loop forest but not present in the current one.
            if !other_headers.is_empty() {
                for (_header, header_loop) in other_headers {
                    log::trace!("Found new loop {header_loop:?}");
                }
                return Err(Report::msg("found new loops when recomputing loop forest"));
            }
        }

        Ok(())
    }

    #[cfg(debug_assertions)]
    fn compare_loops(
        l: Rc<Loop>,
        other_l: Rc<Loop>,
        other_loop_headers: &mut SmallDenseMap<BlockRef, Rc<Loop>, 8>,
    ) -> Result<(), Report> {
        use crate::EntityWithId;

        let header = l.header();
        let other_header = other_l.header();
        if header != other_header {
            return Err(Report::msg(
                "mismatched headers even though found under the same map entry",
            ));
        }

        if l.depth() != other_l.depth() {
            return Err(Report::msg("mismatched loop depth"));
        }

        {
            let mut parent_l = Some(l.clone());
            let mut other_parent_l = Some(other_l.clone());
            while let Some(pl) = parent_l.take() {
                if let Some(opl) = other_parent_l.take() {
                    if pl.header() != opl.header() {
                        return Err(Report::msg("mismatched parent loop headers"));
                    }
                    parent_l = pl.parent_loop();
                    other_parent_l = opl.parent_loop();
                } else {
                    return Err(Report::msg(
                        "`other_l` misreported its depth: expected a parent and got none",
                    ));
                }
            }
        }

        for sl in l.nested().iter() {
            let sl_header = sl.header();
            let other_sl = other_loop_headers.remove(&sl_header);
            match other_sl {
                None => return Err(Report::msg("inner loop is missing in computed loop forest")),
                Some(other_sl) => {
                    Self::compare_loops(sl.clone(), other_sl, other_loop_headers)?;
                }
            }
        }

        let mut blocks = l.blocks.borrow().clone();
        let mut other_blocks = other_l.blocks.borrow().clone();
        blocks.sort_by_key(|b| b.borrow().id());
        other_blocks.sort_by_key(|b| b.borrow().id());
        if blocks != other_blocks {
            log::trace!("blocks:       {}", crate::formatter::DisplayValues::new(blocks.iter()));
            log::trace!(
                "other_blocks: {}",
                crate::formatter::DisplayValues::new(other_blocks.iter())
            );
            return Err(Report::msg("loops report mismatched blocks"));
        }

        let block_set = l.block_set();
        let other_block_set = other_l.block_set();
        let diff = block_set.symmetric_difference(&other_block_set);
        if block_set.len() != other_block_set.len() || !diff.is_empty() {
            log::trace!(
                "block_set:       {}",
                crate::formatter::DisplayValues::new(block_set.iter())
            );
            log::trace!(
                "other_block_set: {}",
                crate::formatter::DisplayValues::new(other_block_set.iter())
            );
            log::trace!("diff:            {}", crate::formatter::DisplayValues::new(diff.iter()));
            return Err(Report::msg("loops report mismatched block sets"));
        }

        Ok(())
    }

    #[cfg(not(debug_assertions))]
    fn compare_loops(
        _l: Rc<Loop>,
        _other_l: Rc<Loop>,
        _other_loop_headers: &mut SmallDenseMap<BlockRef, Rc<Loop>, 8>,
    ) -> Result<(), Report> {
        Ok(())
    }
}

impl fmt::Debug for LoopForest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LoopInfo")
            .field("top_level_loops", &self.top_level_loops)
            .field("block_map", &self.block_map)
            .finish()
    }
}

/// Edge type.
pub type LoopEdge = (BlockRef, BlockRef);

/// [Loop] is used to represent loops that are detected in the control-flow graph.
#[derive(Default)]
pub struct Loop {
    /// If this loop is an outermost loop, this field is `None`.
    ///
    /// Otherwise, it holds a handle to the parent loop which transfers control to this loop.
    parent_loop: Cell<Option<Rc<Loop>>>,
    /// Loops contained entirely within this one.
    ///
    /// All of the loops in this set will have their `parent` set to this loop
    nested: RefCell<SmallVec<[Rc<Loop>; 2]>>,
    /// The list of blocks in this loop.
    ///
    /// The header block is always at index 0.
    blocks: RefCell<SmallVec<[BlockRef; 32]>>,
    /// The uniqued set of blocks present in this loop
    block_set: RefCell<SmallSet<BlockRef, 8>>,
}

impl Eq for Loop {}
impl PartialEq for Loop {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self, other)
    }
}

impl Loop {
    /// Create a new [Loop] with `block` as its header.
    pub fn new(block: BlockRef) -> Self {
        let mut this = Self::default();
        this.blocks.get_mut().push(block);
        this.block_set.get_mut().insert(block);
        this
    }

    /// Get the nesting level of this loop.
    ///
    /// An outer-most loop has depth 1, for consistency with loop depth values used for basic
    /// blocks, where depth 0 is used for blocks not inside any loops.
    pub fn depth(&self) -> usize {
        let mut depth = 1;
        let mut current_loop = self.parent_loop();
        while let Some(curr) = current_loop.take() {
            depth += 1;
            current_loop = curr.parent_loop();
        }
        depth
    }

    /// Get the header block of this loop
    pub fn header(&self) -> BlockRef {
        self.blocks.borrow()[0]
    }

    /// Return the parent loop of this loop, if it has one, or `None` if it is a top-level loop.
    ///
    /// A loop is either top-level in a function (that is, it is not contained in any other loop) or
    /// it is entirely enclosed in some other loop. If a loop is top-level, it has no parent,
    /// otherwise its parent is the innermost loop in which it is enclosed.
    pub fn parent_loop(&self) -> Option<Rc<Loop>> {
        unsafe { (*self.parent_loop.as_ptr()).clone() }
    }

    /// This is a low-level API for bypassing [add_child_loop].
    pub fn set_parent_loop(&self, parent: Option<Rc<Loop>>) {
        self.parent_loop.set(parent);
    }

    /// Discover the outermost loop that contains `self`
    pub fn outermost_loop(self: Rc<Loop>) -> Rc<Loop> {
        let mut l = self;
        while let Some(parent) = l.parent_loop() {
            l = parent;
        }
        l
    }

    /// Return true if the specified loop is contained within in this loop.
    pub fn contains(&self, l: Rc<Loop>) -> bool {
        if core::ptr::addr_eq(self, &*l) {
            return true;
        }

        let Some(parent) = l.parent_loop() else {
            return false;
        };

        self.contains(parent)
    }

    /// Returns true if the specified basic block is in this loop
    pub fn contains_block(&self, block: BlockRef) -> bool {
        self.block_set.borrow().contains(&block)
    }

    /// Returns true if the specified operation is in this loop
    pub fn contains_op(&self, op: &OperationRef) -> bool {
        let Some(block) = op.parent() else {
            return false;
        };
        self.contains_block(block)
    }

    /// Return the loops contained entirely within this loop.
    pub fn nested(&self) -> Ref<'_, [Rc<Loop>]> {
        Ref::map(self.nested.borrow(), |nested| nested.as_slice())
    }

    /// Return true if the loop does not contain any (natural) loops.
    ///
    /// [Loop] does not detect irreducible control flow, just natural loops. That is, it is possible
    /// that there is cyclic control flow within the innermost loop or around the outermost loop.
    pub fn is_innermost(&self) -> bool {
        self.nested.borrow().is_empty()
    }

    /// Return true if the loop does not have a parent (natural) loop (i.e. it is outermost, which
    /// is the same as top-level).
    pub fn is_outermost(&self) -> bool {
        unsafe { (*self.parent_loop.as_ptr()).is_none() }
    }

    /// Get a list of the basic blocks which make up this loop.
    pub fn blocks(&self) -> Ref<'_, [BlockRef]> {
        Ref::map(self.blocks.borrow(), |blocks| blocks.as_slice())
    }

    /// Get a mutable reference to the basic blocks which make up this loop.
    pub fn blocks_mut(&self) -> RefMut<'_, SmallVec<[BlockRef; 32]>> {
        self.blocks.borrow_mut()
    }

    /// Return the number of blocks contained in this loop
    pub fn num_blocks(&self) -> usize {
        self.blocks.borrow().len()
    }

    /// Return a reference to the blocks set.
    pub fn block_set(&self) -> Ref<'_, SmallSet<BlockRef, 8>> {
        self.block_set.borrow()
    }

    /// Return a mutable reference to the blocks set.
    pub fn block_set_mut(&self) -> RefMut<'_, SmallSet<BlockRef, 8>> {
        self.block_set.borrow_mut()
    }

    /// Returns true if the terminator of `block` can branch to another block that is outside of the
    /// current loop.
    ///
    /// # Panics
    ///
    /// This function will panic if `block` is not inside this loop.
    pub fn is_loop_exiting(&self, block: BlockRef) -> bool {
        assert!(self.contains_block(block), "exiting block must be part of the loop");
        BlockRef::children(block).any(|succ| !self.contains_block(succ))
    }

    /// Returns true if `block` is a loop-latch.
    ///
    /// A latch block is a block that contains a branch back to the header.
    ///
    /// This function is useful when there are multiple latches in a loop because `get_loop_latch`
    /// will return `None` in that case.
    pub fn is_loop_latch(&self, block: BlockRef) -> bool {
        assert!(self.contains_block(block), "block does not belong to the loop");
        BlockRef::inverse_children(self.header()).any(|pred| pred == block)
    }

    /// Calculate the number of back edges to the loop header
    pub fn num_backedges(&self) -> usize {
        BlockRef::inverse_children(self.header())
            .filter(|pred| self.contains_block(*pred))
            .count()
    }
}

/// Loop Analysis
///
/// Note that all of these methods can fail on general loops (ie, there may not be a preheader,
/// etc).  For best success, the loop simplification and induction variable canonicalization pass
/// should be used to normalize loops for easy analysis.  These methods assume canonical loops.
impl Loop {
    /// Get all blocks inside the loop that have successors outside of the loop.
    ///
    /// These are the blocks _inside of the current loop_ which branch out. The returned list is
    /// always unique.
    pub fn exiting_blocks(&self) -> SmallVec<[BlockRef; 2]> {
        let mut exiting_blocks = SmallVec::default();
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                // A block must be an exit block if it is not contained in the current loop
                if !self.contains_block(succ) {
                    exiting_blocks.push(block);
                    break;
                }
            }
        }
        exiting_blocks
    }

    /// If [Self::exiting_blocks] would return exactly one block, return it, otherwise `None`.
    pub fn exiting_block(&self) -> Option<BlockRef> {
        let mut exiting_block = None;
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                if !self.contains_block(succ) {
                    if exiting_block.is_some() {
                        return None;
                    } else {
                        exiting_block = Some(block);
                    }
                    break;
                }
            }
        }
        exiting_block
    }

    /// Get all of the successor blocks of this loop.
    ///
    /// These are the blocks _outside of the current loop_ which are branched to.
    pub fn exit_blocks(&self) -> SmallVec<[BlockRef; 2]> {
        let mut exit_blocks = SmallVec::default();
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                if !self.contains_block(succ) {
                    exit_blocks.push(succ);
                }
            }
        }
        exit_blocks
    }

    /// If [Self::exit_blocks] would return exactly one block, return it, otherwise `None`.
    pub fn exit_block(&self) -> Option<BlockRef> {
        let mut exit_block = None;
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                if !self.contains_block(succ) {
                    if exit_block.is_some() {
                        return None;
                    } else {
                        exit_block = Some(succ);
                    }
                }
            }
        }
        exit_block
    }

    /// Returns true if no exit block for the loop has a predecessor that is outside the loop.
    pub fn has_dedicated_exits(&self) -> bool {
        // Each predecessor of each exit block of a normal loop is contained within the loop.
        for exit_block in self.unique_exit_blocks() {
            for pred in BlockRef::inverse_children(exit_block) {
                if !self.contains_block(pred) {
                    return false;
                }
            }
        }

        // All the requirements are met.
        true
    }

    /// Return all unique successor blocks of this loop.
    ///
    /// These are the blocks _outside of the current loop_ which are branched to.
    pub fn unique_exit_blocks(&self) -> SmallVec<[BlockRef; 2]> {
        let mut unique_exits = SmallVec::default();
        unique_exit_blocks_helper(self, &mut unique_exits, |_| true);
        unique_exits
    }

    /// Return all unique successor blocks of this loop, except successors from the latch block
    /// which are not considered. If an exit that comes from the latch block, but also has a non-
    /// latch predecessor in the loop, it will be included.
    ///
    /// These are the blocks _outside of the current loop_ which are branched to.
    pub fn unique_non_latch_exit_blocks(&self) -> SmallVec<[BlockRef; 2]> {
        let latch_block = self.loop_latch().expect("latch must exist");
        let mut unique_exits = SmallVec::default();
        unique_exit_blocks_helper(self, &mut unique_exits, |block| block != latch_block);
        unique_exits
    }

    /// If [Self::unique_exit_blocks] would return exactly one block, return it, otherwise `None`.
    #[inline]
    pub fn unique_exit_block(&self) -> Option<BlockRef> {
        self.exit_block()
    }

    /// Return true if this loop does not have any exit blocks.
    pub fn has_no_exit_blocks(&self) -> bool {
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                if !self.contains_block(succ) {
                    return false;
                }
            }
        }
        true
    }

    /// Return all pairs of (_inside_block_, _outside_block_).
    pub fn exit_edges(&self) -> SmallVec<[LoopEdge; 2]> {
        let mut exit_edges = SmallVec::default();
        for block in self.blocks.borrow().iter().copied() {
            for succ in BlockRef::children(block) {
                if !self.contains_block(succ) {
                    exit_edges.push((block, succ));
                }
            }
        }
        exit_edges
    }

    /// Returns the pre-header for this loop, if there is one.
    ///
    /// A loop has a pre-header if there is only one edge to the header of the loop from outside of
    /// the loop. If this is the case, the block branching to the header of the loop is the
    /// pre-header node.
    ///
    /// This returns `None` if there is no pre-header for the loop.
    pub fn preheader(&self) -> Option<BlockRef> {
        use crate::IteratorExt;

        // Keep track of nodes outside the loop branching to the header...
        let out = self.loop_predecessor()?;

        // Make sure we are allowed to hoist instructions into the predecessor.
        if !out.borrow().is_legal_to_hoist_into() {
            return None;
        }

        // Make sure there is only one exit out of the preheader.
        if !BlockRef::children(out).has_single_element() {
            // Multiple exits from the block, must not be a preheader.
            return None;
        }

        // The predecessor has exactly one successor, so it is a preheader.
        Some(out)
    }

    /// If the given loop's header has exactly one unique predecessor outside the loop, return it.
    ///
    /// This is less strict than the loop "preheader" concept, which requires the predecessor to
    /// have exactly one successor.
    pub fn loop_predecessor(&self) -> Option<BlockRef> {
        // Keep track of nodes outside the loop branching to the header...
        let mut out = None;
        // Loop over the predecessors of the header node...
        let header = self.header();
        for pred in BlockRef::inverse_children(header) {
            if !self.contains_block(pred) {
                if out.as_ref().is_some_and(|out| out != &pred) {
                    // Multiple predecessors outside the loop
                    return None;
                }
                out = Some(pred);
            }
        }
        out
    }

    /// If there is a single latch block for this loop, return it.
    ///
    /// A latch block is a block that contains a branch back to the header.
    pub fn loop_latch(&self) -> Option<BlockRef> {
        let header = self.header();
        let mut latch_block = None;
        for pred in BlockRef::inverse_children(header) {
            if self.contains_block(pred) {
                if latch_block.is_some() {
                    return None;
                }
                latch_block = Some(pred);
            }
        }
        latch_block
    }

    /// Get all loop latch blocks of this loop.
    ///
    /// A latch block is a block that contains a branch back to the header.
    pub fn loop_latches(&self) -> SmallVec<[BlockRef; 2]> {
        BlockRef::inverse_children(self.header())
            .filter(|pred| self.contains_block(*pred))
            .collect()
    }

    /// Return all inner loops in the loop nest rooted by the loop in preorder, with siblings in
    /// forward program order.
    pub fn inner_loops_in_preorder(&self) -> SmallVec<[Rc<Loop>; 2]> {
        let mut worklist = SmallVec::<[Rc<Loop>; 4]>::default();
        worklist.extend(self.nested().iter().rev().cloned());

        let mut results = SmallVec::default();
        while let Some(l) = worklist.pop() {
            // Sub-loops are stored in forward program order, but will process the
            // worklist backwards so append them in reverse order.
            worklist.extend(l.nested().iter().rev().cloned());
            results.push(l);
        }

        results
    }

    /// Return all loops in the loop nest rooted by the loop in preorder, with siblings in forward
    /// program order.
    pub fn loops_in_preorder(self: Rc<Self>) -> SmallVec<[Rc<Loop>; 2]> {
        let mut loops = self.inner_loops_in_preorder();
        loops.insert(0, self);
        loops
    }
}

fn unique_exit_blocks_helper<F>(
    l: &Loop,
    exit_blocks: &mut SmallVec<[BlockRef; 2]>,
    mut predicate: F,
) where
    F: FnMut(BlockRef) -> bool,
{
    let mut visited = SmallSet::<BlockRef, 32>::default();
    for block in l.blocks.borrow().iter().copied().filter(|b| predicate(*b)) {
        for succ in BlockRef::children(block) {
            if !l.contains_block(succ) && visited.insert(succ) {
                exit_blocks.push(succ);
            }
        }
    }
}

/// Updates
impl Loop {
    /// Add `block` to this loop, and as a member of all parent loops.
    ///
    /// It is not valid to replace the loop header using this function.
    ///
    /// This is intended for use by analyses which need to update loop information.
    pub fn add_block_to_loop(self: Rc<Self>, block: BlockRef, forest: &mut LoopForest) {
        assert!(!forest.contains_block(block), "`block` is already in this loop");

        // Add the loop mapping to the LoopForest object...
        forest.block_map.insert(block, self.clone());

        // Add the basic block to this loop and all parent loops...
        let mut next_l = Some(self);
        while let Some(l) = next_l.take() {
            l.add_block_entry(block);
            next_l = l.parent_loop();
        }
    }

    /// Replace `prev` with `new` in the set of children of this loop, updating the parent pointer
    /// of `prev` to `None`, and of `new` to `self`.
    ///
    /// This also updates the loop depth of the new child.
    ///
    /// This is intended for use when splitting loops up.
    pub fn replace_child_loop_with(self: Rc<Self>, prev: Rc<Loop>, new: Rc<Loop>) {
        assert_eq!(prev.parent_loop().as_ref(), Some(&self), "this loop is already broken");
        assert!(new.parent_loop().is_none(), "`new` already has a parent");

        // Set the parent of `new` to `self`
        new.set_parent_loop(Some(self.clone()));
        // Replace `prev` in `self.nested` with `new`
        let mut nested = self.nested.borrow_mut();
        let entry = nested.iter_mut().find(|l| Rc::ptr_eq(l, &prev)).expect("`prev` not in loop");
        let _ = core::mem::replace(entry, new);
        // Set the parent of `prev` to `None`
        prev.set_parent_loop(None);
    }

    /// Add the specified loop to be a child of this loop.
    ///
    /// This updates the loop depth of the new child.
    pub fn add_child_loop(self: Rc<Self>, child: Rc<Loop>) {
        assert!(child.parent_loop().is_none(), "child already has a parent");
        child.set_parent_loop(Some(self.clone()));
        self.nested.borrow_mut().push(child);
    }

    /// This removes subloops of this loop based on the provided predicate, and returns them in a
    /// vector.
    ///
    /// The loops are not deleted, as they will presumably be inserted into another loop.
    pub fn take_child_loops<F>(&self, should_remove: F) -> SmallVec<[Rc<Loop>; 2]>
    where
        F: Fn(&Loop) -> bool,
    {
        let mut taken = SmallVec::default();
        self.nested.borrow_mut().retain(|l| {
            if should_remove(l) {
                l.set_parent_loop(None);
                taken.push(Rc::clone(l));
                false
            } else {
                true
            }
        });
        taken
    }

    /// This removes the specified child from being a subloop of this loop.
    ///
    /// The loop is not deleted, as it will presumably be inserted into another loop.
    pub fn take_child_loop(&self, child: &Loop) -> Option<Rc<Loop>> {
        let mut nested = self.nested.borrow_mut();
        let index = nested.iter().position(|l| core::ptr::addr_eq(&**l, child))?;
        Some(nested.swap_remove(index))
    }

    /// This adds a basic block directly to the basic block list.
    ///
    /// This should only be used by transformations that create new loops.  Other transformations
    /// should use [add_block_to_loop].
    pub fn add_block_entry(&self, block: BlockRef) {
        self.blocks.borrow_mut().push(block);
        self.block_set.borrow_mut().insert(block);
    }

    /// Reverse the order of blocks in this loop starting from `index` to the end.
    pub fn reverse_blocks(&self, index: usize) {
        self.blocks.borrow_mut()[index..].reverse();
    }

    /// Reserve capacity for `capacity` blocks
    pub fn reserve(&self, capacity: usize) {
        self.blocks.borrow_mut().reserve(capacity);
    }

    /// This method is used to move `block` (which must be part of this loop) to be the loop header
    /// of the loop (the block that dominates all others).
    pub fn move_to_header(&self, block: BlockRef) {
        let mut blocks = self.blocks.borrow_mut();
        let index = blocks.iter().position(|b| *b == block).expect("loop does not contain `block`");
        if index == 0 {
            return;
        }
        unsafe {
            blocks.swap_unchecked(0, index);
        }
    }

    /// This removes the specified basic block from the current loop, updating the `self.blocks` as
    /// appropriate. This does not update the mapping in the corresponding [LoopInfo].
    pub fn remove_block_from_loop(&self, block: BlockRef) {
        let mut blocks = self.blocks.borrow_mut();
        let index = blocks.iter().position(|b| *b == block).expect("loop does not contain `block`");
        blocks.swap_remove(index);
        self.block_set.borrow_mut().remove(&block);
    }

    /// Verify loop structure
    #[cfg(debug_assertions)]
    pub fn verify_loop(&self) -> Result<(), Report> {
        use crate::PreOrderBlockIter;

        if self.blocks.borrow().is_empty() {
            return Err(Report::msg("loop header is missing"));
        }

        // Setup for using a depth-first iterator to visit every block in the loop.
        let exit_blocks = self.exit_blocks();
        let mut visit_set = SmallSet::<BlockRef, 8>::default();
        visit_set.extend(exit_blocks.iter().cloned());

        // Keep track of the BBs visited.
        let mut visited_blocks = SmallSet::<BlockRef, 8>::default();

        // Check the individual blocks.
        let header = self.header();
        for block in PreOrderBlockIter::new_with_visited(header, exit_blocks.iter().cloned()) {
            let has_in_loop_successors = BlockRef::children(block).any(|b| self.contains_block(b));
            if !has_in_loop_successors {
                return Err(Report::msg("loop block has no in-loop successors"));
            }

            let has_in_loop_predecessors =
                BlockRef::inverse_children(block).any(|b| self.contains_block(b));
            if !has_in_loop_predecessors {
                return Err(Report::msg("loop block has no in-loop predecessors"));
            }

            let outside_loop_preds = BlockRef::inverse_children(block)
                .filter(|b| !self.contains_block(*b))
                .collect::<SmallVec<[BlockRef; 2]>>();

            if block == header && outside_loop_preds.is_empty() {
                return Err(Report::msg("loop is unreachable"));
            } else if !outside_loop_preds.is_empty() {
                // A non-header loop shouldn't be reachable from outside the loop, though it is
                // permitted if the predecessor is not itself actually reachable.
                let entry = block.parent().unwrap().borrow().entry_block_ref().unwrap();
                for child_block in PreOrderBlockIter::new(entry) {
                    if outside_loop_preds.iter().any(|pred| &child_block == pred) {
                        return Err(Report::msg("loop has multiple entry points"));
                    }
                }
            }
            if block != header.parent().unwrap().borrow().entry_block_ref().unwrap() {
                return Err(Report::msg("loop contains region entry block"));
            }
            visited_blocks.insert(block);
        }

        if visited_blocks.len() != self.num_blocks() {
            log::trace!("The following blocks are unreachable in the loop: ");
            for block in self.blocks().iter() {
                if !visited_blocks.contains(block) {
                    log::trace!("{block}");
                }
            }
            return Err(Report::msg("unreachable block in loop"));
        }

        // Check the subloops
        for subloop in self.nested().iter() {
            // Each block in each subloop should be contained within this loop.
            for block in subloop.blocks().iter() {
                if !self.contains_block(*block) {
                    return Err(Report::msg(
                        "loop does not contain all the blocks of its subloops",
                    ));
                }
            }
        }

        // Check the parent loop pointer.
        if let Some(parent) = self.parent_loop() {
            if !parent.nested().contains(&parent) {
                return Err(Report::msg("loop is not a subloop of its parent"));
            }
        }

        Ok(())
    }

    #[cfg(not(debug_assertions))]
    pub fn verify_loop(&self) -> Result<(), Report> {
        Ok(())
    }

    /// Verify loop structure of this loop and all nested loops.
    pub fn verify_loop_nest(
        self: Rc<Self>,
        loops: &mut SmallSet<Rc<Loop>, 2>,
    ) -> Result<(), Report> {
        loops.insert(self.clone());

        // Verify this loop.
        self.verify_loop()?;

        // Verify the subloops.
        for l in self.nested.borrow().iter().cloned() {
            l.verify_loop_nest(loops)?;
        }

        Ok(())
    }

    /// Print loop with all the blocks inside it.
    pub fn print(&self, verbose: bool) -> impl fmt::Display + '_ {
        PrintLoop {
            loop_info: self,
            nested: true,
            verbose,
        }
    }
}

struct PrintLoop<'a> {
    loop_info: &'a Loop,
    nested: bool,
    verbose: bool,
}

impl crate::formatter::PrettyPrint for PrintLoop<'_> {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let mut doc = const_text("loop containing: ");
        let header = self.loop_info.header();
        for (i, block) in self.loop_info.blocks().iter().copied().enumerate() {
            if !self.verbose {
                if i > 0 {
                    doc += const_text(", ");
                }
                doc += display(block);
            } else {
                doc += nl();
            }

            if block == header {
                doc += const_text("<header>");
            } else if self.loop_info.is_loop_latch(block) {
                doc += const_text("<latch>");
            } else if self.loop_info.is_loop_exiting(block) {
                doc += const_text("<exiting>");
            }

            if self.verbose {
                doc += text(format!("{:?}", &block.borrow()));
            }
        }

        if self.nested {
            let nested = self.loop_info.nested().iter().fold(Document::Empty, |acc, l| {
                let printer = PrintLoop {
                    loop_info: l,
                    nested: true,
                    verbose: self.verbose,
                };
                acc + nl() + printer.render()
            });
            doc + indent(2, nested)
        } else {
            doc
        }
    }
}

impl fmt::Display for PrintLoop<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::formatter::PrettyPrint;
        self.pretty_print(f)
    }
}

impl fmt::Display for Loop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.print(false))
    }
}
impl fmt::Debug for Loop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Loop")
            .field("parent_loop", &self.parent_loop())
            .field("nested", &self.nested())
            .field("blocks", &self.blocks())
            .field("block_set", &self.block_set())
            .finish()
    }
}
