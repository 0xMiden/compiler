use super::*;

struct Reconstructible {
    alias: bool,
    crossing: bool,
}

impl GeneratedDefinition for Reconstructible {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(41)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Fixture::new(u32::from(self.alias), 1).instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
            Some(if self.alias {
                Reconstruction::Alias(vec![Fixture::input(0)])
            } else {
                Reconstruction::Literal(vec![Felt::new_unchecked(7)])
            });
        if self.crossing {
            evidence.program.segments.push(SelectedSegment {
                content_parameters: vec![],
                id: SegmentId(1),
                parameters: vec![],
                units: vec![],
            });
            let mut terminal = evidence.program.edges[0].clone();
            terminal.id = EdgeId(1);
            terminal.source = SegmentId(1);
            evidence.program.edges[0].destination = Some(SegmentId(1));
            evidence.program.edges[0].kind = EdgeKind::Forward;
            evidence.program.edges[0].bindings.clear();
            evidence.program.edges.push(terminal);
            evidence.program.structure = ControlTree::Sequence(vec![
                ControlTree::Segment(SegmentId(0)),
                ControlTree::Edge(EdgeId(0)),
                ControlTree::Segment(SegmentId(1)),
                ControlTree::Edge(EdgeId(1)),
            ]);
        }
        Ok(evidence)
    }
}

fn fixture(alias: bool, crossing: bool, wrong: bool) -> (ProcedurePlan, AnnotatedMasmBody) {
    let definition = Reconstructible { alias, crossing };
    let mut plan = Fixture::new(u32::from(alias), 1).plan();
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let mut code = vec![
        AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
    ];
    if !alias {
        code.push(push(if wrong { 8 } else { 7 }));
    } else if wrong {
        code.extend([inst(Instruction::Drop), push(8)]);
    }
    code.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    if !alias {
        code.push(inst(Instruction::Drop));
        plan.segment_actions.get_mut(&SegmentId(0)).unwrap().push(PlanAction::Drop);
    }
    if crossing {
        code.extend([
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))),
        ]);
        let mut contract = plan.contracts[&SegmentId(0)].clone();
        contract.stack_top_first = if alias {
            vec![Fixture::input(0)]
        } else {
            vec![]
        };
        plan.contracts.insert(SegmentId(1), contract);
        plan.segment_actions.insert(SegmentId(1), vec![]);
    }
    if !alias {
        code.push(push(7));
        plan.segment_actions
            .get_mut(&SegmentId(u32::from(crossing)))
            .unwrap()
            .push(PlanAction::PushLiteral(Felt::new_unchecked(7)));
    }
    code.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(u32::from(crossing)))));
    (plan, ops(code))
}

#[test]
fn checked_literal_can_be_drained_and_rebuilt_without_storage() {
    let (plan, body) = fixture(false, false, false);
    verify(&plan, &frame(), body).unwrap();
}

#[test]
fn checked_literal_survives_an_empty_fresh_contract() {
    let (plan, body) = fixture(false, true, false);
    verify(&plan, &frame(), body).unwrap();
}

#[test]
fn reconstruction_declarations_cannot_bless_wrong_physical_outputs() {
    for alias in [false, true] {
        let (plan, body) = fixture(alias, false, true);
        assert!(verify(&plan, &frame(), body).is_err());
    }
}

#[test]
fn declaration_only_alias_survives_a_fresh_contract() {
    let (plan, body) = fixture(true, true, false);
    verify(&plan, &frame(), body).unwrap();
}

#[test]
fn cyclic_or_forward_reconstruction_aliases_are_rejected_at_selection() {
    struct InvalidAlias;
    impl GeneratedDefinition for InvalidAlias {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(42)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Reconstructible {
                alias: false,
                crossing: false,
            }
            .instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![Fixture::output(0)]));
            Ok(evidence)
        }
    }
    assert!(
        check_selection(
            InvalidAlias.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&InvalidAlias)
        )
        .is_err()
    );
}

struct StoredReconstruction;

