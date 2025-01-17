use alloc::collections::{BTreeMap, BTreeSet};
use core::{cmp::Ordering, fmt};

use smallvec::SmallVec;

use crate::{OperationRef, ValueRef};

/// This represents a node in a [DependencyGraph].
///
/// The node types here are carefully chosen to provide us with the following
/// properties once we've constructed a [DependencyGraph] from a block:
///
/// * Distinguish between block-local operands and those which come from a dominating block. This
///   let's us reason globally about how function arguments and instruction results are used in
///   blocks of the program so that they can be moved/copied as appropriate to keep them live only
///   for as long as they are needed.
/// * Represent the dependencies of individual arguments, this ensures that dependencies between
///   expressions in a block are correctly represented when we compute a [TreeGraph], and that we
///   can determine exactly how many instances of a value are needed in a function.
/// * Represent usage of individual instruction results - both to ensure we make copies of those
///   results as needed, but to ensure we drop unused results immediately if they are not needed.
///
/// Furthermore, the precise layout and ordering of this enum is intentional,
/// as it determines the order in which nodes are sorted, and thus the order
/// in which we visit them during certain operations.
///
/// It is also essential that this is kept in sync with [NodeId], which is
/// a packed representation of [Node] designed to ensure that the order in
/// which [NodeId] is ordered is the same as the corresponding [Node]. Put
/// another way: [Node] is the unpacked form of [NodeId].
///
/// NOTE: Adding variants/fields to this type must be done carefully, to ensure
/// that we can encode a [Node] as a [NodeId], and to preserve the fact that
/// a [NodeId] fits in a `u64`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Node {
    /// This node type represents a value known to be on the
    /// operand stack upon entry to the current block, i.e.
    /// it's definition is external to this block, but available.
    Stack(ValueRef),
    /// This node represents an instruction argument. Only `Inst` may
    /// depend on nodes of this type directly, and it may only depend
    /// on `Result` or `Stack` nodes itself.
    ///
    /// There are different kinds of arguments though, see [ArgumentNode] for details
    Argument(ArgumentNode),
    /// This node acts as a join point for the remaining node types,
    /// i.e. it is the predecessor for `Argument`, and the successor
    /// for `Result` and is used to represent the fact that results
    /// implicitly depend on all arguments to the instruction which
    /// produced them.
    Inst {
        /// The unique id of this instruction
        id: OperationRef,
        /// The position of this instruction in its containing block
        pos: u16,
    },
    /// This node represents an instruction result. `Result` may only have
    /// `Argument` as predecessor (i.e. the argument depends on a result),
    /// and may only have `Inst` as successor (i.e. the instruction which
    /// produced the result is the only way a result can appear in the graph).
    Result {
        /// The id of the value represented by this result
        value: OpResultRef,
        /// The index of this result in the instruction results list
        index: u8,
    },
}
impl core::hash::Hash for Node {
    fn hash<H: core::hash::Hasher>(&self, hasher: &mut H) {
        // Ensure that by hashing either NodeId or Node we get the same hash
        self.id().hash(hasher);
    }
}
impl Node {
    /// Get the identifier corresponding to this node.
    ///
    /// A given [Node] will always have the same identifier, as [NodeId] is
    /// derived from the content of a [Node] (it is in fact a packed representation
    /// of the same data).
    pub fn id(self) -> NodeId {
        NodeId::from(self)
    }

    /// Returns true if this node represents an item in the current block
    ///
    /// The only node type for which this returns false is `Stack`, as such
    /// values are by definition not defined in the current block.
    #[inline]
    pub fn is_block_local(&self) -> bool {
        !matches!(self, Self::Stack(_))
    }

    /// Fallibly converts this node to an instruction identifier
    #[inline]
    pub fn as_instruction(&self) -> Option<OperationRef> {
        match self {
            Self::Inst { id, .. } => Some(*id),
            Self::Argument(ref arg) => Some(arg.inst()),
            _ => None,
        }
    }

    /// Unwraps this node as an instruction identifier, or panics
    pub fn unwrap_inst(&self) -> OperationRef {
        match self {
            Self::Inst { id, .. } => *id,
            Self::Argument(ref arg) => arg.inst(),
            node => panic!("cannot unwrap node as instruction: {node:?}"),
        }
    }

    /// Fallibly converts this node to a value identifier
    #[inline]
    pub fn as_value(&self) -> Option<ValueRef> {
        match self {
            Self::Stack(value) => Some(*value),
            Self::Result { value, .. } => Some(value.borrow().as_value_ref()),
            _ => None,
        }
    }
}
impl From<NodeId> for Node {
    fn from(id: NodeId) -> Self {
        id.into_node()
            .unwrap_or_else(|_| panic!("invalid tag for node id: {:064b}", id.0))
    }
}
impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Stack(value) => write!(f, "{value}"),
            Self::Inst { id, .. } => write!(f, "{id}"),
            Self::Argument(ref arg) => write!(f, "{arg:?}"),
            Self::Result { value, .. } => write!(f, "result({value})"),
        }
    }
}

/// This is a subtype of [Node] which represents the various types of arguments
/// we want to represent in a [DependencyGraph].
///
/// As with [Node], the layout and representation of this type is carefully
/// chosen, and must be kept in sync with [Node] and [NodeId].
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ArgumentNode {
    /// The argument is required by an instruction directly.
    ///
    /// For control-flow instructions, this argument type is used for
    /// non-block arguments, e.g. in `cond_br v0, block1(v1)`, `v0`
    /// would be of this type.
    Direct(OpOperand),
    /// The argument is required by an instruction indirectly.
    ///
    /// This is only applicable to control-flow instructions, and indicates
    /// that the argument is required along all control flow edges for which
    /// the instruction is a predecessor. Each use of a value will get its
    /// own node in the dependency graph to represent the specific position
    /// of the argument in its respective block argument list.
    ///
    /// In the IR of `cond_br v0, block1(v1), block2(v0, v1)`, `v1` would be
    /// of this type, and the dependency graph would have unique nodes for
    /// both uses.
    Indirect(OpOperand),
    /// The argument is conditionally required by an instruction indirectly.
    ///
    /// This is a variation on `Indirect` which represents instructions such
    /// as `cond_br` and `switch` where an argument is passed to a subset of
    /// the successors for the instruction. In such cases, the argument may
    /// not be used at all along the other edges, and if so, can be conditionally
    /// materialized along the subset of edges which actually require it.
    Conditional(OpOperand),
}
impl ArgumentNode {
    /// Return the instruction to which this argument belongs
    #[inline]
    pub fn inst(&self) -> OperationRef {
        match self {
            Self::Direct(operand) | Self::Indirect(operand) | Self::Conditional(operand) => {
                operand.borrow().owner
            }
        }
    }

