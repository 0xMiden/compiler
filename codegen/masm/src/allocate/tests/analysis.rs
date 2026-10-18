use alloc::{boxed::Box, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::Type;

use crate::{allocate::analysis::*, plan::*, select::graph::GraphBuilder, verify::*};

const DEFINITION: GeneratedDefinitionId = GeneratedDefinitionId(100);

struct Definition(fn() -> SelectedProgram);
impl GeneratedDefinition for Definition {
    fn id(&self) -> GeneratedDefinitionId {
        DEFINITION
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        assert!(parameters.is_empty());
        let program = self.0();
        let units = program
            .segments
            .iter()
            .flat_map(|segment| &segment.units)
            .map(|unit| (unit.id, UnitEvidence::default()))
            .collect();
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            program,
            units,
        })
    }
}

fn checked(build: fn() -> SelectedProgram) -> CheckedSelectedProgram {
    check_selection(build(), &SourceContext::Generated(&Definition(build))).unwrap()
}

fn builder() -> GraphBuilder {
    GraphBuilder::new(
        ProcedureOrigin::Generated {
            definition: DEFINITION,
            parameters: vec![],
        },
        ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
    )
}
fn atom(id: u32) -> Atom {
    Atom::new(ValueId(id), 0)
}
fn value(id: u32, owner: u32, index: u32) -> SelectedValue {
    SelectedValue {
        id: ValueId(id),
        origin: ValueOrigin::Parameter {
            segment: SegmentId(owner),
            index,
        },
        layout: ValueLayout::new(Type::Felt).unwrap(),
        reconstruction: None,
    }
}
fn source(step: u32) -> SourceRecord {
    SourceRecord {
        origin: SourceOrigin::Generated {
            definition: DEFINITION,
            stage: step,
        },
        location: SourceLocation::default(),
    }
}
fn unit(id: u32, uses: &[u32], outputs: &[u32]) -> SelectedLowering {
    SelectedLowering {
        id: SelectedStepId(id),
        kind: LoweringKind(0),
        source: SourceStepId(id),
        semantic_inputs: uses.iter().copied().map(ValueId).collect(),
        stack_inputs: vec![],
        stored_inputs: vec![],
        outputs: outputs.iter().copied().map(atom).collect(),
        effects: LoweringEffects::default(),
        scratch: vec![],
        scratch_effects: vec![],
    }
}
fn segment(id: u32, parameters: &[u32], units: Vec<SelectedLowering>) -> SelectedSegment {
    SelectedSegment {
        id: SegmentId(id),
        parameters: parameters.iter().copied().map(ValueId).collect(),
        content_parameters: vec![],
        units,
    }
}
fn edge(
    id: u32,
    from: u32,
    to: Option<u32>,
    kind: EdgeKind,
    bindings: &[(u32, u32)],
) -> TransferEdge {
    TransferEdge {
        id: EdgeId(id),
        source: SegmentId(from),
        destination: to.map(SegmentId),
        kind,
        bindings: bindings
            .iter()
            .map(|(dst, src)| ParameterBinding {
                destination: ValueId(*dst),
                source: ValueId(*src),
            })
            .collect(),
        content_bindings: vec![],
    }
}
fn seq(children: Vec<ControlTree>) -> ControlTree {
    ControlTree::Sequence(children)
}
fn s(id: u32) -> ControlTree {
    ControlTree::Segment(SegmentId(id))
}
fn e(id: u32) -> ControlTree {
    ControlTree::Edge(EdgeId(id))
}
fn branch(
    id: u32,
    predicate: u32,
    yes: u32,
    a: ControlTree,
    no: u32,
    b: ControlTree,
) -> ControlTree {
    ControlTree::If {
        id: ControlId(id),
        predicate: atom(predicate),
        on_true: EdgeId(yes),
        then_body: Box::new(a),
        on_false: EdgeId(no),
        else_body: Box::new(b),
    }
}
fn ids(set: &LiveValues) -> Vec<u32> {
    set.iter().map(|id| id.0).collect()
}

// A blanket edge-operand use would retain dead v12; sequential renaming would lose v11.
fn edge_program() -> SelectedProgram {
    let mut graph = builder();
    for (id, owner, index) in
        [(1, 0, 0), (2, 0, 1), (3, 0, 2), (4, 0, 3), (11, 1, 0), (12, 1, 1), (13, 1, 2)]
    {
        graph.add_value(value(id, owner, index)).unwrap();
    }
    for id in [20, 21] {
        let mut v = value(id, 0, 0);
        v.origin = ValueOrigin::Temporary {
            step: SourceStepId(id - 20),
            index: 0,
        };
        graph.add_value(v).unwrap();
    }
    graph.add_source(SourceStepId(0), source(0)).unwrap();
    graph.add_source(SourceStepId(1), source(1)).unwrap();
    // Debug-only source coverage does not make dead v4 a semantic use.
    graph.add_source(SourceStepId(99), source(99)).unwrap();
    graph.add_segment(segment(0, &[1, 2, 3, 4], vec![])).unwrap();
    let first = unit(0, &[11, 13], &[20]);
    let mut second = unit(1, &[20], &[21]);
    second.stored_inputs.push(StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(atom(3)),
    });
    graph.add_segment(segment(1, &[11, 12, 13], vec![first, second])).unwrap();
    graph.add_segment(segment(2, &[], vec![])).unwrap();
    graph
        .add_edge(edge(0, 0, Some(1), EdgeKind::True, &[(11, 2), (12, 4), (13, 2)]))
        .unwrap();
    graph.add_edge(edge(1, 0, Some(2), EdgeKind::False, &[])).unwrap();
    graph.add_edge(edge(2, 1, None, EdgeKind::Return, &[(21, 21)])).unwrap();
    graph.add_edge(edge(3, 2, None, EdgeKind::Trap, &[])).unwrap();
    graph
        .finish(seq(vec![
            s(0),
            branch(0, 1, 0, seq(vec![s(1), e(2)]), 1, seq(vec![s(2), e(3)])),
        ]))
        .unwrap()
}