impl GeneratedDefinition for StoredReconstruction {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(47)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Reconstructible {
            alias: false,
            crossing: false,
        }
        .instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let mut consumer = evidence.program.segments[0].units[0].clone();
        consumer.id = SelectedStepId(1);
        consumer.source = SourceStepId(1);
        consumer.outputs.clear();
        consumer.semantic_inputs = vec![ValueId(100)];
        consumer.stored_inputs = vec![StoredInput {
            id: StoredInputId(0),
            value: StoredValue::Atom(Fixture::output(0)),
        }];
        evidence.program.segments[0].units.push(consumer);
        evidence.units.insert(SelectedStepId(1), UnitEvidence::default());
        evidence.program.source_map.insert(
            SourceStepId(1),
            SourceRecord {
                origin: SourceOrigin::Generated {
                    definition: self.id(),
                    stage: 1,
                },
                location: SourceLocation::default(),
            },
        );
        evidence.program.edges[0].bindings.clear();
        evidence.program.abi = ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap();
        Ok(evidence)
    }
}

#[test]
fn reconstructible_stored_literal_still_needs_the_actual_physical_store() {
    for stored in [None, Some(7), Some(8)] {
        let mut plan = Fixture::new(0, 0).plan();
        plan.selected = check_selection(
            StoredReconstruction.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&StoredReconstruction),
        )
        .unwrap();
        let slot = SpillSlotId::canonical(Fixture::output(0));
        plan.spill_slots = vec![slot];
        let frame = FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::from([(slot, 0)]),
            BTreeMap::new(),
            4,
        )
        .unwrap();
        plan.lowering_bindings.insert(
            SelectedStepId(1),
            LoweringBindings {
                input_order: crate::plan::InputOrder::Canonical,
                stored_inputs: BTreeMap::from([(StoredInputId(0), slot)]),
                scratch: BTreeMap::new(),
            },
        );
        let mut actions = vec![
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
            PlanAction::Drop,
        ];
        let mut code = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            push(7),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            inst(Instruction::Drop),
        ];
        if let Some(value) = stored {
            actions.extend([
                PlanAction::PushLiteral(Felt::new_unchecked(value)),
                PlanAction::StoreLane(slot),
            ]);
            code.extend([push(value), inst(Instruction::LocStore(0.into()))]);
        }
        actions.push(PlanAction::RunLowering {
            step: SelectedStepId(1),
        });
        code.extend([
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(1))),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(1))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]);
        plan.segment_actions.insert(SegmentId(0), actions);
        let result = verify(&plan, &frame, ops(code));
        assert_eq!(result.is_ok(), stored == Some(7), "{stored:?}: {result:?}");
    }
}

#[test]
fn a_previous_loop_instance_alias_cannot_supply_a_header_capture() {
    struct PreviousAlias;
    impl GeneratedDefinition for PreviousAlias {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(48)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = super::loops::LoopDefinition.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.values.get_mut(&ValueId(31)).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![Atom::new(ValueId(20), 0)]));
            evidence.program.edges[3].bindings[0].source = ValueId(31);
            Ok(evidence)
        }
    }
    assert!(
        check_selection(
            PreviousAlias.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&PreviousAlias)
        )
        .is_err()
    );
}

#[test]
fn reconstructed_parameter_identity_is_checked_on_actual_incoming_edge() {
    struct LiteralParameter;
    impl GeneratedDefinition for LiteralParameter {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(49)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Reconstructible {
                alias: false,
                crossing: true,
            }
            .instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            let value = evidence.program.values.get_mut(&ValueId(100)).unwrap();
            let mut parameter = value.clone();
            value.reconstruction = None;
            parameter.id = ValueId(201);
            parameter.origin = ValueOrigin::Parameter {
                segment: SegmentId(1),
                index: 0,
            };
            evidence.program.values.insert(parameter.id, parameter);
            evidence.program.segments[1].parameters = vec![ValueId(201)];
            evidence.program.edges[0].bindings = vec![ParameterBinding {
                destination: ValueId(201),
                source: ValueId(100),
            }];
            evidence.program.edges[1].bindings = vec![ParameterBinding {
                destination: ValueId(201),
                source: ValueId(201),
            }];
            Ok(evidence)
        }
    }
    let mut plan = Fixture::new(0, 1).plan();
    plan.selected = check_selection(
        LiteralParameter.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&LiteralParameter),
    )
    .unwrap();
    let mut contract = plan.contracts[&SegmentId(0)].clone();
    contract.stack_top_first = vec![Atom::new(ValueId(201), 0)];
    plan.contracts.insert(SegmentId(1), contract);
    plan.segment_actions.insert(SegmentId(1), vec![]);
    for value in [7, 8] {
        let body = ops(vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            push(value),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(1))),
        ]);
        let result = verify(&plan, &frame(), body);
        assert_eq!(result.is_ok(), value == 7, "value={value}: {result:?}");
    }
}

