use alloc::{collections::BTreeMap, rc::Rc};
use core::cell::{Cell, Ref, RefCell};

use smallvec::{SmallVec, smallvec};

use super::{DomTreeBase, DomTreeNode, DomTreeRoots};
use crate::{
    BlockRef, EntityId, EntityWithId, Region,
    cfg::{self, Graph, GraphDiff, Inverse},
    formatter::{DisplayOptional, DisplayValues},
};

/// [SemiNCAInfo] provides functionality for constructing a dominator tree for a control-flow graph
/// based on the Semi-NCA algorithm described in the following dissertation:
///
///   [1] Linear-Time Algorithms for Dominators and Related Problems
///   Loukas Georgiadis, Princeton University, November 2005, pp. 21-23:
///   ftp://ftp.cs.princeton.edu/reports/2005/737.pdf
///
/// The Semi-NCA algorithm runs in O(n^2) worst-case time but usually slightly faster than Simple
/// Lengauer-Tarjan in practice.
///
/// O(n^2) worst cases happen when the computation of nearest common ancestors requires O(n) average
/// time, which is very unlikely in real world. If this ever turns out to be an issue, consider
/// implementing a hybrid algorithm that uses SLT to perform full constructions and SemiNCA for
/// incremental updates.
///
/// The file uses the Depth Based Search algorithm to perform incremental updates (insertion and
/// deletions). The implemented algorithm is based on this publication:
///
///   [2] An Experimental Study of Dynamic Dominators
///   Loukas Georgiadis, et al., April 12 2016, pp. 5-7, 9-10:
///   https://arxiv.org/pdf/1604.02711.pdf
pub struct SemiNCA<const IS_POST_DOM: bool> {
    /// Number to node mapping is 1-based.
    num_to_node: SmallVec<[Option<BlockRef>; 64]>,
    /// Infos are mapped to nodes using block indices
    node_infos: RefCell<SmallVec<[NodeInfo; 64]>>,
    batch_updates: Option<BatchUpdateInfo<IS_POST_DOM>>,
}

/// Get the successors (or predecessors, if `INVERSED == true`) of `node`, incorporating insertions
/// and deletions from `bui` if available.
///
/// The use of "children" here changes meaning depending on:
///
/// * Whether or not the graph traversal is `INVERSED`
/// * Whether or not the graph is a post-dominator tree (i.e. `IS_POST_DOM`)
///
/// If we're traversing a post-dominator tree, then the "children" of a node are actually
/// predecessors of the block in the CFG. However, if the traversal is _also_ `INVERSED`, then the
/// children actually are successors of the block in the CFG.
///
/// For a forward-dominance tree, "children" do correspond to successors in the CFG, but again, if
/// the traversal is `INVERSED`, then the children are actually predecessors.
///
/// This function (and others in this module) are written in such a way that we can abstract over
/// whether the underlying dominator tree is a forward- or post-dominance tree, as much of the
/// implementation is identical.
pub fn get_children_with_batch_updates<const INVERSED: bool, const IS_POST_DOM: bool>(
    node: BlockRef,
    bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
) -> SmallVec<[BlockRef; 8]> {
    use crate::cfg::GraphDiff;

    if let Some(bui) = bui {
        bui.pre_cfg_view.get_children::<INVERSED>(node)
    } else {
        get_children::<INVERSED>(node)
    }
}

/// Get the successors (or predecessors, if `INVERSED == true`) of `node`.
pub fn get_children<const INVERSED: bool>(node: BlockRef) -> SmallVec<[BlockRef; 8]> {
    if INVERSED {
        Inverse::<BlockRef>::children(node).collect()
    } else {
        let mut r = BlockRef::children(node).collect::<SmallVec<[BlockRef; 8]>>();
        r.reverse();
        r
    }
}

#[derive(Default)]
pub struct NodeInfo {
    num: Cell<u32>,
    parent: Cell<u32>,
    semi: Cell<u32>,
    label: Cell<u32>,
    idom: Cell<Option<BlockRef>>,
    reverse_children: SmallVec<[u32; 4]>,
}
impl NodeInfo {
    pub fn idom(&self) -> Option<BlockRef> {
        self.idom.get()
    }

    #[inline]
    pub fn num(&self) -> u32 {
        self.num.get()
    }

    #[inline]
    pub fn parent(&self) -> u32 {
        self.parent.get()
    }

    #[inline]
    pub fn semi(&self) -> u32 {
        self.semi.get()
    }

    #[inline]
    pub fn label(&self) -> u32 {
        self.label.get()
    }

    #[inline]
    pub fn reverse_children(&self) -> &[u32] {
        &self.reverse_children
    }
}

/// [BatchUpdateInfo] represents a batch of insertion/deletion operations that have been applied to
/// the CFG. This information is used to incrementally update the dominance tree as changes are
/// made to the CFG.
#[derive(Default, Clone)]
pub struct BatchUpdateInfo<const IS_POST_DOM: bool> {
    pub pre_cfg_view: cfg::CfgDiff<IS_POST_DOM>,
    pub post_cfg_view: cfg::CfgDiff<IS_POST_DOM>,
    pub num_legalized: usize,
    // Remembers if the whole tree was recomputed at some point during the current batch update
    pub is_recalculated: bool,
}

impl<const IS_POST_DOM: bool> BatchUpdateInfo<IS_POST_DOM> {
    pub fn new(
        pre_cfg_view: cfg::CfgDiff<IS_POST_DOM>,
        post_cfg_view: Option<cfg::CfgDiff<IS_POST_DOM>>,
    ) -> Self {
        let num_legalized = pre_cfg_view.num_legalized_updates();
        Self {
            pre_cfg_view,
            post_cfg_view: post_cfg_view.unwrap_or_default(),
            num_legalized,
            is_recalculated: false,
        }
    }
}

impl<const IS_POST_DOM: bool> SemiNCA<IS_POST_DOM> {
    /// Obtain a fresh [SemiNCA] instance, using the provided set of [BatchUpdateInfo].
    pub fn new(batch_updates: Option<BatchUpdateInfo<IS_POST_DOM>>) -> Self {
        Self {
            num_to_node: smallvec![None],
            node_infos: Default::default(),
            batch_updates,
        }
    }

    /// Reset the [SemiNCA] state so it can be used to compute a dominator tree from scratch.
    pub fn clear(&mut self) {
        // Don't reset the pointer to BatchUpdateInfo here -- if there's an update in progress,
        // we need this information to continue it.
        self.num_to_node.clear();
        self.num_to_node.push(None);
        self.node_infos.get_mut().clear();
    }

