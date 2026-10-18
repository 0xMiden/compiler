//! Backward demand analysis over the frozen selected graph.
//!
//! Semantic liveness is tracked per value, then expanded to raw lanes by preparation.
//! Next-use distances count selected units, with zero-cost graph transfers. Scratch
//! dependencies track incoming cell contents separately from value demand and address
//! provenance. These allocator facts are computed independently of physical verification.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    vec,
    vec::Vec,
};

use petgraph::{algo::tarjan_scc, graph::DiGraph};

use crate::{plan::*, verify::CheckedSelectedProgram};

/// Stable value-ID ordering shared by every bitset in one analysis.
#[derive(Debug, PartialEq, Eq)]
struct ValueDomain {
    ids: Vec<ValueId>,
    index: BTreeMap<ValueId, usize>,
}

/// A dense semantic-value bitset; raw lanes are expanded through the immutable layout table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveValues {
    domain: Rc<ValueDomain>,
    words: Vec<u64>,
}

impl LiveValues {
    fn empty(domain: &Rc<ValueDomain>) -> Self {
        Self {
            domain: domain.clone(),
            words: vec![0; domain.ids.len().div_ceil(64)],
        }
    }

    /// Whether this semantic value is demanded; IDs outside the procedure are absent.
    pub fn contains(&self, value: ValueId) -> bool {
        self.domain
            .index
            .get(&value)
            .is_some_and(|index| self.words[index / 64] & (1 << (index % 64)) != 0)
    }

    /// Visit live semantic values in ascending ID order, without expanding their lanes.
    pub fn iter(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.words.iter().enumerate().flat_map(move |(word, bits)| {
            let mut remaining = *bits;
            core::iter::from_fn(move || {
                if remaining == 0 {
                    return None;
                }
                let bit = remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                Some(self.domain.ids[word * 64 + bit])
            })
        })
    }

    fn insert(&mut self, value: ValueId) {
        let index = self.domain.index[&value];
        self.words[index / 64] |= 1 << (index % 64);
    }

    fn remove(&mut self, value: ValueId) {
        let index = self.domain.index[&value];
        self.words[index / 64] &= !(1 << (index % 64));
    }

    fn union(&mut self, other: &Self) {
        debug_assert!(Rc::ptr_eq(&self.domain, &other.domain));
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word |= other;
        }
    }
}

/// Selected-unit distance to a use, ordered with unreachable demand after finite demand.
/// Infinity remains distinct from the greatest finite saturated distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NextUseDistance {
    /// Number of selected units crossed; a use at the current boundary has distance zero.
    Finite(u32),
    /// The current value has no demanded use within the analyzed graph or loop scope.
    Infinite,
}

impl NextUseDistance {
    /// Add selected-unit work, saturating finite distances without turning them infinite.
    pub fn advance(self, cost: u32) -> Self {
        match self {
            Self::Finite(distance) => Self::Finite(distance.saturating_add(cost)),
            Self::Infinite => self,
        }
    }
}

/// Sparse nearest-use distances; a missing semantic value has infinite distance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UseDistances(BTreeMap<ValueId, u32>);

impl UseDistances {
    /// Return the nearest finite use, or infinity when the value is absent.
    pub fn get(&self, value: ValueId) -> NextUseDistance {
        self.0
            .get(&value)
            .copied()
            .map_or(NextUseDistance::Infinite, NextUseDistance::Finite)
    }

    /// Rank lanes by nearest semantic use, breaking ties by value ID and then lane.
    pub fn priority(&self, atom: Atom) -> (NextUseDistance, ValueId, u8) {
        (self.get(atom.value), atom.value, atom.lane)
    }

    /// Merge a path's use by retaining the shortest distance seen for the value.
    fn include(&mut self, value: ValueId, distance: u32) {
        self.0
            .entry(value)
            .and_modify(|old| *old = (*old).min(distance))
            .or_insert(distance);
    }
}

/// Nearest-use facts at selected execution boundaries.
///
/// Unit maps describe the boundaries immediately before and after a unit. Edge facts
/// use source-side names; segment entry facts use destination-side names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NextUses {
    /// Demand before the segment's first selected unit.
    pub entry: BTreeMap<SegmentId, UseDistances>,
    /// Demand including the unit's own inputs, at distance zero.
    pub before: BTreeMap<SelectedStepId, UseDistances>,
    /// Demand after normal completion, before crossing a later unit.
    pub after: BTreeMap<SelectedStepId, UseDistances>,
    /// Demand carried by each transfer, translated to the predecessor's value names.
    pub edge: BTreeMap<EdgeId, UseDistances>,
}