#[test]
fn edge_liveness_uses_live_destination_bindings() {
    let analysis = analyze(&checked(edge_program));
    assert_eq!(ids(&analysis.edge_live[&EdgeId(0)]), [2, 3]);
    assert_eq!(ids(&analysis.live_in[&SegmentId(0)]), [1, 2, 3]);
    assert_eq!(ids(&analysis.live_in[&SegmentId(1)]), [3, 11, 13]);
    assert_eq!(ids(&analysis.before[&SelectedStepId(0)]), [3, 11, 13]);
    assert_eq!(ids(&analysis.after[&SelectedStepId(0)]), [3, 20]);
    assert_eq!(ids(&analysis.after[&SelectedStepId(1)]), [21]);
    assert!(!analysis.live_in[&SegmentId(0)].contains(ValueId(4)));
    assert_eq!(analysis.reachable.iter().map(|id| id.0).collect::<Vec<_>>(), [0, 1, 2]);
}

#[test]
fn next_use_counts_steps_substitutes_edges_and_preserves_infinity() {
    let analysis = analyze(&checked(edge_program));
    assert_eq!(
        analysis.next_use.entry[&SegmentId(0)].get(ValueId(1)),
        NextUseDistance::Finite(0)
    );
    assert_eq!(analysis.next_use.edge[&EdgeId(0)].get(ValueId(2)), NextUseDistance::Finite(0));
    assert_eq!(
        analysis.next_use.before[&SelectedStepId(0)].get(ValueId(3)),
        NextUseDistance::Finite(1)
    );
    assert_eq!(
        analysis.next_use.before[&SelectedStepId(1)].get(ValueId(3)),
        NextUseDistance::Finite(0)
    );
    assert_eq!(
        analysis.next_use.entry[&SegmentId(0)].get(ValueId(4)),
        NextUseDistance::Infinite
    );
    assert_eq!(
        NextUseDistance::Finite(u32::MAX - 1).advance(7),
        NextUseDistance::Finite(u32::MAX)
    );
    assert_eq!(NextUseDistance::Infinite.advance(7), NextUseDistance::Infinite);
    assert!(NextUseDistance::Finite(u32::MAX) < NextUseDistance::Infinite);
    assert!(
        analysis.next_use.entry[&SegmentId(0)].priority(atom(2))
            < analysis.next_use.entry[&SegmentId(0)].priority(atom(3))
    );
}

fn wide_loop_program() -> SelectedProgram {
    let mut graph = builder();
    for (owner, start, count) in [(0, 1000, 18), (1, 10, 18), (2, 100, 20), (3, 200, 20)] {
        for index in 0..count {
            graph.add_value(value(start + index, owner, index)).unwrap();
        }
    }
    for (index, id) in [30, 31].into_iter().enumerate() {
        graph.add_value(value(id, 0, 18 + index as u32)).unwrap();
    }
    for (id, step) in [(300, 0), (400, 1)] {
        let mut output = value(id, 0, 0);
        output.origin = ValueOrigin::Temporary {
            step: SourceStepId(step),
            index: 0,
        };
        graph.add_value(output).unwrap();
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    let entry = (1000..1018).chain([30, 31]).collect::<Vec<_>>();
    graph.add_segment(segment(0, &entry, vec![])).unwrap();
    graph.add_segment(segment(1, &(10..28).collect::<Vec<_>>(), vec![])).unwrap();
    graph
        .add_segment(segment(2, &(100..120).collect::<Vec<_>>(), vec![unit(0, &[110, 30], &[300])]))
        .unwrap();
    graph
        .add_segment(segment(3, &(200..220).collect::<Vec<_>>(), vec![unit(1, &[200, 31], &[400])]))
        .unwrap();
    graph
        .add_edge(edge(
            0,
            0,
            Some(1),
            EdgeKind::Forward,
            &(0..18).map(|i| (10 + i, 1000 + i)).collect::<Vec<_>>(),
        ))
        .unwrap();
    graph
        .add_edge(edge(
            1,
            1,
            Some(2),
            EdgeKind::True,
            &(0..20).map(|i| (100 + i, 10 + i % 18)).collect::<Vec<_>>(),
        ))
        .unwrap();
    graph
        .add_edge(edge(
            2,
            1,
            Some(3),
            EdgeKind::False,
            &(0..20).map(|i| (200 + i, 10 + i % 18)).collect::<Vec<_>>(),
        ))
        .unwrap();
    graph
        .add_edge(edge(
            3,
            2,
            Some(1),
            EdgeKind::Backedge,
            &(0..18)
                .map(|i| (10 + i, if i == 0 { 300 } else { 100 + i }))
                .collect::<Vec<_>>(),
        ))
        .unwrap();
    graph.add_edge(edge(4, 3, None, EdgeKind::Return, &[(400, 400)])).unwrap();
    graph
        .finish(seq(vec![
            s(0),
            e(0),
            ControlTree::Loop {
                id: ControlId(0),
                condition: ControlId(1),
                header: SegmentId(1),
                predicate: atom(10),
                on_true: EdgeId(1),
                on_false: EdgeId(2),
                body: Box::new(seq(vec![s(2), e(3)])),
                backedges: vec![EdgeId(3)],
            },
            s(3),
            e(4),
        ]))
        .unwrap()
}

#[test]
fn independent_wide_while_groups_have_zero_trip_and_cyclic_demand() {
    let analysis = analyze(&checked(wide_loop_program));
    assert_eq!(ids(&analysis.live_in[&SegmentId(1)]), [10, 20, 30, 31]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(1)]), [20, 30, 31]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(2)]), [10, 31]);
    assert_eq!(ids(&analysis.before[&SelectedStepId(0)]), [30, 31, 110]);
    assert_eq!(analysis.sccs[&SegmentId(1)], [SegmentId(1), SegmentId(2)]);
    assert_eq!(analysis.scc_membership[&SegmentId(2)], SegmentId(1));
    let loop_uses = &analysis.loops[&ControlId(0)].next_use.entry[&SegmentId(1)];
    assert_eq!(loop_uses.get(ValueId(30)), NextUseDistance::Finite(0));
    assert_eq!(loop_uses.get(ValueId(31)), NextUseDistance::Infinite);
    assert_eq!(
        analysis.next_use.entry[&SegmentId(1)].get(ValueId(31)),
        NextUseDistance::Finite(0)
    );
}