    /// Return the index of this argument in its corresponding argument list
    ///
    /// NOTE: Different argument types correspond to different argument lists, you
    /// must make sure you are using the index returned here with the correct list.
    #[inline]
    pub fn index(&self) -> u8 {
        match self {
            Self::Direct(operand) | Self::Indirect(operand) | Self::Conditional(operand) => {
                operand.borrow().index
            }
        }
    }

    /// For indirect/conditional arguments, returns the index of the successor in the
    /// successor list of the instruction.
    #[inline]
    pub fn successor(&self) -> Option<u8> {
        match self {
            Self::Direct(_) => None,
            Self::Indirect(operand) | Self::Conditional(operand) => {
                Some(operand.borrow().operand_group())
            }
        }
    }

    #[inline]
    fn kind(&self) -> usize {
        match self {
            Self::Direct(_) => 0,
            Self::Indirect(_) => 1,
            Self::Conditional(_) => 2,
        }
    }
}
impl fmt::Debug for ArgumentNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let inst = self.inst();
        let index = self.index();
        let successor = self.successor();
        match self {
            Self::Direct(operand) => write!(f, "arg({index} of {inst})"),
            Self::Indirect(operand) => {
                write!(f, "block_arg({index} of {inst} for {})", successor.unwrap())
            }
            Self::Conditional(operand) => {
                write!(f, "conditional_block_arg({index} of {inst} for {})", successor.unwrap())
            }
        }
    }
}
impl Ord for ArgumentNode {
    /// NOTE: This must match the ordering behavior of [NodeId]
    fn cmp(&self, other: &Self) -> Ordering {
        // Order by instruction, then by successor (if applicable), then by index
        //
        // After ordering in this way, Direct is always ordered before Indirect/Conditional,
        // to account for the fact that an instruction's direct parameters always are needed
        // before the successor arguments. However, Indirect/Conditional may never compare equal
        // to each other after ordering based on the fields described above, because to do so
        // would represent the same argument position being represented using two different,
        // conflicting types.
        let x_inst = self.inst();
        let y_inst = other.inst();
        self.kind()
            .cmp(&other.kind())
            .then_with(|| self.inst().cmp(&other.inst()))
            .then_with(|| self.index().cmp(&other.index()))
    }
}
impl PartialOrd for ArgumentNode {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid node identifier")]
pub struct InvalidNodeIdError;

/// Produce a bit-packed representation of [Node] which is naturally
/// sortable as if it was the expanded [Node] type.
///
/// We currently only need 5 unique values for the node type, so we
/// use 3 bits, which gives us 7 unique values, thus we have 2 extra
/// tag values if we ever need them. This leaves us with 61 bits, of
/// which 32 is reserved for the instruction or value identifier, and
/// the remaining 29 are available for storing any type-specific data.
///
/// We choose a layout that ensures that when compared as an integer,
/// the sort order of the corresponding [Node] would be identical. This
/// is a bit tricky, since [ArgumentNode] for example ignores the difference
/// between Indirect/Conditional argument types when sorted, so in that
/// case we use the same tag for those types, and differentiate them by
/// using one of the payload bits as a "conditional" marker. With this
/// modification, we can place the tag bits first, followed by a
/// type-specific layout which obeys the ordering rules for that type.
///
/// The following is the key for any comments describing bit layout:
///
/// * `t`: tag
/// * `i`: inst
/// * `v`: value
/// * `s`: successor
/// * `x`: index
/// * `c`: conditional marker
/// * `0`: unused/zero
///
/// This is the layout used for argument nodes
///
/// ```text,ignore
/// tttiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiissssssssxxxxxxxx000000000000c
/// |--tag (3)--|--inst (32)--|--successor (8)--|--index (8)--|--unused/zero (12)--|--conditional (1)--|
/// ```
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NodeId(u64);
impl NodeId {
    const IS_CONDITIONAL_ARG: u64 = 1;
    const TAG_ARG_DIRECT: u64 = 1 << 60;
    const TAG_ARG_INDIRECT: u64 = 2 << 60;
    const TAG_INST: u64 = 3 << 60;
    const TAG_MASK: u64 = 0b111 << 60;
    const TAG_RESULT: u64 = 4 << 60;

    /// Returns true if the [Node] corresponding to this identifier is of `Stack` type
    #[inline]
    pub fn is_stack(&self) -> bool {
        self.0 & Self::TAG_MASK == 0
    }

    /// Returns true if the [Node] corresponding to this identifier is of `Result` type
    #[inline]
    pub fn is_result(&self) -> bool {
        self.0 & Self::TAG_MASK == Self::TAG_RESULT
    }

    /// Returns true if the [Node] corresponding to this identifier is of `Inst` type
    #[inline]
    pub fn is_instruction(&self) -> bool {
        self.0 & Self::TAG_MASK == Self::TAG_INST
    }

    /// Returns true if the [Node] corresponding to this identifier is of `Argument` type
    #[inline]
    pub fn is_argument(&self) -> bool {
        matches!(self.0 & Self::TAG_MASK, Self::TAG_ARG_DIRECT | Self::TAG_ARG_INDIRECT)
    }

    /// Decode this identifier into its corresponding [Node]
    #[inline(always)]
    pub fn expand(self) -> Node {
        self.into()
    }