#[test]
fn representation_alias_lanes_are_checked_and_preserved_across_hypotheses() {
    struct LaneAlias;
    impl GeneratedDefinition for LaneAlias {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(51)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Reconstructible {
                alias: true,
                crossing: true,
            }
            .instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            for id in [ValueId(0), ValueId(100)] {
                evidence.program.values.get_mut(&id).unwrap().layout =
                    ValueLayout::new(Type::U64).unwrap();
            }
            evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![Atom::new(ValueId(0), 1), Fixture::input(0)]));
            evidence.program.segments[0].units[0]
                .stack_inputs
                .push(Atom::new(ValueId(0), 1));
            evidence.program.segments[0].units[0].outputs.push(Atom::new(ValueId(100), 1));
            evidence.program.abi = ProcedureAbi::new(CallingConvention::SameContext, 2, 2).unwrap();
            Ok(evidence)
        }
    }
    let mut plan = Fixture::new(1, 1).plan();
    plan.selected = check_selection(
        LaneAlias.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&LaneAlias),
    )
    .unwrap();
    plan.contracts
        .get_mut(&SegmentId(0))
        .unwrap()
        .stack_top_first
        .push(Atom::new(ValueId(0), 1));
    let mut contract = plan.contracts[&SegmentId(0)].clone();
    contract.stack_top_first.reverse();
    plan.contracts.insert(SegmentId(1), contract);
    plan.segment_actions.insert(SegmentId(1), vec![]);
    for swap in [false, true] {
        let mut code = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        ];
        if swap {
            code.push(inst(Instruction::Swap1));
        }
        code.extend([
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(1))),
        ]);
        let result = verify(&plan, &frame(), ops(code));
        assert_eq!(result.is_ok(), swap, "swap={swap}: {result:?}");
    }
}

#[test]
fn a_wrong_literal_is_rejected_at_publication_before_reconstruction() {
    let definition = Reconstructible {
        alias: false,
        crossing: false,
    };
    let mut plan = Fixture::new(0, 1).plan();
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let error = verify(
        &plan,
        &frame(),
        body(vec![Primitive::Push(Felt::new_unchecked(8)).instruction().unwrap()]),
    )
    .unwrap_err();
    assert_eq!(error.message, "output violates checked reconstruction identity");
}

#[test]
fn a_reconstruction_declaration_cannot_invent_an_unpublished_lane() {
    struct MissingLane;
    impl GeneratedDefinition for MissingLane {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(52)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Reconstructible {
                alias: true,
                crossing: true,
            }
            .instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            for id in [ValueId(0), ValueId(100)] {
                evidence.program.values.get_mut(&id).unwrap().layout =
                    ValueLayout::new(Type::U64).unwrap();
            }
            evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![Fixture::input(0), Atom::new(ValueId(0), 1)]));
            evidence.program.segments[0].units[0]
                .stack_inputs
                .push(Atom::new(ValueId(0), 1));
            Ok(evidence)
        }
    }
    assert!(
        check_selection(
            MissingLane.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&MissingLane)
        )
        .is_err()
    );
}

/// A reachability oracle: removing a dominator must disconnect its dominated node.
/// This does not share the verifier's dominator computation or traversal order.
fn reachable_without(edges: &[(usize, usize)], count: usize, removed: usize) -> Vec<bool> {
    let mut seen = vec![false; count];
    let mut pending = vec![0];
    while let Some(node) = pending.pop() {
        if node == removed || seen[node] {
            continue;
        }
        seen[node] = true;
        pending.extend(edges.iter().filter(|(from, _)| *from == node).map(|(_, to)| *to));
    }
    seen
}

