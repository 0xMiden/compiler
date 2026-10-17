//! Source-declared identities, checked against executable publication before reuse.
use alloc::collections::{BTreeMap, BTreeSet};

use super::{
    error,
    selection::validate_atom,
    stack::State,
    tokens::{Token, Tokens},
    *,
};

/// Segment dominance relation, including each segment itself.
pub(super) type Dominators = BTreeMap<SegmentId, BTreeSet<SegmentId>>;

/// Compute graph dominance with the first selected segment as entry; graph IDs must be checked.
pub(super) fn dominators(program: &SelectedProgram) -> Dominators {
    let all: BTreeSet<_> = program.segments.iter().map(|segment| segment.id).collect();
    let mut result: Dominators = all.iter().map(|id| (*id, all.clone())).collect();
    let Some(entry) = program.segments.first() else {
        return result;
    };
    result.insert(entry.id, BTreeSet::from([entry.id]));
    loop {
        let previous = result.clone();
        for segment in program.segments.iter().skip(1) {
            let mut incoming = program
                .edges
                .iter()
                .filter(|edge| edge.destination == Some(segment.id))
                .map(|edge| &previous[&edge.source]);
            let mut common = incoming.next().cloned().unwrap_or_default();
            for predecessor in incoming {
                common.retain(|id| predecessor.contains(id));
            }
            common.insert(segment.id);
            result.insert(segment.id, common);
        }
        if result == previous {
            return result;
        }
    }
}

fn definition(
    program: &SelectedProgram,
    atom: Atom,
) -> Result<(SegmentId, usize), VerificationError> {
    for segment in &program.segments {
        if segment.parameters.contains(&atom.value) {
            return Ok((segment.id, 0));
        }
        for (index, unit) in segment.units.iter().enumerate() {
            if unit.outputs.contains(&atom) {
                return Ok((segment.id, index + 1));
            }
        }
    }
    Err(error("reconstruction references a value without a selected definition"))
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
                let (destination, position) = definition(program, Atom::new(id, lane))?;
                if let Reconstruction::Alias(sources) = reconstruction {
                    let source = sources[usize::from(lane)];
                    validate_atom(program, source)?;
                    let (origin, prior) = definition(program, source)?;
                    if !dom[&destination].contains(&origin)
                        || origin == destination
                            && (prior > position || prior == position && position != 0)
                    {
                        return Err(error(
                            "reconstruction alias source does not precede its definition",
                        ));
                    }
                    visit(program, source.value, visiting, done, dom)?;
                }
            }
        }
        visiting.remove(&id);
        done.insert(id);
        Ok(())
    }
    let dom = dominators(program);
    let mut done = BTreeSet::new();
    for id in program.values.keys() {
        visit(program, *id, &mut BTreeSet::new(), &mut done, &dom)?;
    }
    let use_at =
        |id: ValueId, segment: SegmentId, position: usize| -> Result<(), VerificationError> {
            if program.values[&id].reconstruction.is_some() {
                for lane in 0..program.values[&id].layout.lanes() {
                    let (owner, prior) = definition(program, Atom::new(id, lane))?;
                    if !dom[&segment].contains(&owner) || owner == segment && prior >= position {
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
                body,
                ..
            } => {
                controls.push(body.as_ref());
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
    names: &mut BTreeMap<Atom, Token>,
    tokens: &mut Tokens,
) -> Result<(), VerificationError> {
    fn resolve(
        program: &SelectedProgram,
        atom: Atom,
        segment: SegmentId,
        dom: &Dominators,
        names: &mut BTreeMap<Atom, Token>,
        tokens: &mut Tokens,
        done: &mut BTreeSet<Atom>,
    ) -> Result<Token, VerificationError> {
        if !done.insert(atom) {
            return Ok(names[&atom]);
        }
        let token = match &program.values[&atom.value].reconstruction {
            Some(Reconstruction::Literal(lanes)) => tokens.literal(lanes[usize::from(atom.lane)]),
            Some(Reconstruction::Alias(lanes)) => {
                let (owner, position) = definition(program, atom)?;
                if dom[&segment].contains(&owner) && (owner != segment || position == 0) {
                    resolve(
                        program,
                        lanes[usize::from(atom.lane)],
                        segment,
                        dom,
                        names,
                        tokens,
                        done,
                    )?
                } else {
                    names[&atom]
                }
            }
            None => names[&atom],
        };
        names.insert(atom, token);
        Ok(token)
    }
    let dom = dominators(program);
    let mut done = BTreeSet::new();
    for atom in names.keys().copied().collect::<alloc::vec::Vec<_>>() {
        resolve(program, atom, segment, &dom, names, tokens, &mut done)?;
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
