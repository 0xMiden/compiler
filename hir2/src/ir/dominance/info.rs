use alloc::{collections::BTreeMap, rc::Rc};
use core::cell::{LazyCell, Ref, RefCell};

use smallvec::SmallVec;

use super::*;
use crate::{
    pass::Analysis, Block, BlockRef, Operation, OperationRef, RegionKindInterface, RegionRef,
};

/// [DominanceInfo] provides a high-level API for querying dominance information.
///
/// Note that this type is aware of the different types of regions, and returns a region-kind
/// specific notion of dominance. See [RegionKindInterface] for details.
#[derive(Default)]
pub struct DominanceInfo {
    info: DominanceInfoBase<false>,
}

impl Analysis for DominanceInfo {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "dominance"
    }

    fn analyze(&mut self, op: &Self::Target, _analysis_manager: crate::pass::AnalysisManager) {
        if !op.has_regions() {
            return;
        }
        self.info.recompute(op);
    }

    fn invalidate(&self, _preserved_analyses: &mut crate::pass::PreservedAnalyses) -> bool {
        true
    }
}

impl DominanceInfo {
    /// Compute the dominance information for `op`
    pub fn new(op: &Operation) -> Self {
        Self {
            info: DominanceInfoBase::new(op),
        }
    }

    #[doc(hidden)]
    #[inline(always)]
    pub(crate) fn info(&self) -> &DominanceInfoBase<false> {
        &self.info
    }

    /// Returns true if `a` dominates `b`.
    ///
    /// Note that if `a == b`, this returns true, if you want strict dominance, see
    /// [Self::properly_dominates] instead.
    ///
    /// The specific details of how dominance is computed is specific to the types involved. See
    /// the implementations of the [Dominates] trait for that information.
    pub fn dominates<A, B>(&self, a: &A, b: &B) -> bool
    where
        A: Dominates<B>,
    {
        a.dominates(b, self)
    }

    /// Returns true if `a` properly dominates `b`.
    ///
    /// This always returns false if `a == b`.
    ///
    /// The specific details of how dominance is computed is specific to the types involved. See
    /// the implementations of the [Dominates] trait for that information.
    pub fn properly_dominates<A, B>(&self, a: &A, b: &B) -> bool
    where
        A: Dominates<B>,
    {
        a.properly_dominates(b, self)
    }

    /// An implementation of `properly_dominates` for operations, where we sometimes wish to treat
    /// `a` as dominating `b`, if `b` is enclosed by a region of `a`. This behavior is controlled
    /// by the `enclosing_op_ok` flag.
    pub fn properly_dominates_with_options(
        &self,
        a: OperationRef,
        mut b: OperationRef,
        enclosing_op_ok: bool,
    ) -> bool {
        let a_block = a.borrow().parent().expect("`a` must be in a block");
        let mut b_block = b.borrow().parent().expect("`b` must be in a block");

        // An instruction dominates itself, but does not properly dominate itself, unless this is
        // a graph region.
        if a == b {
            return !a_block.borrow().has_ssa_dominance();
        }

        // If these ops are in different regions, then normalize one into the other.
        let a_region = a_block.borrow().parent().unwrap();
        if a_region != b_block.borrow().parent().unwrap() {
            // Walk up `b`'s region tree until we find an operation in `a`'s region that encloses
            // it. If this fails, then we know there is no post-dominance relation.
            let Some(found) = a_region.borrow().find_ancestor_op(b) else {
                return false;
            };
            b = found;
            b_block = b.borrow().parent().expect("`b` must be in a block");
            assert!(b_block.borrow().parent().unwrap() == a_region);

            // If `a` encloses `b`, then we consider it to dominate.
            if a == b && enclosing_op_ok {
                return true;
            }
        }

        // Ok, they are in the same region now.
        if a_block == b_block {
            // Dominance changes based on the region type. In a region with SSA dominance, uses
            // insde the same block must follow defs. In other region kinds, uses and defs can
            // come in any order inside a block.
            return if a_block.borrow().has_ssa_dominance() {
                // If the blocks are the same, then check if `b` is before `a` in the block.
                a.borrow().is_before_in_block(&b)
            } else {
                true
            };
        }

        // If the blocks are different, use the dominance tree to resolve the query
        self.info.dominance(a_region).properly_dominates(Some(a_block), Some(b_block))
    }
}

