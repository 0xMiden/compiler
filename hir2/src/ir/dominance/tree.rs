use alloc::rc::Rc;
use core::{
    cell::{Cell, RefCell},
    fmt,
    num::NonZeroU32,
};

use smallvec::{smallvec, SmallVec};

use super::{BatchUpdateInfo, SemiNCA};
use crate::{
    cfg::{self, Graph, Inverse, InvertibleGraph},
    formatter::DisplayOptional,
    BlockRef, EntityId, EntityWithId, RegionRef,
};

#[derive(Debug, thiserror::Error)]
pub enum DomTreeError {
    /// Tried to compute a dominator tree for an empty region
    #[error("unable to create dominance tree for empty region")]
    EmptyRegion,
}

/// The level of verification to use with [DominatorTreeBase::verify]
pub enum DomTreeVerificationLevel {
    /// Checks basic tree structure and compares with a freshly constructed tree
    ///
    /// O(n^2) time worst case, but is faster in practice.
    Fast,
    /// Checks if the tree is correct, but compares it to a freshly constructed tree instead of
    /// checking the sibling property.
    ///
    /// O(n^2) time.
    Basic,
    /// Verifies if the tree is correct by making sure all the properties, including the parent
    /// and sibling property, hold.
    ///
    /// O(n^3) time.
    Full,
}

/// A forward dominance tree
pub type DominanceTree = DomTreeBase<false>;

/// A post (backward) dominance tree
pub type PostDominanceTree = DomTreeBase<true>;

pub type DomTreeRoots = SmallVec<[Option<BlockRef>; 4]>;

/// A dominator tree implementation that abstracts over the type of dominance it represents.
pub struct DomTreeBase<const IS_POST_DOM: bool> {
    /// The roots from which dominance is traced.
    ///
    /// For forward dominance trees, there is always a single root. For post-dominance trees, there
    /// may be multiple, one for each exit from the region.
    roots: DomTreeRoots,
    /// The nodes represented in this dominance tree
    #[allow(clippy::type_complexity)]
    nodes: SmallVec<[Option<(Option<BlockRef>, Rc<DomTreeNode>)>; 64]>,
    /// The root dominance tree node.
    root: Option<Rc<DomTreeNode>>,
    /// The parent region for which this dominance tree was computed
    parent: RegionRef,
    /// Whether this dominance tree is valid (true), or outdated (false)
    valid: Cell<bool>,
    /// A counter for expensive queries that may cause us to perform some extra work in order to
    /// speed up those queries after a certain point.
    slow_queries: Cell<u32>,
}

/// A node in a [DomTreeBase].
pub struct DomTreeNode {
    /// The block represented by this node
    block: Option<BlockRef>,
    /// The immediate dominator of this node, if applicable
    idom: Cell<Option<Rc<DomTreeNode>>>,
    /// The children of this node in the tree
    children: RefCell<SmallVec<[Rc<DomTreeNode>; 4]>>,
    /// The depth of this node in the tree
    level: Cell<u32>,
    /// The DFS visitation order (forward)
    num_in: Cell<Option<NonZeroU32>>,
    /// The DFS visitation order (backward)
    num_out: Cell<Option<NonZeroU32>>,
}

impl fmt::Display for DomTreeNode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", DisplayOptional(self.block.as_ref().map(|b| b.borrow().id()).as_ref()))
    }
}

impl fmt::Debug for DomTreeNode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::EntityWithId;

        f.debug_struct("DomTreeNode")
            .field_with("block", |f| match self.block.as_ref() {
                None => f.write_str("None"),
                Some(block_ref) => write!(f, "{}", block_ref.borrow().id()),
            })
            .field("idom", &unsafe { &*self.idom.as_ptr() }.as_ref().map(|n| n.block))
            .field_with("children", |f| {
                f.debug_list()
                    .entries(self.children.borrow().iter().map(|child| child.block))
                    .finish()
            })
            .field("level", &self.level.get())
            .field("num_in", &self.num_in.get())
            .field("num_out", &self.num_out.get())
            .finish()
    }
}