    /// Look up information about a block in the Semi-NCA state
    pub fn node_info(&self, block: Option<BlockRef>) -> Ref<'_, NodeInfo> {
        match block {
            None => Ref::map(self.node_infos.borrow(), |ni| {
                ni.first().expect("no virtual node present")
            }),
            Some(block) => {
                let index = block.borrow().id().as_usize() + 1;

                if index >= self.node_infos.borrow().len() {
                    self.node_infos.borrow_mut().resize_with(index + 1, NodeInfo::default);
                }

                Ref::map(self.node_infos.borrow(), |ni| unsafe { ni.get_unchecked(index) })
            }
        }
    }

    /// Get a mutable reference to the stored informaton for `block`
    pub fn node_info_mut(&mut self, block: Option<BlockRef>) -> &mut NodeInfo {
        match block {
            None => self.node_infos.get_mut().get_mut(0).expect("no virtual node present"),
            Some(block) => {
                let index = block.borrow().id().as_usize() + 1;

                let node_infos = self.node_infos.get_mut();
                if index >= node_infos.len() {
                    node_infos.resize_with(index + 1, NodeInfo::default);
                }

                unsafe { node_infos.get_unchecked_mut(index) }
            }
        }
    }

    /// Look up the immediate dominator for `block`, if it has one.
    ///
    /// A value of `None` for `block` is meaningless, as virtual nodes only are present in post-
    /// dominance graphs, and always post-dominate all other nodes in the graph. However, it is
    /// convenient to have many of the APIs in this module take a `Option<BlockRef>` for uniformity.
    pub fn idom(&self, block: Option<BlockRef>) -> Option<BlockRef> {
        self.node_info(block).idom()
    }

    /// Get or compute the dominance tree node information for `block`, in `tree`, using the current
    /// Semi-NCA state.
    pub fn node_for_block(
        &self,
        block: Option<BlockRef>,
        tree: &mut DomTreeBase<IS_POST_DOM>,
    ) -> Option<Rc<DomTreeNode>> {
        let node = tree.get(block);
        if node.is_some() {
            return node;
        }

        // Haven't calculated this node yet? Get or calculate the node for the immediate dominator
        let idom = self.idom(block);
        let idom_node = match idom {
            None => Some(tree.get(None).expect("expected idom or virtual node")),
            Some(idom_block) => self.node_for_block(Some(idom_block), tree),
        };

        // Add a new tree node for this node, and link it as a child of idom_node
        Some(tree.create_node(block, idom_node))
    }

    /// Custom DFS implementation which can skip nodes based on a provided predicate.
    ///
    /// It also collects reverse children so that we don't have to spend time getting predecessors
    /// in SemiNCA.
    ///
    /// If `IsReverse` is set to true, the DFS walk will be performed backwards relative to IS_POST_DOM
    /// -- using reverse edges for dominators and forward edges for post-dominators.
    ///
    /// If `succ_order` is specified then that is the order in which the DFS traverses the children,
    /// otherwise the order is implied by the results of `get_children`.
    pub fn run_dfs<const REVERSE: bool, C>(
        &mut self,
        v: Option<BlockRef>,
        mut last_num: u32,
        mut condition: C,
        attach_to_num: u32,
        succ_order: Option<&BTreeMap<BlockRef, u32>>,
    ) -> u32
    where
        C: FnMut(Option<BlockRef>, Option<BlockRef>) -> bool,
    {
        let v = v.expect("expected valid root node for search");

        let mut worklist = SmallVec::<[(BlockRef, u32); 64]>::from_iter([(v, attach_to_num)]);

        self.node_info_mut(Some(v)).parent.set(attach_to_num);

        while let Some((block, parent_num)) = worklist.pop() {
            let block_info = self.node_info_mut(Some(block));
            block_info.reverse_children.push(parent_num);

            // Visited nodes always have positive DFS numbers.
            if block_info.num.get() != 0 {
                continue;
            }

            block_info.parent.set(parent_num);
            last_num += 1;
            block_info.num.set(last_num);
            block_info.semi.set(last_num);
            block_info.label.set(last_num);
            self.num_to_node.push(Some(block));

            let mut successors = if const { REVERSE != IS_POST_DOM } {
                get_children_with_batch_updates::<true, IS_POST_DOM>(
                    block,
                    self.batch_updates.as_ref(),
                )
            } else {
                get_children_with_batch_updates::<false, IS_POST_DOM>(
                    block,
                    self.batch_updates.as_ref(),
                )
            };
            if let Some(succ_order) = succ_order
                && successors.len() > 1
            {
                successors.sort_by(|a, b| succ_order[a].cmp(&succ_order[b]));
            }

            for succ in successors.into_iter().filter(|succ| condition(Some(block), Some(*succ))) {
                worklist.push((succ, last_num));
            }
        }

        last_num
    }

    // V is a predecessor of W. eval() returns V if V < W, otherwise the minimum
    // of sdom(U), where U > W and there is a virtual forest path from U to V. The
    // virtual forest consists of linked edges of processed vertices.
    //
    // We can follow Parent pointers (virtual forest edges) to determine the
    // ancestor U with minimum sdom(U). But it is slow and thus we employ the path
    // compression technique to speed up to O(m*log(n)). Theoretically the virtual
    // forest can be organized as balanced trees to achieve almost linear
    // O(m*alpha(m,n)) running time. But it requires two auxiliary arrays (Size
    // and Child) and is unlikely to be faster than the simple implementation.
    //
    // For each vertex V, its Label points to the vertex with the minimal sdom(U)
    // (Semi) in its path from V (included) to NodeToInfo[V].Parent (excluded).
    fn eval<'a, 'b: 'a>(
        v: u32,
        last_linked: u32,
        eval_stack: &mut SmallVec<[&'a NodeInfo; 32]>,
        num_to_info: &'b [Option<Ref<'b, NodeInfo>>],
    ) -> u32 {
        let mut v_info = &**num_to_info[v as usize].as_ref().unwrap();
        if v_info.parent.get() < last_linked {
            return v_info.label.get();
        }

        // Store ancestors except the last (root of a virtual tree) into a stack.
        eval_stack.clear();
        loop {
            let parent = &**num_to_info[v_info.parent.get() as usize].as_ref().unwrap();
            eval_stack.push(v_info);
            v_info = parent;
            if v_info.parent.get() < last_linked {
                break;
            }
        }

        // Path compression. Point each vertex's `parent` to the root and update its `label` if any
        // of its ancestors `label` has a smaller `semi`
        let mut p_info = v_info;
        let mut p_label_info = &**num_to_info[p_info.label.get() as usize].as_ref().unwrap();
        while let Some(info) = eval_stack.pop() {
            v_info = info;
            v_info.parent.set(p_info.parent.get());
            let v_label_info = &**num_to_info[v_info.label.get() as usize].as_ref().unwrap();
            if p_label_info.semi.get() < v_label_info.semi.get() {
                v_info.label.set(p_info.label.get());
            } else {
                p_label_info = v_label_info;
            }
            p_info = v_info;
        }

        v_info.label.get()
    }

    /// This function requires DFS to be run before calling it.
    pub fn run(&mut self) {
        let next_num = self.num_to_node.len();
        let mut num_to_info = SmallVec::<[Option<Ref<'_, NodeInfo>>; 8]>::default();
        num_to_info.reserve(next_num);
        num_to_info.push(None);

        // Initialize idoms to spanning tree parents
        for i in 1..next_num {
            let v = self.num_to_node[i].unwrap();
            let v_info = self.node_info(Some(v));
            v_info.idom.set(self.num_to_node[v_info.parent() as usize]);
            assert_eq!(i, num_to_info.len());
            num_to_info.push(Some(v_info));
        }

        // Step 1: Calculate the semi-dominators of all vertices
        let mut eval_stack = SmallVec::<[&NodeInfo; 32]>::default();
        for i in (2..next_num).rev() {
            let w_info = num_to_info[i].as_ref().unwrap();

            // Initialize the semi-dominator to point to the parent node.
            w_info.semi.set(w_info.parent());
            for n in w_info.reverse_children.iter().copied() {
                let semi_u = num_to_info
                    [Self::eval(n, i as u32 + 1, &mut eval_stack, &num_to_info) as usize]
                    .as_ref()
                    .unwrap()
                    .semi
                    .get();
                if semi_u < w_info.semi.get() {
                    w_info.semi.set(semi_u);
                }
            }
        }

        // Step 2: Explicitly define the immediate dominator of each vertex.
        //
        //     IDom[i] = NCA(SDom[i], SpanningTreeParent(i))
        //
        // Note that the parents were stored in IDoms and later got invalidated during path
        // compression in `eval`
        for i in 2..next_num {
            let w_info = num_to_info[i].as_ref().unwrap();
            assert_ne!(w_info.semi.get(), 0);
            let s_dom_num = num_to_info[w_info.semi.get() as usize].as_ref().unwrap().num.get();
            let mut w_idom_candidate = w_info.idom();
            loop {
                let w_idom_candidate_info = self.node_info(w_idom_candidate);
                if w_idom_candidate_info.num.get() <= s_dom_num {
                    break;
                }
                w_idom_candidate = w_idom_candidate_info.idom();
            }

            w_info.idom.set(w_idom_candidate);
        }
    }

    /// [PostDominatorTree] always has a virtual root that represents a virtual CFG node that serves
    /// as a single exit from the region.
    ///
    /// All the other exits (CFG nodes with terminators and nodes in infinite loops) are logically
    /// connected to this virtual CFG exit node.
    ///
    /// This function maps a null CFG node to the virtual root tree node.
    fn add_virtual_root(&mut self) {
        if const { IS_POST_DOM } {
            assert_eq!(self.num_to_node.len(), 1, "SemiNCAInfo must be freshly constructed");

            let info = self.node_info_mut(None);
            info.num.set(1);
            info.semi.set(1);
            info.label.set(1);

            // num_to_node[1] = None
            self.num_to_node.push(None);
        }
    }

    /// For postdominators, nodes with no forward successors are trivial roots that
    /// are always selected as tree roots. Roots with forward successors correspond
    /// to CFG nodes within infinite loops.
    fn has_forward_successors(
        n: Option<BlockRef>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
    ) -> bool {
        let n = n.expect("`n` must be a valid node");
        !get_children_with_batch_updates::<false, IS_POST_DOM>(n, bui).is_empty()
    }

    fn entry_node(tree: &DomTreeBase<IS_POST_DOM>) -> BlockRef {
        tree.parent()
            .borrow()
            .entry_block_ref()
            .expect("expected region to have an entry block")
    }

    pub fn find_roots(
        tree: &DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
    ) -> DomTreeRoots {
        let mut roots = DomTreeRoots::default();

        // For dominators, region entry CFG node is always a tree root node.
        if !IS_POST_DOM {
            roots.push(Some(Self::entry_node(tree)));
            return roots;
        }

        let mut snca = Self::new(bui.cloned());

        // PostDominatorTree always has a virtual root.
        snca.add_virtual_root();
        let mut num = 1u32;

        log::trace!("looking for trivial roots");

        // Step 1: Find all the trivial roots that are going to definitely remain tree roots
        let mut total = 0;
        // It may happen that there are some new nodes in the CFG that are result of the ongoing
        // batch update, but we cannot really pretend that they don't exist -- we won't see any
        // outgoing or incoming edges to them, so it's fine to discover them here, as they would end
        // up appearing in the CFG at some point anyway.
        let region = tree.parent().borrow();
        let mut region_body = region.body().front();
        while let Some(n) = region_body.as_pointer() {
            region_body.move_next();
            total += 1;
            // If it has no successors, it is definitely a root
            if !Self::has_forward_successors(Some(n), bui) {
                roots.push(Some(n));
                // Run DFS not to walk this part of CFG later.
                num = snca.run_dfs::<false, _>(Some(n), num, always_descend, 1, None);
                log::trace!("found a new trivial root: {}", n.borrow().id());
                match snca.num_to_node.get(num as usize) {
                    None => log::trace!("last visited node: None"),
                    Some(None) => {
                        log::trace!("last visited virtual node")
                    }
                    Some(Some(last_visited)) => {
                        log::trace!("last visited node: {}", last_visited.borrow().id())
                    }
                }
            }
        }

        log::trace!("looking for non-trivial roots");

        // Step 2: Find all non-trivial root candidates.
        //
        // Those are CFG nodes that are reverse-unreachable were not visited by previous DFS walks
        // (i.e. CFG nodes in infinite loops).
        //
        // Accounting for the virtual exit, see if we had any reverse-unreachable nodes.
        let has_non_trivial_roots = total + 1 != num;
        if has_non_trivial_roots {
            // `succ_order` is the order of blocks in the region. It is needed to make the
            // calculation of the `furthest_away` node and the whole PostDominanceTree immune to
            // swapping successors (e.g. canonicalizing branch predicates). `succ_order` is
            // initialized lazily only for successors of reverse unreachable nodes.
            #[derive(Default)]
            struct LazySuccOrder {
                succ_order: BTreeMap<BlockRef, u32>,
                initialized: bool,
            }
            impl LazySuccOrder {
                pub fn get_or_init<'a, 'b: 'a, const IS_POST_DOM: bool>(
                    &'b mut self,
                    region: &Region,
                    bui: Option<&'a BatchUpdateInfo<IS_POST_DOM>>,
                    snca: &SemiNCA<IS_POST_DOM>,
                ) -> &'a BTreeMap<BlockRef, u32> {
                    if !self.initialized {
                        let mut region_body = region.body().front();
                        while let Some(n) = region_body.as_pointer() {
                            region_body.move_next();
                            let n_num = snca.node_info(Some(n)).num.get();
                            if n_num == 0 {
                                for succ in
                                    get_children_with_batch_updates::<false, IS_POST_DOM>(n, bui)
                                {
                                    self.succ_order.insert(succ, 0);
                                }
                            }
                        }

                        // Add mapping for all entries of succ_order
                        let mut node_num = 0;
                        let mut region_body = region.body().front();
                        while let Some(n) = region_body.as_pointer() {
                            region_body.move_next();
                            node_num += 1;
                            if let Some(order) = self.succ_order.get_mut(&n) {
                                assert_eq!(*order, 0);
                                *order = node_num;
                            }
                        }
                        self.initialized = true;
                    }

                    &self.succ_order
                }
            }

            let mut succ_order = LazySuccOrder::default();

            // Make another DFS pass over all other nodes to find the reverse-unreachable blocks,
            // and find the furthest paths we'll be able to make.
            //
            // Note that this looks N^2, but it's really 2N worst case, if every node is unreachable.
            // This is because we are still going to only visit each unreachable node once, we may
            // just visit it in two directions, depending on how lucky we get.
            let mut region_body = region.body().front();
            while let Some(n) = region_body.as_pointer() {
                region_body.move_next();

                if snca.node_info(Some(n)).num.get() == 0 {
                    log::trace!("visiting node {n}");

                    // Find the furthest away we can get by following successors, then
                    // follow them in reverse.  This gives us some reasonable answer about
                    // the post-dom tree inside any infinite loop. In particular, it
                    // guarantees we get to the farthest away point along *some*
                    // path. This also matches the GCC's behavior.
                    // If we really wanted a totally complete picture of dominance inside
                    // this infinite loop, we could do it with SCC-like algorithms to find
                    // the lowest and highest points in the infinite loop.  In theory, it
                    // would be nice to give the canonical backedge for the loop, but it's
                    // expensive and does not always lead to a minimal set of roots.
                    log::trace!("running forward DFS..");

                    let succ_order = succ_order.get_or_init(&region, bui, &snca);
                    let new_num = snca.run_dfs::<true, _>(
                        Some(n),
                        num,
                        always_descend,
                        num,
                        Some(succ_order),
                    );
                    let furthest_away = snca.num_to_node[new_num as usize];
                    match furthest_away {
                        None => log::trace!(
                            "found a new furthest away node (non-trivial root): virtual node"
                        ),
                        Some(furthest_away) => {
                            log::trace!(
                                "found a new furthest away node (non-trivial root): \
                                 {furthest_away}"
                            );
                        }
                    }
                    roots.push(furthest_away);
                    log::trace!("previous `num`: {num}, new `num` {new_num}");
                    log::trace!("removing DFS info..");
                    for i in ((num + 1)..=new_num).rev() {
                        let n = snca.num_to_node[i as usize];
                        match n {
                            None => log::trace!("removing DFS info for virtual node"),
                            Some(n) => log::trace!("removing DFS info for {n}"),
                        }
                        *snca.node_info_mut(n) = Default::default();
                        snca.num_to_node.pop();
                    }
                    let prev_num = num;
                    log::trace!("running reverse depth-first search");
                    num = snca.run_dfs::<false, _>(furthest_away, num, always_descend, 1, None);
                    for i in (prev_num + 1)..num {
                        match snca.num_to_node[i as usize] {
                            None => log::trace!("found virtual node"),
                            Some(n) => log::trace!("found node {n}"),
                        }
                    }
                }
            }
        }

        log::trace!("total: {total}, num: {num}");
        log::trace!("discovered cfg nodes:");
        for i in 0..num {
            match &snca.num_to_node[i as usize] {
                None => log::trace!("    {i}: virtual node"),
                Some(n) => log::trace!("    {i}: {n}"),
            }
        }

        assert_eq!(total + 1, num, "everything should have been visited");

        // Step 3: If we found some non-trivial roots, make them non-redundant.
        if has_non_trivial_roots {
            Self::remove_redundant_roots(snca.batch_updates.as_ref(), &mut roots);
        }

        log::trace!(
            "found roots: {}",
            DisplayValues::new(roots.iter().map(|v| DisplayOptional(v.as_ref())))
        );

        roots
    }

    // This function only makes sense for postdominators.
    //
    // We define roots to be some set of CFG nodes where (reverse) DFS walks have to start in order
    // to visit all the CFG nodes (including the reverse-unreachable ones).
    //
    // When the search for non-trivial roots is done it may happen that some of the non-trivial
    // roots are reverse-reachable from other non-trivial roots, which makes them redundant. This
    // function removes them from the set of input roots.
    fn remove_redundant_roots(
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        roots: &mut SmallVec<[Option<BlockRef>; 4]>,
    ) {
        assert!(IS_POST_DOM, "this function is for post-dominators only");

        log::trace!("removing redundant roots..");

        let mut snca = Self::new(bui.cloned());

        let mut root_index = 0;
        'roots: while root_index < roots.len() {
            let root = roots[root_index];

            // Trivial roots are never redundant
            if !Self::has_forward_successors(root, bui) {
                continue;
            }

            log::trace!("checking if {} remains a root", DisplayOptional(root.as_ref()));
            snca.clear();

            // Do a forward walk looking for the other roots.
            let num = snca.run_dfs::<true, _>(root, 0, always_descend, 0, None);
            // Skip the start node and begin from the second one (note that DFS uses 1-based indexing)
            for x in 2..(num as usize) {
                let n = snca.num_to_node[x].unwrap();

                // If we found another root in a (forward) DFS walk, remove the current root from
                // the set of roots, as it is reverse-reachable from the other one.
                if roots.iter().any(|r| r.as_ref().is_some_and(|root| root == &n)) {
                    log::trace!("forward DFS walk found another root {n}");
                    log::trace!("removing root {}", DisplayOptional(root.as_ref()));
                    roots.swap_remove(root_index);

                    // Root at the back takes the current root's place, so revisit the same index on
                    // the next iteration
                    continue 'roots;
                }
            }

            root_index += 1;
        }
    }

    pub fn do_full_dfs_walk<C>(&mut self, tree: &DomTreeBase<IS_POST_DOM>, condition: C)
    where
        for<'a> C: Copy + Fn(Option<BlockRef>, Option<BlockRef>) -> bool + 'a,
    {
        if const { !IS_POST_DOM } {
            assert_eq!(tree.num_roots(), 1, "dominators should have a single root");
            self.run_dfs::<false, _>(tree.roots()[0], 0, condition, 0, None);
            return;
        }

        self.add_virtual_root();
        let mut num = 1;
        for root in tree.roots().iter().copied() {
            num = self.run_dfs::<false, _>(root, num, condition, 1, None);
        }
    }

    pub fn attach_new_subtree(
        &mut self,
        tree: &mut DomTreeBase<IS_POST_DOM>,
        attach_to: Rc<DomTreeNode>,
    ) {
        // Attach the first unreachable block to `attach_to`
        self.node_info(self.num_to_node[1]).idom.set(attach_to.block());
        // Loop over all of the discovered blocks in the function...
        for w in self.num_to_node.iter().copied().skip(1) {
            if tree.get(w).is_some() {
                // Already computed the node before
                continue;
            }

            let idom = self.idom(w);

            // Get or compute the node for the immediate dominator
            let idom_node = self.node_for_block(idom, tree);

            // Add a new tree node for this basic block, and link it as a child of idom_node
            tree.create_node(w, idom_node);
        }
    }

    pub fn reattach_existing_subtree(
        &mut self,
        tree: &mut DomTreeBase<IS_POST_DOM>,
        attach_to: Rc<DomTreeNode>,
    ) {
        self.node_info(self.num_to_node[1]).idom.set(attach_to.block());
        for n in self.num_to_node.iter().copied().skip(1) {
            let node = tree.get(n).unwrap();
            let idom = tree.get(self.node_info(n).idom()).unwrap();
            node.set_idom(idom);
        }
    }

    // Checks if a node has proper support, as defined on the page 3 and later
    // explained on the page 7 of [2].
    pub fn has_proper_support(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        node: &DomTreeNode,
    ) -> bool {
        log::trace!("is reachable from idom {node}");

        let Some(block) = node.block() else {
            return false;
        };

        let preds = if IS_POST_DOM {
            get_children_with_batch_updates::<false, IS_POST_DOM>(block, bui)
        } else {
            get_children_with_batch_updates::<true, IS_POST_DOM>(block, bui)
        };

        for pred in preds {
            log::trace!("pred {pred}");
            if tree.get(Some(pred)).is_none() {
                continue;
            }

            let support = tree.find_nearest_common_dominator(block, pred);
            log::trace!("support {}", DisplayOptional(support.as_ref()));
            if support != Some(block) {
                log::trace!(
                    "{node} is reachable from support {}",
                    DisplayOptional(support.as_ref())
                );
                return true;
            }
        }

        false
    }
}