/// [PostDominanceInfo] provides a high-level API for querying post-dominance information.
///
/// Note that this type is aware of the different types of regions, and returns a region-kind
/// specific notion of dominance. See [RegionKindInterface] for details.
#[derive(Default)]
pub struct PostDominanceInfo {
    info: DominanceInfoBase<true>,
}

impl Analysis for PostDominanceInfo {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "post-dominance"
    }

    fn analyze(&mut self, op: &Self::Target, _analysis_manager: crate::pass::AnalysisManager) {
        if !op.has_regions() {
            return;
        }
        self.info.recompute(op);
    }

    fn invalidate(&self, _preserved_analyses: &mut crate::pass::PreservedAnalyses) -> bool {
        true
    }
}

impl PostDominanceInfo {
    /// Compute the post-dominance information for `op`
    pub fn new(op: &Operation) -> Self {
        Self {
            info: DominanceInfoBase::new(op),
        }
    }

    #[doc(hidden)]
    #[inline(always)]
    pub(crate) fn info(&self) -> &DominanceInfoBase<true> {
        &self.info
    }

    /// Returns true if `a` post-dominates `b`.
    ///
    /// Note that if `a == b`, this returns true, if you want strict post-dominance, see
    /// [Self::properly_post_dominates] instead.
    ///
    /// The specific details of how dominance is computed is specific to the types involved. See
    /// the implementations of the [PostDominates] trait for that information.
    pub fn post_dominates<A, B>(&self, a: &A, b: &B) -> bool
    where
        A: PostDominates<B>,
    {
        a.post_dominates(b, self)
    }

    /// Returns true if `a` properly post-dominates `b`.
    ///
    /// This always returns false if `a == b`.
    ///
    /// The specific details of how dominance is computed is specific to the types involved. See
    /// the implementations of the [PostDominates] trait for that information.
    pub fn properly_post_dominates<A, B>(&self, a: &A, b: &B) -> bool
    where
        A: PostDominates<B>,
    {
        a.properly_post_dominates(b, self)
    }
}

/// This type carries the dominance information for a single region, lazily computed on demand.
pub struct RegionDominanceInfo<const IS_POST_DOM: bool> {
    /// The dominator tree for this region
    domtree: LazyCell<Option<Rc<DomTreeBase<IS_POST_DOM>>>, RegionDomTreeCtor<IS_POST_DOM>>,
    /// A flag that indicates where blocks in this region have SSA dominance
    has_ssa_dominance: bool,
}

impl<const IS_POST_DOM: bool> RegionDominanceInfo<IS_POST_DOM> {
    /// Construct a new [RegionDominanceInfo] for `region`
    pub fn new(region: RegionRef) -> Self {
        let r = region.borrow();
        let parent_op = r.parent().unwrap();
        // A region has SSA dominance if it tells us one way or the other, otherwise we must assume
        // that it does.
        let has_ssa_dominance = parent_op
            .borrow()
            .as_trait::<dyn RegionKindInterface>()
            .map(|rki| rki.has_ssa_dominance())
            .unwrap_or(true);

        Self::create(region, has_ssa_dominance, r.has_one_block())
    }

    fn create(region: RegionRef, has_ssa_dominance: bool, has_one_block: bool) -> Self {
        // We only create a dominator tree for multi-block regions
        if has_one_block {
            Self {
                domtree: LazyCell::new(RegionDomTreeCtor(None)),
                has_ssa_dominance,
            }
        } else {
            Self {
                domtree: LazyCell::new(RegionDomTreeCtor(Some(region))),
                has_ssa_dominance,
            }
        }
    }

    /// Get the dominance tree for this region.
    ///
    /// Returns `None` if the region was empty or had only a single block.
    pub fn dominance(&self) -> Option<Rc<DomTreeBase<IS_POST_DOM>>> {
        self.domtree.clone()
    }
}

/// This type provides shared functionality to both [DominanceInfo] and [PostDominanceInfo].
#[derive(Default)]
pub(crate) struct DominanceInfoBase<const IS_POST_DOM: bool> {
    /// A mapping of regions to their dominator tree and a flag that indicates whether or not they
    /// have SSA dominance.
    ///
    /// This map does not contain dominator trees for empty or single block regions, however we
    /// still compute whether or not they have SSA dominance regardless.
    dominance_infos: RefCell<BTreeMap<RegionRef, RegionDominanceInfo<IS_POST_DOM>>>,
}