    /// Extract the [midenc_hir::Inst] associated with the corresponding [Node], or panic
    /// if the node type does not have an associated instruction identifier.
    pub fn unwrap_inst(self) -> hir::Inst {
        let tag = self.0 & Self::TAG_MASK;
        match tag {
            Self::TAG_ARG_DIRECT | Self::TAG_ARG_INDIRECT => {
                hir::Inst::from_u32(((self.0 >> 28) & (u32::MAX as u64)) as u32)
            }
            Self::TAG_INST => hir::Inst::from_u32(((self.0 >> 16) & (u32::MAX as u64)) as u32),
            0 | Self::TAG_RESULT => panic!("cannot unwrap node id as instruction: {self:?}"),
            _invalid => panic!("invalid node id: {:064b}", self.0),
        }
    }

    /// Safely convert this identifier into a [Node].
    ///
    /// This can be used in cases where the source of the [NodeId] is untrusted.
    pub fn into_node(self) -> Result<Node, InvalidNodeIdError> {
        let tag = self.0 & Self::TAG_MASK;
        match tag {
            0 => {
                let value = (self.0 & (u32::MAX as u64)) as u32;
                Ok(Node::Stack(hir::Value::from_u32(value)))
            }
            Self::TAG_INST => {
                let pos = (self.0 & (u16::MAX as u64)) as u16;
                let id = hir::Inst::from_u32(((self.0 >> 16) & (u32::MAX as u64)) as u32);
                Ok(Node::Inst { id, pos })
            }
            Self::TAG_ARG_DIRECT => {
                let mut shifted = self.0 >> 12;
                let index = (shifted & (u8::MAX as u64)) as u8;
                shifted >>= 16;
                let inst = (shifted & (u32::MAX as u64)) as u32;
                Ok(Node::Argument(ArgumentNode::Direct {
                    inst: hir::Inst::from_u32(inst),
                    index,
                }))
            }
            Self::TAG_ARG_INDIRECT => {
                let is_conditional = self.0 & Self::IS_CONDITIONAL_ARG == Self::IS_CONDITIONAL_ARG;
                let mut shifted = self.0 >> 12;
                let index = (shifted & (u8::MAX as u64)) as u8;
                shifted >>= 8;
                let successor = (shifted & (u8::MAX as u64)) as u8;
                shifted >>= 8;
                let inst = hir::Inst::from_u32((shifted & (u32::MAX as u64)) as u32);
                Ok(Node::Argument(if is_conditional {
                    ArgumentNode::Conditional {
                        inst,
                        successor,
                        index,
                    }
                } else {
                    ArgumentNode::Indirect {
                        inst,
                        successor,
                        index,
                    }
                }))
            }
            Self::TAG_RESULT => {
                let value = hir::Value::from_u32((self.0 & (u32::MAX as u64)) as u32);
                let index = ((self.0 >> 52) & (u8::MAX as u64)) as u8;
                Ok(Node::Result { value, index })
            }
            _ => Err(InvalidNodeIdError),
        }
    }
}
impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&Node::from(*self), f)
    }
}
impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&Node::from(*self), f)
    }
}
impl From<Node> for NodeId {
    fn from(node: Node) -> Self {
        use cranelift_entity::EntityRef;
        match node {
            // ttt00000000000000000000000000000vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv
            Node::Stack(value) => Self(value.index() as u64),
            // ttt0000000000000iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiixxxxxxxxxxxxxxxx
            Node::Inst { id, pos } => {
                let inst = (id.index() as u64) << 16;
                let index = pos as u64;
                Self(Self::TAG_INST | inst | index)
            }
            // tttiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiissssssssxxxxxxxx000000000000c
            Node::Argument(arg) => match arg {
                ArgumentNode::Direct { inst, index } => {
                    let inst = (inst.index() as u64) << 28;
                    let index = (index as u64) << 12;
                    Self(Self::TAG_ARG_DIRECT | inst | index)
                }
                ArgumentNode::Indirect {
                    inst,
                    successor,
                    index,
                } => {
                    let inst = (inst.index() as u64) << 28;
                    let successor = (successor as u64) << 20;
                    let index = (index as u64) << 12;
                    Self(Self::TAG_ARG_INDIRECT | inst | successor | index)
                }
                ArgumentNode::Conditional {
                    inst,
                    successor,
                    index,
                } => {
                    let inst = (inst.index() as u64) << 28;
                    let successor = (successor as u64) << 20;
                    let index = (index as u64) << 12;
                    Self(
                        Self::TAG_ARG_INDIRECT
                            | inst
                            | successor
                            | index
                            | Self::IS_CONDITIONAL_ARG,
                    )
                }
            },
            // tttdddddddd000000000000000000000vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv
            Node::Result { value, index } => {
                let value = value.index() as u64;
                let index = (index as u64) << 52;
                Self(Self::TAG_RESULT | index | value)
            }
        }
    }
}
impl<'a> From<&'a Node> for NodeId {
    #[inline]
    fn from(node: &'a Node) -> Self {
        (*node).into()
    }
}