#[derive(Eq, PartialEq)]
struct InsertionInfoItem {
    node: Rc<DomTreeNode>,
}
impl From<Rc<DomTreeNode>> for InsertionInfoItem {
    fn from(node: Rc<DomTreeNode>) -> Self {
        Self { node }
    }
}
impl PartialOrd for InsertionInfoItem {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for InsertionInfoItem {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.node.level().cmp(&other.node.level())
    }
}

#[derive(Default)]
struct InsertionInfo {
    bucket: crate::adt::SmallPriorityQueue<InsertionInfoItem, 8>,
    visited: crate::adt::SmallSet<Rc<DomTreeNode>, 8>,
    affected: SmallVec<[Rc<DomTreeNode>; 8]>,
}

/// Insertion and Deletion
impl<const IS_POST_DOM: bool> SemiNCA<IS_POST_DOM> {
    pub fn insert_edge(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        from: Option<BlockRef>,
        to: Option<BlockRef>,
    ) {
        assert!(
            from.as_ref().is_some() || IS_POST_DOM,
            "'from' has to be a valid cfg node or a virtual root"
        );
        let to = to.expect("expected a valid `to` node");

        log::trace!("inserting edge {from:?} -> {to}");

        let from_node = tree.get(from);
        let from_node = if let Some(from_node) = from_node {
            from_node
        } else {
            // Ignore edges from unreachable nodes for (forward) dominators.
            if !IS_POST_DOM {
                return;
            }

            // The unreachable node becomes a new root -- a tree node for it.
            let virtual_root = tree.get(None);
            let from_node = tree.create_node(from, virtual_root);
            tree.roots_mut().push(from);
            from_node
        };

        tree.mark_invalid();

        let to_node = tree.get(Some(to));
        match to_node {
            None => Self::insert_unreachable(tree, bui, from_node, to),
            Some(to_node) => Self::insert_reachable(tree, bui, from_node, to_node),
        }
    }

