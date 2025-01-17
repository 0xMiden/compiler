use core::ops::ControlFlow;

use smallvec::SmallVec;

use super::Graph;
use crate::adt::SmallSet;

/// By implementing this trait, you can refine the traversal performed by [DfsVisitor], as well as
/// hook in custom behavior to be executed upon reaching a node in both pre-order and post-order
/// visits.
///
/// There are two callbacks, both with default implementations that align with the default
/// semantics of a depth-first traversal.
///
/// If you wish to prune the search, the best place to do so is [GraphVisitor::on_node_reached],
/// as it provides the opportunity to control whether or not the visitor will visit any of the
/// node's successors as well as emit the node during iteration.
#[allow(unused_variables)]
pub trait GraphVisitor {
    type Node;

    /// Called when a node is first reached during a depth-first traversal, i.e. pre-order
    ///
    /// If this function returns `ControlFlow::Break`, none of `node`'s successors will be visited,
    /// and `node` will not be emitted by the visitor. This can be used to prune the traversal,
    /// e.g. confining a visit to a specific loop in a CFG.
    fn on_node_reached(&mut self, from: Option<&Self::Node>, node: &Self::Node) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Called when all successors of a node have been visited by the depth-first traversal, i.e.
    /// post-order.
    fn on_block_visited(&mut self, node: &Self::Node) {}
}

/// A useful no-op visitor for when you want the default behavior.
pub struct DefaultGraphVisitor<T>(core::marker::PhantomData<T>);
impl<T> Default for DefaultGraphVisitor<T> {
    fn default() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<T> GraphVisitor for DefaultGraphVisitor<T> {
    type Node = T;
}

/// A basic iterator over a depth-first traversal of nodes in a graph, producing them in pre-order.
#[repr(transparent)]
pub struct PreOrderIter<G>(LazyDfsVisitor<G, DefaultGraphVisitor<<G as Graph>::Node>>)
where
    G: Graph;
impl<G> PreOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
    /// Visit all nodes reachable from `root` in pre-order
    pub fn new(root: <G as Graph>::Node) -> Self {
        Self(LazyDfsVisitor::new(root, DefaultGraphVisitor::default()))
    }

    /// Visit all nodes reachable from `root` in pre-order, treating the nodes in `visited` as
    /// already visited, skipping them (and their successors) during the traversal.
    pub fn new_with_visited(
        root: <G as Graph>::Node,
        visited: impl IntoIterator<Item = <G as Graph>::Node>,
    ) -> Self {
        Self(LazyDfsVisitor::new_with_visited(root, DefaultGraphVisitor::default(), visited))
    }
}
impl<G> core::iter::FusedIterator for PreOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
}
impl<G> Iterator for PreOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
    type Item = <G as Graph>::Node;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next::<false>()
    }
}

/// A basic iterator over a depth-first traversal of nodes in a graph, producing them in post-order.
#[repr(transparent)]
pub struct PostOrderIter<G>(LazyDfsVisitor<G, DefaultGraphVisitor<<G as Graph>::Node>>)
where
    G: Graph;
impl<G> PostOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
    /// Visit all nodes reachable from `root` in post-order
    #[inline]
    pub fn new(root: <G as Graph>::Node) -> Self {
        Self(LazyDfsVisitor::new(root, DefaultGraphVisitor::default()))
    }

    /// Visit all nodes reachable from `root` in post-order, treating the nodes in `visited` as
    /// already visited, skipping them (and their successors) during the traversal.
    pub fn new_with_visited(
        root: <G as Graph>::Node,
        visited: impl IntoIterator<Item = <G as Graph>::Node>,
    ) -> Self {
        Self(LazyDfsVisitor::new_with_visited(root, DefaultGraphVisitor::default(), visited))
    }
}
impl<G> core::iter::FusedIterator for PostOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
}
impl<G> Iterator for PostOrderIter<G>
where
    G: Graph,
    <G as Graph>::Node: Eq,
{
    type Item = <G as Graph>::Node;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next::<true>()
    }
}

/// This type is an iterator over a depth-first traversal of a graph, with customization hooks
/// provided via the [GraphVisitor] trait.
///
/// The order in which nodes are produced by the iterator depends on how you invoke the `next`
/// method - it must be instantiated with a constant boolean that indicates whether or not the
/// iteration is to produce nodes in post-order.
///
/// As a result, this type does not implement `Iterator` itself - it is meant to be consumed as
/// an internal detail of higher-level iterator types. Two such types are provided in this module
/// for common pre- and post-order iterations:
///
/// * [PreOrderIter], for iterating in pre-order
/// * [PostOrderIter], for iterating in post-order
///
pub struct LazyDfsVisitor<G: Graph, V> {
    /// The nodes we have already visited, or wish to consider visited
    visited: SmallSet<<G as Graph>::Node, 32>,
    /// The stack of discovered nodes currently being visited
    stack: SmallVec<[VisitNode<<G as Graph>::Node>; 8]>,
    /// A [GraphVisitor] implementation used to hook into the traversal machinery
    visitor: V,
}

