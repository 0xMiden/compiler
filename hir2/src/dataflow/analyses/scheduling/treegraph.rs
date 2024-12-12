use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use core::{cmp::Ordering, fmt};

use smallvec::SmallVec;

use super::depgraph::*;

/// [OrderedTreeGraph] represents an immutable, fully-constructed and topologically sorted
/// [TreeGraph].
///
/// This is the representation we use during instruction scheduling.
#[derive(Default, Debug)]
pub struct OrderedTreeGraph {
    /// The topological order of nodes in `graph`
    ordering: Vec<NodeId>,
    /// For each tree in `graph`, a data structure which tells us in what order
    /// the nodes of that tree will be visited. The smaller the index, the earlier we
    /// will emit that node.
    indices: BTreeMap<NodeId, DependencyGraphIndices>,
    /// The underlying [TreeGraph]
    graph: TreeGraph,
}
impl TryFrom<DependencyGraph> for OrderedTreeGraph {
    type Error = UnexpectedCycleError;

    fn try_from(depgraph: DependencyGraph) -> Result<Self, Self::Error> {
        let graph = TreeGraph::from(depgraph.clone());
        let ordering = graph.toposort()?;
        let indices = ordering.iter().copied().try_fold(BTreeMap::default(), |mut acc, root| {
            acc.insert(root, depgraph.indexed(root)?);
            Ok(acc)
        })?;

        Ok(Self {
            ordering,
            indices,
            graph,
        })
    }
}
impl OrderedTreeGraph {
    /// Compute an [OrderedTreeGraph] corresponding to the given [DependencyGraph]
    pub fn new(depgraph: &DependencyGraph) -> Result<Self, UnexpectedCycleError> {
        Self::try_from(depgraph.clone())
    }