    fn insert_unreachable(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        from: Rc<DomTreeNode>,
        to: BlockRef,
    ) {
        log::trace!("inserting {from} -> {to} (unreachable)");

        // Collect discovered edges to already reachable nodes
        // Discover and connect nodes that became reachable with the insertion.
        let mut discovered_edges_to_reachable = SmallVec::default();
        Self::compute_unreachable_dominators(
            tree,
            bui,
            to,
            from.clone(),
            &mut discovered_edges_to_reachable,
        );

        log::trace!("inserted {from} -> {to} (prev unreachable)");

        // Use the discovered edges and insert discovered connecting (incoming) edges
        for (from_block_ref, to_node) in discovered_edges_to_reachable {
            log::trace!("inserting discovered connecting edge {from_block_ref:?} -> {to_node}",);
            let from_node = tree.get(from_block_ref).unwrap();
            Self::insert_reachable(tree, bui, from_node, to_node);
        }
    }

    fn insert_reachable(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        from: Rc<DomTreeNode>,
        to: Rc<DomTreeNode>,
    ) {
        log::trace!("reachable {from} -> {to}");

        if const { IS_POST_DOM } {
            let rebuilt = SemiNCA::<true>::update_roots_before_insertion(
                unsafe {
                    core::mem::transmute::<&mut DomTreeBase<IS_POST_DOM>, &mut DomTreeBase<true>>(
                        tree,
                    )
                },
                bui.map(|bui| unsafe {
                    core::mem::transmute::<&BatchUpdateInfo<IS_POST_DOM>, &BatchUpdateInfo<true>>(
                        bui,
                    )
                }),
                to.clone(),
            );
            if rebuilt {
                return;
            }
        }

        // find_nearest_common_dominator expects both pointers to be valid. When `from` is a virtual
        // root, then its CFG block pointer is `None`, so we have to "compute" the NCD manually
        let ncd_block = if from.block().is_some() && to.block().is_some() {
            tree.find_nearest_common_dominator(from.block().unwrap(), to.block().unwrap())
        } else {
            None
        };
        assert!(ncd_block.is_some() || tree.is_post_dominator());
        let ncd = tree.get(ncd_block).unwrap();

        log::trace!("nearest common dominator == {ncd}");

        // Based on Lemma 2.5 from [2], after insertion of (from, to), `v` is affected iff
        // depth(ncd) + 1 < depth(v) && a path `P` from `to` to `v` exists where every `w` on `P`
        // s.t. depth(v) <= depth(w)
        //
        // This reduces to a widest path problem (maximizing the depth of the minimum vertex in
        // the path) which can be solved by a modified version of Dijkstra with a bucket queue
        // (named depth-based search in [2]).
        //
        // `to` is in the path, so depth(ncd) + 1 < depth(v) <= depth(to). Nothing affected if
        // this does not hold.
        let ncd_level = ncd.level();
        if ncd_level + 1 >= to.level() {
            return;
        }

        let mut insertion_info = InsertionInfo::default();
        let mut unaffected_on_current_level = SmallVec::<[Rc<DomTreeNode>; 8]>::default();
        insertion_info.bucket.push(to.clone().into());
        insertion_info.visited.insert(to);

        while let Some(InsertionInfoItem { mut node }) = insertion_info.bucket.pop() {
            insertion_info.affected.push(node.clone());

            let current_level = node.level();
            log::trace!("mark {node} as affected, current level: {current_level}");

            assert!(node.block().is_some() && insertion_info.visited.contains(&node));

            loop {
                // Unlike regular Dijkstra, we have an inner loop to expand more
                // vertices. The first iteration is for the (affected) vertex popped
                // from II.Bucket and the rest are for vertices in
                // UnaffectedOnCurrentLevel, which may eventually expand to affected
                // vertices.
                //
                // Invariant: there is an optimal path from `To` to TN with the minimum
                // depth being CurrentLevel.
                for succ in get_children_with_batch_updates::<IS_POST_DOM, IS_POST_DOM>(
                    node.block().unwrap(),
                    bui,
                ) {
                    let succ_node = tree
                        .get(Some(succ))
                        .expect("unreachable successor found during reachable insertion");
                    let succ_level = succ_node.level();
                    log::trace!("successor {succ_node}, level = {succ_level}");

                    // There is an optimal path from `To` to Succ with the minimum depth
                    // being min(CurrentLevel, SuccLevel).
                    //
                    // If depth(NCD)+1 < depth(Succ) is not satisfied, Succ is unaffected
                    // and no affected vertex may be reached by a path passing through it.
                    // Stop here. Also, Succ may be visited by other predecessors but the
                    // first visit has the optimal path. Stop if Succ has been visited.
                    if succ_level <= ncd_level + 1
                        || !insertion_info.visited.insert(succ_node.clone())
                    {
                        continue;
                    }

                    if succ_level > current_level {
                        // succ is unaffected, but it may (transitively) expand to affected vertices.
                        // Store it in unaffected_on_current_level
                        log::trace!("marking visiting not affected {succ}");
                        unaffected_on_current_level.push(succ_node.clone());
                    } else {
                        // The condition is satisfied (Succ is affected). Add Succ to the
                        // bucket queue.
                        log::trace!("add {succ} to a bucket");
                        insertion_info.bucket.push(succ_node.clone().into());
                    }
                }

                if unaffected_on_current_level.is_empty() {
                    break;
                }

                if let Some(n) = unaffected_on_current_level.pop() {
                    node = n;
                } else {
                    break;
                }
                log::trace!("next: {node}");
            }
        }

        // Finish by updating immediate dominators and levels.
        Self::update_insertion(tree, bui, ncd, &insertion_info);
    }