fn nested_program() -> SelectedProgram {
    let mut graph = builder();
    for id in 0..5 {
        graph.add_value(value(id, 0, id)).unwrap();
    }
    for step in 0..3 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    for id in 0..6 {
        let units = match id {
            3 => vec![unit(0, &[2], &[])],
            4 => vec![unit(1, &[3], &[])],
            5 => vec![unit(2, &[4], &[])],
            _ => vec![],
        };
        graph
            .add_segment(segment(id, if id == 0 { &[0, 1, 2, 3, 4] } else { &[] }, units))
            .unwrap();
    }
    for transfer in [
        edge(0, 0, Some(1), EdgeKind::Forward, &[]),
        edge(1, 1, Some(2), EdgeKind::True, &[]),
        edge(2, 1, Some(5), EdgeKind::False, &[]),
        edge(3, 2, Some(3), EdgeKind::True, &[]),
        edge(4, 2, Some(4), EdgeKind::False, &[]),
        edge(5, 3, Some(2), EdgeKind::Backedge, &[]),
        edge(6, 4, Some(1), EdgeKind::Backedge, &[]),
        edge(7, 5, None, EdgeKind::Return, &[]),
    ] {
        graph.add_edge(transfer).unwrap();
    }
    graph
        .finish(seq(vec![
            s(0),
            e(0),
            ControlTree::Loop {
                id: ControlId(0),
                condition: ControlId(10),
                header: SegmentId(1),
                predicate: atom(0),
                on_true: EdgeId(1),
                on_false: EdgeId(2),
                body: Box::new(seq(vec![
                    ControlTree::Loop {
                        id: ControlId(1),
                        condition: ControlId(11),
                        header: SegmentId(2),
                        predicate: atom(1),
                        on_true: EdgeId(3),
                        on_false: EdgeId(4),
                        body: Box::new(seq(vec![s(3), e(5)])),
                        backedges: vec![EdgeId(5)],
                    },
                    s(4),
                    e(6),
                ])),
                backedges: vec![EdgeId(6)],
            },
            s(5),
            e(7),
        ]))
        .unwrap()
}

#[test]
fn nested_loop_demand_distinguishes_inner_outer_and_exit_only_uses() {
    let analysis = analyze(&checked(nested_program));
    assert_eq!(
        analysis.sccs[&SegmentId(1)],
        [SegmentId(1), SegmentId(2), SegmentId(3), SegmentId(4)]
    );
    let outer = &analysis.loops[&ControlId(0)].next_use.entry[&SegmentId(1)];
    let inner = &analysis.loops[&ControlId(1)].next_use.entry[&SegmentId(2)];
    assert_eq!(outer.get(ValueId(2)), NextUseDistance::Finite(0));
    assert_eq!(outer.get(ValueId(3)), NextUseDistance::Finite(0));
    assert_eq!(outer.get(ValueId(4)), NextUseDistance::Infinite);
    assert_eq!(inner.get(ValueId(2)), NextUseDistance::Finite(0));
    assert_eq!(inner.get(ValueId(3)), NextUseDistance::Infinite);
}

#[test]
fn analysis_is_independent_of_segment_and_predecessor_traversal_order() {
    fn reordered() -> SelectedProgram {
        let mut program = wide_loop_program();
        program.segments[1..].reverse();
        program.edges.reverse();
        program
    }
    assert_eq!(analyze(&checked(wide_loop_program)), analyze(&checked(reordered)));
}

fn missing_else_program() -> SelectedProgram {
    let mut graph = builder();
    graph.add_value(value(0, 0, 0)).unwrap();
    graph.add_value(value(1, 0, 1)).unwrap();
    graph.add_source(SourceStepId(0), source(0)).unwrap();
    graph.add_segment(segment(0, &[0, 1], vec![])).unwrap();
    graph.add_segment(segment(1, &[], vec![unit(0, &[1], &[])])).unwrap();
    graph.add_segment(segment(2, &[], vec![])).unwrap();
    for transfer in [
        edge(0, 0, Some(1), EdgeKind::True, &[]),
        edge(1, 0, Some(2), EdgeKind::False, &[]),
        edge(2, 1, Some(2), EdgeKind::Forward, &[]),
        edge(3, 2, None, EdgeKind::Return, &[]),
    ] {
        graph.add_edge(transfer).unwrap();
    }
    graph
        .finish(seq(vec![
            s(0),
            branch(0, 0, 0, seq(vec![s(1), e(2)]), 1, seq(vec![])),
            s(2),
            e(3),
        ]))
        .unwrap()
}

#[test]
fn missing_else_identity_edge_does_not_invent_uses() {
    let analysis = analyze(&checked(missing_else_program));
    assert_eq!(ids(&analysis.live_in[&SegmentId(0)]), [0, 1]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(0)]), [1]);
    assert!(ids(&analysis.edge_live[&EdgeId(1)]).is_empty());
    assert_eq!(
        analysis.next_use.entry[&SegmentId(2)].get(ValueId(1)),
        NextUseDistance::Infinite
    );
}