/// This structure represents the relationship between dependent and
/// dependency in a [DependencyGraph].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The node which has the dependency.
    pub dependent: NodeId,
    /// The node which is being depended upon.
    pub dependency: NodeId,
}
impl Dependency {
    /// Construct a new [Dependency].
    ///
    /// In debug builds this will raise an assertion if the dependency being described
    /// has nonsensical semantics. In release builds this assertion is elided.
    #[inline]
    pub fn new(dependent: NodeId, dependency: NodeId) -> Self {
        is_valid_dependency(dependent, dependency);
        Self {
            dependent,
            dependency,
        }
    }
}
impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} => {}", self.dependent, self.dependency)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Edge {
    node: NodeId,
    direction: Direction,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Direction {
    Dependent,
    Dependency,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidDependencyGraphQuery;

/// This error type is returned by [DependencyGraph::toposort]
#[derive(Debug, thiserror::Error)]
#[error("an unexpected cycle was detected when attempting to topologically sort a treegraph")]
pub struct UnexpectedCycleError;

/// [DependencyGraph] is a directed, acyclic graph used to represent control
/// and data dependencies in a single basic block of a function in Miden IR.
///
/// Once constructed, we can use the graph to query information such as:
///
/// * What is the source for each argument of an instruction
/// * Is a given instruction result used? How many times and by who?
/// * Can a given argument consume its source value, or must it be copied
/// * What node represents the last use of a value
/// * Is an instruction dead code?
///
/// Most importantly however, a [DependencyGraph] is required in order to
/// compute a [TreeGraph] for the block in question, which is essential for
/// instruction scheduling and code generation.
#[derive(Default, Clone)]
pub struct DependencyGraph {
    /// The set of nodes represented in the graph
    nodes: BTreeSet<NodeId>,
    /// A map of every node in the graph to other nodes in the graph with which it has
    /// a relationship, and which dependencies describe that relationship.
    edges: BTreeMap<NodeId, SmallVec<[Edge; 1]>>,
}
impl DependencyGraph {
    /// Create a new, empty [DependencyGraph]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `node` to the dependency graph, if it is not already present
    pub fn add_node(&mut self, node: Node) -> NodeId {
        let id = node.id();
        if self.nodes.insert(id) {
            self.edges.insert(id, Default::default());
        }
        id
    }

    /// Returns true if this graph contains `node`
    #[inline]
    pub fn contains(&self, node: &Node) -> bool {
        let id = node.id();
        self.contains_id(&id)
    }

    /// Returns true if this graph contains `node`
    #[inline]
    pub fn contains_id(&self, node: &NodeId) -> bool {
        self.nodes.contains(node)
    }

    /// Returns true if there is a path to `b` from `a` in the graph.
    pub fn is_reachable_from(&self, a: NodeId, b: NodeId) -> bool {
        if !self.nodes.contains(&a) || !self.nodes.contains(&b) {
            return false;
        }

        let mut visited = BTreeSet::default();
        let mut worklist = std::collections::VecDeque::from([a]);
        while let Some(node_id) = worklist.pop_front() {
            if !visited.insert(node_id) {
                continue;
            }

            if node_id == b {
                return true;
            }

            worklist.extend(self.successor_ids(node_id));
        }

        false
    }

    /// Add a dependency from `a` to `b`
    pub fn add_dependency(&mut self, a: NodeId, b: NodeId) {
        assert_ne!(a, b, "cannot add a self-referential dependency");

        let edge = Edge {
            node: b,
            direction: Direction::Dependent,
        };
        let edges = self.edges.get_mut(&a).unwrap();
        if edges.contains(&edge) {
            return;
        }
        edges.push(edge);
        let edge = Edge {
            node: a,
            direction: Direction::Dependency,
        };
        let edges = self.edges.get_mut(&b).unwrap();
        debug_assert!(!edges.contains(&edge));
        edges.push(edge);
    }

    /// Get a [Dependency] corresponding to the edge from `from` to `to`
    ///
    /// This will panic if there is no edge between the two nodes given.
    pub fn edge(&self, from: NodeId, to: NodeId) -> Dependency {
        let edges = self.edges.get(&from).unwrap();
        let edge = Edge {
            node: to,
            direction: Direction::Dependent,
        };
        assert!(self.nodes.contains(&from));
        assert!(self.nodes.contains(&to));
        if edges.contains(&edge) {
            Dependency::new(from, to)
        } else {
            panic!(
                "invalid edge: there is no dependency from {} to {}",
                from.expand(),
                to.expand(),
            );
        }
    }

    /// Removes `node` from the graph, along with all edges in which it appears
    pub fn remove_node<N: Into<NodeId>>(&mut self, node: N) {
        let id = node.into();
        if self.nodes.remove(&id) {
            let edges = self.edges.remove(&id).unwrap();
            for Edge {
                node: other_node_id,
                ..
            } in edges.into_iter()
            {
                self.edges.get_mut(&other_node_id).unwrap().retain(|e| e.node != id);
            }
        }
    }

    /// Removes an edge from `a` to `b`.
    ///
    /// If `value` is provided, the use corresponding to that value is removed, rather than
    /// the entire edge from `a` to `b`. However, if removing `value` makes the edge dead, or
    /// `value` is not provided, then the entire edge is removed.
    pub fn remove_edge(&mut self, a: NodeId, b: NodeId) {
        // Get the edge id that connects a <-> b
        if let Some(edges) = self.edges.get_mut(&a) {
            edges.retain(|e| e.node != b || e.direction == Direction::Dependency);
        }
        if let Some(edges) = self.edges.get_mut(&b) {
            edges.retain(|e| e.node != a || e.direction == Direction::Dependent);
        }
    }

    /// Returns the number of predecessors, i.e. dependents, for `node` in the graph
    pub fn num_predecessors<N: Into<NodeId>>(&self, node: N) -> usize {
        let id = node.into();
        self.edges
            .get(&id)
            .map(|es| es.iter().filter(|e| e.direction == Direction::Dependency).count())
            .unwrap_or_default()
    }

    /// Returns an iterator over the nodes in this graph
    pub fn nodes(&self) -> impl Iterator<Item = Node> + '_ {
        self.nodes.iter().copied().map(Node::from)
    }

    /// Returns an iterator over the nodes in this graph
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().copied()
    }

    /// Return the sole predecessor of `node`, if `node` has any predecessors.
    ///
    /// Returns `Err` if `node` has multiple predecessors
    pub fn parent(
        &self,
        node: impl Into<NodeId>,
    ) -> Result<Option<NodeId>, InvalidDependencyGraphQuery> {
        let mut predecessors = self.predecessors(node);
        match predecessors.next() {
            None => Ok(None),
            Some(parent) => {
                if predecessors.next().is_some() {
                    Err(InvalidDependencyGraphQuery)
                } else {
                    Ok(Some(parent.dependent))
                }
            }
        }
    }

    /// Like `parent`, but panics if `node` does not have a single parent
    pub fn unwrap_parent(&self, node: impl Into<NodeId>) -> NodeId {
        let node = node.into();
        self.parent(node)
            .unwrap_or_else(|_| {
                panic!("expected {node} to have a single parent, but found multiple")
            })
            .unwrap_or_else(|| panic!("expected {node} to have a parent, but it has none"))
    }

    /// Return the sole successor of `node`, if `node` has any successors.
    ///
    /// Returns `Err` if `node` has multiple successors
    pub fn child(
        &self,
        node: impl Into<NodeId>,
    ) -> Result<Option<NodeId>, InvalidDependencyGraphQuery> {
        let mut successors = self.successors(node);
        match successors.next() {
            None => Ok(None),
            Some(child) => {
                if successors.next().is_some() {
                    Err(InvalidDependencyGraphQuery)
                } else {
                    Ok(Some(child.dependency))
                }
            }
        }
    }

    /// Like `child`, but panics if `node` does not have a single child
    pub fn unwrap_child(&self, node: impl Into<NodeId>) -> NodeId {
        let node = node.into();
        self.child(node)
            .unwrap_or_else(|_| {
                panic!("expected {node} to have a single child, but found multiple")
            })
            .unwrap_or_else(|| panic!("expected {node} to have a child, but it has none"))
    }

    /// Returns an iterator over the predecessors, or dependents, of `node` in the graph
    pub fn predecessors<'a, 'b: 'a>(&'b self, node: impl Into<NodeId>) -> Predecessors<'a> {
        let id = node.into();
        Predecessors {
            node: id,
            iter: self.edges[&id].iter(),
        }
    }

    /// Like `predecessors`, but avoids decoding [Node] values, instead producing the raw [NodeId]
    /// values.
    pub fn predecessor_ids(&self, node: impl Into<NodeId>) -> impl Iterator<Item = NodeId> + '_ {
        let id = node.into();
        self.edges[&id].iter().filter_map(|edge| {
            if matches!(edge.direction, Direction::Dependency) {
                Some(edge.node)
            } else {
                None
            }
        })
    }

    /// Returns an iterator over the successors, or dependencies, of `node` in the graph
    pub fn successors<'a, 'b: 'a>(&'b self, node: impl Into<NodeId>) -> Successors<'a> {
        let id = node.into();
        Successors {
            node: id,
            iter: self.edges[&id].iter(),
        }
    }

    /// Like `successors`, but avoids decoding [Node] values, instead producing the raw [NodeId]
    /// values.
    pub fn successor_ids(&self, node: impl Into<NodeId>) -> impl Iterator<Item = NodeId> + '_ {
        let id = node.into();
        self.edges[&id].iter().filter_map(|edge| {
            if matches!(edge.direction, Direction::Dependent) {
                Some(edge.node)
            } else {
                None
            }
        })
    }

    /// Returns a data structure which assigns an index to each node in the graph for which `root`
    /// is an ancestor, including `root` itself. The assigned index indicates the order in which
    /// nodes will be emitted during code generation - the lower the index, the earlier the node
    /// is emitted. Conversely, a higher index indicates that a node will be scheduled later in
    /// the program, so values will be materialized from lowest index to highest.
    pub fn indexed(
        &self,
        root: impl Into<NodeId>,
    ) -> Result<DependencyGraphIndices, UnexpectedCycleError> {
        let root = root.into();

        let mut output = BTreeMap::<NodeId, usize>::new();
        let mut stack = vec![root];
        let mut discovered = BTreeSet::<NodeId>::default();
        let mut finished = BTreeSet::<NodeId>::default();

        while let Some(node) = stack.last().copied() {
            if discovered.insert(node) {
                if node.is_instruction() {
                    for arg in self.successors(node).filter(|succ| succ.dependency.is_argument()) {
                        let arg_source_id = self.unwrap_child(arg.dependency);
                        if !discovered.contains(&arg_source_id) {
                            stack.push(arg_source_id);
                        }
                    }
                    for other in self.successors(node).filter(|succ| !succ.dependency.is_argument())
                    {
                        let succ_node_id = if other.dependency.is_instruction() {
                            other.dependency
                        } else {
                            assert!(other.dependency.is_result());
                            self.unwrap_child(other.dependency)
                        };
                        if !discovered.contains(&succ_node_id) {
                            stack.push(succ_node_id);
                        }
                    }
                } else if node.is_result() {
                    let inst_node = self.unwrap_child(node);
                    if !discovered.contains(&inst_node) {
                        stack.push(inst_node);
                    }
                }
            } else {
                stack.pop();
                if finished.insert(node) {
                    let index = output.len();
                    output.insert(node, index);
                }
            }
        }

        Ok(DependencyGraphIndices { sorted: output })
    }

    /// Get the topographically-sorted nodes of this graph for which `root` is an ancestor.
    pub fn toposort(&self, root: impl Into<NodeId>) -> Result<Vec<NodeId>, UnexpectedCycleError> {
        use std::collections::VecDeque;

        let root = root.into();
        let mut depgraph = self.clone();
        let mut output = Vec::<NodeId>::with_capacity(depgraph.nodes.len());

        // Remove all predecessor edges to the root
        if let Some(edges) = depgraph.edges.get_mut(&root) {
            edges.retain(|e| e.direction == Direction::Dependent);
        }

        let mut roots = VecDeque::from_iter([root]);
        let mut successors = SmallVec::<[NodeId; 4]>::default();
        while let Some(nid) = roots.pop_front() {
            output.push(nid);
            successors.clear();
            successors.extend(depgraph.successor_ids(nid));
            for mid in successors.drain(..) {
                depgraph.remove_edge(nid, mid);
                if depgraph.num_predecessors(mid) == 0 {
                    roots.push_back(mid);
                }
            }
        }

        let has_cycle = depgraph.edges.iter().any(|(n, es)| output.contains(n) && !es.is_empty());
        if has_cycle {
            Err(UnexpectedCycleError)
        } else {
            Ok(output)
        }
    }

    /// This function is used to represent the dependency of an instruction on values
    /// it uses as arguments. We do so by adding the appropriate argument node to the
    /// graph, and adding edges between the instruction and the argument node, and the
    /// argument node and the stack value or instruction result which it references.
    pub fn add_data_dependency(
        &mut self,
        dependent_id: NodeId,
        argument: ArgumentNode,
        value: hir::Value,
        pp: hir::ProgramPoint,
        function: &hir::Function,
    ) {
        debug_assert!(dependent_id.is_instruction());

        let dependency_id = self.add_node(Node::Argument(argument));
        match function.dfg.value_data(value) {
            hir::ValueData::Inst {
                inst: dep_inst,
                num,
                ..
            } => {
                let dep_inst = *dep_inst;
                let block_id = function.dfg.pp_block(pp);
                if function.dfg.insts[dep_inst].block == block_id {
                    let dep_inst_index =
                        function.dfg.block_insts(block_id).position(|id| id == dep_inst).unwrap();
                    let result_inst_node_id = self.add_node(Node::Inst {
                        id: dep_inst,
                        pos: dep_inst_index as u16,
                    });
                    let result_node_id = self.add_node(Node::Result {
                        value,
                        index: *num as u8,
                    });
                    self.add_dependency(result_node_id, result_inst_node_id);
                    self.add_dependency(dependency_id, result_node_id);
                } else {
                    let operand_node_id = self.add_node(Node::Stack(value));
                    self.add_dependency(dependency_id, operand_node_id);
                };
            }
            hir::ValueData::Param { .. } => {
                let operand_node_id = self.add_node(Node::Stack(value));
                self.add_dependency(dependency_id, operand_node_id);
            }
        }
        self.add_dependency(dependent_id, dependency_id);
    }
}
impl fmt::Debug for DependencyGraph {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DependencyGraph")
            .field("nodes", &DebugNodes(self))
            .field("edges", &DebugEdges(self))
            .finish()
    }
}