/// Represents a node in the graph which has been reached during traversal, and is in the process of
/// being visited.
struct VisitNode<T> {
    /// The parent node in the graph from which this node was drived
    parent: Option<T>,
    /// The node in the underlying graph being visited
    node: T,
    /// The successors of this node
    successors: SmallVec<[T; 2]>,
    /// Set to `true` once this node has been handled by [GraphVisitor::on_node_reached]
    reached: bool,
}
impl<T> VisitNode<T>
where
    T: Clone,
{
    #[inline]
    pub fn node(&self) -> T {
        self.node.clone()
    }

    /// Returns true if no successors remain to be visited under this node
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.successors.is_empty()
    }

    /// Get the next successor of this node, and advance the `next_child` index
    ///
    /// It is expected that the caller has already checked [is_empty]. Failing to do so
    /// will cause this function to panic once all successors have been visited.
    pub fn pop_successor(&mut self) -> T {
        self.successors.pop().unwrap()
    }
}

impl<G, V> LazyDfsVisitor<G, V>
where
    G: Graph,
    <G as Graph>::Node: Eq,
    V: GraphVisitor<Node = <G as Graph>::Node>,
{
    /// Visit the graph rooted under `from`, using the provided visitor for customization hooks.
    pub fn new(from: <G as Graph>::Node, visitor: V) -> Self {
        Self::new_with_visited(from, visitor, None::<<G as Graph>::Node>)
    }

    /// Visit the graph rooted under `from`, using the provided visitor for customization hooks.
    ///
    /// The initial set of "visited" nodes is seeded with `visited`. Any node in this set (and their
    /// children) will be skipped during iteration and by the traversal itself. If `from` is in this
    /// set, then the resulting iterator will be empty (i.e. produce no nodes, and perform no
    /// traversal).
    pub fn new_with_visited(
        from: <G as Graph>::Node,
        visitor: V,
        visited: impl IntoIterator<Item = <G as Graph>::Node>,
    ) -> Self {
        use smallvec::smallvec;

        let visited = visited.into_iter().collect::<SmallSet<_, 32>>();
        if visited.contains(&from) {
            // The root node itself is being ignored, return an empty iterator
            return Self {
                visited,
                stack: smallvec![],
                visitor,
            };
        }

        let successors = SmallVec::from_iter(G::children(from.clone()));
        Self {
            visited,
            stack: smallvec![VisitNode {
                parent: None,
                node: from,
                successors,
                reached: false,
            }],
            visitor,
        }
    }

    /// Step the visitor forward one step.
    ///
    /// The semantics of a step depend on the value of `POSTORDER`:
    ///
    /// * If `POSTORDER == true`, then we resume traversal of the graph until the next node that
    ///   has had all of its successors visited is on top of the visit stack.
    /// * If `POSTORDER == false`, then we resume traversal of the graph until the next unvisited
    ///   node is reached for the first time.
    ///
    /// In both cases, the node we find by the search is what is returned. If no more nodes remain
    /// to be visited, this returns `None`.
    ///
    /// This function invokes the associated [GraphVisitor] callbacks during the traversal, at the
    /// appropriate time.
    #[allow(clippy::should_implement_trait)]
    pub fn next<const POSTORDER: bool>(&mut self) -> Option<<G as Graph>::Node> {
        loop {
            let Some(node) = self.stack.last_mut() else {
                break None;
            };

            if !node.reached {
                node.reached = true;
                let unvisited = self.visited.insert(node.node());
                if !unvisited {
                    let _ = unsafe { self.stack.pop().unwrap_unchecked() };
                    continue;
                }

                // Handle pre-order visit
                let should_visit =
                    self.visitor.on_node_reached(node.parent.as_ref(), &node.node).is_continue();
                if !should_visit {
                    // It was indicated we shouldn't visit this node, so move to the next
                    let _ = unsafe { self.stack.pop().unwrap_unchecked() };
                    continue;
                }

                if POSTORDER {
                    // We need to visit this node's successors first
                    continue;
                } else {
                    // We're going to visit this node's successors on the next call
                    break Some(node.node.clone());
                }
            }

            // Otherwise, we're visiting a successor of this node.
            //
            // If this node has no successors, we're done visiting it
            // If we've visited all successors of this node, we've got our next item
            if node.is_empty() {
                let node = unsafe { self.stack.pop().unwrap_unchecked() };
                self.visitor.on_block_visited(&node.node);
                if POSTORDER {
                    break Some(node.node);
                } else {
                    continue;
                }
            }

            // Otherwise, continue visiting successors
            let parent = node.node();
            let successor = node.pop_successor();
            let successors = SmallVec::from_iter(G::children(successor.clone()));
            self.stack.push(VisitNode {
                parent: Some(parent),
                node: successor,
                successors,
                reached: false,
            });
        }
    }
}