    /// Returns an iterator over nodes in the graph, in topological order.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = NodeId> + '_ {
        self.ordering.iter().copied()
    }

    /// Returns true if `a` is scheduled before `b` according to the graph
    ///
    /// See `cmp_scheduling` for details on what scheduling before/after implies.
    #[inline]
    pub fn is_scheduled_before<A, B>(&self, a: A, b: B) -> bool
    where
        A: Into<NodeId>,
        B: Into<NodeId>,
    {
        self.cmp_scheduling(a, b).is_lt()
    }

    /// Returns true if `a` is scheduled after `b` according to the graph
    ///
    /// See `cmp_scheduling` for details on what scheduling before/after implies.
    #[inline]
    pub fn is_scheduled_after<A, B>(&self, a: A, b: B) -> bool
    where
        A: Into<NodeId>,
        B: Into<NodeId>,
    {
        self.cmp_scheduling(a, b).is_gt()
    }

    /// Compare two nodes in terms of their scheduling in the graph.
    ///
    /// If `a` compares less than `b`, then `a` is scheduled before `b`,
    /// and vice versa. Two nodes can only compare equal if they are the
    /// same node.
    ///
    /// "Scheduled" here refers to when a node will be visited by the scheduler
    /// during its planning phase, which is the opposite order that a given node
    /// will be emitted during code generation. This is due to the fact that we visit
    /// the dependency graph of a block lazily and bottom-up (i.e. for a given block,
    /// we start at the terminator and then materialize any instructions/values
    /// referenced by it).
    pub fn cmp_scheduling<A, B>(&self, a: A, b: B) -> Ordering
    where
        A: Into<NodeId>,
        B: Into<NodeId>,
    {
        let a = a.into();
        let b = b.into();
        if a == b {
            return Ordering::Equal;
        }

        let a_tree = self.graph.root_id(a);
        let b_tree = self.graph.root_id(b);

        // If the nodes reside in the same tree, the precise scheduling
        // order is determined by the indexing order for that tree.
        if a_tree == b_tree {
            let indices = &self.indices[&a_tree];
            let a_idx = indices.get(a).unwrap();
            let b_idx = indices.get(b).unwrap();

            return a_idx.cmp(&b_idx).reverse();
        }

        // Whichever tree appears first in the topological order will be
        // scheduled before the other.
        assert!(
            !self.ordering.is_empty(),
            "invalid treegraph: the topographical ordering is empty even though the underlying \
             graph is not"
        );
        for n in self.ordering.iter().copied() {
            if n == a_tree {
                return Ordering::Less;
            }
            if n == b_tree {
                return Ordering::Greater;
            }
        }

        unreachable!(
            "invalid treegraph: there are roots in the dependency graph not represented in the \
             topographical ordering"
        )
    }
}
impl core::convert::AsRef<TreeGraph> for OrderedTreeGraph {
    #[inline(always)]
    fn as_ref(&self) -> &TreeGraph {
        &self.graph
    }
}
impl core::ops::Deref for OrderedTreeGraph {
    type Target = TreeGraph;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

/// A [TreeGraph] is used to represent dependencies between expression trees in a program,
/// derived from a [DependencyGraph].
///
/// ## What is a TreeGraph?
///
/// An expression tree is a component of a [DependencyGraph] where each node has at most
/// one predecessor. For example, `(a + b - c) / 2` is an obvious case of such a tree,
/// as each sub-expression produces a single result which is used by the next operator,
/// ultimately producing the final result of the outermost expression.
///
/// A program can be made up of many such trees, and in some cases the values produced by
/// a tree, or even sub-expressions within the tree, may be used multiple times.
/// To riff on our example above, consider the following:
///
/// ```text,ignore
/// let d = (a + b - c);
/// let e = d / 2;
/// let f = e * e;
/// d == f
/// ```
///
/// This still contains the expression tree from before, but with parts of it reused from
/// another expression tree that, had it duplicated the expressions that are reused, would
/// have formed a larger expression tree. Instead, the reuse results in a forest of two
/// trees, where the "outer" tree depends on the results of the "inner" tree.
///
/// This is the essence of a [TreeGraph] - it represents a program as a forest of expression
/// trees, just without requiring the program itself to be a tree, resulting in a directed,
/// acyclic graph rather than a forest in the graph-theoretic sense.
///
/// Nodes in this graph are the roots of the expression trees represented, i.e. each expression
/// tree is condensed into a single node. This is because the graph is largely only concerned with
/// connections between the trees, but we still would be able to answer questions like:
///
/// * Is a given [Node] a root in the tree graph
/// * If not, what is the root [Node] corresponding to that node
/// * Is a [Node] a member of a given tree
/// * What are the dependencies between trees
/// * What dependencies are condensed in the edge connecting two treegraph nodes
///
/// The specific way we have implemented [TreeGraph] lets us do all of the above.
///
/// ## Use Case
///
/// The [TreeGraph] forms the foundation around which instruction scheduling is performed during
/// code generation. We construct a [DependencyGraph] for each block in a function, and derive
/// a corresponding [TreeGraph]. We then use the reverse topological ordering of the resulting
/// graph as the instruction schedule for that block.
///
/// Fundamentally though, a [TreeGraph] is a data structure designed to solve the issue of
/// how to efficiently generate code for a stack machine from a non-stack-oriented program
/// representation. It allows one to keep everything on the operand stack, rather than requiring
/// loads/stores to temporaries, and naturally places operands exactly where they are needed,
/// when they are needed. This is a particularly good fit for Miden, because Miden IR is in SSA
/// form, and we need to convert it to efficient Miden Assembly which is a stack machine ISA.
///
/// ## Additional Reading
///
/// The treegraph data structure was (to my knowledge) first described in
/// [this paper](https://www.sciencedirect.com/science/article/pii/S1571066111001538?via=ihub)
/// by Park, et al., called _Treegraph-based Instruction Scheduling for Stack-based Virtual
/// Machines_.
///
/// The implementation and usage of both [DependencyGraph] and [TreeGraph] are based on the design
/// and algorithms described in that paper, so it is worth reading if you are curious about it.
/// Our implementation here is tailored for our use case (i.e. the way we represent nodes has a
/// specific effect on the order in which we visit instructions and their arguments during
/// scheduling), but the overall properties of the data structure described in that paper hold for
/// [TreeGraph] as well.
#[derive(Default, Clone)]
pub struct TreeGraph {
    /// The nodes which are explicitly represented in the graph
    nodes: BTreeSet<NodeId>,
    /// Edges between nodes in the graph, where an edge may carry multiple dependencies
    edges: BTreeMap<EdgeId, SmallVec<[DependencyEdge; 1]>>,
    /// A mapping of condensed nodes to the root node of the tree they were condensed into
    condensed: BTreeMap<NodeId, NodeId>,
}

/// Represents an edge between [TreeGraph] roots.
///
/// Each pair of nodes in a treegraph may only have one [EdgeId], but
/// multiple [DependencyEdge]s which represents each unique dependency
/// from predecessor root to successor root.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeId {
    /// The treegraph root which is predecessor
    predecessor: NodeId,
    /// The treegraph root which is successor
    successor: NodeId,
}