/// This structure is produced by [DependencyGraph::indexed], which assigns
/// an ordinal index to every [Node] in the graph based on the order in which it
/// is visited during code generation. The lower the index, the earlier it is
/// visited.
///
/// This is used to compare nodes in the graph with a common dependency to see which
/// one is the last dependent, which allows us to be more precise when we manipulate
/// the operand stack.
#[derive(Default)]
pub struct DependencyGraphIndices {
    /// The topographically sorted nodes for the component of the
    /// dependency graph for which we have constructed this set.
    sorted: BTreeMap<NodeId, usize>,
}
impl DependencyGraphIndices {
    /// Get the index of `node`
    ///
    /// NOTE: This function will panic if `node` was not in the corresponding dependency graph, or
    /// is unresolved
    #[inline]
    pub fn get(&self, node: impl Into<NodeId>) -> Option<usize> {
        let id = node.into();
        self.sorted.get(&id).copied()
    }
}
impl fmt::Debug for DependencyGraphIndices {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map().entries(self.sorted.iter()).finish()
    }
}

/// An iterator over each successor edge, or [Dependency], of a given node in a [DependencyGraph]
pub struct Successors<'a> {
    node: NodeId,
    iter: core::slice::Iter<'a, Edge>,
}
impl<'a> Iterator for Successors<'a> {
    type Item = Dependency;