    pub fn delete_edge(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        from: Option<BlockRef>,
        to: Option<BlockRef>,
    ) {
        let from = from.expect("cannot disconnect virtual node");
        let to = to.expect("cannot disconnect virtual node");

        log::trace!("deleting edge {from} -> {to}");

        // Deletion in an unreachable subtree -- nothing to do.
        let Some(from_node) = tree.get(Some(from)) else {
            return;
        };

        let Some(to_node) = tree.get(Some(to)) else {
            log::trace!("to {to} already unreachable -- there is no edge to delete",);
            return;
        };

        let ncd_block = tree.find_nearest_common_dominator(from, to);
        let ncd = tree.get(ncd_block);

        // If to dominates from -- nothing to do.
        if Some(&to_node) != ncd.as_ref() {
            tree.mark_invalid();

            let to_idom = to_node.idom();
            log::trace!(
                "ncd {}, to_idom {}",
                DisplayOptional(ncd.as_ref()),
                DisplayOptional(to_idom.as_ref())
            );

            // To remains reachable after deletion (based on caption under figure 4, from [2])
            if (Some(&from_node) != to_idom.as_ref())
                || Self::has_proper_support(tree, bui, &to_node)
            {
                Self::delete_reachable(tree, bui, from_node, to_node)
            } else {
                Self::delete_unreachable(tree, bui, to_node)
            }

            if const { IS_POST_DOM } {
                SemiNCA::<true>::update_roots_after_update(
                    unsafe {
                        core::mem::transmute::<&mut DomTreeBase<IS_POST_DOM>, &mut DomTreeBase<true>>(
                            tree,
                        )
                    },
                    bui.map(|bui| unsafe {
                        core::mem::transmute::<&BatchUpdateInfo<IS_POST_DOM>, &BatchUpdateInfo<true>>(
                            bui,
                        )
                    }),
                );
            }
        }
    }

