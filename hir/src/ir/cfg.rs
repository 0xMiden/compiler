mod diff;
mod scc;
mod visit;

pub use self::{
    diff::{CfgDiff, CfgUpdate, CfgUpdateKind, GraphDiff},
    scc::StronglyConnectedComponents,
    visit::{DefaultGraphVisitor, GraphVisitor, LazyDfsVisitor, PostOrderIter, PreOrderIter},
};

/// This is an abstraction over graph-like structures used in the IR:
///
/// * The CFG of a region, i.e. graph of blocks
/// * The CFG reachable from a single block, i.e. graph of blocks
/// * The dominator graph of a region, i.e. graph of dominator nodes
/// * The call graph of a program
/// * etc...
///
/// It isn't strictly necessary, but it provides some uniformity, and is useful particularly
/// for implementation of various analyses.
pub trait Graph {
    /// The type of node represented in the graph.
    ///
    /// Typically this should be a pointer-like reference type, cheap to copy/clone.
    type Node: Clone;
    /// Type used to iterate over children of a node in the graph.
    type ChildIter: ExactSizeIterator<Item = Self::Node>;
    /// The type used to represent an edge in the graph.
    ///
    /// This should be cheap to copy/clone.
    type Edge;
    /// Type used to iterate over child edges of a node in the graph.
    type ChildEdgeIter: ExactSizeIterator<Item = Self::Edge>;

    /// An empty graph has no nodes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.size() == 0
    }
    /// Get the number of nodes in this graph
    fn size(&self) -> usize;
    /// Get the entry node of the graph.
    ///
    /// It is expected that a graph always has an entry. As such, this function will panic if
    /// called on an "empty" graph. You should check whether the graph is empty _first_, if you
    /// are working with a possibly-empty graph.
    fn entry_node(&self) -> Self::Node;
    /// Get an iterator over the children of `parent`
    fn children(parent: Self::Node) -> Self::ChildIter;
    /// Get an iterator over the children edges of `parent`
    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter;
    /// Return the destination node of an edge.
    fn edge_dest(edge: Self::Edge) -> Self::Node;
}

impl<G: Graph> Graph for &G {
    type ChildEdgeIter = <G as Graph>::ChildEdgeIter;
    type ChildIter = <G as Graph>::ChildIter;
    type Edge = <G as Graph>::Edge;
    type Node = <G as Graph>::Node;

    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }

    fn size(&self) -> usize {
        (**self).size()
    }

    fn entry_node(&self) -> Self::Node {
        (**self).entry_node()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        <G as Graph>::children(parent)
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        <G as Graph>::children_edges(parent)
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        <G as Graph>::edge_dest(edge)
    }
}

/// An [InvertibleGraph] is a [Graph] which can be "inverted", i.e. edges are reversed.
///
/// Technically, any graph is invertible, however we are primarily interested in supporting graphs
/// for which an inversion of itself has some semantic value. For example, visiting a CFG in
/// reverse is useful in various contexts, such as constructing dominator trees.
///
/// This is primarily consumed via [Inverse].
pub trait InvertibleGraph: Graph {
    /// The type of this graph's inversion
    ///
    /// This is primarily useful in cases where you inverse the inverse of a graph - by allowing
    /// the types to differ, you can recover the original graph, rather than having to emulate
    /// both uninverted graphs using the inverse type.
    ///
    /// See [Inverse] for an example of how this is used.
    type Inverse: Graph;
    /// The type of iterator used to visit "inverted" children of a node in this graph, i.e.
    /// the predecessors.
    type InvertibleChildIter: ExactSizeIterator<Item = Self::Node>;
    /// The type of iterator used to obtain the set of "inverted" children edges of a node in this
    /// graph, i.e. the predecessor edges.
    type InvertibleChildEdgeIter: ExactSizeIterator<Item = Self::Edge>;

    /// Get an iterator over the predecessors of `parent`.
    ///
    /// NOTE: `parent` in this case will actually be a child of the nodes in the iterator, but we
    /// preserve the naming so as to make it apparent we are working with an inversion of the
    /// original graph.
    fn inverse_children(parent: Self::Node) -> Self::InvertibleChildIter;
    /// Get an iterator over the predecessor edges of `parent`.
    fn inverse_children_edges(parent: Self::Node) -> Self::InvertibleChildEdgeIter;
    /// Obtain the inversion of this graph
    fn inverse(self) -> Self::Inverse;
}

/// This is a wrapper type for [Graph] implementations, used to indicate that iterating a
/// graph should be iterated in "inverse" order, the semantics of which depend on the graph.
///
/// If used with an [InvertibleGraph], it uses the graph impls inverse iterators. If used with a
/// graph that is _not_ invertible, it uses the graph impls normal iterators. Effectively, this is
/// a specialization marker type.
pub struct Inverse<G: Graph> {
    graph: G,
}

impl<G: Graph> Inverse<G> {
    /// Construct an inversion over `graph`
    #[inline]
    pub fn new(graph: G) -> Self {
        Self { graph }
    }
}

impl<G: InvertibleGraph> Graph for Inverse<G> {
    type ChildEdgeIter = InverseChildEdgeIter<<G as InvertibleGraph>::InvertibleChildEdgeIter>;
    type ChildIter = InverseChildIter<<G as InvertibleGraph>::InvertibleChildIter>;
    type Edge = <G as Graph>::Edge;
    type Node = <G as Graph>::Node;

    fn is_empty(&self) -> bool {
        self.graph.is_empty()
    }

    fn size(&self) -> usize {
        self.graph.size()
    }

    fn entry_node(&self) -> Self::Node {
        self.graph.entry_node()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        InverseChildIter::new(<G as InvertibleGraph>::inverse_children(parent))
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        InverseChildEdgeIter::new(<G as InvertibleGraph>::inverse_children_edges(parent))
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        <G as Graph>::edge_dest(edge)
    }
}

impl<G: InvertibleGraph> InvertibleGraph for Inverse<G> {
    type Inverse = G;
    type InvertibleChildEdgeIter = <G as Graph>::ChildEdgeIter;
    type InvertibleChildIter = <G as Graph>::ChildIter;

    fn inverse_children(parent: Self::Node) -> Self::InvertibleChildIter {
        <G as Graph>::children(parent)
    }

    fn inverse_children_edges(parent: Self::Node) -> Self::InvertibleChildEdgeIter {
        <G as Graph>::children_edges(parent)
    }

    fn inverse(self) -> Self::Inverse {
        self.graph
    }
}

/// An iterator returned by `children` that iterates over `inverse_children` of the underlying graph
#[doc(hidden)]
pub struct InverseChildIter<I> {
    iter: I,
}

impl<I: ExactSizeIterator> InverseChildIter<I> {
    pub fn new(iter: I) -> Self {
        Self { iter }
    }
}
impl<I: ExactSizeIterator> ExactSizeIterator for InverseChildIter<I> {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}
impl<I: Iterator> Iterator for InverseChildIter<I> {
    type Item = <I as Iterator>::Item;

    default fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// An iterator returned by `children_edges` that iterates over `inverse_children_edges` of the
/// underlying graph.
#[doc(hidden)]
pub struct InverseChildEdgeIter<I> {
    iter: I,
}
impl<I: ExactSizeIterator> InverseChildEdgeIter<I> {
    pub fn new(iter: I) -> Self {
        Self { iter }
    }
}
impl<I: ExactSizeIterator> ExactSizeIterator for InverseChildEdgeIter<I> {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}
impl<I: Iterator> Iterator for InverseChildEdgeIter<I> {
    type Item = <I as Iterator>::Item;

    default fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