fn switch_program() -> SelectedProgram {
    let mut graph = builder();
    // The source definition selects binary comparisons for cases 7 and 11.
    // All physical branch alternatives remain checked True/False edges.
    for id in 0..5 {
        graph.add_value(value(id, 0, id)).unwrap();
    }
    for id in 5..7 {
        let mut output = value(id, 0, 0);
        output.origin = ValueOrigin::Temporary {
            step: SourceStepId(id - 5),
            index: 0,
        };
        graph.add_value(output).unwrap();
    }
    for step in 0..5 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    for (id, params, units) in [
        (0, vec![0, 1, 2, 3, 4], vec![unit(0, &[0], &[5])]),
        (1, vec![], vec![unit(2, &[2], &[])]),
        (2, vec![], vec![unit(1, &[0], &[6])]),
        (3, vec![], vec![unit(3, &[3], &[])]),
        (4, vec![], vec![unit(4, &[4], &[])]),
    ] {
        graph.add_segment(segment(id, &params, units)).unwrap();
    }
    for transfer in [
        edge(0, 0, Some(1), EdgeKind::True, &[]),
        edge(1, 0, Some(2), EdgeKind::False, &[]),
        edge(2, 2, Some(3), EdgeKind::True, &[]),
        edge(3, 2, Some(4), EdgeKind::False, &[]),
        edge(4, 1, None, EdgeKind::Return, &[]),
        edge(5, 3, None, EdgeKind::Trap, &[]),
        edge(6, 4, None, EdgeKind::Return, &[]),
    ] {
        graph.add_edge(transfer).unwrap();
    }
    graph
        .finish(seq(vec![
            s(0),
            branch(
                0,
                5,
                0,
                seq(vec![s(1), e(4)]),
                1,
                seq(vec![s(2), branch(1, 6, 2, seq(vec![s(3), e(5)]), 3, seq(vec![s(4), e(6)]))]),
            ),
        ]))
        .unwrap()
}

#[test]
fn selected_switch_cases_default_and_terminal_dispatch_keep_all_alternatives() {
    let analysis = analyze(&checked(switch_program));
    assert_eq!(ids(&analysis.live_in[&SegmentId(0)]), [0, 2, 3, 4]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(0)]), [2]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(1)]), [0, 3, 4]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(2)]), [3]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(3)]), [4]);
    assert_eq!(
        analysis.next_use.before[&SelectedStepId(0)].get(ValueId(3)),
        NextUseDistance::Finite(2)
    );
    assert_eq!(
        analysis.next_use.before[&SelectedStepId(0)].get(ValueId(1)),
        NextUseDistance::Infinite
    );
}

fn omitted_dead_tail_program() -> SelectedProgram {
    let mut graph = builder();
    graph.add_source(SourceStepId(99), source(99)).unwrap();
    graph.add_segment(segment(0, &[], vec![])).unwrap();
    graph.add_edge(edge(0, 0, None, EdgeKind::Trap, &[])).unwrap();
    graph.finish(seq(vec![s(0), e(0)])).unwrap()
}

#[test]
fn registered_unreachable_tail_omission_retains_coverage_without_semantic_uses() {
    let program = checked(omitted_dead_tail_program);
    assert!(program.source_map.contains_key(&SourceStepId(99)));
    let analysis = analyze(&program);
    assert_eq!(analysis.reachable.iter().copied().collect::<Vec<_>>(), [SegmentId(0)]);
    assert!(ids(&analysis.live_in[&SegmentId(0)]).is_empty());
    let mut candidate = omitted_dead_tail_program();
    candidate.source_map.clear();
    assert!(
        check_selection(
            candidate,
            &SourceContext::Generated(&Definition(omitted_dead_tail_program))
        )
        .is_err()
    );
}

fn scratch_requirement(uses: &[u32]) -> ScratchRequirement {
    ScratchRequirement {
        id: ScratchId(0),
        size: 1,
        alignment: 1,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: uses.iter().copied().map(SelectedStepId).collect(),
    }
}
fn fact(content: ContentRequirement) -> ScratchCellFact {
    ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content,
    }
}
fn content(id: u32) -> ScratchCellFact {
    fact(ContentRequirement::Content(ContentId(id)))
}

fn content_diamond_program() -> SelectedProgram {
    let mut graph = builder();
    graph.add_value(value(0, 0, 0)).unwrap();
    for step in 0..3 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    graph.add_segment(segment(0, &[0], vec![])).unwrap();
    for (id, step) in [(1, 0), (2, 1), (3, 2)] {
        let mut work = unit(step, &[], &[]);
        work.scratch = vec![scratch_requirement(&[0, 1, 2])];
        work.scratch_effects = if step == 2 {
            vec![ScratchEffect::Read(content(2)), ScratchEffect::Read(content(3))]
        } else {
            vec![ScratchEffect::Write(content(step))]
        };
        let mut block = segment(id, &[], vec![work]);
        if id == 3 {
            block.content_parameters = vec![ContentId(2), ContentId(3)];
        }
        graph.add_segment(block).unwrap();
    }
    for transfer in [
        edge(0, 0, Some(1), EdgeKind::True, &[]),
        edge(1, 0, Some(2), EdgeKind::False, &[]),
        edge(4, 3, None, EdgeKind::Return, &[]),
    ] {
        graph.add_edge(transfer).unwrap();
    }
    for (id, source) in [(2, 1), (3, 2)] {
        let mut transfer = edge(id, source, Some(3), EdgeKind::Forward, &[]);
        transfer.content_bindings = [2, 3]
            .into_iter()
            .map(|destination| ContentBinding {
                destination: ContentId(destination),
                source: ContentId(source - 1),
            })
            .collect();
        graph.add_edge(transfer).unwrap();
    }
    graph
        .finish(seq(vec![
            s(0),
            branch(0, 0, 0, seq(vec![s(1), e(2)]), 1, seq(vec![s(2), e(3)])),
            s(3),
            e(4),
        ]))
        .unwrap()
}