#[allow(unused)]
impl<const IS_POST_DOM: bool> DominanceInfoBase<IS_POST_DOM> {
    /// Compute dominance information for all of the regions in `op`.
    pub fn new(op: &Operation) -> Self {
        let mut this = Self::default();
        this.recompute(op);
        this
    }

    /// Recompute dominance information for all of the regions in `op`.
    pub fn recompute(&mut self, op: &Operation) {
        let dominance_infos = self.dominance_infos.get_mut();
        dominance_infos.clear();

        let has_ssa_dominance = op
            .as_trait::<dyn RegionKindInterface>()
            .is_none_or(|rki| rki.has_ssa_dominance());
        for region in op.regions() {
            let has_one_block = region.has_one_block();
            let region = region.as_region_ref();
            let info = RegionDominanceInfo::<IS_POST_DOM>::create(
                region,
                has_ssa_dominance,
                has_one_block,
            );
            dominance_infos.insert(region, info);
        }
    }

    /// Invalidate all dominance info.
    ///
    /// This can be used by clients that make major changes to the CFG and don't have a good way to
    /// update it.
    pub fn invalidate(&mut self) {
        self.dominance_infos.get_mut().clear();
    }

    /// Invalidate dominance info for the given region.
    ///
    /// This can be used by clients that make major changes to the CFG and don't have a good way to
    /// update it.
    pub fn invalidate_region(&mut self, region: RegionRef) {
        self.dominance_infos.get_mut().remove(&region);
    }

    /// Finds the nearest common dominator block for the two given blocks `a` and `b`.
    ///
    /// If no common dominator can be found, this function will return `None`.
    pub fn find_nearest_common_dominator_of(
        &self,
        a: Option<BlockRef>,
        b: Option<BlockRef>,
    ) -> Option<BlockRef> {
        // If either `a` or `b` are `None`, then conservatively return `None`
        let a = a?;
        let b = b?;

        // If they are the same block, then we are done.
        if a == b {
            return Some(a);
        }

        // Try to find blocks that are in the same region.
        let (a, b) = Block::get_blocks_in_same_region(a, b)?;

        // If the common ancestor in a common region is the same block, then return it.
        if a == b {
            return Some(a);
        }

        // Otherwise, there must be multiple blocks in the region, check the dominance tree
        self.dominance(a.borrow().parent().unwrap()).find_nearest_common_dominator(a, b)
    }

    /// Finds the nearest common dominator block for the given range of blocks.
    ///
    /// If no common dominator can be found, this function will return `None`.
    pub fn find_nearest_common_dominator_of_all(
        &self,
        mut blocks: impl ExactSizeIterator<Item = BlockRef>,
    ) -> Option<BlockRef> {
        let mut dom = blocks.next();

        for block in blocks {
            dom = self.find_nearest_common_dominator_of(dom, Some(block));
        }

        dom
    }

    /// Get the root dominance node of the given region.
    ///
    /// Panics if `region` is not a multi-block region.
    pub fn root_node(&self, region: RegionRef) -> Rc<DomTreeNode> {
        self.get_dominance_info(region)
            .domtree
            .as_deref()
            .expect("`region` isn't multi-block")
            .root_node()
            .expect("expected region to have a root node")
    }

    /// Return the dominance node for the region containing `block`.
    ///
    /// Panics if `block` is not a member of a multi-block region.
    pub fn node(&self, block: BlockRef) -> Option<Rc<DomTreeNode>> {
        self.get_dominance_info(block.borrow().parent().expect("block isn't attached to region"))
            .domtree
            .as_deref()
            .expect("`block` isn't in a multi-block region")
            .get(Some(block))
    }

    /// Return true if the specified block is reachable from the entry block of its region.
    pub fn is_reachable_from_entry(&self, block: BlockRef) -> bool {
        // If this is the first block in its region, then it is trivially reachable.
        if block.borrow().is_entry_block() {
            return true;
        }

        let region = block.borrow().parent().expect("block isn't attached to region");
        self.dominance(region).is_reachable_from_entry(block)
    }