    /// Handles deletions that leave destination nodes reachable.
    fn delete_reachable(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        from: Rc<DomTreeNode>,
        to: Rc<DomTreeNode>,
    ) {
        log::trace!("deleting reachable {from} -> {to} - rebuilding subtree..");

        // Find the top of the subtree that needs to be rebuilt (based on the lemma 2.6 from [2])
        let to_idom =
            tree.find_nearest_common_dominator(from.block().unwrap(), to.block().unwrap());
        assert!(to_idom.is_some() || tree.is_post_dominator());
        let to_idom_node = tree.get(to_idom).unwrap();
        let prev_idom_subtree = to_idom_node.idom();
        // Top of the subtree to rebuild is the root node. Rebuild the tree from scratch.
        let Some(prev_idom_subtree) = prev_idom_subtree else {
            log::trace!("the entire tree needs to be rebuilt");
            Self::compute_from_scratch(tree, bui.cloned());
            return;
        };

        // Only visit nodes in the subtree starting at `to`
        let level = to_idom_node.level();
        let descend_below = |_: Option<BlockRef>, to: Option<BlockRef>| -> bool {
            tree.get(to).unwrap().level() > level
        };

        log::trace!("top of subtree {to_idom_node}");

        let mut snca = Self::new(bui.cloned());
        snca.run_dfs::<false, _>(to_idom, 0, descend_below, 0, None);
        log::trace!("running Semi-NCA");
        snca.run();
        snca.reattach_existing_subtree(tree, prev_idom_subtree);
    }