fn dominance_program(count: usize, edges: &[(usize, usize)]) -> SelectedProgram {
    let mut program = Fixture::new(0, 0).instantiate(&[]).unwrap().program;
    // IDs deliberately differ from their traversal and storage indices.
    let id = |index: usize| SegmentId((count - index) as u32 * 7);
    program.segments = (0..count)
        .map(|index| SelectedSegment {
            id: id(index),
            parameters: vec![],
            content_parameters: vec![],
            units: vec![],
        })
        .collect();
    program.edges = edges
        .iter()
        .enumerate()
        .map(|(index, (from, to))| TransferEdge {
            id: EdgeId(index as u32),
            source: id(*from),
            destination: Some(id(*to)),
            kind: EdgeKind::Forward,
            bindings: vec![],
            content_bindings: vec![],
        })
        .collect();
    program
}

#[test]
fn dominance_agrees_with_independent_path_removal() {
    // Every directed four-node graph, including cycles, duplicate entry paths and
    // irreducible loops. Only compare reachable nodes, as checked graphs require.
    let pairs: Vec<_> = (0..4)
        .flat_map(|from| (0..4).filter(move |to| *to != from).map(move |to| (from, to)))
        .collect();
    for mask in 0..(1 << pairs.len()) {
        let edges: Vec<_> = pairs
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, edge)| *edge)
            .collect();
        let reachable = reachable_without(&edges, 4, usize::MAX);
        if reachable.iter().any(|seen| !seen) {
            continue;
        }
        let program = dominance_program(4, &edges);
        let dom = super::super::reconstruction::dominators(&program);
        for owner in 0..4 {
            let without = reachable_without(&edges, 4, owner);
            for (node, remains_reachable) in without.iter().enumerate() {
                assert_eq!(
                    dom.dominates(program.segments[owner].id, program.segments[node].id),
                    !remains_reachable,
                    "edges={edges:?}, owner={owner}, node={node}"
                );
            }
        }
    }
}

#[test]
fn dominance_long_chain_scaling() {
    for count in [64, 128, 256] {
        let edges: Vec<_> = (1..count).map(|node| (node - 1, node)).collect();
        let program = dominance_program(count, &edges);
        let start = std::time::Instant::now();
        let dom = super::super::reconstruction::dominators(&program);
        std::eprintln!("dominance chain {count}: {:?}", start.elapsed());
        for node in 0..count {
            assert!(dom.dominates(program.segments[0].id, program.segments[node].id));
            if node + 1 < count {
                assert!(!dom.dominates(program.segments[node + 1].id, program.segments[node].id));
            }
        }
    }
}

#[test]
fn dominance_handles_empty_graph_and_duplicate_self_edges() {
    let empty = dominance_program(0, &[]);
    let _ = super::super::reconstruction::dominators(&empty);
    let program = dominance_program(3, &[(0, 0), (0, 1), (0, 1), (1, 1), (1, 2), (2, 1)]);
    let dom = super::super::reconstruction::dominators(&program);
    for owner in 0..3 {
        for node in 0..3 {
            assert_eq!(
                dom.dominates(program.segments[owner].id, program.segments[node].id),
                owner <= node
            );
        }
    }
}

/// Repeated transfers keep an alias live without growing any segment's physical work.
struct ReconstructionChain(u32);

impl GeneratedDefinition for ReconstructionChain {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(53)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Reconstructible {
            alias: true,
            crossing: true,
        }
        .instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let segment = evidence.program.segments[1].clone();
        let forward = evidence.program.edges[0].clone();
        let terminal = evidence.program.edges[1].clone();
        evidence.program.segments.truncate(1);
        evidence.program.edges.clear();
        let mut structure = vec![];
        for index in 0..self.0 {
            if index != 0 {
                let mut segment = segment.clone();
                segment.id = SegmentId(index);
                evidence.program.segments.push(segment);
            }
            let mut edge = if index + 1 == self.0 {
                terminal.clone()
            } else {
                let mut edge = forward.clone();
                edge.destination = Some(SegmentId(index + 1));
                edge
            };
            edge.id = EdgeId(index);
            edge.source = SegmentId(index);
            evidence.program.edges.push(edge);
            structure
                .extend([ControlTree::Segment(SegmentId(index)), ControlTree::Edge(EdgeId(index))]);
        }
        evidence.program.structure = ControlTree::Sequence(structure);
        Ok(evidence)
    }
}