/// Represents a unique edge between dependency graph nodes in a [TreeGraph].
///
/// The referenced nodes do not have to be represented explicitly in the treegraph as
/// roots, they may also be condensed members of one of the trees in the graph.
/// To help illustrate what I mean, consider an instruction whose result is used
/// twice: once as a block argument to a successor block, and once as an operand
/// to another instruction that is part of an expression tree. The result node will
/// be represented in the treegraph as a root (because it has multiple uses); as will
/// the block terminator (because a terminator can have no uses). However, the last
/// instruction which uses our result is part of an expression tree, so it will not
/// be a root in the treegraph, and instead will be condensed under a root representing
/// the expression tree. Thus we will have two [DependencyEdge] items, one where the
/// predecessor (dependent) is a root; and one where it is not.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyEdge {
    /// The specific node in the dependency graph which is predecessor
    predecessor: NodeId,
    /// The specific node in the dependency graph which is successor
    successor: NodeId,
}

impl TreeGraph {
    /// Returns true if `node` represents a tree in this graph
    #[inline(always)]
    pub fn is_root(&self, node: impl Into<NodeId>) -> bool {
        self.nodes.contains(&node.into())
    }

    /// Return the node representing the root of the tree to which `node` belongs
    ///
    /// If `node` is the root of its tree, it simply returns itself.
    ///
    /// NOTE: This function will panic if `node` is not a root OR a node condensed
    /// in any tree of the graph.
    #[inline]
    pub fn root(&self, node: impl Into<NodeId>) -> Node {
        self.condensed[&node.into()].into()
    }

    /// Same as [TreeGraph::root], but returns the node identifier, which avoids
    /// decoding the [NodeId] into a [Node] if not needed.
    #[inline]
    pub fn root_id(&self, node: impl Into<NodeId>) -> NodeId {
        self.condensed[&node.into()]
    }

    /// Returns true if `node` is a member of the tree represented by `root`
    ///
    /// NOTE: This function will panic if `root` is not a tree root
    pub fn is_member_of<A, B>(&self, node: A, root: B) -> bool
    where
        A: Into<NodeId>,
        B: Into<NodeId>,
    {
        let root = root.into();
        assert!(self.is_root(root));
        self.root(node).id() == root
    }

    /// Return the number of times that `node` is referenced as a dependency.
    ///
    /// Here, `node` can be either explicitly represented in the graph as a tree
    /// root, or implicitly, as a condensed node belonging to a tree.
    ///
    /// For tree roots, the number of dependencies can be different than the
    /// number of predecessors, as edges can carry multiple dependencies, and
    /// those dependencies can reference the root. However, tree roots by construction
    /// must have either zero, or more than one dependent.
    ///
    /// Condensed nodes on the other hand, by construction have only one dependent, as they
    /// belong to a tree; but they may also be referenced by dependencies in the edges
    /// between treegraph nodes, so we must check all edges inbound on the tree containing
    /// the node for dependencies on `node`.
    pub fn num_dependents(&self, node: impl Into<NodeId>) -> usize {
        let node = node.into();
        if self.is_root(node) {
            self.dependents(node).count()
        } else {
            self.dependents(node).count() + 1
        }
    }

