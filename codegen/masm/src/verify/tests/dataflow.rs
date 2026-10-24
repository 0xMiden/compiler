//! Transfer-analysis tests deliberately construct graphs directly. These test the fixed-point
//! equations; source checking and executable control-tree validation have separate tests.
use alloc::collections::BTreeSet;

use super::*;
use crate::verify::{liveness::Liveness, storage::StorageDemand};

mod reference_live {
    // Original snapshot solver: deliberately independent of the production worklist.
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
}

mod reference_storage {
    // Original snapshot solver: deliberately independent of the production worklist.
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
                        demand.extend(unit.scratch_effects.iter().filter_map(
                            |effect| match effect {
                                ScratchEffect::Read(fact) => Some(*fact),
                                _ => None,
                            },
                        ));
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
}

fn assert_reference(program: &SelectedProgram) {
    let actual = Liveness::derive(program);
    let expected = reference_live::Liveness::derive(program);
    assert_eq!(actual.entry, expected.entry, "live entry");
    assert_eq!(actual.before, expected.before, "live before");
    assert_eq!(actual.after, expected.after, "live after");
    assert_eq!(actual.exit, expected.exit, "live exit");
    let actual = StorageDemand::derive(program);
    let expected = reference_storage::StorageDemand::derive(program);
    assert_eq!(actual.entry, expected.entry, "storage entry");
    assert_eq!(actual.before, expected.before, "storage before");
    assert_eq!(actual.after, expected.after, "storage after");
    assert_eq!(actual.exit, expected.exit, "storage exit");
}

fn blank_unit(index: u32) -> SelectedLowering {
    let mut unit =
        Fixture::new(0, 0).instantiate(&[]).unwrap().program.segments[0].units[0].clone();
    unit.id = SelectedStepId(index);
    unit.source = SourceStepId(index);
    unit
}

fn segment(id: u32, unit: SelectedLowering) -> SelectedSegment {
    SelectedSegment {
        id: SegmentId(id),
        parameters: vec![],
        content_parameters: vec![],
        units: vec![unit],
    }
}

fn edge(id: u32, source: u32, destination: Option<u32>) -> TransferEdge {
    TransferEdge {
        id: EdgeId(id),
        source: SegmentId(source),
        destination: destination.map(SegmentId),
        kind: if destination.is_some() {
            EdgeKind::Forward
        } else {
            EdgeKind::Return
        },
        bindings: vec![],
        content_bindings: vec![],
    }
}

fn fact(offset: u32, content: ContentRequirement) -> ScratchCellFact {
    ScratchCellFact {
        object: ScratchId(3),
        offset,
        content,
    }
}

#[test]
fn sparse_cycles_parallel_edges_and_simultaneous_bindings_match_snapshot() {
    let mut program = Fixture::new(3, 0).instantiate(&[]).unwrap().program;
    let a = Atom::new(ValueId(0), 0);
    let b = Atom::new(ValueId(1), 0);
    let mut reader = blank_unit(99);
    reader.stack_inputs = vec![a];
    reader.stored_inputs = vec![StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(b),
    }];
    reader.scratch_effects = vec![
        ScratchEffect::Read(fact(0, ContentRequirement::Content(ContentId(1)))),
        ScratchEffect::Read(fact(1, ContentRequirement::Atom(a))),
    ];
    program.segments =
        vec![segment(83, blank_unit(1)), segment(7, reader), segment(901, blank_unit(2))];
    let mut renamed = edge(400, 83, Some(7));
    renamed.bindings = vec![
        ParameterBinding {
            destination: a.value,
            source: b.value,
        },
        ParameterBinding {
            destination: b.value,
            source: a.value,
        },
    ];
    renamed.content_bindings = vec![
        ContentBinding {
            destination: ContentId(1),
            source: ContentId(2),
        },
        ContentBinding {
            destination: ContentId(2),
            source: ContentId(1),
        },
    ];
    let mut self_edge = renamed.clone();
    self_edge.id = EdgeId(999);
    self_edge.source = SegmentId(7);
    program.edges = vec![
        edge(52, 7, Some(901)),
        renamed,
        self_edge,
        edge(65, 83, Some(7)),
        edge(79, 901, Some(83)),
    ];
    program.structure = ControlTree::Sequence(vec![]);
    assert_reference(&program);
    program.segments.reverse();
    program.edges.reverse();
    assert_reference(&program);
}