    /// Handle deletions that make destination node unreachable.
    ///
    /// (Based on the lemma 2.7 from the [2].)
    fn delete_unreachable(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        to: Rc<DomTreeNode>,
    ) {
        log::trace!("deleting unreachable subtree {to}");
        assert!(to.block().is_some());

        if IS_POST_DOM {
            // Deletion makes a region reverse-unreachable and creates a new root.
            //
            // Simulate that by inserting an edge from the virtual root to `to` and adding it as a new
            // root.
            log::trace!("deletion made a region reverse-unreachable, adding new root {to}");
            tree.roots_mut().push(to.block());
            Self::insert_reachable(tree, bui, tree.get(None).unwrap(), to);
            return;
        }

        let mut affected_queue = SmallVec::<[Option<BlockRef>; 16]>::default();
        let level = to.level();

        // Traverse destination node's descendants with greater level in the tree
        // and collect visited nodes.
        let descend_and_collect = |_: Option<BlockRef>, to: Option<BlockRef>| -> bool {
            let node = tree.get(to).unwrap();
            if node.level() > level {
                return true;
            }
            if !affected_queue.contains(&to) {
                affected_queue.push(to)
            }
            false
        };

        let mut snca = Self::new(bui.cloned());
        let last_dfs_num = snca.run_dfs::<false, _>(to.block(), 0, descend_and_collect, 0, None);

        let mut min_node = to.clone();
        // Identify the top of the subtree to rebuild by finding the NCD of all the affected nodes.
        for n in affected_queue {
            let node = tree.get(n).unwrap();
            let ncd_block =
                tree.find_nearest_common_dominator(node.block().unwrap(), to.block().unwrap());
            assert!(ncd_block.is_some() || tree.is_post_dominator());
            let ncd = tree.get(ncd_block).unwrap();
            log::trace!(
                "processing affected node {node} with: nearest common dominator = {ncd}, min node \
                 = {min_node}"
            );
            if ncd != node && ncd.level() < min_node.level() {
                min_node = ncd;
            }
        }

        // Root reached, rebuild the whole tree from scratch.
        if min_node.idom().is_none() {
            log::trace!("the entire tree needs to be rebuilt");
            Self::compute_from_scratch(tree, bui.cloned());
            return;
        }

        // Erase the unreachable subtree in reverse preorder to process all children before deleting
        // their parent.
        for i in (1..=(last_dfs_num as usize)).rev() {
            if let Some(n) = snca.num_to_node[i] {
                log::trace!("erasing node {n}");
                tree.erase_node(n);
            }
        }

        // The affected subtree start at the `to` node -- there's no extra work to do.
        if min_node == to {
            return;
        }

        log::trace!("delete_unreachable: running dfs with min_node = {min_node}");
        let min_level = min_node.level();
        let prev_idom = min_node.idom().unwrap();
        snca.clear();

        // Identify nodes that remain in the affected subtree.
        let descend_below = |_: Option<BlockRef>, to: Option<BlockRef>| -> bool {
            let to_node = tree.get(to);
            to_node.is_some_and(|to_node| to_node.level() > min_level)
        };
        snca.run_dfs::<false, _>(min_node.block(), 0, descend_below, 0, None);

        log::trace!("previous idom(min_node) = {prev_idom}");
        log::trace!("running Semi-NCA");

        // Rebuild the remaining part of affected subtree.
        snca.run();
        snca.reattach_existing_subtree(tree, prev_idom);
    }

    pub fn apply_updates(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        mut pre_view_cfg: cfg::CfgDiff<IS_POST_DOM>,
        post_view_cfg: cfg::CfgDiff<IS_POST_DOM>,
    ) {
        // Note: the `post_view_cfg` is only used when computing from scratch. It's data should
        // already included in the `pre_view_cfg` for incremental updates.
        let num_updates = pre_view_cfg.num_legalized_updates();
        match num_updates {
            0 => (),
            1 => {
                // Take the fast path for a single update and avoid running the batch update machinery.
                let update = pre_view_cfg.pop_update_for_incremental_updates();
                let bui = if post_view_cfg.is_empty() {
                    None
                } else {
                    Some(BatchUpdateInfo::new(post_view_cfg.clone(), Some(post_view_cfg)))
                };
                match update.kind() {
                    cfg::CfgUpdateKind::Insert => {
                        Self::insert_edge(
                            tree,
                            bui.as_ref(),
                            Some(update.from()),
                            Some(update.to()),
                        );
                    }
                    cfg::CfgUpdateKind::Delete => {
                        Self::delete_edge(
                            tree,
                            bui.as_ref(),
                            Some(update.from()),
                            Some(update.to()),
                        );
                    }
                }
            }
            _ => {
                let mut bui = BatchUpdateInfo::new(pre_view_cfg, Some(post_view_cfg));
                // Recalculate the DominatorTree when the number of updates exceeds a threshold,
                // which usually makes direct updating slower than recalculation. We select this
                // threshold proportional to the size of the DominatorTree. The constant is selected
                // by choosing the one with an acceptable performance on some real-world inputs.

                // Make unittests of the incremental algorithm work
                // TODO(pauls): review this
                if tree.len() <= 100 {
                    if bui.num_legalized > tree.len() {
                        Self::compute_from_scratch(tree, Some(bui.clone()));
                    }
                } else if bui.num_legalized > tree.len() / 40 {
                    Self::compute_from_scratch(tree, Some(bui.clone()));
                }

                // If the DominatorTree was recalculated at some point, stop the batch updates. Full
                // recalculations ignore batch updates and look at the actual CFG.
                for _ in 0..bui.num_legalized {
                    if bui.is_recalculated {
                        break;
                    }

                    Self::apply_next_update(tree, &mut bui);
                }
            }
        }
    }

    fn apply_next_update(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: &mut BatchUpdateInfo<IS_POST_DOM>,
    ) {
        // Popping the next update, will move the `pre_view_cfg` to the next snapshot.
        let current_update = bui.pre_cfg_view.pop_update_for_incremental_updates();
        log::trace!("applying update: {current_update:?}");

        match current_update.kind() {
            cfg::CfgUpdateKind::Insert => {
                Self::insert_edge(
                    tree,
                    Some(bui),
                    Some(current_update.from()),
                    Some(current_update.to()),
                );
            }
            cfg::CfgUpdateKind::Delete => {
                Self::delete_edge(
                    tree,
                    Some(bui),
                    Some(current_update.from()),
                    Some(current_update.to()),
                );
            }
        }
    }