#[test]
fn distinct_scratch_contents_substitute_simultaneously_without_ssa_operand_demand() {
    let analysis = analyze(&checked(content_diamond_program));
    assert_eq!(ids(&analysis.live_in[&SegmentId(0)]), [0]);
    assert!(ids(&analysis.live_in[&SegmentId(3)]).is_empty());
    assert_eq!(analysis.scratch.entry[&SegmentId(3)], vec![content(2), content(3)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(2)], vec![content(0)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(3)], vec![content(1)]);
    assert_eq!(analysis.scratch.before[&SelectedStepId(0)], vec![]);
    assert_eq!(analysis.scratch.after[&SelectedStepId(0)], vec![content(0)]);
    assert_eq!(analysis.scratch.entry[&SegmentId(0)], vec![]);
}

fn scratch_atom_program() -> SelectedProgram {
    let mut graph = builder();
    let mut output = value(10, 0, 0);
    output.origin = ValueOrigin::Temporary {
        step: SourceStepId(0),
        index: 0,
    };
    graph.add_value(output).unwrap();
    for step in 0..2 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    let mut produce = unit(0, &[], &[10]);
    produce.scratch = vec![scratch_requirement(&[0, 1])];
    produce.scratch_effects = vec![ScratchEffect::Write(content(0))];
    let mut consume = unit(1, &[], &[]);
    consume.scratch = produce.scratch.clone();
    consume.scratch_effects = vec![ScratchEffect::Read(fact(ContentRequirement::Atom(atom(10))))];
    graph.add_segment(segment(0, &[], vec![produce, consume])).unwrap();
    graph.add_edge(edge(0, 0, None, EdgeKind::Return, &[])).unwrap();
    graph.finish(seq(vec![s(0), e(0)])).unwrap()
}

#[test]
fn cell_definitions_kill_incoming_scratch_demand_but_keep_outgoing_identity() {
    let analysis = analyze(&checked(scratch_atom_program));
    assert_eq!(analysis.scratch.before[&SelectedStepId(0)], vec![]);
    assert_eq!(
        analysis.scratch.after[&SelectedStepId(0)],
        vec![fact(ContentRequirement::Atom(atom(10)))]
    );
    assert!(ids(&analysis.after[&SelectedStepId(0)]).is_empty());
    assert_eq!(
        analysis.next_use.after[&SelectedStepId(0)].get(ValueId(10)),
        NextUseDistance::Infinite
    );
}

#[test]
fn explicit_reads_and_possible_writes_preserve_incoming_scratch_requirements() {
    fn read_then_write() -> SelectedProgram {
        let mut program = scratch_atom_program();
        program.segments[0].units[0]
            .scratch_effects
            .insert(0, ScratchEffect::Read(fact(ContentRequirement::Initialized)));
        program
    }
    fn possible_write() -> SelectedProgram {
        let mut program = scratch_atom_program();
        program.segments[0].units[0].scratch_effects = vec![ScratchEffect::PossibleWrite {
            object: ScratchId(0),
            offset: 0,
            size: 1,
        }];
        program.segments[0].units[1].scratch_effects =
            vec![ScratchEffect::Read(fact(ContentRequirement::Initialized))];
        program
    }
    for build in [read_then_write as fn() -> SelectedProgram, possible_write] {
        let analysis = analyze(&checked(build));
        assert_eq!(
            analysis.scratch.before[&SelectedStepId(0)],
            vec![fact(ContentRequirement::Initialized)]
        );
    }
}

#[test]
fn definite_borrowed_writes_end_dependencies_while_internal_read_preparation_stays_local() {
    fn borrowed() -> SelectedProgram {
        let mut program = scratch_atom_program();
        program.segments[0].units[0].scratch_effects = vec![ScratchEffect::Borrow {
            object: ScratchId(0),
            offset: 0,
            size: 1,
            read: true,
            write: true,
            definite_write: true,
            retained: false,
        }];
        program
    }
    let analysis = analyze(&checked(borrowed));
    assert_eq!(analysis.scratch.before[&SelectedStepId(0)], vec![]);
    assert_eq!(
        analysis.scratch.after[&SelectedStepId(0)],
        vec![fact(ContentRequirement::Atom(atom(10)))]
    );
}

#[test]
fn graph_builder_rejects_duplicate_ids_and_preserves_structural_entry_order() {
    let mut graph = builder();
    graph.add_segment(segment(8, &[], vec![])).unwrap();
    graph.add_segment(segment(1, &[], vec![])).unwrap();
    assert!(graph.add_segment(segment(8, &[], vec![])).is_err());
    graph.add_edge(edge(0, 8, Some(1), EdgeKind::Forward, &[])).unwrap();
    assert!(graph.add_edge(edge(0, 8, Some(1), EdgeKind::Forward, &[])).is_err());
    graph.add_edge(edge(1, 1, None, EdgeKind::Return, &[])).unwrap();
    let program = graph.finish(seq(vec![s(8), e(0), s(1), e(1)])).unwrap();
    assert_eq!(
        program.segments.iter().map(|s| s.id).collect::<Vec<_>>(),
        [SegmentId(8), SegmentId(1)]
    );
}

#[test]
fn malformed_bindings_terminators_and_skeletons_fail_before_analysis() {
    struct Invalid(u8);
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            DEFINITION
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Definition(missing_else_program).instantiate(&[])?;
            match self.0 {
                0 => evidence.program.edges[0].source = SegmentId(88),
                1 => evidence.program.edges[0].bindings.push(ParameterBinding {
                    destination: ValueId(1),
                    source: ValueId(0),
                }),
                2 => evidence.program.edges.last_mut().unwrap().kind = EdgeKind::Forward,
                3 => evidence.program.structure = seq(vec![s(0), e(3)]),
                _ => unreachable!(),
            }
            Ok(evidence)
        }
    }
    for variant in 0..4 {
        let source = Invalid(variant);
        let candidate = source.instantiate(&[]).unwrap().program;
        assert!(
            check_selection(candidate, &SourceContext::Generated(&source)).is_err(),
            "variant {variant}"
        );
    }
}