    fn next(&mut self) -> Option<Self::Item> {
        for Edge { node, direction } in &mut self.iter {
            if matches!(direction, Direction::Dependent) {
                return Some(Dependency::new(self.node, *node));
            }
        }

        None
    }
}
impl<'a> DoubleEndedIterator for Successors<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(Edge { node, direction }) = self.iter.next_back() {
            if matches!(direction, Direction::Dependent) {
                return Some(Dependency::new(self.node, *node));
            }
        }

        None
    }
}
impl<'a> ExactSizeIterator for Successors<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

/// An iterator over each predecessor edge, or [Dependency], of a given node in a [DependencyGraph]
pub struct Predecessors<'a> {
    node: NodeId,
    iter: core::slice::Iter<'a, Edge>,
}
impl<'a> Iterator for Predecessors<'a> {
    type Item = Dependency;

    fn next(&mut self) -> Option<Self::Item> {
        for Edge { node, direction } in &mut self.iter {
            if matches!(direction, Direction::Dependency) {
                return Some(Dependency::new(*node, self.node));
            }
        }

        None
    }
}
impl<'a> DoubleEndedIterator for Predecessors<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(Edge { node, direction }) = self.iter.next_back() {
            if matches!(direction, Direction::Dependency) {
                return Some(Dependency::new(*node, self.node));
            }
        }

        None
    }
}
impl<'a> ExactSizeIterator for Predecessors<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

struct DebugNodes<'a>(&'a DependencyGraph);
impl<'a> fmt::Debug for DebugNodes<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.0.nodes.iter()).finish()
    }
}

struct DebugEdges<'a>(&'a DependencyGraph);
impl<'a> fmt::Debug for DebugEdges<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut edges = f.debug_list();
        for node in self.0.nodes.iter().copied() {
            for edge in self.0.successors(node) {
                edges.entry(&format_args!("{}", edge));
            }
        }
        edges.finish()
    }
}