    pub fn compute(tree: &mut DomTreeBase<IS_POST_DOM>) {
        Self::compute_from_scratch(tree, None);
    }

    pub fn compute_from_scratch(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        mut bui: Option<BatchUpdateInfo<IS_POST_DOM>>,
    ) {
        use crate::cfg::GraphDiff;

        tree.reset();

        // If the update is using the actual CFG, `bui` is `None`. If it's using a view, `bui` is
        // `Some` and the `pre_cfg_view` is used. When calculating from scratch, make the
        // `pre_cfg_view` equal to the `post_cfg_view`, so `post` is used.
        let post_view_bui = bui.clone().and_then(|mut bui| {
            if !bui.post_cfg_view.is_empty() {
                bui.pre_cfg_view = bui.post_cfg_view.clone();
                Some(bui)
            } else {
                None
            }
        });

        // This is rebuilding the whole tree, not incrementally, but `post_view_bui` is used in case
        // the caller needs a dominator tree update with a cfg view
        let mut snca = Self::new(post_view_bui);

        // Step 0: Number blocks in depth-first order, and initialize variables used in later stages
        // of the algorithm.
        let roots = Self::find_roots(tree, bui.as_ref());
        *tree.roots_mut() = roots;
        snca.do_full_dfs_walk(tree, always_descend);

        snca.run();
        if let Some(bui) = bui.as_mut() {
            bui.is_recalculated = true;
            log::trace!("dominator tree recalculated, skipping future batch updates");
        }

        if tree.roots().is_empty() {
            return;
        }

        // Add a node for the root. If the tree is a post-dominator tree, it will be the virtual
        // exit (denoted by a block ref of `None`), which post-dominates all real exits (including
        // multiple exit blocks, infinite loops).
        let root = if IS_POST_DOM { None } else { tree.roots()[0] };

        let new_root = tree.create_node(root, None);
        tree.set_root(new_root);
        let root_node = tree.root_node().expect("expected root node");
        snca.attach_new_subtree(tree, root_node);
    }

    fn update_insertion(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        ncd: Rc<DomTreeNode>,
        insertion_info: &InsertionInfo,
    ) {
        log::trace!("updating nearest common dominator = {ncd}");

        for to_node in insertion_info.affected.iter().cloned() {
            log::trace!("idom({to_node}) = {ncd}");
            to_node.set_idom(ncd.clone());
        }

        if IS_POST_DOM {
            SemiNCA::<true>::update_roots_after_update(
                unsafe {
                    core::mem::transmute::<&mut DomTreeBase<IS_POST_DOM>, &mut DomTreeBase<true>>(
                        tree,
                    )
                },
                bui.map(|bui| unsafe {
                    core::mem::transmute::<&BatchUpdateInfo<IS_POST_DOM>, &BatchUpdateInfo<true>>(
                        bui,
                    )
                }),
            );
        }
    }

    /// Connects nodes that become reachable with an insertion
    fn compute_unreachable_dominators(
        tree: &mut DomTreeBase<IS_POST_DOM>,
        bui: Option<&BatchUpdateInfo<IS_POST_DOM>>,
        root: BlockRef,
        incoming: Rc<DomTreeNode>,
        discovered_connecting_edges: &mut SmallVec<[(Option<BlockRef>, Rc<DomTreeNode>); 8]>,
    ) {
        assert!(tree.get(Some(root)).is_none(), "root must not be reachable");

        // Visit only previously unreachable nodes
        let unreachable_descender = |from: Option<BlockRef>, to: Option<BlockRef>| -> bool {
            let to_node = tree.get(to);
            match to_node {
                None => true,
                Some(to_node) => {
                    discovered_connecting_edges.push((from, to_node));
                    false
                }
            }
        };

        let mut snca = Self::new(bui.cloned());
        snca.run_dfs::<false, _>(Some(root), 0, unreachable_descender, 0, None);
        snca.run();
        snca.attach_new_subtree(tree, incoming);

        log::trace!("after adding unreachable nodes");
    }
}

/// Verification
impl<const IS_POST_DOM: bool> SemiNCA<IS_POST_DOM> {
    pub fn verify_roots(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }

    pub fn verify_reachability(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }

    pub fn verify_levels(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }

    pub fn verify_dfs_numbers(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }

    pub fn verify_parent_property(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }

    pub fn verify_sibling_property(&self, _tree: &DomTreeBase<IS_POST_DOM>) -> bool {
        true
    }
}

impl SemiNCA<true> {
    /// Determines if some existing root becomes reverse-reachable after the insertion.
    ///
    /// Rebuilds the whole tree if that situation happens.
    fn update_roots_before_insertion(
        tree: &mut DomTreeBase<true>,
        bui: Option<&BatchUpdateInfo<true>>,
        to: Rc<DomTreeNode>,
    ) -> bool {
        // Destination node is not attached to the virtual root, so it cannot be a root
        if !tree.is_virtual_root(&to.idom().unwrap()) {
            return false;
        }

        if !tree.roots().contains(&to.block()) {
            // To is not a root, nothing to update
            return false;
        }

        log::trace!("after the insertion, {to} is no longer a root - rebuilding the tree..");

        Self::compute_from_scratch(tree, bui.cloned());
        true
    }

    /// Updates the set of roots after insertion or deletion.
    ///
    /// This ensures that roots are the same when after a series of updates and when the tree would
    /// be built from scratch.
    fn update_roots_after_update(
        tree: &mut DomTreeBase<true>,
        bui: Option<&BatchUpdateInfo<true>>,
    ) {
        // The tree has only trivial roots -- nothing to update.
        if !tree.roots().iter().copied().any(|n| Self::has_forward_successors(n, bui)) {
            return;
        }

        // Recalculate the set of roots
        let roots = Self::find_roots(tree, bui);
        if !is_permutation(tree.roots(), &roots) {
            // The roots chosen in the CFG have changed. This is because the incremental algorithm
            // does not really know or use the set of roots and can make a different (implicit)
            // decision about which node within an infinite loop becomes a root.
            log::trace!(
                "roots are different in updated trees - the entire tree needs to be rebuilt"
            );
            // It may be possible to update the tree without recalculating it, but we do not know
            // yet how to do it, and it happens rarely in practice.
            Self::compute_from_scratch(tree, bui.cloned());
        }
    }
}

fn is_permutation(a: &[Option<BlockRef>], b: &[Option<BlockRef>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let set = crate::adt::SmallSet::<_, 4>::from_iter(a.iter().cloned());
    for n in b {
        if !set.contains(n) {
            return false;
        }
    }
    true
}

#[doc(hidden)]
#[inline(always)]
const fn always_descend(_: Option<BlockRef>, _: Option<BlockRef>) -> bool {
    true
}
