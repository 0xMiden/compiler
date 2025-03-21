use alloc::vec::Vec;

use super::*;
use crate::FxHashMap;

#[derive(Clone)]
pub struct StronglyConnectedComponent<G: Graph> {
    nodes: Vec<<G as Graph>::Node>,
}

impl<G> core::fmt::Debug for StronglyConnectedComponent<G>
where
    G: Graph,
    <G as Graph>::Node: Eq + Clone + core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StronglyConnectedComponent")
            .field("nodes", &self.nodes)
            .finish()
    }
}

impl<G> Default for StronglyConnectedComponent<G>
where
    G: Graph,
{
    fn default() -> Self {
        Self {
            nodes: Default::default(),
        }
    }
}

impl<G, N> StronglyConnectedComponent<G>
where
    N: Clone + Eq,
    G: Graph<Node = N>,
{
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    pub fn as_slice(&self) -> &[N] {
        self.nodes.as_slice()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn push(&mut self, node: N) {
        self.nodes.push(node);
    }

    pub fn iter(&self) -> core::slice::Iter<'_, N> {
        self.nodes.iter()
    }

    /// Test if the current SCC has a cycle.
    ///
    /// If the SCC has more than one node, this is trivially true.  If not, it may still contain a
    /// cycle if the node has an edge back to itself.
    pub fn has_cycle(&self) -> bool {
        assert!(!self.is_empty());

        if self.len() > 1 {
            return true;
        }

        let node = self.nodes[0].clone();
        for child_node in <G as Graph>::children(node.clone()) {
            if child_node == node {
                return true;
            }
        }

        false
    }
}

impl<G: Graph> IntoIterator for StronglyConnectedComponent<G> {
    type IntoIter = alloc::vec::IntoIter<Self::Item>;
    type Item = <G as Graph>::Node;

    fn into_iter(self) -> Self::IntoIter {
        self.nodes.into_iter()
    }
}

pub struct StronglyConnectedComponents<G: Graph> {
    /// Global visit counter
    next_visit_num: usize,
    /// The per-node visit counters used to detect when a complete SCC is on the stack.
    ///
    /// The counters are also used as DFS flags
    visit_numbers: FxHashMap<<G as Graph>::Node, usize>,
    /// Stack holding nodes of the SCC
    node_stack: Vec<<G as Graph>::Node>,
    /// The current SCC
    current: StronglyConnectedComponent<G>,
    /// DFS stack, used to maintain the ordering.
    ///
    /// The top contains the current node, the next child to visit, and the minimum uplink value
    /// of all children.
    visit_stack: Vec<StackElement<G>>,
}

impl<G> core::fmt::Debug for StronglyConnectedComponents<G>
where
    G: Graph,
    <G as Graph>::Node: Eq + Clone + core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StronglyConnectedComponents")
            .field("next_visit_num", &self.next_visit_num)
            .field("visit_numbers", &self.visit_numbers)
            .field("node_stack", &self.node_stack)
            .field("current", &self.current)
            .field("visit_stack", &self.visit_stack)
            .finish()
    }
}

struct StackElement<G: Graph> {
    // Current node pointer
    node: <G as Graph>::Node,
    // The next child, modified in-place during DFS
    next_child: <G as Graph>::ChildIter,
    // Minimum uplink value of all children of `node`
    min_visited: usize,
}

impl<G> core::fmt::Debug for StackElement<G>
where
    G: Graph,
    <G as Graph>::Node: Eq + Clone + core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StackElement")
            .field("node", &self.node)
            .field("min_visited", &self.min_visited)
            .finish_non_exhaustive()
    }
}

impl<G, N> StronglyConnectedComponents<G>
where
    N: Clone + Eq + core::hash::Hash + core::fmt::Debug,
    G: Graph<Node = N>,
{
    pub fn new(graph: &G) -> Self {
        Self::init(graph.entry_node())
    }

    fn init(node: N) -> Self {
        let mut this = Self {
            next_visit_num: 0,
            visit_numbers: Default::default(),
            node_stack: Default::default(),
            current: Default::default(),
            visit_stack: Default::default(),
        };

        this.visit_one(node);
        this.next_scc();

        this
    }

    fn is_at_end(&self) -> bool {
        assert!(!self.current.is_empty() || self.visit_stack.is_empty());
        self.current.is_empty()
    }

    /// Inform the iterator that the specified old node has been deleted, and `new` is to be used
    /// in its place.
    #[allow(unused)]
    pub fn replace_node(&mut self, old: N, new: N) {
        let count = self.visit_numbers.remove(&old).expect("'old' not in scc iterator");
        *self.visit_numbers.entry(new).or_default() = count;
    }

    /// A single "visit" within the non-recursive DFS traversal
    fn visit_one(&mut self, node: N) {
        let visit_num = self.next_visit_num;
        self.next_visit_num += 1;
        self.visit_numbers.insert(node.clone(), visit_num);
        self.node_stack.push(node.clone());
        let next_child = <G as Graph>::children(node.clone());
        self.visit_stack.push(StackElement {
            node,
            next_child,
            min_visited: visit_num,
        });
    }

    /// The stack-based DFS traversal
    fn visit_children(&mut self) {
        assert!(!self.visit_stack.is_empty());

        while let Some(child_node) = self.visit_stack.last_mut().unwrap().next_child.next() {
            let visited = self.visit_numbers.get(&child_node).copied();
            match visited {
                None => {
                    // This node has never been seen
                    self.visit_one(child_node);
                    continue;
                }
                Some(child_num) => {
                    let tos = self.visit_stack.last_mut().unwrap();
                    tos.min_visited = core::cmp::min(tos.min_visited, child_num);
                }
            }
        }
    }

    /// Compute the next SCC using the DFS traversal
    fn next_scc(&mut self) {
        self.current.clear();

        while !self.visit_stack.is_empty() {
            self.visit_children();

            // Pop the leaf on top of the visit stack
            let mut visiting = self.visit_stack.pop().unwrap();
            assert!(visiting.next_child.next().is_none());

            // Propagate min_visited to parent so we can detect the SCC starting node
            if let Some(last) = self.visit_stack.last_mut() {
                last.min_visited = core::cmp::min(last.min_visited, visiting.min_visited);
            }

            if visiting.min_visited != self.visit_numbers[&visiting.node] {
                continue;
            }

            // A full SCC is on the node stack! It includes all nodes below `visiting` on the stack.
            // Copy those nodes to `self.current`, reset their `min_visited` values, and return (
            // this suspends the DFS traversal till a subsequent call to `next`)
            loop {
                let node = self.node_stack.pop().unwrap();
                let should_continue = node != visiting.node;
                *self.visit_numbers.get_mut(&node).unwrap() = usize::MAX;
                self.current.push(node);

                if !should_continue {
                    return;
                }
            }
        }
    }
}

impl<G, N> Iterator for StronglyConnectedComponents<G>
where
    N: Clone + Eq + core::hash::Hash + core::fmt::Debug,
    G: Graph<Node = N>,
{
    type Item = StronglyConnectedComponent<G>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_at_end() {
            return None;
        }

        let scc = core::mem::take(&mut self.current);

        self.next_scc();

        Some(scc)
    }
}