#[test]
fn graph_replay_retains_tokens_for_one_segment_not_the_whole_graph() {
    use crate::verify::replay::Verifier;

    for segments in [2, 32] {
        let definition = ReconstructionChain(segments);
        let (mut plan, _) = fixture(true, true, false);
        plan.selected = check_selection(
            definition.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&definition),
        )
        .unwrap();
        let contract = plan.contracts[&SegmentId(1)].clone();
        let mut code = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ];
        for index in 1..segments {
            plan.contracts.insert(SegmentId(index), contract.clone());
            plan.segment_actions.insert(SegmentId(index), vec![]);
            code.extend([
                AnnotatedOp::Boundary(Boundary::Segment(SegmentId(index))),
                AnnotatedOp::Boundary(Boundary::Edge(EdgeId(index))),
            ]);
        }
        let body = ops(code);
        let frame = frame();
        verify(&plan, &frame, body.clone()).unwrap();
        let mut verifier = Verifier::new(&plan, &frame).unwrap();
        for _ in 0..3 {
            verifier.verify_graph(&body).unwrap();
            // Each segment has just two semantic lanes. Leave room for local proof
            // bookkeeping, but not for identities retained from earlier segments/passes.
            let retained = verifier.tokens.fresh().0;
            assert!(retained <= 8, "{segments} segments retained {retained} tokens");
        }
    }
}

#[test]
fn definition_index_preserves_parameter_lanes_and_unit_order() {
    use crate::verify::reconstruction::Definitions;

    let mut program = Reconstructible {
        alias: true,
        crossing: true,
    }
    .instantiate(&[])
    .unwrap()
    .program;
    // Indexing records declared definitions, not inferred aliases or absent lanes.
    program.values.get_mut(&ValueId(0)).unwrap().layout = ValueLayout::new(Type::U64).unwrap();
    let mut second = program.segments[0].units[0].clone();
    second.outputs = vec![Atom::new(ValueId(101), 0)];
    program.segments[0].units.push(second);
    program.segments[1].parameters.push(ValueId(100));
    program.segments[0].units[0].outputs = vec![Atom::new(ValueId(102), 0)];

    let definitions = Definitions::new(&program);
    for (atom, position) in [
        (Atom::new(ValueId(0), 0), (SegmentId(0), 0)),
        (Atom::new(ValueId(0), 1), (SegmentId(0), 0)),
        (Atom::new(ValueId(102), 0), (SegmentId(0), 1)),
        (Atom::new(ValueId(101), 0), (SegmentId(0), 2)),
        (Atom::new(ValueId(100), 0), (SegmentId(1), 0)),
    ] {
        assert_eq!(definitions.get(atom).unwrap(), position);
    }
    assert!(definitions.get(Atom::new(ValueId(0), 2)).is_err());
    assert!(definitions.get(Atom::new(ValueId(101), 1)).is_err());
    assert!(definitions.get(Atom::new(ValueId(999), 0)).is_err());
}

/// Source-owned semantic alias fixture; malformed variants exercise source-schema checks,
/// rather than only equality between a candidate and a valid reconstructed definition.
pub(super) struct SemanticAlias(pub u8);

impl GeneratedDefinition for SemanticAlias {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(73)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Fixture::new(1, 1).instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
            Some(Reconstruction::Alias(vec![Fixture::input(0)]));
        let unit = &mut evidence.program.segments[0].units[0];
        unit.publication = OutputPublication::Aliases;
        unit.stack_inputs.clear();
        unit.effects = LoweringEffects::default();
        evidence.units.get_mut(&unit.id).unwrap().aliases =
            LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
        match self.0 {
            0 => {}
            1 => unit.semantic_inputs.clear(),
            2 => unit.stack_inputs.push(Fixture::input(0)),
            3 => unit.effects.may_trap = true,
            4 => unit.effects.memory = SourceMemoryEffects::READ,
            5 => unit.stored_inputs.push(StoredInput {
                id: StoredInputId(0),
                value: StoredValue::Atom(Fixture::input(0)),
            }),
            6 => evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction = None,
            7 => {
                evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
                    Some(Reconstruction::Literal(vec![Felt::ONE]))
            }
            8 => {
                evidence.program.values.get_mut(&ValueId(100)).unwrap().reconstruction =
                    Some(Reconstruction::Alias(vec![]))
            }
            9 => {
                evidence.program.values.get_mut(&ValueId(100)).unwrap().layout =
                    ValueLayout::new(Type::U64).unwrap();
            }
            10 => unit.scratch = Fixture::new(1, 1).scratch(1).scratch,
            _ => unreachable!(),
        }
        Ok(evidence)
    }
}