impl<const IS_POST_DOM: bool> fmt::Debug for DomTreeBase<IS_POST_DOM> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_struct("DomTreeBase");
        builder
            .field("valid", &self.valid.get())
            .field("slow_queries", &self.slow_queries.get())
            .field_with("root", |f| match self.root.as_ref().and_then(|root| root.block) {
                Some(root) => write!(f, "{root}"),
                None => f.write_str("<virtual>"),
            });
        if IS_POST_DOM {
            builder.field_with("roots", |f| {
                let mut builder = f.debug_set();
                for root in self.roots.iter() {
                    builder.entry_with(|f| match root {
                        Some(root) => write!(f, "{root}"),
                        None => f.write_str("<virtual>"),
                    });
                }
                builder.finish()
            });
        }

        builder.field_with("nodes", |f| {
            f.debug_set()
                .entries(self.nodes.iter().filter_map(|node| node.as_ref().map(|(_, n)| n.clone())))
                .finish()
        });

        builder.finish()
    }
}

/// An iterator over nodes in a dominance tree produced by a depth-first, pre-order traversal
pub type PreOrderDomTreeIter = cfg::PreOrderIter<Rc<DomTreeNode>>;
/// An iterator over nodes in a dominance tree produced by a depth-first, post-order traversal
pub type PostOrderDomTreeIter = cfg::PostOrderIter<Rc<DomTreeNode>>;

impl Graph for Rc<DomTreeNode> {
    type ChildEdgeIter = DomTreeSuccessorIter;
    type ChildIter = DomTreeSuccessorIter;
    type Edge = Rc<DomTreeNode>;
    type Node = Rc<DomTreeNode>;

    fn size(&self) -> usize {
        self.children.borrow().len()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        DomTreeSuccessorIter::new(parent)
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        DomTreeSuccessorIter::new(parent)
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        // The edge is the child node
        edge
    }

    fn entry_node(&self) -> Self::Node {
        Rc::clone(self)
    }
}

pub struct DomTreeSuccessorIter {
    node: Rc<DomTreeNode>,
    num_children: usize,
    index: usize,
}
impl DomTreeSuccessorIter {
    pub fn new(node: Rc<DomTreeNode>) -> Self {
        let num_children = node.num_children();
        Self {
            node,
            num_children,
            index: 0,
        }
    }
}
impl core::iter::FusedIterator for DomTreeSuccessorIter {}
impl ExactSizeIterator for DomTreeSuccessorIter {
    #[inline]
    fn len(&self) -> usize {
        self.num_children.saturating_sub(self.index)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.index >= self.num_children
    }
}
impl Iterator for DomTreeSuccessorIter {
    type Item = Rc<DomTreeNode>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.num_children {
            return None;
        }
        let index = self.index;
        self.index += 1;
        Some(self.node.children.borrow()[index].clone())
    }
}
impl DoubleEndedIterator for DomTreeSuccessorIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.num_children == 0 {
            return None;
        }
        let index = self.num_children;
        self.num_children -= 1;
        Some(self.node.children.borrow()[index].clone())
    }
}

impl DomTreeNode {
    /// Create a new node for `block`, with the specified immediate dominator.
    ///
    /// If `block` is `None`, this must be a node in a post-dominator tree, and the resulting node
    /// is a virtual node that post-dominates all nodes in the tree
    pub fn new(block: Option<BlockRef>, idom: Option<Rc<DomTreeNode>>) -> Self {
        let this = Self {
            block,
            idom: Cell::new(None),
            children: Default::default(),
            level: Cell::new(0),
            num_in: Cell::new(None),
            num_out: Cell::new(None),
        };
        if let Some(idom) = idom {
            this.with_idom(idom)
        } else {
            this
        }
    }

    /// Build this node with the specified immediate dominator.
    pub fn with_idom(self, idom: Rc<Self>) -> Self {
        self.level.set(idom.level.get() + 1);
        self.idom.set(Some(idom));
        self
    }

    pub fn block(&self) -> Option<BlockRef> {
        self.block
    }

