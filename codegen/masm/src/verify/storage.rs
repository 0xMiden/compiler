//! Scratch demand is a physical-cell obligation, separate from ordinary operand demand.
use alloc::collections::{BTreeMap, BTreeSet};

use super::*;

/// Scratch-cell facts that future execution requires to remain valid.
pub(super) type Demand = BTreeSet<ScratchCellFact>;

/// Backward scratch-content fixed point, computed independently of ordinary value liveness.
/// A definite write kills incoming cell demand; a possible write does not establish contents.
pub(super) struct StorageDemand {
    /// Facts required before the segment executes.
    pub entry: BTreeMap<SegmentId, Demand>,
    /// Facts required before a unit, including its declared scratch reads.
    pub before: BTreeMap<SelectedStepId, Demand>,
    /// Facts that normal unit completion must preserve or establish.
    pub after: BTreeMap<SelectedStepId, Demand>,
    /// Successor facts translated to predecessor identities.
    pub exit: BTreeMap<SegmentId, Demand>,
}

/// Translate only the content identity through an edge's simultaneous bindings; the
/// scratch object and its cell offset remain unchanged.
pub(super) fn source_fact(mut fact: ScratchCellFact, edge: &TransferEdge) -> ScratchCellFact {
    fact.content = match fact.content {
        ContentRequirement::Content(id) => ContentRequirement::Content(
            edge.content_bindings
                .iter()
                .find(|binding| binding.destination == id)
                .map_or(id, |binding| binding.source),
        ),
        ContentRequirement::Atom(atom) => ContentRequirement::Atom(Atom::new(
            edge.bindings
                .iter()
                .find(|binding| binding.destination == atom.value)
                .map_or(atom.value, |binding| binding.source),
            atom.lane,
        )),
        ContentRequirement::Initialized => ContentRequirement::Initialized,
    };
    fact
}

impl StorageDemand {
    /// Solve required contents over the checked graph, accounting for definite writes/borrows.
    pub fn derive(program: &SelectedProgram) -> Self {
        let mut result = Self {
            entry: BTreeMap::new(),
            before: BTreeMap::new(),
            after: BTreeMap::new(),
            exit: BTreeMap::new(),
        };
        loop {
            let previous = result.entry.clone();
            for segment in program.segments.iter().rev() {
                let mut demand = Demand::new();
                for edge in program.edges.iter().filter(|edge| edge.source == segment.id) {
                    if let Some(destination) = edge.destination {
                        demand.extend(
                            previous
                                .get(&destination)
                                .into_iter()
                                .flatten()
                                .map(|fact| source_fact(*fact, edge)),
                        );
                    }
                }
                result.exit.insert(segment.id, demand.clone());
                for unit in segment.units.iter().rev() {
                    result.after.insert(unit.id, demand.clone());
                    // A definite cell definition ends its incoming dependency, regardless
                    // of the outgoing name. Replay must still prove every after-demand.
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
                                    && (*offset..offset + size).contains(&fact.offset)
                            }
                            _ => false,
                        })
                    });
                    demand.extend(unit.scratch_effects.iter().filter_map(|effect| match effect {
                        ScratchEffect::Read(fact) => Some(*fact),
                        _ => None,
                    }));
                    result.before.insert(unit.id, demand.clone());
                }
                result.entry.insert(segment.id, demand);
            }
            if result.entry == previous {
                return result;
            }
        }
    }
}

/// Check unique, dominating content definitions and complete content-parameter bindings.
/// Procedure entry cannot assume compiler scratch has already been initialized.
pub(super) fn validate(program: &SelectedProgram) -> Result<(), VerificationError> {
    let dom = super::reconstruction::dominators(program);
    let mut definitions = BTreeMap::new();
    for segment in &program.segments {
        for parameter in &segment.content_parameters {
            if definitions.insert(*parameter, (segment.id, 0)).is_some() {
                return Err(error("duplicate content definition or parameter"));
            }
        }
        for (index, unit) in segment.units.iter().enumerate() {
            for effect in &unit.scratch_effects {
                if let ScratchEffect::Write(ScratchCellFact {
                    content: ContentRequirement::Content(id),
                    ..
                }) = effect
                    && definitions.insert(*id, (segment.id, index + 1)).is_some()
                {
                    return Err(error("duplicate content definition or parameter"));
                }
            }
        }
    }
    let available = |id: ContentId, segment: SegmentId, position: usize| {
        definitions.get(&id).is_some_and(|(owner, prior)| {
            dom[&segment].contains(owner) && (*owner != segment || *prior < position)
        })
    };
    for segment in &program.segments {
        for (index, unit) in segment.units.iter().enumerate() {
            for effect in &unit.scratch_effects {
                if let ScratchEffect::Read(ScratchCellFact {
                    content: ContentRequirement::Content(id),
                    ..
                }) = effect
                    && !available(*id, segment.id, index + 1)
                {
                    return Err(error("scratch read lacks a dominating content definition"));
                }
            }
        }
    }
    for edge in &program.edges {
        let mut destinations = BTreeSet::new();
        let target = edge
            .destination
            .and_then(|id| program.segments.iter().find(|segment| segment.id == id));
        for binding in &edge.content_bindings {
            if !destinations.insert(binding.destination)
                || target
                    .is_none_or(|target| !target.content_parameters.contains(&binding.destination))
                || !available(binding.source, edge.source, usize::MAX)
            {
                return Err(error("invalid, duplicate or undefined content binding"));
            }
        }
        if target.is_some_and(|target| destinations.len() != target.content_parameters.len()) {
            return Err(error("omitted content parameter binding"));
        }
    }
    if program
        .segments
        .first()
        .is_some_and(|entry| !entry.content_parameters.is_empty())
    {
        return Err(error("procedure entry cannot assume compiler scratch content"));
    }
    Ok(())
}