    /// Return an iterator over every node which depends on `node`
    pub fn dependents(&self, node: impl Into<NodeId>) -> impl Iterator<Item = Node> + '_ {
        let dependency_id = node.into();
        let root_id = self.root_id(dependency_id);
        self.edges
            .iter()
            .filter_map(move |(eid, edges)| {
                if eid.successor == root_id {
                    Some(edges.as_slice())
                } else {
                    None
                }
            })
            .flat_map(move |edges| {
                edges.iter().filter_map(move |e| {
                    if e.successor == dependency_id {
                        Some(e.predecessor.into())
                    } else {
                        None
                    }
                })
            })
    }

    /// Return an iterator over each [Dependency] in the edge from `a` to `b`
    ///
    /// NOTE: This function will panic if either `a` or `b` are not tree roots
    pub fn edges(&self, a: NodeId, b: NodeId) -> impl Iterator<Item = Dependency> + '_ {
        let id = EdgeId {
            predecessor: a,
            successor: b,
        };
        self.edges[&id].iter().map(|e| Dependency {
            dependent: e.predecessor,
            dependency: e.successor,
        })
    }

    /// Return the number of predecessors for `node` in this graph.
    ///
    /// NOTE: This function will panic if `node` is not a tree root
    pub fn num_predecessors(&self, node: impl Into<NodeId>) -> usize {
        let node_id = node.into();
        self.edges.keys().filter(|e| e.successor == node_id).count()
    }

    /// Return an iterator over [Node]s which are predecessors of `node` in this graph.
    ///
    /// NOTE: This function will panic if `node` is not a tree root
    pub fn predecessors(&self, node: impl Into<NodeId>) -> impl Iterator<Item = Node> + '_ {
        let node_id = node.into();
        self.edges.keys().filter_map(move |eid| {
            if eid.successor == node_id {
                Some(eid.predecessor.into())
            } else {
                None
            }
        })
    }

    /// Return an iterator over [Node]s which are successors of `node` in this graph.
    ///
    /// NOTE: This function will panic if `node` is not a tree root
    pub fn successors(&self, node: impl Into<NodeId>) -> impl Iterator<Item = Node> + '_ {
        let node_id = node.into();
        self.edges.keys().filter_map(move |eid| {
            if eid.predecessor == node_id {
                Some(eid.successor.into())
            } else {
                None
            }
        })
    }

    /// Return an iterator over [NodeId]s for successors of `node` in this graph.
    ///
    /// NOTE: This function will panic if `node` is not a tree root
    pub fn successor_ids(&self, node: impl Into<NodeId>) -> impl Iterator<Item = NodeId> + '_ {
        let node_id = node.into();
        self.edges.keys().filter_map(move |eid| {
            if eid.predecessor == node_id {
                Some(eid.successor)
            } else {
                None
            }
        })
    }

    /// Remove the edge connecting `a` and `b`.
    ///
    /// NOTE: This function will panic if either `a` or `b` are not tree roots
    pub fn remove_edge(&mut self, a: NodeId, b: NodeId) {
        self.edges.remove(&EdgeId {
            predecessor: a,
            successor: b,
        });
    }

    /// Returns a vector of [TreeGraph] roots, sorted in topological order.
    ///
    /// Returns `Err` if a cycle is detected, making a topological sort impossible.
    ///
    /// The reverse topological ordering of a [TreeGraph] represents a valid scheduling of
    /// the nodes in that graph, as each node is observed before any of it's dependents.
    ///
    /// Additionally, we ensure that any topological ordering of the graph schedules instruction
    /// operands in stack order naturally, i.e. if we emit code from the schedule, there should
    /// be little to no stack manipulation required to get instruction operands in the correct
    /// order.
    ///
    /// This is done in the form of an implicit heuristic: the natural ordering for nodes in the
    /// graph uses the [Ord] implementation of [NodeId]. When visiting nodes, this order is used,
    /// and in the specific case of instruction arguments, which by construction only have a single
    /// parent (dependent), they will be visited in this order. In general, this heuristic can be
    /// thought of as falling back to the original program order when no other criteria is available
    /// for sorting. In reality, it's more of a natural synergy in the data structures representing
    /// the graph and nodes in the graph, so it is not nearly so explicit - but the effect is the
    /// same.
    ///
    /// ## Example
    ///
    /// To better understand how the IR translates to the topological ordering described above,
    /// consider the following IR:
    ///
    /// ```miden-ir,ignore
    /// blk0(a, b):
    ///   c = mul b, b          % inst1
    ///   d = add a, c          % inst2
    ///   e = eq.imm d, 0       % inst3
    ///   br blk1(d, e, b)      % inst4
    /// ```
    ///
    /// This code would be a simple expression tree, except we have an instruction result which
    /// is used twice in order to pass an intermediate result to the successor block. This is
    /// what we'd like to examine in order to determine how such a block will get scheduled.
    ///
    /// Above we've annotated each instruction in the block with the instruction identifier, which
    /// we'll use when referring to those instructions from now on.
    ///
    /// This IR would get represented in a [DependencyGraph] like so:
    ///
    /// ```text,ignore
    ///         inst4 ----------
    ///       /       \         |
    ///       v       v         v
    ///     arg(0)   arg(1)   arg(2)
    ///       |        |        |
    ///       |        v        |
    ///       |     result(e)   |
    ///       |        |        |
    ///       |        v        |
    ///       |      inst3      |
    ///       v        |        |
    ///   result(d)<---         |
    ///       |                 |
    ///       v                 |
    ///     inst2               |
    ///       |_______          |
    ///       v       v         |
    ///     arg(0)  arg(1)      |
    ///       |       |         |
    ///       v       v         |
    ///    stack(a) result(c)   |
    ///               |         |
    ///               v         |
    ///             inst1       |
    ///            /     \      |
    ///           v       v     |
    ///         arg(0)  arg(1)  |
    ///            \    /       |
    ///             v   v       |
    ///            stack(b)<----
    /// ```
    ///
    /// As you can see, arguments and results are explicitly represented in the dependency graph
    /// so that we can precisely represent a few key properties:
    ///
    /// 1. The unique arguments of an instruction
    /// 2. The source of an argument (instruction result or stack operand)
    /// 3. Which instruction results are used or unused
    /// 4. Which instruction results have multiple uses
    /// 5. Which values must be copied or moved, and at which points that must happen
    ///
    /// In any case, the dependency graph above gets translated to the following [TreeGraph]:
    ///
    /// ```text,ignore
    ///        inst4
    ///      /     |
    ///     v      |
    ///  result(d) |
    ///      \     |
    ///       v    v
    ///       stack(b)
    /// ```
    ///
    /// That might be confusing, but the intuition here is straightforward: the nodes which are
    /// explicitly represented in a [TreeGraph] are those nodes in the original [DependencyGraph]
    /// with either no dependents, or multiple dependents. Nodes in the original dependency graph
    /// with a single dependent are condensed in the [TreeGraph] under whichever ancestor node
    /// is a root in the graph. Thus, entire expression trees are collapsed into a single node
    /// representing the root of that tree.
    ///
    /// One thing not shown above, but present in the [TreeGraph] structure, is what information
    /// is carried on the edge between treegraph roots, e.g. `inst4` and `result(d)`.
    /// In [TreeGraph] terms, the edge simply indicates that the tree represented by `inst4`
    /// depends on the tree represented by `result(d)`. However, internally the [TreeGraph]
    /// structure also encodes all of the individual dependencies between the two trees that
    /// drove the formation of that edge. As a result, we can answer questions such as the number
    /// of dependencies on a specific node by finding the treegraph root of the node, and for
    /// each predecessor root in the graph, unpacking all of the dependencies along that edge
    /// which reference the node in question.
    ///
    /// So the topological ordering of this graph is simply `[inst4, result(d), stack(b)]`.
    ///
    /// ## Algorithm
    ///
    /// The algorithm used to produce the topological ordering is simple:
    ///
    /// 1. Seed a queue with the set of treegraph roots with no predecessors, enqueuing them
    ///    in their natural sort order.
    /// 2. Pop the next root from the queue, remove all edges between that root and its
    ///    dependencies, and add the root to the output vector. If any of the dependencies have no
    ///    remaining predecessors after the aforementioned edge was removed, it is added to the
    ///    queue.
    /// 3. The process in step 2 is repeated until the queue is empty.
    /// 4. If there are any edges remaining in the graph, there was a cycle, and thus a
    ///    topological sort is impossible, this will result in an error being returned.
    /// 5. Otherwise, the sort is complete.
    ///
    /// In effect, a node is only emitted once all of its dependents are emitted, so for codegen,
    /// we can use this ordering for instruction scheduling, by visiting nodes in reverse
    /// topological order (i.e. visiting dependencies before any of their dependents). This also
    /// has the effect of placing items on the stack in the correct order needed for each
    /// instruction, as instruction operands will be pushed on the stack right-to-left, so that
    /// the first operand to an instruction is on top of the stack.
    pub fn toposort(&self) -> Result<Vec<NodeId>, UnexpectedCycleError> {
        let mut treegraph = self.clone();
        let mut output = Vec::<NodeId>::with_capacity(treegraph.nodes.len());
        let mut roots = treegraph
            .nodes
            .iter()
            .copied()
            .filter(|nid| treegraph.num_predecessors(*nid) == 0)
            .collect::<VecDeque<_>>();

        let mut successors = SmallVec::<[NodeId; 4]>::default();
        while let Some(nid) = roots.pop_front() {
            output.push(nid);
            successors.clear();
            successors.extend(treegraph.successor_ids(nid));
            for mid in successors.drain(..) {
                treegraph.remove_edge(nid, mid);
                if treegraph.num_predecessors(mid) == 0 {
                    roots.push_back(mid);
                }
            }
        }

        let has_cycle = treegraph.edges.values().any(|es| !es.is_empty());
        if has_cycle {
            Err(UnexpectedCycleError)
        } else {
            Ok(output)
        }
    }
}
impl From<DependencyGraph> for TreeGraph {
    fn from(mut depgraph: DependencyGraph) -> Self {
        let mut cutset = Vec::<(NodeId, NodeId)>::default();
        let mut treegraph = Self::default();

        // Build cutset
        for node_id in depgraph.node_ids() {
            let is_multi_use = depgraph.num_predecessors(node_id) > 1;
            if is_multi_use {
                cutset.extend(depgraph.predecessors(node_id).map(|d| (d.dependent, d.dependency)));
            }
        }

        // Apply cutset
        for (dependent_id, dependency_id) in cutset.iter() {
            depgraph.remove_edge(*dependent_id, *dependency_id);
        }

        // Add roots to treegraph
        for node_id in depgraph.node_ids() {
            if depgraph.num_predecessors(node_id) == 0 {
                treegraph.nodes.insert(node_id);
            }
        }

        // Construct mapping from dependency graph nodes to their
        // corresponding treegraph nodes
        let mut worklist = VecDeque::<NodeId>::default();
        for root in treegraph.nodes.iter().copied() {
            worklist.push_back(root);
            while let Some(node) = worklist.pop_front() {
                treegraph.condensed.insert(node, root);
                for dependency in depgraph.successor_ids(node) {
                    worklist.push_back(dependency);
                }
            }
        }

        // Add cutset edges to treegraph
        for (dependent_id, dependency_id) in cutset.into_iter() {
            let a = treegraph.condensed[&dependent_id];
            let b = treegraph.condensed[&dependency_id];
            let edges = treegraph
                .edges
                .entry(EdgeId {
                    predecessor: a,
                    successor: b,
                })
                .or_insert_with(Default::default);
            let edge = DependencyEdge {
                predecessor: dependent_id,
                successor: dependency_id,
            };
            if edges.contains(&edge) {
                continue;
            }
            edges.push(edge);
        }

        treegraph
    }
}