/// Repeated-work scope for one structured loop, excluding terminal arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopDemand {
    /// Entry segment reentered by this loop's cyclic transfers.
    pub header: SegmentId,
    /// Structurally enclosed segments in the header's strongly connected component.
    pub members: BTreeSet<SegmentId>,
    /// Only cyclic paths within this loop contribute; nested uses count for enclosing loops.
    pub next_use: NextUses,
}

/// Scratch contents required on entry to later reads, in sorted and deduplicated vectors.
///
/// Exact writes and definite borrowed writes satisfy demand; possible writes do not.
/// Explicit selected reads create incoming demand. Borrow reads may be prepared inside
/// the opaque expansion and do not themselves create an incoming cell requirement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScratchDependencies {
    /// Requirements before any unit in the segment executes.
    pub entry: BTreeMap<SegmentId, Vec<ScratchCellFact>>,
    /// Requirements including explicit reads by this unit.
    pub before: BTreeMap<SelectedStepId, Vec<ScratchCellFact>>,
    /// Requirements following normal completion of this unit.
    pub after: BTreeMap<SelectedStepId, Vec<ScratchCellFact>>,
    /// Successor requirements translated through value and content parameter bindings.
    pub edge: BTreeMap<EdgeId, Vec<ScratchCellFact>>,
}

/// Procedure-wide demand and cycle structure used to choose frozen physical contracts.
///
/// Execution-point maps cover reachable segments and their units/edges. Liveness names
/// whole semantic values, even when only one raw lane appears in a lowering contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllocationAnalysis {
    /// Segments reachable from the first selected segment through destination-bearing edges.
    pub reachable: BTreeSet<SegmentId>,
    /// Semantic demand before the first unit, including parameters used by that segment.
    pub live_in: BTreeMap<SegmentId, LiveValues>,
    /// Demand after the last unit, including a structured control predicate.
    pub live_out: BTreeMap<SegmentId, LiveValues>,
    /// Semantic demand immediately before each selected unit.
    pub before: BTreeMap<SelectedStepId, LiveValues>,
    /// Semantic demand immediately after each selected unit's normal completion.
    pub after: BTreeMap<SelectedStepId, LiveValues>,
    /// Successor demand in source-side names; return edges demand their result bindings.
    pub edge_live: BTreeMap<EdgeId, LiveValues>,
    /// Nearest-use distances over the entire reachable graph.
    pub next_use: NextUses,
    /// Component identity is its least SegmentId, independent of DFS or storage order.
    pub sccs: BTreeMap<SegmentId, Vec<SegmentId>>,
    /// Reverse lookup from each reachable segment to its stable component identity.
    pub scc_membership: BTreeMap<SegmentId, SegmentId>,
    /// Structured loop scopes, which can distinguish nested loops in a single graph SCC.
    pub loops: BTreeMap<ControlId, LoopDemand>,
    /// Required scratch cell contents, independent of stack representation.
    pub scratch: ScratchDependencies,
}

/// Deterministic graph indexing shared by the backward fixed-point analyses.
struct Graph<'a> {
    program: &'a CheckedSelectedProgram,
    segments: BTreeMap<SegmentId, &'a SelectedSegment>,
    outgoing: BTreeMap<SegmentId, Vec<&'a TransferEdge>>,
    predecessors: BTreeMap<SegmentId, BTreeSet<SegmentId>>,
    reachable: BTreeSet<SegmentId>,
    predicates: BTreeMap<SegmentId, ValueId>,
    domain: Rc<ValueDomain>,
}