// Publishing separate lanes of one semantic value from different units cannot be represented
// by value-level liveness. Reject that shape instead of treating the whole value as initialized.
fn phased_lane_program() -> SelectedProgram {
    let mut graph = builder();
    graph
        .add_value(SelectedValue {
            id: ValueId(10),
            origin: ValueOrigin::Temporary {
                step: SourceStepId(0),
                index: 0,
            },
            layout: ValueLayout::new(Type::U64).unwrap(),
            reconstruction: None,
        })
        .unwrap();
    for step in 0..3 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    let first = unit(0, &[], &[10]);
    let mut second = unit(1, &[], &[]);
    second.outputs = vec![Atom::new(ValueId(10), 1)];
    graph
        .add_segment(segment(0, &[], vec![first, second, unit(2, &[10], &[])]))
        .unwrap();
    graph.add_edge(edge(0, 0, None, EdgeKind::Return, &[])).unwrap();
    graph.finish(seq(vec![s(0), e(0)])).unwrap()
}

#[test]
fn phased_lane_publication_is_rejected_before_allocation() {
    assert!(
        check_selection(
            phased_lane_program(),
            &SourceContext::Generated(&Definition(phased_lane_program))
        )
        .is_err()
    );
}

fn complete_phase_temporary_program() -> SelectedProgram {
    let mut graph = builder();
    for (id, step) in [(1, 0), (2, 1), (10, 2)] {
        let mut output = value(id, 0, 0);
        output.origin = ValueOrigin::Temporary {
            step: SourceStepId(step),
            index: 0,
        };
        if id == 10 {
            output.layout = ValueLayout::new(Type::U64).unwrap();
            output.reconstruction = Some(Reconstruction::Alias(vec![atom(1), atom(2)]));
        }
        graph.add_value(output).unwrap();
    }
    for step in 0..4 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    let mut join = unit(2, &[1, 2], &[10]);
    join.stack_inputs = vec![atom(1), atom(2)];
    join.outputs.push(Atom::new(ValueId(10), 1));
    graph
        .add_segment(segment(
            0,
            &[],
            vec![unit(0, &[], &[1]), unit(1, &[], &[2]), join, unit(3, &[10], &[])],
        ))
        .unwrap();
    graph.add_edge(edge(0, 0, None, EdgeKind::Return, &[])).unwrap();
    graph.finish(seq(vec![s(0), e(0)])).unwrap()
}

#[test]
fn complete_phase_temporaries_remain_live_until_alias_join_completion() {
    let analysis = analyze(&checked(complete_phase_temporary_program));
    assert!(ids(&analysis.before[&SelectedStepId(0)]).is_empty());
    assert_eq!(ids(&analysis.after[&SelectedStepId(0)]), [1]);
    assert_eq!(ids(&analysis.before[&SelectedStepId(1)]), [1]);
    assert_eq!(ids(&analysis.after[&SelectedStepId(1)]), [1, 2]);
    assert_eq!(ids(&analysis.after[&SelectedStepId(2)]), [10]);
    assert_eq!(
        analysis.next_use.after[&SelectedStepId(0)].get(ValueId(1)),
        NextUseDistance::Finite(1)
    );
    assert_eq!(
        analysis.next_use.after[&SelectedStepId(0)].get(ValueId(2)),
        NextUseDistance::Infinite
    );
    assert_eq!(
        analysis.next_use.before[&SelectedStepId(2)].get(ValueId(1)),
        NextUseDistance::Finite(0)
    );
    assert_eq!(
        analysis.next_use.after[&SelectedStepId(2)].get(ValueId(10)),
        NextUseDistance::Finite(0)
    );
    let after_join = &analysis.next_use.after[&SelectedStepId(2)];
    assert!(after_join.priority(atom(10)) < after_join.priority(Atom::new(ValueId(10), 1)));
}

#[test]
fn dense_domain_handles_sparse_ids_across_word_boundaries() {
    fn build() -> SelectedProgram {
        let mut graph = builder();
        let parameters: Vec<_> = (0..130).map(|i| 1000 + 10 * i).chain([u32::MAX]).collect();
        for (index, id) in parameters.iter().copied().enumerate() {
            graph.add_value(value(id, 0, index as u32)).unwrap();
        }
        graph.add_source(SourceStepId(0), source(0)).unwrap();
        graph
            .add_segment(segment(
                0,
                &parameters,
                vec![unit(0, &[1000, 1630, 1640, 2270, 2290, u32::MAX], &[])],
            ))
            .unwrap();
        graph.add_edge(edge(0, 0, None, EdgeKind::Return, &[])).unwrap();
        graph.finish(seq(vec![s(0), e(0)])).unwrap()
    }
    let analysis = analyze(&checked(build));
    assert_eq!(
        ids(&analysis.before[&SelectedStepId(0)]),
        [1000, 1630, 1640, 2270, 2290, u32::MAX]
    );
    assert!(analysis.before[&SelectedStepId(0)].contains(ValueId(u32::MAX)));
    assert!(!analysis.before[&SelectedStepId(0)].contains(ValueId(2280)));
    assert!(ids(&analysis.after[&SelectedStepId(0)]).is_empty());
}