    pub fn idom(&self) -> Option<Rc<Self>> {
        unsafe { &*self.idom.as_ptr() }.clone()
    }

    pub(super) fn set_idom(self: Rc<Self>, new_idom: Rc<Self>) {
        let idom = self.idom.take().expect("no immediate dominator?");
        if idom == new_idom {
            self.idom.set(Some(idom));
            return;
        }

        {
            let mut children = idom.children.borrow_mut();
            let child_index = children
                .iter()
                .position(|n| Rc::ptr_eq(n, &self))
                .expect("not in immediate dominator children!");
            children.remove(child_index);
        }

        {
            let mut children = new_idom.children.borrow_mut();
            children.push(Rc::clone(&self));
        }
        self.idom.set(Some(new_idom));

        self.update_level();
    }

    #[inline(always)]
    pub fn level(&self) -> u32 {
        self.level.get()
    }

    pub fn is_leaf(&self) -> bool {
        self.children.borrow().is_empty()
    }

    pub fn num_children(&self) -> usize {
        self.children.borrow().len()
    }

    pub fn add_child(&self, child: Rc<DomTreeNode>) {
        self.children.borrow_mut().push(child);
    }

    pub fn clear_children(&self) {
        self.children.borrow_mut().clear();
    }

    /// Returns true if `self` is dominated by `other` in the tree.
    pub fn is_dominated_by(&self, other: &Self) -> bool {
        let num_in = self.num_in.get().expect("you forgot to call update_dfs_numbers").get();
        let other_num_in = other.num_in.get().expect("you forgot to call update_dfs_numbers").get();
        let num_out = self.num_out.get().unwrap().get();
        let other_num_out = other.num_out.get().unwrap().get();
        num_in >= other_num_in && num_out <= other_num_out
    }

    /// Recomputes this node's depth in the dominator tree
    fn update_level(self: Rc<Self>) {
        let idom_level = self.idom().expect("expected to have an immediate dominator").level();
        if self.level() == idom_level + 1 {
            return;
        }

        let mut stack = SmallVec::<[Rc<DomTreeNode>; 64]>::from_iter([self.clone()]);
        while let Some(current) = stack.pop() {
            current.level.set(current.idom().unwrap().level() + 1);
            for child in current.children.borrow().iter() {
                assert!(child.idom().is_some());
                if child.level() != child.idom().unwrap().level() + 1 {
                    stack.push(Rc::clone(child));
                }
            }
        }
    }
}

impl Eq for DomTreeNode {}
impl PartialEq for DomTreeNode {
    fn eq(&self, other: &Self) -> bool {
        self.block == other.block
    }
}

impl DomTreeBase<false> {
    #[inline]
    pub fn root(&self) -> BlockRef {
        self.roots[0].unwrap()
    }