impl<'a> Graph<'a> {
    /// Index checked edges and control predicates, and find entry-reachable segments.
    fn new(program: &'a CheckedSelectedProgram) -> Self {
        let segments: BTreeMap<_, _> =
            program.segments.iter().map(|segment| (segment.id, segment)).collect();
        let mut outgoing: BTreeMap<_, Vec<_>> = segments.keys().map(|id| (*id, vec![])).collect();
        let mut predecessors: BTreeMap<_, BTreeSet<_>> =
            segments.keys().map(|id| (*id, BTreeSet::new())).collect();
        for edge in &program.edges {
            outgoing.get_mut(&edge.source).unwrap().push(edge);
            if let Some(destination) = edge.destination {
                predecessors.get_mut(&destination).unwrap().insert(edge.source);
            }
        }
        for edges in outgoing.values_mut() {
            edges.sort_by_key(|edge| edge.id);
        }
        let mut reachable = BTreeSet::new();
        let mut pending = BTreeSet::from([program.segments[0].id]);
        while let Some(id) = pending.pop_first() {
            if !reachable.insert(id) {
                continue;
            }
            pending.extend(outgoing[&id].iter().filter_map(|edge| edge.destination));
        }
        let mut predicates = BTreeMap::new();
        fn visit(
            tree: &ControlTree,
            edges: &[TransferEdge],
            predicates: &mut BTreeMap<SegmentId, ValueId>,
        ) {
            match tree {
                ControlTree::If {
                    predicate,
                    on_true,
                    then_body,
                    else_body,
                    ..
                } => {
                    let source = edges.iter().find(|edge| edge.id == *on_true).unwrap().source;
                    predicates.insert(source, predicate.value);
                    visit(then_body, edges, predicates);
                    visit(else_body, edges, predicates);
                }
                ControlTree::Loop {
                    header,
                    predicate,
                    body,
                    ..
                } => {
                    predicates.insert(*header, predicate.value);
                    visit(body, edges, predicates);
                }
                ControlTree::Sequence(children) => {
                    for child in children {
                        visit(child, edges, predicates);
                    }
                }
                _ => {}
            }
        }
        visit(&program.structure, &program.edges, &mut predicates);
        let ids: Vec<_> = program.values.keys().copied().collect();
        let index = ids.iter().enumerate().map(|(index, id)| (*id, index)).collect();
        Self {
            program,
            segments,
            outgoing,
            predecessors,
            reachable,
            predicates,
            domain: Rc::new(ValueDomain { ids, index }),
        }
    }

    fn empty(&self) -> LiveValues {
        LiveValues::empty(&self.domain)
    }

    /// Translate a successor parameter demand without renaming unchanged captures.
    fn source_value(edge: &TransferEdge, destination: ValueId) -> ValueId {
        edge.bindings
            .iter()
            .find(|binding| binding.destination == destination)
            .map_or(destination, |binding| binding.source)
    }

    /// Collect semantic, stack, stored, and alias-reconstruction dependencies of a unit.
    fn uses(&self, unit: &SelectedLowering) -> BTreeSet<ValueId> {
        let mut uses: BTreeSet<_> = unit.semantic_inputs.iter().copied().collect();
        uses.extend(unit.stack_inputs.iter().map(|atom| atom.value));
        uses.extend(unit.stored_inputs.iter().filter_map(|input| match input.value {
            StoredValue::Atom(atom) => Some(atom.value),
            _ => None,
        }));
        for atom in &unit.outputs {
            if let Some(Reconstruction::Alias(sources)) =
                &self.program.values[&atom.value].reconstruction
            {
                uses.extend(sources.iter().map(|source| source.value));
            }
        }
        uses
    }
}