#[test]
fn diamond_next_use_takes_the_shortest_alternative_and_stable_value_ties() {
    fn build() -> SelectedProgram {
        let mut program = missing_else_program();
        program.segments[0].units = vec![unit(0, &[], &[])];
        program.segments[1].units = vec![unit(1, &[], &[]), unit(2, &[], &[])];
        program.segments[2].units = vec![unit(3, &[1], &[])];
        for step in 1..4 {
            program.source_map.insert(SourceStepId(step), source(step));
        }
        program
    }
    let analysis = analyze(&checked(build));
    assert_eq!(analysis.next_use.edge[&EdgeId(0)].get(ValueId(1)), NextUseDistance::Finite(2));
    assert_eq!(analysis.next_use.edge[&EdgeId(1)].get(ValueId(1)), NextUseDistance::Finite(0));
    assert_eq!(
        analysis.next_use.entry[&SegmentId(0)].get(ValueId(1)),
        NextUseDistance::Finite(1)
    );
    let at_entry = &analysis.next_use.entry[&SegmentId(0)];
    assert_eq!(at_entry.get(ValueId(0)), NextUseDistance::Finite(1));
    assert!(at_entry.priority(atom(0)) < at_entry.priority(atom(1)));
}

fn scratch_loop_program() -> SelectedProgram {
    let mut graph = builder();
    graph.add_value(value(0, 0, 0)).unwrap();
    for step in 0..3 {
        graph.add_source(SourceStepId(step), source(step)).unwrap();
    }
    let mut initialize = unit(0, &[], &[]);
    initialize.scratch = vec![scratch_requirement(&[0, 1, 2])];
    initialize.scratch_effects = vec![ScratchEffect::Write(content(0))];
    let mut iterate = unit(1, &[], &[]);
    iterate.scratch = initialize.scratch.clone();
    iterate.scratch_effects =
        vec![ScratchEffect::Read(content(1)), ScratchEffect::Write(content(2))];
    let mut exit = unit(2, &[], &[]);
    exit.scratch = initialize.scratch.clone();
    exit.scratch_effects = vec![ScratchEffect::Read(content(1))];
    graph.add_segment(segment(0, &[0], vec![initialize])).unwrap();
    let mut header = segment(1, &[], vec![]);
    header.content_parameters = vec![ContentId(1)];
    graph.add_segment(header).unwrap();
    graph.add_segment(segment(2, &[], vec![iterate])).unwrap();
    graph.add_segment(segment(3, &[], vec![exit])).unwrap();
    for mut transfer in [
        edge(0, 0, Some(1), EdgeKind::Forward, &[]),
        edge(1, 1, Some(2), EdgeKind::True, &[]),
        edge(2, 1, Some(3), EdgeKind::False, &[]),
        edge(3, 2, Some(1), EdgeKind::Backedge, &[]),
        edge(4, 3, None, EdgeKind::Return, &[]),
    ] {
        if let source @ (0 | 3) = transfer.id.0 {
            transfer.content_bindings = vec![ContentBinding {
                destination: ContentId(1),
                source: ContentId(if source == 0 { 0 } else { 2 }),
            }];
        }
        graph.add_edge(transfer).unwrap();
    }
    graph
        .finish(seq(vec![
            s(0),
            e(0),
            ControlTree::Loop {
                id: ControlId(0),
                condition: ControlId(1),
                header: SegmentId(1),
                predicate: atom(0),
                on_true: EdgeId(1),
                on_false: EdgeId(2),
                body: Box::new(seq(vec![s(2), e(3)])),
                backedges: vec![EdgeId(3)],
            },
            s(3),
            e(4),
        ]))
        .unwrap()
}

#[test]
fn scratch_loop_dependencies_include_zero_trip_and_current_backedge_contents() {
    let analysis = analyze(&checked(scratch_loop_program));
    assert_eq!(analysis.scratch.entry[&SegmentId(0)], vec![]);
    assert_eq!(analysis.scratch.after[&SelectedStepId(0)], vec![content(0)]);
    assert_eq!(analysis.scratch.entry[&SegmentId(1)], vec![content(1)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(0)], vec![content(0)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(2)], vec![content(1)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(3)], vec![content(2)]);
    assert_eq!(analysis.scratch.before[&SelectedStepId(1)], vec![content(1)]);
    assert_eq!(analysis.scratch.after[&SelectedStepId(1)], vec![content(2)]);
    assert_eq!(ids(&analysis.live_in[&SegmentId(1)]), [0]);
    fn reordered() -> SelectedProgram {
        let mut program = scratch_loop_program();
        program.segments[1..].reverse();
        program.edges.reverse();
        program
    }
    assert_eq!(analysis, analyze(&checked(reordered)));
}

#[test]
fn scratch_loop_invariants_remain_required_on_initial_and_repeated_entries() {
    fn build() -> SelectedProgram {
        let mut program = scratch_loop_program();
        program.segments[2].units[0].scratch_effects.pop();
        program.edges[3].content_bindings[0].source = ContentId(1);
        program
    }
    let analysis = analyze(&checked(build));
    assert_eq!(analysis.scratch.edge[&EdgeId(0)], vec![content(0)]);
    assert_eq!(analysis.scratch.edge[&EdgeId(3)], vec![content(1)]);
    assert_eq!(analysis.scratch.after[&SelectedStepId(1)], vec![content(1)]);
}