    /// Get all the nodes of this tree as a vector in pre-order visitation order
    pub fn preorder(&self) -> Vec<Rc<DomTreeNode>> {
        let mut nodes = self
            .nodes
            .iter()
            .filter_map(|entry| match entry {
                Some((Some(_), node)) => Some(node.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.num_in.get().cmp(&b.num_in.get()));
        nodes
    }

    /// Get all the nodes of this tree as a vector in post-order visitation order
    pub fn postorder(&self) -> Vec<Rc<DomTreeNode>> {
        let mut nodes = self
            .nodes
            .iter()
            .filter_map(|entry| match entry {
                Some((Some(_), node)) => Some(node.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.num_out.get().cmp(&b.num_out.get()));
        nodes
    }

    /// Get all the nodes of this tree as a vector in reverse post-order visitation order
    ///
    /// This differs from `preorder` in that it is the exact inverse of `postorder`.
    /// Where `preorder` represents the order in which each node is first seen when traversing the
    /// CFG from the entry point, `postorder` is the order in which nodes are visited, once all their
    /// children are visited. Thus, a reverse post-order traversal is equivalent to a preorder
    /// traversal of the dominance tree, where `preorder` corresponds to a traversal of the CFG.
    pub fn reverse_postorder(&self) -> Vec<Rc<DomTreeNode>> {
        let mut nodes = self
            .nodes
            .iter()
            .filter_map(|entry| match entry {
                Some((Some(_), node)) => Some(node.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.num_out.get().cmp(&b.num_out.get()).reverse());
        nodes
    }
}

impl<const IS_POST_DOM: bool> DomTreeBase<IS_POST_DOM> {
    /// Compute a dominator tree for `region`
    pub fn new(region: RegionRef) -> Result<Self, DomTreeError> {
        let entry = region.borrow().entry_block_ref().ok_or(DomTreeError::EmptyRegion)?;
        let root = Rc::new(DomTreeNode::new(Some(entry), None));
        let root_id = entry.borrow().id().as_usize() + 1;
        let mut nodes = SmallVec::default();
        nodes.resize(root_id + 2, None);
        nodes[root_id] = Some((Some(entry), root.clone()));
        let roots = smallvec![Some(entry)];

        let mut this = Self {
            parent: region,
            root: Some(root),
            roots,
            nodes,
            valid: Cell::new(false),
            slow_queries: Cell::new(0),
        };

        this.compute();

        Ok(this)
    }

    #[inline]
    pub fn parent(&self) -> RegionRef {
        self.parent
    }

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|entry| entry.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() || self.nodes.iter().all(|entry| entry.is_none())
    }

    #[inline]
    pub fn num_roots(&self) -> usize {
        self.roots.len()
    }

    #[inline]
    pub fn roots(&self) -> &[Option<BlockRef>] {
        &self.roots
    }

    #[inline]
    pub fn roots_mut(&mut self) -> &mut DomTreeRoots {
        &mut self.roots
    }

    pub(super) fn set_root(&mut self, root: Rc<DomTreeNode>) {
        self.root = Some(root);
    }

    /// Returns true if this tree is a post-dominance tree.
    #[inline(always)]
    pub const fn is_post_dominator(&self) -> bool {
        IS_POST_DOM
    }

    pub(super) fn mark_invalid(&self) {
        self.valid.set(false);
    }

    /// Get the node for `block`, if one exists in the tree.
    ///
    /// Use `None` to get the virtual node, if this is a post-dominator tree
    pub fn get(&self, block: Option<BlockRef>) -> Option<Rc<DomTreeNode>> {
        let index = self.node_index(block);
        self.nodes.get(index).and_then(|entry| match entry {
            Some((_, node)) => Some(node.clone()),
            _ => None,
        })
    }

    #[inline]
    fn node_index(&self, block: Option<BlockRef>) -> usize {
        assert!(
            block.is_none_or(|block| block.parent().is_some_and(|parent| parent == self.parent)),
            "cannot get dominance info of block with different parent"
        );
        if let Some(block) = block {
            block.borrow().id().as_usize() + 1
        } else {
            // Reserve index 0 for None
            0
        }
    }

    /// Returns the entry node for the CFG of the region.
    ///
    /// However, if this tree represents the post-dominance relations for a region, this root may be
    /// a node with `block` set to `None`.  This is the case when there are multiple exit nodes from
    /// a particular function.  Consumers of post-dominance information must be capable of dealing
    /// with this possibility.
    pub fn root_node(&self) -> Option<Rc<DomTreeNode>> {
        self.root.clone()
    }

    /// Get all nodes dominated by `r`, including `r` itself
    pub fn get_descendants(&self, r: BlockRef) -> SmallVec<[BlockRef; 2]> {
        let mut results = SmallVec::default();
        let Some(rn) = self.get(Some(r)) else {
            return results;
        };
        let mut worklist = SmallVec::<[Rc<DomTreeNode>; 8]>::default();
        worklist.push(rn);

        while let Some(n) = worklist.pop() {
            let Some(n_block) = n.block() else {
                continue;
            };
            results.push(n_block);
            worklist.extend(n.children.borrow().iter().cloned());
        }

        results
    }

    /// Return true if `a` is dominated by the entry block of the region containing it.
    pub fn is_reachable_from_entry(&self, a: BlockRef) -> bool {
        assert!(!self.is_post_dominator(), "unimplemented for post dominator trees");

        self.get(Some(a)).is_some()
    }

    #[inline]
    pub const fn is_reachable_from_entry_node(&self, a: Option<&Rc<DomTreeNode>>) -> bool {
        a.is_some()
    }

    /// Returns true if and only if `a` dominates `b` and `a != b`
    ///
    /// Note that this is not a constant time operation.
    pub fn properly_dominates(&self, a: Option<BlockRef>, b: Option<BlockRef>) -> bool {
        if a == b {
            return false;
        }
        let a = self.get(a);
        let b = self.get(b);
        if a.is_none() || b.is_none() {
            return false;
        }
        self.properly_dominates_node(a, b)
    }

    /// Returns true if and only if `a` dominates `b` and `a != b`
    ///
    /// Note that this is not a constant time operation.
    pub fn properly_dominates_node(
        &self,
        a: Option<Rc<DomTreeNode>>,
        b: Option<Rc<DomTreeNode>>,
    ) -> bool {
        a != b && self.dominates_node(a, b)
    }

    /// Returns true iff `a` dominates `b`.
    ///
    /// Note that this is not a constant time operation
    pub fn dominates(&self, a: Option<BlockRef>, b: Option<BlockRef>) -> bool {
        if a == b {
            return true;
        }
        let a = self.get(a);
        let b = self.get(b);
        self.dominates_node(a, b)
    }

    /// Returns true iff `a` dominates `b`.
    ///
    /// Note that this is not a constant time operation
    pub fn dominates_node(&self, a: Option<Rc<DomTreeNode>>, b: Option<Rc<DomTreeNode>>) -> bool {
        // A trivially dominates itself
        if a == b {
            return true;
        }

        // An unreachable node is dominated by anything
        if b.is_none() {
            return true;
        }

        // And dominates nothing.
        if a.is_none() {
            return false;
        }

        let a = a.unwrap();
        let b = b.unwrap();

        if b.idom().is_some_and(|idom| idom == a) {
            return true;
        }

        if a.idom().is_some_and(|idom| idom == b) {
            return false;
        }

        // A can only dominate B if it is higher in the tree
        if a.level() >= b.level() {
            return false;
        }

        if self.valid.get() {
            return b.is_dominated_by(&a);
        }

        // If we end up with too many slow queries, just update the DFS numbers on the assumption
        // that we are going to keep querying
        self.slow_queries.set(self.slow_queries.get() + 1);
        if self.slow_queries.get() > 32 {
            self.update_dfs_numbers();
            return b.is_dominated_by(&a);
        }

        self.dominated_by_slow_tree_walk(a, b)
    }

    /// Finds the nearest block which is a common dominator of both `a` and `b`
    pub fn find_nearest_common_dominator(&self, a: BlockRef, b: BlockRef) -> Option<BlockRef> {
        assert!(a.parent() == b.parent(), "two blocks are not in same region");

        // If either A or B is an entry block then it is nearest common dominator (for forward
        // dominators).
        if !self.is_post_dominator() {
            let parent = a.parent().unwrap();
            let entry = parent.borrow().entry_block_ref().unwrap();
            if a == entry || b == entry {
                return Some(entry);
            }
        }

        let mut a = self.get(Some(a)).expect("'a' must be in the tree");
        let mut b = self.get(Some(b)).expect("'b' must be in the tree");

        // Use level information to go up the tree until the levels match. Then continue going up
        // until we arrive at the same node.
        while a != b {
            if a.level() < b.level() {
                core::mem::swap(&mut a, &mut b);
            }

            a = a.idom().unwrap();
        }

        a.block()
    }
}

impl<const IS_POST_DOM: bool> DomTreeBase<IS_POST_DOM> {
    pub fn insert_edge(&mut self, mut from: Option<BlockRef>, mut to: Option<BlockRef>) {
        if self.is_post_dominator() {
            core::mem::swap(&mut from, &mut to);
        }
        SemiNCA::<IS_POST_DOM>::insert_edge(self, None, from, to)
    }

    pub fn delete_edge(&mut self, mut from: Option<BlockRef>, mut to: Option<BlockRef>) {
        if self.is_post_dominator() {
            core::mem::swap(&mut from, &mut to);
        }
        SemiNCA::<IS_POST_DOM>::delete_edge(self, None, from, to)
    }

    pub fn apply_updates(
        &mut self,
        pre_view_cfg: cfg::CfgDiff<IS_POST_DOM>,
        post_view_cfg: cfg::CfgDiff<IS_POST_DOM>,
    ) {
        SemiNCA::<IS_POST_DOM>::apply_updates(self, pre_view_cfg, post_view_cfg);
    }

    pub fn compute(&mut self) {
        SemiNCA::<IS_POST_DOM>::compute_from_scratch(self, None);
    }

    pub fn compute_with_updates(&mut self, updates: impl ExactSizeIterator<Item = cfg::CfgUpdate>) {
        // FIXME: Updated to use the PreViewCFG and behave the same as until now.
        // This behavior is however incorrect; this actually needs the PostViewCFG.
        let pre_view_cfg = cfg::CfgDiff::new(updates, true);
        let bui = BatchUpdateInfo::new(pre_view_cfg, None);
        SemiNCA::<IS_POST_DOM>::compute_from_scratch(self, Some(bui));
    }

    pub fn verify(&self, level: DomTreeVerificationLevel) -> bool {
        let snca = SemiNCA::new(None);

        // Simplest check is to compare against a new tree. This will also usefully print the old
        // and ne3w trees, if they are different.
        if !self.is_same_as_fresh_tree() {
            return false;
        }

        // Common checks to verify the properties of the tree. O(n log n) at worst.
        if !snca.verify_roots(self)
            || !snca.verify_reachability(self)
            || !snca.verify_levels(self)
            || !snca.verify_dfs_numbers(self)
        {
            return false;
        }

        // Extra checks depending on verification level. Up to O(n^3)
        match level {
            DomTreeVerificationLevel::Basic => {
                if !snca.verify_parent_property(self) {
                    return false;
                }
            }
            DomTreeVerificationLevel::Full => {
                if !snca.verify_parent_property(self) || !snca.verify_sibling_property(self) {
                    return false;
                }
            }
            _ => (),
        }

        true
    }

    fn is_same_as_fresh_tree(&self) -> bool {
        let fresh = Self::new(self.parent).unwrap();
        let is_same = self == &fresh;
        if !is_same {
            log::error!(
                "{} is different than a freshly computed one!",
                if IS_POST_DOM {
                    "post-dominator tree"
                } else {
                    "dominator tree"
                }
            );
            log::error!("Current: {self}");
            log::error!("Fresh: {fresh}");
        }

        is_same
    }

    pub fn is_virtual_root(&self, node: &DomTreeNode) -> bool {
        self.is_post_dominator() && node.block.is_none()
    }

    pub fn add_new_block(&mut self, block: BlockRef, idom: Option<BlockRef>) -> Rc<DomTreeNode> {
        assert!(self.get(Some(block)).is_none(), "block already in dominator tree");
        let idom = self.get(idom).expect("no immediate dominator specified for `idom`");
        self.mark_invalid();
        self.create_node(Some(block), Some(idom))
    }

    pub fn set_new_root(&mut self, block: BlockRef) -> Rc<DomTreeNode> {
        assert!(self.get(Some(block)).is_none(), "block already in dominator tree");
        assert!(!self.is_post_dominator(), "cannot change root of post-dominator tree");

        self.valid.set(false);
        let node = self.create_node(Some(block), None);
        if self.roots.is_empty() {
            self.roots.push(Some(block));
        } else {
            assert_eq!(self.roots.len(), 1);
            let old_node = self.get(self.roots[0]).unwrap();
            node.add_child(old_node.clone());
            old_node.idom.set(Some(node.clone()));
            old_node.update_level();
            self.roots[0] = Some(block);
        }
        self.root = Some(node.clone());
        node
    }

    pub fn change_immediate_dominator(&mut self, n: BlockRef, idom: Option<BlockRef>) {
        let n = self.get(Some(n)).expect("expected `n` to be in tree");
        let idom = self.get(idom).expect("expected `idom` to be in tree");
        self.change_immediate_dominator_node(n, idom);
    }

    pub fn change_immediate_dominator_node(&mut self, n: Rc<DomTreeNode>, idom: Rc<DomTreeNode>) {
        self.valid.set(false);
        n.idom.set(Some(idom));
    }

    /// Removes a node from the dominator tree.
    ///
    /// Block must not dominate any other blocks.
    ///
    /// Removes node from the children of its immediate dominator. Deletes dominator node associated
    /// with `block`.
    pub fn erase_node(&mut self, block: BlockRef) {
        let node_index = self.node_index(Some(block));
        let entry = unsafe { self.nodes.get_unchecked_mut(node_index).take() };
        let Some((_, node)) = entry else {
            panic!("no node in tree for {block}");
        };
        assert!(node.is_leaf(), "node is not a leaf node");

        self.valid.set(false);

        // Remove node from immediate dominator's children
        if let Some(idom) = node.idom() {
            idom.children.borrow_mut().retain(|child| child != &node);
        }

        if !IS_POST_DOM {
            return;
        }

        // Remember to update PostDominatorTree roots
        if let Some(root_index) = self.roots.iter().position(|r| r.is_some_and(|r| r == block)) {
            self.roots.remove(root_index);
        }
    }

    /// Assign in and out numbers to the nodes while walking the dominator tree in DFS order.
    pub fn update_dfs_numbers(&self) {
        if self.valid.get() {
            self.slow_queries.set(0);
            return;
        }

        let mut worklist = SmallVec::<[(Rc<DomTreeNode>, usize); 32]>::default();
        let this_root = self.root_node().unwrap();

        // Both dominators and postdominators have a single root node. In the case of
        // PostDominatorTree, this node is a virtual root.
        this_root.num_in.set(NonZeroU32::new(1));
        worklist.push((this_root, 0));

        let mut dfs_num = 1u32;

        while let Some((node, child_index)) = worklist.last_mut() {
            // If we visited all of the children of this node, "recurse" back up the
            // stack setting the DFOutNum.
            if *child_index >= node.num_children() {
                node.num_out.set(Some(unsafe { NonZeroU32::new_unchecked(dfs_num) }));
                dfs_num += 1;
                worklist.pop();
            } else {
                // Otherwise, recursively visit this child.
                let index = *child_index;
                *child_index += 1;
                let child = node.children.borrow()[index].clone();
                child.num_in.set(Some(unsafe { NonZeroU32::new_unchecked(dfs_num) }));
                dfs_num += 1;
                worklist.push((child, 0));
            }
        }

        self.slow_queries.set(0);
        self.valid.set(true);
    }

    /// Reset the dominator tree state
    pub fn reset(&mut self) {
        self.nodes.clear();
        self.root.take();
        self.roots.clear();
        self.valid.set(false);
        self.slow_queries.set(0);
    }

    pub(super) fn create_node(
        &mut self,
        block: Option<BlockRef>,
        idom: Option<Rc<DomTreeNode>>,
    ) -> Rc<DomTreeNode> {
        let node = Rc::new(DomTreeNode::new(block, idom.clone()));
        let node_index = self.node_index(block);
        if node_index >= self.nodes.len() {
            self.nodes.resize(node_index + 1, None);
        }
        self.nodes[node_index] = Some((block, node.clone()));
        if let Some(idom) = idom {
            idom.add_child(node.clone());
        }
        node
    }

    /// `block` is split and now it has one successor.
    ///
    /// Update dominator tree to reflect the change.
    pub fn split_block(&mut self, block: BlockRef) {
        if IS_POST_DOM {
            self.split::<Inverse<BlockRef>>(block);
        } else {
            self.split::<BlockRef>(block);
        }
    }

    // `block` is split and now it has one successor. Update dominator tree to reflect this change.
    fn split<G>(&mut self, block: <G as Graph>::Node)
    where
        G: InvertibleGraph<Node = BlockRef>,
    {
        let mut successors = G::children(block);
        assert_eq!(successors.len(), 1, "`block` should have a single successor");

        let succ = successors.next().unwrap();
        let predecessors = G::inverse_children(block).collect::<SmallVec<[BlockRef; 4]>>();

        assert!(!predecessors.is_empty(), "expected at at least one predecessor");

        let mut block_dominates_succ = true;
        for pred in G::inverse_children(succ) {
            if pred != block
                && !self.dominates(Some(succ), Some(pred))
                && self.is_reachable_from_entry(pred)
            {
                block_dominates_succ = false;
                break;
            }
        }

        // Find `block`'s immediate dominator and create new dominator tree node for `block`.
        let idom = predecessors.iter().find(|p| self.is_reachable_from_entry(**p)).copied();

        // It's possible that none of the predecessors of `block` are reachable;
        // in that case, `block` itself is unreachable, so nothing needs to be
        // changed.
        let Some(idom) = idom else {
            return;
        };

        let idom = predecessors.iter().copied().fold(idom, |idom, p| {
            if self.is_reachable_from_entry(p) {
                self.find_nearest_common_dominator(idom, p).expect("expected idom")
            } else {
                idom
            }
        });

        // Create the new dominator tree node... and set the idom of `block`.
        let node = self.add_new_block(block, Some(idom));

        // If NewBB strictly dominates other blocks, then it is now the immediate
        // dominator of NewBBSucc.  Update the dominator tree as appropriate.
        if block_dominates_succ {
            let succ_node = self.get(Some(succ)).expect("expected 'succ' to be in dominator tree");
            self.change_immediate_dominator_node(succ_node, node);
        }
    }

    fn dominated_by_slow_tree_walk(&self, a: Rc<DomTreeNode>, b: Rc<DomTreeNode>) -> bool {
        assert_ne!(a, b);

        let a_level = a.level();
        let mut b = b;

        // Don't walk nodes above A's subtree. When we reach A's level, we must
        // either find A or be in some other subtree not dominated by A.
        while let Some(b_idom) = b.idom() {
            if b_idom.level() >= a_level {
                // Walk up the tree
                b = b_idom;
            }
        }

        b == a
    }
}

impl<const IS_POST_DOM: bool> fmt::Display for DomTreeBase<IS_POST_DOM> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;

        f.write_str("=============================--------------------------------\n")?;
        if IS_POST_DOM {
            f.write_str("Inorder PostDominator Tree: ")?;
        } else {
            f.write_str("Inorder Dominator Tree: ")?;
        }
        if !self.valid.get() {
            write!(f, "DFS numbers invalid: {} slow queries.", self.slow_queries.get())?;
        }
        f.write_char('\n')?;

        // The postdom tree can have a `None` root if there are no returns.
        if let Some(root_node) = self.root_node() {
            print_dom_tree(root_node, 1, f)?
        }
        f.write_str("Roots: ")?;
        for (i, block) in self.roots.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            if let Some(block) = block {
                write!(f, "{block}")?;
            } else {
                f.write_str("<virtual>")?;
            }
        }
        f.write_char('\n')
    }
}

fn print_dom_tree(
    node: Rc<DomTreeNode>,
    level: usize,
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    write!(f, "{: <1$}", "", level)?;
    writeln!(f, "[{level}] {node}")?;
    for child_node in node.children.borrow().iter().cloned() {
        print_dom_tree(child_node, level + 1, f)?;
    }
    Ok(())
}

impl<const IS_POST_DOM: bool> Eq for DomTreeBase<IS_POST_DOM> {}
impl<const IS_POST_DOM: bool> PartialEq for DomTreeBase<IS_POST_DOM> {
    fn eq(&self, other: &Self) -> bool {
        self.parent == other.parent
            && self.roots.len() == other.roots.len()
            && self.roots.iter().all(|root| other.roots.contains(root))
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().all(|entry| match entry {
                Some((_, node)) => {
                    let block = node.block();
                    other.get(block).is_some_and(|n| node == &n)
                }
                None => true,
            })
    }
}