#[test]
fn killed_entry_still_refreshes_exit_and_unit_after() {
    let mut program = Fixture::new(1, 0).instantiate(&[]).unwrap().program;
    let a = Atom::new(ValueId(0), 0);
    let required = fact(0, ContentRequirement::Initialized);
    let mut writer = blank_unit(0);
    writer.outputs = vec![a];
    writer.scratch_effects = vec![ScratchEffect::Write(required)];
    let mut reader = blank_unit(1);
    reader.stack_inputs = vec![a];
    reader.scratch_effects = vec![ScratchEffect::Read(required)];
    // Reverse execution order forces the producer to run before its successor initially.
    program.segments = vec![segment(8, reader), segment(70, writer)];
    program.edges = vec![edge(44, 70, Some(8)), edge(91, 8, None)];
    program.structure = ControlTree::Sequence(vec![]);
    assert_reference(&program);
    let live = Liveness::derive(&program);
    assert!(live.entry[&SegmentId(70)].is_empty());
    assert!(live.exit[&SegmentId(70)].contains(&a));
    assert!(live.after[&SelectedStepId(0)].contains(&a));
    let storage = StorageDemand::derive(&program);
    assert!(storage.entry[&SegmentId(70)].is_empty());
    assert!(storage.exit[&SegmentId(70)].contains(&required));
    assert!(storage.after[&SelectedStepId(0)].contains(&required));
}

#[test]
fn scratch_possible_and_definite_borrow_writes_match_snapshot() {
    for definite in [false, true] {
        let mut program = Fixture::new(0, 0).instantiate(&[]).unwrap().program;
        let mut writer = blank_unit(0);
        writer.scratch_effects = vec![
            ScratchEffect::PossibleWrite {
                object: ScratchId(3),
                offset: 0,
                size: 3,
            },
            ScratchEffect::Borrow {
                object: ScratchId(3),
                offset: 1,
                size: 1,
                read: false,
                write: true,
                definite_write: definite,
                retained: false,
            },
        ];
        let mut reader = blank_unit(1);
        reader.scratch_effects = (0..3)
            .map(|offset| ScratchEffect::Read(fact(offset, ContentRequirement::Initialized)))
            .collect();
        program.segments = vec![segment(10, writer), segment(20, reader)];
        program.edges = vec![edge(1, 10, Some(20)), edge(2, 20, None)];
        program.structure = ControlTree::Sequence(vec![]);
        assert_reference(&program);
        let demand = StorageDemand::derive(&program);
        assert_eq!(demand.entry[&SegmentId(10)].len(), if definite { 2 } else { 3 });
    }
}

#[test]
fn control_return_semantic_and_stored_inputs_include_every_lane() {
    let mut program = Fixture::new(3, 0).instantiate(&[]).unwrap().program;
    program.values.get_mut(&ValueId(0)).unwrap().layout = ValueLayout::new(Type::U64).unwrap();
    let mut unit = blank_unit(0);
    unit.semantic_inputs = vec![ValueId(0)];
    unit.stored_inputs = vec![
        StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Atom(Atom::new(ValueId(1), 0)),
        },
        StoredInput {
            id: StoredInputId(1),
            value: StoredValue::Literal(Felt::ONE),
        },
    ];
    program.segments = vec![segment(80, unit), segment(4, blank_unit(1))];
    let mut returned = edge(900, 4, None);
    returned.bindings = vec![ParameterBinding {
        destination: ValueId(0),
        source: ValueId(0),
    }];
    program.edges = vec![edge(500, 80, Some(4)), edge(501, 80, Some(4)), returned];
    program.structure = ControlTree::If {
        id: ControlId(0),
        predicate: Atom::new(ValueId(2), 0),
        on_true: EdgeId(500),
        on_false: EdgeId(501),
        then_body: alloc::boxed::Box::new(ControlTree::Sequence(vec![])),
        else_body: alloc::boxed::Box::new(ControlTree::Sequence(vec![])),
    };
    assert_reference(&program);
    let live = Liveness::derive(&program);
    assert!(live.exit[&SegmentId(80)].contains(&Atom::new(ValueId(2), 0)));
    assert!(live.before[&SelectedStepId(0)].contains(&Atom::new(ValueId(1), 0)));
    for lane in 0..program.values[&ValueId(0)].layout.lanes() {
        assert!(live.exit[&SegmentId(4)].contains(&Atom::new(ValueId(0), lane)));
        assert!(live.before[&SelectedStepId(0)].contains(&Atom::new(ValueId(0), lane)));
    }
}