/// Compute demand to a fixed point and identify stable cycle and structured-loop scopes.
///
/// Checked selection supplies valid definitions and graph references, so this analysis
/// has no recovery path for malformed input. It never reads a physical plan or verifier state.
pub(crate) fn analyze(program: &CheckedSelectedProgram) -> AllocationAnalysis {
    let graph = Graph::new(program);
    let mut result = AllocationAnalysis {
        reachable: graph.reachable.clone(),
        live_in: BTreeMap::new(),
        live_out: BTreeMap::new(),
        before: BTreeMap::new(),
        after: BTreeMap::new(),
        edge_live: BTreeMap::new(),
        next_use: NextUses::default(),
        sccs: BTreeMap::new(),
        scc_membership: BTreeMap::new(),
        loops: BTreeMap::new(),
        scratch: ScratchDependencies::default(),
    };
    let mut pending = graph.reachable.clone();
    while let Some(id) = pending.pop_first() {
        let mut live = graph.empty();
        if let Some(predicate) = graph.predicates.get(&id) {
            live.insert(*predicate);
        }
        for edge in &graph.outgoing[&id] {
            let mut edge_live = graph.empty();
            if let Some(destination) = edge.destination {
                if let Some(incoming) = result.live_in.get(&destination) {
                    for value in incoming.iter() {
                        edge_live.insert(Graph::source_value(edge, value));
                    }
                }
            } else if edge.kind == EdgeKind::Return {
                for binding in &edge.bindings {
                    edge_live.insert(binding.source);
                }
            }
            live.union(&edge_live);
            result.edge_live.insert(edge.id, edge_live);
        }
        result.live_out.insert(id, live.clone());
        for unit in graph.segments[&id].units.iter().rev() {
            result.after.insert(unit.id, live.clone());
            for atom in &unit.outputs {
                live.remove(atom.value);
            }
            for value in graph.uses(unit) {
                live.insert(value);
            }
            result.before.insert(unit.id, live.clone());
        }
        let changed = result.live_in.get(&id) != Some(&live);
        result.live_in.insert(id, live);
        if changed {
            pending.extend(graph.predecessors[&id].intersection(&graph.reachable).copied());
        }
    }
    result.next_use = next_use(&graph, &graph.reachable);
    let mut cfg = DiGraph::<SegmentId, ()>::new();
    let nodes: BTreeMap<_, _> = graph.reachable.iter().map(|id| (*id, cfg.add_node(*id))).collect();
    for id in &graph.reachable {
        for edge in &graph.outgoing[id] {
            if let Some(destination) = edge.destination {
                cfg.add_edge(nodes[id], nodes[&destination], ());
            }
        }
    }
    for component in tarjan_scc(&cfg) {
        let mut members: Vec<_> = component.into_iter().map(|node| cfg[node]).collect();
        members.sort();
        let id = members[0];
        result.scc_membership.extend(members.iter().map(|member| (*member, id)));
        result.sccs.insert(id, members);
    }
    collect_loops(&program.structure, &graph, &result.scc_membership, &mut result.loops);
    result.scratch = scratch_dependencies(&graph);
    result
}

/// Solve shortest backward demand paths within `members`; crossing out of the scope
/// contributes no successor demand, while explicit return results remain immediate uses.
fn next_use(graph: &Graph<'_>, members: &BTreeSet<SegmentId>) -> NextUses {
    let mut result = NextUses::default();
    let mut pending = members.clone();
    while let Some(id) = pending.pop_first() {
        let mut distances = UseDistances::default();
        if let Some(predicate) = graph.predicates.get(&id) {
            distances.include(*predicate, 0);
        }
        for edge in &graph.outgoing[&id] {
            let mut edge_distances = UseDistances::default();
            if let Some(destination) = edge.destination {
                if members.contains(&destination)
                    && let Some(incoming) = result.entry.get(&destination)
                {
                    for (value, distance) in &incoming.0 {
                        edge_distances.include(Graph::source_value(edge, *value), *distance);
                    }
                }
            } else if edge.kind == EdgeKind::Return {
                for binding in &edge.bindings {
                    edge_distances.include(binding.source, 0);
                }
            }
            for (value, distance) in &edge_distances.0 {
                distances.include(*value, *distance);
            }
            result.edge.insert(edge.id, edge_distances);
        }
        for unit in graph.segments[&id].units.iter().rev() {
            result.after.insert(unit.id, distances.clone());
            // Unit steps have cost one; graph transfers introduce no fictitious work.
            for distance in distances.0.values_mut() {
                *distance = distance.saturating_add(1);
            }
            for atom in &unit.outputs {
                distances.0.remove(&atom.value);
            }
            for value in graph.uses(unit) {
                distances.include(value, 0);
            }
            result.before.insert(unit.id, distances.clone());
        }
        let changed = result.entry.get(&id) != Some(&distances);
        result.entry.insert(id, distances);
        if changed {
            pending.extend(graph.predecessors[&id].intersection(members).copied());
        }
    }
    result
}