#[cfg(debug_assertions)]
#[inline(never)]
fn is_valid_dependency(dependent: NodeId, dependency: NodeId) -> bool {
    match (dependent.into(), dependency.into()) {
        (Node::Argument(_), Node::Stack(_) | Node::Result { .. }) => true,
        (Node::Argument(_), Node::Inst { .. } | Node::Argument(_)) => {
            panic!(
                "{dependent} -> {dependency} is invalid: arguments may only depend on results or \
                 operands"
            );
        }
        (Node::Inst { .. }, Node::Inst { .. } | Node::Result { .. } | Node::Argument(_)) => true,
        (Node::Inst { .. }, _) => panic!(
            "{dependent} -> {dependency} is invalid: instruction nodes may only depend directly \
             on arguments"
        ),
        (Node::Result { .. }, Node::Inst { .. }) => true,
        (Node::Result { .. }, _) => panic!(
            "{dependent} -> {dependency} is invalid: result nodes may only depend directly on \
             instructions"
        ),
        (Node::Stack(_), _) => {
            panic!("{dependent} -> {dependency} is invalid: stack nodes may not have dependencies")
        }
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
const fn is_valid_dependency(_dependent: NodeId, _dependency: NodeId) -> bool {
    true
}

/// Helper function to produce a graph for:
///
/// ```text,ignore
/// block0(v0: i32):
///   v1 = inst0 v0
///   v3 = inst3
///   v2 = inst1 v1, v0
///   inst2 v2, block1(v1), block2(v1, v0)
/// ```
///
/// This graph represents:
///
/// * All node types
/// * All three argument types
/// * All types of result usage (unused, singly/multiply used)
/// * Instruction and value identifiers which are added out of order with respect to program order
#[cfg(test)]
pub(crate) fn simple_dependency_graph() -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let v0 = hir::Value::from_u32(0);
    let v1 = hir::Value::from_u32(1);
    let v2 = hir::Value::from_u32(2);
    let v3 = hir::Value::from_u32(3);
    let inst0 = hir::Inst::from_u32(0);
    let inst1 = hir::Inst::from_u32(1);
    let inst2 = hir::Inst::from_u32(2);
    let inst3 = hir::Inst::from_u32(3);

    let v0_node = graph.add_node(Node::Stack(v0));
    let v1_node = graph.add_node(Node::Result {
        value: v1,
        index: 0,
    });
    let v2_node = graph.add_node(Node::Result {
        value: v2,
        index: 0,
    });
    let v3_node = graph.add_node(Node::Result {
        value: v3,
        index: 0,
    });
    let inst0_node = graph.add_node(Node::Inst { id: inst0, pos: 0 });
    let inst1_node = graph.add_node(Node::Inst { id: inst1, pos: 2 });
    let inst2_node = graph.add_node(Node::Inst { id: inst2, pos: 3 });
    let inst3_node = graph.add_node(Node::Inst { id: inst3, pos: 1 });
    let inst0_arg0_node = graph.add_node(Node::Argument(ArgumentNode::Direct {
        inst: inst0,
        index: 0,
    }));
    let inst1_arg0_node = graph.add_node(Node::Argument(ArgumentNode::Direct {
        inst: inst1,
        index: 0,
    }));
    let inst1_arg1_node = graph.add_node(Node::Argument(ArgumentNode::Direct {
        inst: inst1,
        index: 1,
    }));
    let inst2_arg0_node = graph.add_node(Node::Argument(ArgumentNode::Direct {
        inst: inst2,
        index: 0,
    }));
    let inst2_block1_arg0_node = graph.add_node(Node::Argument(ArgumentNode::Indirect {
        inst: inst2,
        index: 0,
        successor: 0,
    }));
    let inst2_block2_arg0_node = graph.add_node(Node::Argument(ArgumentNode::Indirect {
        inst: inst2,
        index: 0,
        successor: 1,
    }));
    let inst2_block2_arg1_node = graph.add_node(Node::Argument(ArgumentNode::Conditional {
        inst: inst2,
        index: 1,
        successor: 1,
    }));
    graph.add_dependency(v1_node, inst0_node);
    graph.add_dependency(inst0_node, inst0_arg0_node);
    graph.add_dependency(inst0_arg0_node, v0_node);
    graph.add_dependency(v2_node, inst1_node);
    graph.add_dependency(inst1_node, inst1_arg0_node);
    graph.add_dependency(inst1_node, inst1_arg1_node);
    graph.add_dependency(inst1_arg0_node, v1_node);
    graph.add_dependency(inst1_arg1_node, v0_node);
    graph.add_dependency(inst2_node, inst2_arg0_node);
    graph.add_dependency(inst2_node, inst2_block1_arg0_node);
    graph.add_dependency(inst2_node, inst2_block2_arg0_node);
    graph.add_dependency(inst2_node, inst2_block2_arg1_node);
    graph.add_dependency(inst2_arg0_node, v2_node);
    graph.add_dependency(inst2_block1_arg0_node, v1_node);
    graph.add_dependency(inst2_block2_arg0_node, v1_node);
    graph.add_dependency(inst2_block2_arg1_node, v0_node);
    graph.add_dependency(v3_node, inst3_node);
    graph
}

#[cfg(test)]
mod tests {
    use midenc_hir::{self as hir, assert_matches};

    use super::*;

    #[test]
    fn dependency_graph_construction() {
        let graph = simple_dependency_graph();

        let v0 = hir::Value::from_u32(0);
        let v1 = hir::Value::from_u32(1);
        let v2 = hir::Value::from_u32(2);
        let v3 = hir::Value::from_u32(3);
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
        let v3_node = Node::Result {
            value: v3,
            index: 0,
        };
        let inst0_node = Node::Inst { id: inst0, pos: 0 };
        let inst1_node = Node::Inst { id: inst1, pos: 2 };
        let inst2_node = Node::Inst { id: inst2, pos: 3 };
        let inst3_node = Node::Inst { id: inst3, pos: 1 };
        let inst0_arg0_node = Node::Argument(ArgumentNode::Direct {
            inst: inst0,
            index: 0,
        });
        let inst1_arg0_node = Node::Argument(ArgumentNode::Direct {
            inst: inst1,
            index: 0,
        });
        let inst1_arg1_node = Node::Argument(ArgumentNode::Direct {
            inst: inst1,
            index: 1,
        });
        let inst2_arg0_node = Node::Argument(ArgumentNode::Direct {
            inst: inst2,
            index: 0,
        });
        let inst2_block1_arg0_node = Node::Argument(ArgumentNode::Indirect {
            inst: inst2,
            index: 0,
            successor: 0,
        });
        let inst2_block2_arg0_node = Node::Argument(ArgumentNode::Indirect {
            inst: inst2,
            index: 0,
            successor: 1,
        });
        let inst2_block2_arg1_node = Node::Argument(ArgumentNode::Conditional {
            inst: inst2,
            index: 1,
            successor: 1,
        });

        // Make sure all the nodes are in the graph
        assert!(graph.contains(&v0_node));
        assert!(graph.contains(&v1_node));
        assert!(graph.contains(&v2_node));
        assert!(graph.contains(&v3_node));
        assert!(graph.contains(&inst0_node));
        assert!(graph.contains(&inst1_node));
        assert!(graph.contains(&inst2_node));
        assert!(graph.contains(&inst3_node));
        assert!(graph.contains(&inst0_arg0_node));
        assert!(graph.contains(&inst1_arg0_node));
        assert!(graph.contains(&inst1_arg1_node));
        assert!(graph.contains(&inst2_arg0_node));
        assert!(graph.contains(&inst2_block1_arg0_node));
        assert!(graph.contains(&inst2_block2_arg0_node));
        assert!(graph.contains(&inst2_block2_arg1_node));

        // Results depend on the instructions which produce them
        assert_eq!(graph.child(v1_node), Ok(Some(inst0_node.into())));
        assert_eq!(graph.child(v2_node), Ok(Some(inst1_node.into())));

        // Instructions depend on their arguments
        assert_eq!(graph.child(inst0_node), Ok(Some(inst0_arg0_node.into())));
        let mut inst1_successors = graph.successors(inst1_node).map(|s| s.dependency);
        assert_eq!(inst1_successors.next(), Some(inst1_arg0_node.into()));
        assert_eq!(inst1_successors.next(), Some(inst1_arg1_node.into()));
        assert_eq!(inst1_successors.next(), None);

        // Arguments depend on stack values or instruction results
        assert_eq!(graph.child(inst0_arg0_node), Ok(Some(v0_node.into())));
        assert_eq!(graph.child(inst1_arg0_node), Ok(Some(v1_node.into())));
        assert_eq!(graph.child(inst1_arg1_node), Ok(Some(v0_node.into())));
        assert_eq!(graph.child(inst2_arg0_node), Ok(Some(v2_node.into())));
        assert_eq!(graph.child(inst2_block1_arg0_node), Ok(Some(v1_node.into())));
        assert_eq!(graph.child(inst2_block2_arg0_node), Ok(Some(v1_node.into())));
        assert_eq!(graph.child(inst2_block2_arg1_node), Ok(Some(v0_node.into())));

        // Arguments only have one dependent, the instruction they belong to
        assert_eq!(graph.parent(inst0_arg0_node), Ok(Some(inst0_node.into())));
        assert_eq!(graph.parent(inst1_arg0_node), Ok(Some(inst1_node.into())));
        assert_eq!(graph.parent(inst1_arg1_node), Ok(Some(inst1_node.into())));
        assert_eq!(graph.parent(inst2_arg0_node), Ok(Some(inst2_node.into())));
        assert_eq!(graph.parent(inst2_block1_arg0_node), Ok(Some(inst2_node.into())));
        assert_eq!(graph.parent(inst2_block2_arg0_node), Ok(Some(inst2_node.into())));
        assert_eq!(graph.parent(inst2_block2_arg1_node), Ok(Some(inst2_node.into())));

        // Results which are unused have no dependents
        assert_eq!(graph.parent(v3_node), Ok(None));

        // Results which are used have one or more dependents
        assert_eq!(graph.parent(v2_node), Ok(Some(inst2_arg0_node.into())));
        assert_matches!(graph.parent(v1_node), Err(_));
        let mut v1_dependents = graph.predecessors(v1_node).map(|p| p.dependent);
        assert_eq!(v1_dependents.next(), Some(inst1_arg0_node.into()));
        assert_eq!(v1_dependents.next(), Some(inst2_block1_arg0_node.into()));
        assert_eq!(v1_dependents.next(), Some(inst2_block2_arg0_node.into()));
        assert_eq!(v1_dependents.next(), None);

        // Nodes with multiple dependents will raise an error if you ask for the parent
        assert_matches!(graph.parent(v0_node), Err(_));
        // Stack nodes can have no dependencies
        assert_eq!(graph.child(v0_node), Ok(None));
    }

    /// We're expecting the graph to correspond to the following expression graph
    ///
    /// ```text,ignore
    /// inst2
    ///   |- inst2_arg0 -> v2 -> inst1---------
    ///   |                                   |
    ///   |                             _____________
    ///   |                            |             |
    ///   |                          inst1_arg0  inst1_arg1
    ///   |                            |             |
    ///   |                            v             |
    ///   |- inst2_block1_arg0 ------> v1 -> inst0   |
    ///   |                            ^      |      |
    ///   |                            |      v      |
    ///   |                            |  inst0_arg0 |
    ///   |- inst2_block2_arg0 --------       |      |
    ///   |                                   v      |
    ///   |- inst2_block2_arg1 -------------> v0 <---
    /// ```
    ///
    /// Which should correspond to the following index assignment:
    ///
    /// 0. v0
    /// 1. inst0
    /// 2. result(v1)
    /// 3. inst1
    /// 4. result(v2)
    /// 5. inst2
    ///
    /// For reference, this is the IR we have a graph of:
    ///
    /// ```text,ignore
    /// block0(v0: i32):
    ///   v1 = inst0 v0
    ///   v3 = inst3
    ///   v2 = inst1 v1, v0
    ///   inst2 v2, block1(v1), block2(v1, v0)
    /// ```
    #[test]
    fn dependency_graph_indexed() {
        let graph = simple_dependency_graph();

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

        let indices = graph.indexed(inst2_node).unwrap();

        assert_eq!(indices.get(inst3_node), None);
        assert_eq!(indices.get(inst2_node), Some(5));
        assert_eq!(indices.get(v2_node), Some(4));
        assert_eq!(indices.get(inst1_node), Some(3));
        assert_eq!(indices.get(v1_node), Some(2));
        assert_eq!(indices.get(inst0_node), Some(1));
        assert_eq!(indices.get(v0_node), Some(0));
    }
}