fn chain(count: u32) -> SelectedProgram {
    let mut program = Fixture::new(1, 0).instantiate(&[]).unwrap().program;
    program.segments = (0..count).map(|i| segment(i * 3 + 7, blank_unit(i))).collect();
    program.edges = (0..count)
        .map(|i| edge(i * 5 + 11, i * 3 + 7, (i + 1 < count).then_some((i + 1) * 3 + 7)))
        .collect();
    program.segments.last_mut().unwrap().units[0].semantic_inputs = vec![ValueId(0)];
    program.segments.last_mut().unwrap().units[0].scratch_effects =
        vec![ScratchEffect::Read(fact(0, ContentRequirement::Initialized))];
    program.structure = ControlTree::Sequence(vec![]);
    program
}

#[test]
fn long_chain_matches_snapshot_with_descriptive_timing() {
    let count = 128;
    let program = chain(count);
    let start = std::time::Instant::now();
    let live = Liveness::derive(&program);
    let storage = StorageDemand::derive(&program);
    let worklist_elapsed = start.elapsed();
    let start = std::time::Instant::now();
    let expected_live = reference_live::Liveness::derive(&program);
    let expected_storage = reference_storage::StorageDemand::derive(&program);
    let snapshot_elapsed = start.elapsed();
    std::eprintln!(
        "{count}-segment chain: production {worklist_elapsed:?}, snapshot {snapshot_elapsed:?}"
    );
    assert_eq!(live.entry, expected_live.entry);
    assert_eq!(live.before, expected_live.before);
    assert_eq!(live.after, expected_live.after);
    assert_eq!(live.exit, expected_live.exit);
    assert_eq!(storage.entry, expected_storage.entry);
    assert_eq!(storage.before, expected_storage.before);
    assert_eq!(storage.after, expected_storage.after);
    assert_eq!(storage.exit, expected_storage.exit);
}

#[test]
fn thousand_segment_demands_reach_the_entry() {
    let program = chain(1024);
    let start = std::time::Instant::now();
    let live = Liveness::derive(&program);
    let storage = StorageDemand::derive(&program);
    std::eprintln!("1024-segment semantic and scratch demand: {:?}", start.elapsed());
    for segment in &program.segments {
        assert_eq!(live.entry[&segment.id], BTreeSet::from([Fixture::input(0)]));
        assert_eq!(
            storage.entry[&segment.id],
            BTreeSet::from([fact(0, ContentRequirement::Initialized)])
        );
    }
}

/// A Boolean backward transfer isolates scheduling cost from demand-set size.
fn propagation_visits(program: &SelectedProgram) -> Vec<usize> {
    let indices: BTreeMap<_, _> =
        program.segments.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    let mut facts = vec![false; program.segments.len()];
    let mut work = super::super::dataflow::BackwardWorklist::new(program);
    let mut visits = vec![];
    while let Some(index) = work.next() {
        visits.push(index);
        let next = work
            .outgoing(index)
            .iter()
            .any(|edge| program.edges[*edge].destination.is_none_or(|to| facts[indices[&to]]));
        if facts[index] != next {
            facts[index] = next;
            work.changed(index);
        }
    }
    assert!(facts.iter().all(|live| *live));
    visits
}

#[test]
fn backward_queue_visits_chains_linearly_and_deduplicates_parallel_edges() {
    let mut program = chain(1024);
    assert_eq!(propagation_visits(&program).len(), 1024);
    program.segments.reverse();
    assert_eq!(propagation_visits(&program).len(), 2047);
    let mut program = chain(2);
    program.edges.push(edge(999, 7, Some(10)));
    program.edges.push(edge(1000, 10, Some(10)));
    assert_eq!(propagation_visits(&program), vec![1, 0, 1]);
}
