//! Derived from checked semantic uses, definitions, and simultaneous edge bindings.
use alloc::collections::{BTreeMap, BTreeSet};

use crate::plan::*;

/// Semantic lanes that must remain physically available or provably reconstructible.
pub(super) type Live = BTreeSet<Atom>;
/// Backward fixed point derived independently from the checked program, rather than
/// accepting the allocator's own liveness as proof of preservation.
pub(super) struct Liveness {
    /// Demand before any unit in each segment.
    pub entry: BTreeMap<SegmentId, Live>,
    /// Demand including this unit's semantic, stack, and stored inputs.
    pub before: BTreeMap<SelectedStepId, Live>,
    /// Demand after publication of this unit's outputs.
    pub after: BTreeMap<SelectedStepId, Live>,
    /// Successor demand translated through simultaneous bindings, plus control/return uses.
    pub exit: BTreeMap<SegmentId, Live>,
}
impl Liveness {
    /// Solve lane demand across all selected edges; requires a structurally checked program.
    pub fn derive(program: &SelectedProgram) -> Self {
        let mut result = Self {
            entry: BTreeMap::new(),
            before: BTreeMap::new(),
            after: BTreeMap::new(),
            exit: BTreeMap::new(),
        };
        fn conditions(
            tree: &ControlTree,
            program: &SelectedProgram,
            out: &mut BTreeMap<SegmentId, Live>,
        ) {
            match tree {
                ControlTree::If {
                    predicate,
                    on_true,
                    then_body,
                    else_body,
                    ..
                } => {
                    if let Some(edge) = program.edges.iter().find(|edge| edge.id == *on_true) {
                        out.entry(edge.source).or_default().insert(*predicate);
                    }
                    conditions(then_body, program, out);
                    conditions(else_body, program, out);
                }
                ControlTree::Sequence(children) => {
                    for child in children {
                        conditions(child, program, out);
                    }
                }
                ControlTree::Loop {
                    header,
                    predicate,
                    body,
                    ..
                } => {
                    out.entry(*header).or_default().insert(*predicate);
                    conditions(body, program, out);
                }
                _ => {}
            }
        }
        let mut predicates = BTreeMap::new();
        conditions(&program.structure, program, &mut predicates);
        loop {
            let previous = result.entry.clone();
            for segment in program.segments.iter().rev() {
                let mut live = predicates.get(&segment.id).cloned().unwrap_or_default();
                for edge in program.edges.iter().filter(|edge| edge.source == segment.id) {
                    if let Some(destination) = edge.destination {
                        for atom in previous.get(&destination).into_iter().flatten() {
                            let source = edge
                                .bindings
                                .iter()
                                .find(|binding| binding.destination == atom.value)
                                .map_or(atom.value, |binding| binding.source);
                            live.insert(Atom::new(source, atom.lane));
                        }
                    } else if edge.kind == EdgeKind::Return {
                        for binding in &edge.bindings {
                            add_value(&mut live, program, binding.source);
                        }
                    }
                }
                result.exit.insert(segment.id, live.clone());
                for unit in segment.units.iter().rev() {
                    result.after.insert(unit.id, live.clone());
                    for atom in &unit.outputs {
                        live.remove(atom);
                    }
                    live.extend(unit.stack_inputs.iter().copied());
                    for value in &unit.semantic_inputs {
                        add_value(&mut live, program, *value);
                    }
                    for input in &unit.stored_inputs {
                        if let StoredValue::Atom(atom) = input.value {
                            live.insert(atom);
                        }
                    }
                    result.before.insert(unit.id, live.clone());
                }
                result.entry.insert(segment.id, live);
            }
            if result.entry == previous {
                break;
            }
        }
        result
    }
}
fn add_value(live: &mut Live, program: &SelectedProgram, value: ValueId) {
    if let Some(value) = program.values.get(&value) {
        live.extend((0..value.layout.lanes()).map(|lane| Atom::new(value.id, lane)));
    }
}