/// Intersect structured loop membership with its header's SCC before ranking repeated work.
/// Nested uses count in an enclosing loop, but exits and terminal arms do not.
fn collect_loops(
    tree: &ControlTree,
    graph: &Graph<'_>,
    components: &BTreeMap<SegmentId, SegmentId>,
    loops: &mut BTreeMap<ControlId, LoopDemand>,
) {
    fn segments(tree: &ControlTree, out: &mut BTreeSet<SegmentId>) {
        match tree {
            ControlTree::Segment(id) => {
                out.insert(*id);
            }
            ControlTree::Sequence(children) => {
                for child in children {
                    segments(child, out);
                }
            }
            ControlTree::If {
                then_body,
                else_body,
                ..
            } => {
                segments(then_body, out);
                segments(else_body, out);
            }
            ControlTree::Loop { header, body, .. } => {
                out.insert(*header);
                segments(body, out);
            }
            ControlTree::Edge(_) => {}
        }
    }
    match tree {
        ControlTree::Loop {
            id, header, body, ..
        } => {
            let mut members = BTreeSet::from([*header]);
            segments(body, &mut members);
            // A terminal arm is structurally inside the loop but is not repeated work.
            // The structured scope separates nested loops that share one outer graph SCC.
            members.retain(|id| components.get(id) == components.get(header));
            let demand = LoopDemand {
                header: *header,
                next_use: next_use(graph, &members),
                members,
            };
            loops.insert(*id, demand);
            collect_loops(body, graph, components, loops);
        }
        ControlTree::If {
            then_body,
            else_body,
            ..
        } => {
            collect_loops(then_body, graph, components, loops);
            collect_loops(else_body, graph, components, loops);
        }
        ControlTree::Sequence(children) => {
            for child in children {
                collect_loops(child, graph, components, loops);
            }
        }
        _ => {}
    }
}

/// Propagate explicit read requirements backward, substituting both lane and content names
/// at edges and removing requirements satisfied by definite writes.
fn scratch_dependencies(graph: &Graph<'_>) -> ScratchDependencies {
    let mut entry: BTreeMap<SegmentId, BTreeSet<ScratchCellFact>> = BTreeMap::new();
    let mut result = ScratchDependencies::default();
    let mut pending = graph.reachable.clone();
    while let Some(id) = pending.pop_first() {
        let mut demand = BTreeSet::new();
        for edge in &graph.outgoing[&id] {
            let incoming: BTreeSet<_> = edge
                .destination
                .and_then(|id| entry.get(&id))
                .into_iter()
                .flatten()
                .map(|fact| {
                    let mut fact = *fact;
                    fact.content = match fact.content {
                        ContentRequirement::Atom(atom) => ContentRequirement::Atom(Atom::new(
                            Graph::source_value(edge, atom.value),
                            atom.lane,
                        )),
                        ContentRequirement::Content(id) => ContentRequirement::Content(
                            edge.content_bindings
                                .iter()
                                .find(|binding| binding.destination == id)
                                .map_or(id, |binding| binding.source),
                        ),
                        content => content,
                    };
                    fact
                })
                .collect();
            demand.extend(incoming.iter().copied());
            result.edge.insert(edge.id, incoming.into_iter().collect());
        }
        for unit in graph.segments[&id].units.iter().rev() {
            result.after.insert(unit.id, demand.iter().copied().collect());
            demand.retain(|fact| {
                !unit.scratch_effects.iter().any(|effect| match effect {
                    ScratchEffect::Write(written) => {
                        fact.object == written.object && fact.offset == written.offset
                    }
                    ScratchEffect::Borrow {
                        object,
                        offset,
                        size,
                        write: true,
                        definite_write: true,
                        ..
                    } => {
                        fact.object == *object
                            && fact.offset >= *offset
                            && fact.offset - offset < *size
                    }
                    _ => false,
                })
            });
            // Borrow reads occur inside an expansion, which can prepare their cells locally.
            // Incoming requirements are the selected explicit Read effects.
            for effect in &unit.scratch_effects {
                if let ScratchEffect::Read(fact) = effect {
                    demand.insert(*fact);
                }
            }
            result.before.insert(unit.id, demand.iter().copied().collect());
        }
        let changed = entry.get(&id) != Some(&demand);
        entry.insert(id, demand);
        if changed {
            pending.extend(graph.predecessors[&id].intersection(&graph.reachable).copied());
        }
    }
    result.entry = entry.into_iter().map(|(id, facts)| (id, facts.into_iter().collect())).collect();
    result
}