    /// Return true if operations in the specified block are known to obey SSA dominance rules.
    ///
    /// Returns false if the block is a graph region or unknown.
    pub fn block_has_ssa_dominance(&self, block: BlockRef) -> bool {
        let region = block.borrow().parent().expect("block isn't attached to region");
        self.get_dominance_info(region).has_ssa_dominance
    }

    /// Return true if operations in the specified region are known to obey SSA dominance rules.
    ///
    /// Returns false if the region is a graph region or unknown.
    pub fn region_has_ssa_dominance(&self, region: RegionRef) -> bool {
        self.get_dominance_info(region).has_ssa_dominance
    }

    /// Returns the dominance tree for `region`.
    ///
    /// Panics if `region` is a single-block region.
    pub fn dominance(&self, region: RegionRef) -> Rc<DomTreeBase<IS_POST_DOM>> {
        self.get_dominance_info(region)
            .dominance()
            .expect("cannot get dominator tree for single block regions")
    }

    /// Return the dominance information for `region`.
    ///
    /// NOTE: The dominance tree for single-block regions will be `None`
    fn get_dominance_info(&self, region: RegionRef) -> Ref<'_, RegionDominanceInfo<IS_POST_DOM>> {
        // Check to see if we already have this information.
        self.dominance_infos
            .borrow_mut()
            .entry(region)
            .or_insert_with(|| RegionDominanceInfo::new(region));

        Ref::map(self.dominance_infos.borrow(), |di| &di[&region])
    }

    /// Return true if the specified block A properly dominates block B.
    pub fn properly_dominates(&self, a: BlockRef, mut b: BlockRef) -> bool {
        // A block dominates itself, but does not properly dominate itself.
        if a == b {
            return false;
        }

        // If both blocks are not in the same region, `a` properly dominates `b` if `b` is defined
        // in an operation region that (recursively) ends up being dominated by `a`. Walk up the
        // ancestors of `b`.
        let a_region = a.borrow().parent();
        if a_region != b.borrow().parent() {
            // If we could not find a valid block `b` then it is not a dominator.
            let Some(found) = a_region.as_ref().and_then(|r| r.borrow().find_ancestor_block(b))
            else {
                return false;
            };

            b = found;

            // Check to see if the ancestor of `b` is the same block as `a`. `a` properly dominates
            // `b` if it contains an op that contains the `b` block
            if a == b {
                return true;
            }
        }

        // Otherwise, they are two different blocks in the same region, use dominance tree
        self.dominance(a_region.unwrap()).properly_dominates(Some(a), Some(b))
    }
}

impl DominanceInfoBase<true> {
    #[allow(unused)]
    pub fn root_nodes(&self, region: RegionRef) -> SmallVec<[BlockRef; 4]> {
        self.dominance_infos
            .borrow()
            .get(&region)
            .and_then(|dominfo| dominfo.domtree.as_deref())
            .map(|domtree| domtree.roots().iter().filter_map(|maybe_root| *maybe_root).collect())
            .unwrap_or_default()
    }
}

/// A faux-constructor for [RegionDominanceInfo] for use with [LazyCell] without boxing.
struct RegionDomTreeCtor<const IS_POST_DOM: bool>(Option<RegionRef>);
impl<const IS_POST_DOM: bool> FnOnce<()> for RegionDomTreeCtor<IS_POST_DOM> {
    type Output = Option<Rc<DomTreeBase<IS_POST_DOM>>>;

    extern "rust-call" fn call_once(self, _args: ()) -> Self::Output {
        self.0.and_then(|region| DomTreeBase::new(region).ok().map(Rc::new))
    }
}
impl<const IS_POST_DOM: bool> FnMut<()> for RegionDomTreeCtor<IS_POST_DOM> {
    extern "rust-call" fn call_mut(&mut self, _args: ()) -> Self::Output {
        self.0.and_then(|region| DomTreeBase::new(region).ok().map(Rc::new))
    }
}
impl<const IS_POST_DOM: bool> Fn<()> for RegionDomTreeCtor<IS_POST_DOM> {
    extern "rust-call" fn call(&self, _args: ()) -> Self::Output {
        self.0.and_then(|region| DomTreeBase::new(region).ok().map(Rc::new))
    }
}