impl fmt::Debug for TreeGraph {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("TreeGraph")
            .field("nodes", &DebugNodes(self))
            .field("edges", &DebugEdges(self))
            .finish()
    }
}

struct DebugNodes<'a>(&'a TreeGraph);
impl<'a> fmt::Debug for DebugNodes<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.0.nodes.iter()).finish()
    }
}

struct DebugEdges<'a>(&'a TreeGraph);
impl<'a> fmt::Debug for DebugEdges<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut edges = f.debug_list();
        for EdgeId {
            predecessor,
            successor,
        } in self.0.edges.keys()
        {
            edges.entry(&format_args!("{predecessor} => {successor}"));
        }
        edges.finish()
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir as hir;

    use super::*;

    /// See [simple_dependency_graph] for details on the input dependency graph.
    ///
    /// We're expecting to have a treegraph that looks like the following:
    ///
    /// ```text,ignore
    /// inst2 (no predecessors)     v3 (no predecessors)
    ///   |
    ///   --> v1 (multiply-used)
    ///   |   |
    ///   |   v
    ///   --> v0 (multiply-used)
    /// ```
    ///
    /// We expect that in terms of scheduling, trees earlier in the topographical
    /// sort of the treegraph are visited earlier during codegen, and within a
    /// tree, nodes earlier in the topographical sort of that tree's dependency
    /// graph component are visited earlier than other nodes in that tree.
    ///
    /// For reference, here's the original IR:
    ///
    /// ```text,ignore
    /// block0(v0: i32):
    ///   v1 = inst0 v0
    ///   v3 = inst3
    ///   v2 = inst1 v1, v0
    ///   inst2 v2, block1(v1), block2(v1, v0)
    /// ```
    #[test]
    fn treegraph_construction() {
        let graph = simple_dependency_graph();
        let treegraph = OrderedTreeGraph::try_from(graph).unwrap();

        let v0 = hir::Value::from_u32(0);
        let v1 = hir::Value::from_u32(1);
        let v2 = hir::Value::from_u32(2);
        let inst0 = hir::Inst::from_u32(0);
        let inst1 = hir::Inst::from_u32(1);
        let inst2 = hir::Inst::from_u32(2);
        let inst3 = hir::Inst::from_u32(3);
        let v0_node = Node::Stack(v0);
        let v1_node = Node::Result {
            value: v1,
            index: 0,
        };
        let v2_node = Node::Result {
            value: v2,
            index: 0,
        };
        let inst0_node = Node::Inst { id: inst0, pos: 0 };
        let inst1_node = Node::Inst { id: inst1, pos: 2 };
        let inst2_node = Node::Inst { id: inst2, pos: 3 };
        let inst3_node = Node::Inst { id: inst3, pos: 1 };

        assert_eq!(treegraph.cmp_scheduling(inst2_node, inst2_node), Ordering::Equal);
        assert_eq!(treegraph.cmp_scheduling(inst2_node, inst3_node), Ordering::Less);
        assert_eq!(treegraph.cmp_scheduling(inst2_node, inst1_node), Ordering::Less);
        assert_eq!(treegraph.cmp_scheduling(inst1_node, inst2_node), Ordering::Greater);
        assert_eq!(treegraph.cmp_scheduling(inst1_node, inst0_node), Ordering::Less);
        assert_eq!(treegraph.cmp_scheduling(inst1_node, v1_node), Ordering::Less);
        assert_eq!(treegraph.cmp_scheduling(inst0_node, v0_node), Ordering::Less);

        // Instructions must be scheduled before all of their dependencies
        assert!(treegraph.is_scheduled_before(inst2_node, inst3_node));
        assert!(treegraph.is_scheduled_before(inst2_node, inst1_node));
        assert!(treegraph.is_scheduled_before(inst2_node, inst0_node));
        assert!(treegraph.is_scheduled_before(inst2_node, v0_node));
        assert!(treegraph.is_scheduled_before(inst2_node, v1_node));
        assert!(treegraph.is_scheduled_before(inst2_node, v2_node));
        assert!(treegraph.is_scheduled_before(inst1_node, v0_node));
        assert!(treegraph.is_scheduled_before(inst1_node, v1_node));
        assert!(treegraph.is_scheduled_before(inst0_node, v0_node));
        // Results are scheduled before instructions which produce them
        assert!(treegraph.is_scheduled_before(v2_node, inst1_node));
    }
}