#[test]
fn scratch_atom_bindings_do_not_create_ordinary_operand_demand() {
    fn build() -> SelectedProgram {
        let mut graph = builder();
        graph.add_value(value(0, 0, 0)).unwrap();
        graph.add_value(value(10, 1, 0)).unwrap();
        for step in 0..2 {
            graph.add_source(SourceStepId(step), source(step)).unwrap();
        }
        let mut produce = unit(0, &[0], &[]);
        produce.scratch = vec![scratch_requirement(&[0, 1])];
        produce.scratch_effects =
            vec![ScratchEffect::Write(fact(ContentRequirement::Atom(atom(0))))];
        let mut consume = unit(1, &[], &[]);
        consume.scratch = produce.scratch.clone();
        consume.scratch_effects =
            vec![ScratchEffect::Read(fact(ContentRequirement::Atom(atom(10))))];
        graph.add_segment(segment(0, &[0], vec![produce])).unwrap();
        graph.add_segment(segment(1, &[10], vec![consume])).unwrap();
        graph.add_edge(edge(0, 0, Some(1), EdgeKind::Forward, &[(10, 0)])).unwrap();
        graph.add_edge(edge(1, 1, None, EdgeKind::Return, &[])).unwrap();
        graph.finish(seq(vec![s(0), e(0), s(1), e(1)])).unwrap()
    }
    let analysis = analyze(&checked(build));
    assert!(ids(&analysis.after[&SelectedStepId(0)]).is_empty());
    assert!(ids(&analysis.live_in[&SegmentId(1)]).is_empty());
    assert!(ids(&analysis.edge_live[&EdgeId(0)]).is_empty());
    assert_eq!(analysis.scratch.edge[&EdgeId(0)], vec![fact(ContentRequirement::Atom(atom(0)))]);
    assert_eq!(
        analysis.scratch.entry[&SegmentId(1)],
        vec![fact(ContentRequirement::Atom(atom(10)))]
    );
}

#[test]
fn loop_demand_excludes_terminal_arms_outside_the_cyclic_component() {
    fn build() -> SelectedProgram {
        let mut graph = builder();
        for id in 0..4 {
            graph.add_value(value(id, 0, id)).unwrap();
        }
        for step in 0..2 {
            graph.add_source(SourceStepId(step), source(step)).unwrap();
        }
        for id in 0..6 {
            let units = match id {
                3 => vec![unit(0, &[3], &[])],
                4 => vec![unit(1, &[2], &[])],
                _ => vec![],
            };
            graph
                .add_segment(segment(id, if id == 0 { &[0, 1, 2, 3] } else { &[] }, units))
                .unwrap();
        }
        for transfer in [
            edge(0, 0, Some(1), EdgeKind::Forward, &[]),
            edge(1, 1, Some(2), EdgeKind::True, &[]),
            edge(2, 1, Some(5), EdgeKind::False, &[]),
            edge(3, 2, Some(3), EdgeKind::True, &[]),
            edge(4, 2, Some(4), EdgeKind::False, &[]),
            edge(5, 3, None, EdgeKind::Trap, &[]),
            edge(6, 4, Some(1), EdgeKind::Backedge, &[]),
            edge(7, 5, None, EdgeKind::Return, &[]),
        ] {
            graph.add_edge(transfer).unwrap();
        }
        graph
            .finish(seq(vec![
                s(0),
                e(0),
                ControlTree::Loop {
                    id: ControlId(0),
                    condition: ControlId(10),
                    header: SegmentId(1),
                    predicate: atom(0),
                    on_true: EdgeId(1),
                    on_false: EdgeId(2),
                    backedges: vec![EdgeId(6)],
                    body: Box::new(seq(vec![
                        s(2),
                        branch(1, 1, 3, seq(vec![s(3), e(5)]), 4, seq(vec![s(4), e(6)])),
                    ])),
                },
                s(5),
                e(7),
            ]))
            .unwrap()
    }
    let analysis = analyze(&checked(build));
    assert_eq!(analysis.sccs[&SegmentId(1)], vec![SegmentId(1), SegmentId(2), SegmentId(4)]);
    assert_eq!(
        analysis.next_use.entry[&SegmentId(1)].get(ValueId(3)),
        NextUseDistance::Finite(0)
    );
    assert_eq!(
        analysis.loops[&ControlId(0)].members.iter().copied().collect::<Vec<_>>(),
        [SegmentId(1), SegmentId(2), SegmentId(4)]
    );
    assert_eq!(
        analysis.loops[&ControlId(0)].next_use.entry[&SegmentId(1)].get(ValueId(3)),
        NextUseDistance::Infinite
    );
    assert_eq!(
        analysis.loops[&ControlId(0)].next_use.entry[&SegmentId(1)].get(ValueId(2)),
        NextUseDistance::Finite(0)
    );
}

#[test]
fn backedge_parameter_permutation_uses_simultaneous_source_names() {
    fn build() -> SelectedProgram {
        let mut program = wide_loop_program();
        program.edges[3].bindings[0].source = ValueId(11);
        program.edges[3].bindings[1].source = ValueId(10);
        program
    }
    let analysis = analyze(&checked(build));
    assert_eq!(ids(&analysis.live_in[&SegmentId(1)]), [10, 11, 20, 30, 31]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(3)]), [10, 11, 30, 31, 110]);
    assert_eq!(ids(&analysis.after[&SelectedStepId(0)]), [10, 11, 30, 31, 110]);
    assert_eq!(analysis.next_use.edge[&EdgeId(3)].get(ValueId(11)), NextUseDistance::Finite(0));
    assert_eq!(analysis.next_use.edge[&EdgeId(3)].get(ValueId(10)), NextUseDistance::Finite(1));
}

#[test]
fn present_literal_branch_alternatives_remain_reachable() {
    fn build() -> SelectedProgram {
        let mut program = switch_program();
        program.values.get_mut(&ValueId(5)).unwrap().reconstruction =
            Some(Reconstruction::Literal(vec![Felt::ZERO]));
        program
    }
    let analysis = analyze(&checked(build));
    assert_eq!(
        analysis.reachable.iter().copied().collect::<Vec<_>>(),
        [SegmentId(0), SegmentId(1), SegmentId(2), SegmentId(3), SegmentId(4)]
    );
    assert_eq!(ids(&analysis.edge_live[&EdgeId(0)]), [2]);
    assert_eq!(ids(&analysis.edge_live[&EdgeId(1)]), [0, 3, 4]);
}