pub(super) fn semantic_alias_plan() -> ProcedurePlan {
    let mut plan = Fixture::new(1, 1).plan();
    plan.selected = check_selection(
        SemanticAlias(0).instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&SemanticAlias(0)),
    )
    .unwrap();
    plan
}

#[test]
fn semantic_alias_publication_accepts_spill_only_source_without_loading_it() {
    let mut plan = semantic_alias_plan();
    let slot = SpillSlotId::canonical(Fixture::input(0));
    plan.spill_slots = vec![slot];
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::StoreLane(slot),
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
            PlanAction::LoadLane(slot),
        ],
    );
    verify(
        &plan,
        &frame,
        ops(vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            inst(Instruction::LocStore(0.into())),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            inst(Instruction::LocLoad(0.into())),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]),
    )
    .unwrap();
}

#[test]
fn semantic_alias_resolution_requires_recoverability_not_just_a_name() {
    use crate::verify::{reconstruction::alias_outputs, stack::State, tokens::Tokens};
    let plan = semantic_alias_plan();
    let mut tokens = Tokens::default();
    let token = tokens.fresh();
    let mut state = State::new(vec![], BTreeMap::from([(Fixture::input(0), token)]));
    assert!(
        alias_outputs(&plan.selected, &[Fixture::output(0)], &state, &tokens, &frame()).is_err()
    );
    state.stack.push(token);
    assert_eq!(
        alias_outputs(&plan.selected, &[Fixture::output(0)], &state, &tokens, &frame()).unwrap(),
        vec![token]
    );
}

#[test]
fn semantic_alias_publication_accepts_reconstructed_only_literal() {
    let definition = Reconstructible {
        alias: false,
        crossing: false,
    };
    let mut evidence = definition.instantiate(&[]).unwrap();
    let mut alias = evidence.program.segments[0].units[0].clone();
    alias.id = SelectedStepId(1);
    alias.source = SourceStepId(1);
    alias.semantic_inputs = vec![ValueId(100)];
    alias.outputs = vec![Atom::new(ValueId(101), 0)];
    alias.publication = OutputPublication::Aliases;
    alias.effects = LoweringEffects::default();
    evidence.program.values.insert(
        ValueId(101),
        SelectedValue {
            id: ValueId(101),
            origin: ValueOrigin::Temporary {
                step: alias.source,
                index: 0,
            },
            layout: ValueLayout::new(Type::Felt).unwrap(),
            reconstruction: Some(Reconstruction::Alias(vec![Fixture::output(0)])),
        },
    );
    evidence.program.source_map.insert(
        alias.source,
        SourceRecord {
            origin: SourceOrigin::Generated {
                definition: definition.id(),
                stage: 1,
            },
            location: SourceLocation::default(),
        },
    );
    evidence.units.insert(alias.id, UnitEvidence::default());
    evidence.program.segments[0].units.push(alias);
    evidence.program.edges[0].bindings[0].source = ValueId(101);
    evidence.program.edges[0].bindings[0].destination = ValueId(101);
    struct LiteralAlias(DefinitionEvidence);
    impl GeneratedDefinition for LiteralAlias {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(41)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            Ok(self.0.clone())
        }
    }
    let definition = LiteralAlias(evidence);
    let mut plan = Fixture::new(0, 1).plan();
    plan.selected =
        check_selection(definition.0.program.clone(), &SourceContext::Generated(&definition))
            .unwrap();
    plan.lowering_bindings.insert(SelectedStepId(1), LoweringBindings::default());
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
            PlanAction::Drop,
            PlanAction::RunLowering {
                step: SelectedStepId(1),
            },
            PlanAction::PushLiteral(Felt::new_unchecked(7)),
        ],
    );
    verify(
        &plan,
        &frame(),
        ops(vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            push(7),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            inst(Instruction::Drop),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(1))),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(1))),
            push(7),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]),
    )
    .unwrap();
}
