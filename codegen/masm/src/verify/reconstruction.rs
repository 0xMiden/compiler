//! Source-declared identities, checked against executable publication before reuse.
use alloc::collections::{BTreeMap, BTreeSet};

use super::{
    error,
    selection::validate_atom,
    stack::State,
    tokens::{Token, Tokens},
    *,
};

/// Dominator-tree intervals for independently checked selected segments.
/// The relation is reflexive; unreachable nodes prove no dominance of other nodes.
/// Storing one interval per node avoids retaining a quadratic ancestor relation.
pub(super) struct Dominators(BTreeMap<SegmentId, (usize, usize)>);
impl Dominators {
    pub fn dominates(&self, owner: SegmentId, node: SegmentId) -> bool {
        let (start, end) = self.0[&owner];
        let (position, _) = self.0[&node];
        owner == node || start <= position && position < end
    }
}

/// Derive dominance from checked graph IDs, independently of allocator analyses.
/// Indexed predecessors and reverse postorder feed the iterative immediate-dominator
/// algorithm. Each update uses current facts, so a chain converges in one forward pass.
/// Iterative traversals also avoid host-stack limits for deep selected graphs.
pub(super) fn dominators(program: &SelectedProgram) -> Dominators {
    let count = program.segments.len();
    if count == 0 {
        return Dominators(BTreeMap::new());
    }
    let indices: BTreeMap<_, _> = program
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| (segment.id, index))
        .collect();
    let mut successors = alloc::vec![alloc::vec::Vec::new(); count];
    let mut predecessors = alloc::vec![alloc::vec::Vec::new(); count];
    for edge in &program.edges {
        if let Some(destination) = edge.destination {
            let from = indices[&edge.source];
            let to = indices[&destination];
            successors[from].push(to);
            predecessors[to].push(from);
        }
    }
    let mut seen = alloc::vec![false; count];
    seen[0] = true;
    let mut traversal = alloc::vec![(0, 0)];
    let mut order = alloc::vec::Vec::with_capacity(count);
    while let Some((node, next)) = traversal.last_mut() {
        if let Some(&child) = successors[*node].get(*next) {
            *next += 1;
            if !seen[child] {
                seen[child] = true;
                traversal.push((child, 0));
            }
        } else {
            order.push(*node);
            traversal.pop();
        }
    }
    order.reverse();
    let mut rank = alloc::vec![usize::MAX; count];
    for (index, node) in order.iter().enumerate() {
        rank[*node] = index;
    }
    let mut parent = alloc::vec![None; count];
    parent[0] = Some(0);
    loop {
        let mut changed = false;
        for &node in order.iter().skip(1) {
            let mut common: Option<usize> = None;
            for &predecessor in &predecessors[node] {
                if parent[predecessor].is_none() {
                    continue;
                }
                common = Some(match common {
                    None => predecessor,
                    Some(mut left) => {
                        let mut right = predecessor;
                        while left != right {
                            while rank[left] > rank[right] {
                                left = parent[left].unwrap();
                            }
                            while rank[right] > rank[left] {
                                right = parent[right].unwrap();
                            }
                        }
                        left
                    }
                });
            }
            if parent[node] != common {
                parent[node] = common;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut children = alloc::vec![alloc::vec::Vec::new(); count];
    for &node in order.iter().skip(1) {
        children[parent[node].unwrap()].push(node);
    }
    let mut intervals = alloc::vec![(usize::MAX, usize::MAX); count];
    let mut clock = 0;
    let mut pending = alloc::vec![(0, false)];
    while let Some((node, exiting)) = pending.pop() {
        if exiting {
            intervals[node].1 = clock;
        } else {
            intervals[node].0 = clock;
            clock += 1;
            pending.push((node, true));
            pending.extend(children[node].iter().rev().map(|child| (*child, false)));
        }
    }
    Dominators(indices.into_iter().map(|(id, index)| (id, intervals[index])).collect())
}

/// Source definition positions indexed independently of allocator entry contracts.
/// Parameters occupy position zero; unit outputs occupy their unit's one-based position.
/// Build after selected definitions have been validated, then reuse for all hypotheses.
pub(super) struct Definitions(BTreeMap<Atom, (SegmentId, usize)>);
impl Definitions {
    /// Record validated parameter lanes and published outputs in source execution order.
    pub fn new(program: &SelectedProgram) -> Self {
        let mut definitions = BTreeMap::new();
        for segment in &program.segments {
            for parameter in &segment.parameters {
                for lane in 0..program.values[parameter].layout.lanes() {
                    definitions.insert(Atom::new(*parameter, lane), (segment.id, 0));
                }
            }
            for (index, unit) in segment.units.iter().enumerate() {
                for atom in &unit.outputs {
                    definitions.insert(*atom, (segment.id, index + 1));
                }
            }
        }
        Self(definitions)
    }

    /// Resolve an exact lane; absent lanes never acquire a definition through an alias.
    pub fn get(&self, atom: Atom) -> Result<(SegmentId, usize), VerificationError> {
        self.0
            .get(&atom)
            .copied()
            .ok_or_else(|| error("reconstruction references a value without a selected definition"))
    }
}

/// Validate recipe widths, acyclic preceding aliases, and dominance of semantic uses.
/// This checks that a recipe is well formed; replay still proves its actual publication.
pub(super) fn validate(program: &SelectedProgram) -> Result<(), VerificationError> {
    fn visit(
        program: &SelectedProgram,
        id: ValueId,
        visiting: &mut BTreeSet<ValueId>,
        done: &mut BTreeSet<ValueId>,
        dom: &Dominators,
        definitions: &Definitions,
    ) -> Result<(), VerificationError> {
        if done.contains(&id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(error("cyclic reconstruction alias"));
        }
        let value = &program.values[&id];
        if let Some(reconstruction) = &value.reconstruction {
            let width = match reconstruction {
                Reconstruction::Literal(lanes) => lanes.len(),
                Reconstruction::Alias(lanes) => lanes.len(),
            };
            if width != usize::from(value.layout.lanes()) {
                return Err(error("reconstruction width disagrees with value layout"));
            }
            for lane in 0..value.layout.lanes() {
                let (destination, position) = definitions.get(Atom::new(id, lane))?;
                if let Reconstruction::Alias(sources) = reconstruction {
                    let source = sources[usize::from(lane)];
                    validate_atom(program, source)?;
                    let (origin, prior) = definitions.get(source)?;
                    if !dom.dominates(origin, destination)
                        || origin == destination
                            && (prior > position || prior == position && position != 0)
                    {
                        return Err(error(
                            "reconstruction alias source does not precede its definition",
                        ));
                    }
                    visit(program, source.value, visiting, done, dom, definitions)?;
                }
            }
        }
        visiting.remove(&id);
        done.insert(id);
        Ok(())
    }
    let dom = dominators(program);
    let definitions = Definitions::new(program);
    let mut done = BTreeSet::new();
    for id in program.values.keys() {
        visit(program, *id, &mut BTreeSet::new(), &mut done, &dom, &definitions)?;
    }
    let use_at =
        |id: ValueId, segment: SegmentId, position: usize| -> Result<(), VerificationError> {
            if program.values[&id].reconstruction.is_some() {
                for lane in 0..program.values[&id].layout.lanes() {
                    let (owner, prior) = definitions.get(Atom::new(id, lane))?;
                    if !dom.dominates(owner, segment) || owner == segment && prior >= position {
                        return Err(error("reconstruction use precedes its current definition"));
                    }
                }
            }
            Ok(())
        };
    for segment in &program.segments {
        for (index, unit) in segment.units.iter().enumerate() {
            for input in unit
                .semantic_inputs
                .iter()
                .copied()
                .chain(unit.stack_inputs.iter().map(|atom| atom.value))
                .chain(unit.stored_inputs.iter().filter_map(|input| match input.value {
                    StoredValue::Atom(atom) => Some(atom.value),
                    _ => None,
                }))
            {
                use_at(input, segment.id, index + 1)?;
            }
        }
    }
    for edge in &program.edges {
        for binding in &edge.bindings {
            use_at(binding.source, edge.source, usize::MAX)?;
        }
    }
    let mut controls = alloc::vec![&program.structure];
    while let Some(tree) = controls.pop() {
        let condition = match tree {
            ControlTree::If {
                predicate,
                on_true,
                then_body,
                else_body,
                ..
            } => {
                controls.extend([then_body.as_ref(), else_body.as_ref()]);
                Some((*predicate, *on_true))
            }
            ControlTree::Loop {
                predicate,
                on_true,
                before,
                body,
                ..
            } => {
                controls.extend([before.as_ref(), body.as_ref()]);
                Some((*predicate, *on_true))
            }
            ControlTree::Sequence(children) => {
                controls.extend(children);
                None
            }
            _ => None,
        };
        if let Some((predicate, edge)) = condition {
            validate_atom(program, predicate)?;
            let source = program.edges.iter().find(|item| item.id == edge).unwrap().source;
            use_at(predicate.value, source, usize::MAX)?;
        }
    }
    Ok(())
}

/// Check a newly published token against its source recipe. A value without a recipe has
/// no extra equality constraint here; ordinary input/output obligations are checked elsewhere.
pub(super) fn matches(
    program: &SelectedProgram,
    atom: Atom,
    token: Token,
    state: &State,
    tokens: &Tokens,
) -> Result<bool, VerificationError> {
    Ok(match &program.values[&atom.value].reconstruction {
        Some(Reconstruction::Literal(lanes)) => {
            tokens.constant(token) == Some(lanes[usize::from(atom.lane)].as_canonical_u64())
        }
        Some(Reconstruction::Alias(lanes)) => state.token(lanes[usize::from(atom.lane)])? == token,
        None => true,
    })
}

/// Only relations whose defining dynamic instance dominates this entry are hypotheses.
/// A body result from the preceding iteration cannot constrain the new loop header.
pub(super) fn seed(
    program: &SelectedProgram,
    segment: SegmentId,
    dom: &Dominators,
    definitions: &Definitions,
    names: &mut BTreeMap<Atom, Token>,
    tokens: &mut Tokens,
) -> Result<(), VerificationError> {
    struct Entry<'a> {
        program: &'a SelectedProgram,
        segment: SegmentId,
        dom: &'a Dominators,
        definitions: &'a Definitions,
    }
    impl Entry<'_> {
        fn resolve(
            &self,
            atom: Atom,
            names: &mut BTreeMap<Atom, Token>,
            tokens: &mut Tokens,
            done: &mut BTreeSet<Atom>,
        ) -> Result<Token, VerificationError> {
            if !done.insert(atom) {
                return Ok(names[&atom]);
            }
            let token = match &self.program.values[&atom.value].reconstruction {
                Some(Reconstruction::Literal(lanes)) => {
                    tokens.literal(lanes[usize::from(atom.lane)])
                }
                Some(Reconstruction::Alias(lanes)) => {
                    let (owner, position) = self.definitions.get(atom)?;
                    if self.dom.dominates(owner, self.segment)
                        && (owner != self.segment || position == 0)
                    {
                        self.resolve(lanes[usize::from(atom.lane)], names, tokens, done)?
                    } else {
                        names[&atom]
                    }
                }
                None => names[&atom],
            };
            names.insert(atom, token);
            Ok(token)
        }
    }
    let entry = Entry {
        program,
        segment,
        dom,
        definitions,
    };
    let mut done = BTreeSet::new();
    for atom in names.keys().copied().collect::<alloc::vec::Vec<_>>() {
        entry.resolve(atom, names, tokens, &mut done)?;
    }
    Ok(())
}

/// Recognize a proven literal or recursively recoverable alias without requiring a physical
/// copy. Requires recipes already checked for cycles and identities established in `state`.
pub(super) fn recoverable(
    program: &SelectedProgram,
    atom: Atom,
    state: &State,
    tokens: &Tokens,
) -> bool {
    match &program.values[&atom.value].reconstruction {
        Some(Reconstruction::Literal(lanes)) => state.names.get(&atom).is_some_and(|token| {
            tokens.constant(*token) == Some(lanes[usize::from(atom.lane)].as_canonical_u64())
        }),
        Some(Reconstruction::Alias(lanes)) => {
            let source = lanes[usize::from(atom.lane)];
            state
                .names
                .get(&atom)
                .is_some_and(|token| state.names.get(&source) == Some(token))
                && recoverable(program, source, state, tokens)
        }
        None => false,
    }
}
