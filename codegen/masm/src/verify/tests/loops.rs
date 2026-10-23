use super::*;

pub(super) struct LoopDefinition;

impl GeneratedDefinition for LoopDefinition {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(4)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut values = BTreeMap::new();
        for (id, segment, index) in
            [(0, 0, 0), (1, 0, 1), (10, 1, 0), (11, 1, 1), (20, 2, 0), (40, 3, 0)]
        {
            values.insert(
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(segment),
                        index,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        for (id, step, index) in [(30, 0, 0), (31, 0, 1), (41, 1, 0)] {
            values.insert(
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(step),
                        index,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        let unit = |step, input, outputs: Vec<u32>| SelectedLowering {
            publication: OutputPublication::Stack,
            address_results: vec![],
            id: SelectedStepId(step),
            kind: LoweringKind(0),
            source: SourceStepId(step),
            semantic_inputs: vec![ValueId(input)],
            stack_inputs: vec![Atom::new(ValueId(input), 0)],
            stored_inputs: vec![],
            outputs: outputs.into_iter().map(|id| Atom::new(ValueId(id), 0)).collect(),
            effects: LoweringEffects::default(),
            scratch: vec![],
            scratch_effects: vec![],
        };
        let edge =
            |id, source, destination: Option<u32>, kind, bindings: Vec<(u32, u32)>| TransferEdge {
                content_bindings: vec![],
                id: EdgeId(id),
                source: SegmentId(source),
                destination: destination.map(SegmentId),
                kind,
                bindings: bindings
                    .into_iter()
                    .map(|(destination, source)| ParameterBinding {
                        destination: ValueId(destination),
                        source: ValueId(source),
                    })
                    .collect(),
            };
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![
                    SelectedSegment {
                        content_parameters: vec![],
                        id: SegmentId(0),
                        parameters: vec![ValueId(0), ValueId(1)],
                        units: vec![],
                    },
                    SelectedSegment {
                        content_parameters: vec![],
                        id: SegmentId(1),
                        parameters: vec![ValueId(10), ValueId(11)],
                        units: vec![],
                    },
                    SelectedSegment {
                        content_parameters: vec![],
                        id: SegmentId(2),
                        parameters: vec![ValueId(20)],
                        units: vec![unit(0, 20, vec![30, 31])],
                    },
                    SelectedSegment {
                        content_parameters: vec![],
                        id: SegmentId(3),
                        parameters: vec![ValueId(40)],
                        units: vec![unit(1, 40, vec![41])],
                    },
                ],
                edges: vec![
                    edge(0, 0, Some(1), EdgeKind::Forward, vec![(10, 0), (11, 1)]),
                    edge(1, 1, Some(2), EdgeKind::True, vec![(20, 11)]),
                    edge(2, 2, Some(1), EdgeKind::Backedge, vec![(10, 30), (11, 31)]),
                    edge(3, 1, Some(3), EdgeKind::False, vec![(40, 11)]),
                    edge(4, 3, None, EdgeKind::Return, vec![(41, 41)]),
                ],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                    ControlTree::Loop {
                        id: ControlId(0),
                        condition: ControlId(10),
                        on_true: EdgeId(1),
                        on_false: EdgeId(3),
                        header: SegmentId(1),

                        before: Box::new(ControlTree::Segment(SegmentId(1))),
                        predicate: Atom::new(ValueId(10), 0),
                        body: alloc::boxed::Box::new(ControlTree::Sequence(vec![
                            ControlTree::Segment(SegmentId(2)),
                            ControlTree::Edge(EdgeId(2)),
                        ])),
                        backedges: vec![EdgeId(2)],
                    },
                    ControlTree::Segment(SegmentId(3)),
                    ControlTree::Edge(EdgeId(4)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 2, 1).unwrap(),
                source_map: (0..2)
                    .map(|step| {
                        (
                            SourceStepId(step),
                            SourceRecord {
                                origin: SourceOrigin::Generated {
                                    definition: self.id(),
                                    stage: step,
                                },
                                location: SourceLocation::default(),
                            },
                        )
                    })
                    .collect(),
            },
            units: BTreeMap::from([
                (SelectedStepId(0), UnitEvidence::default()),
                (
                    SelectedStepId(1),
                    UnitEvidence {
                        aliases: LaneAliases::new(vec![(
                            Atom::new(ValueId(41), 0),
                            Atom::new(ValueId(40), 0),
                        )]),
                        ..Default::default()
                    },
                ),
            ]),
        })
    }
}
pub(in crate::verify) fn loop_fixture(
    repair: bool,
) -> (ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let selected = check_selection(
        LoopDefinition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&LoopDefinition),
    )
    .unwrap();
    let slot = SpillSlotId::canonical(Atom::new(ValueId(11), 0));
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let mut contracts = BTreeMap::new();
    let mut actions = BTreeMap::new();
    for segment in &selected.segments {
        contracts.insert(
            segment.id,
            EntryContract {
                stack_top_first: segment.parameters.iter().map(|id| Atom::new(*id, 0)).collect(),
                required_spill_slots: if segment.id == SegmentId(1) {
                    vec![(slot, Atom::new(ValueId(11), 0))]
                } else if segment.id == SegmentId(2) {
                    vec![(slot, Atom::new(ValueId(20), 0))]
                } else {
                    vec![]
                },
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: TailPolicy::PreserveSuffix,
            },
        );
        actions.insert(
            segment.id,
            segment
                .units
                .iter()
                .map(|unit| PlanAction::RunLowering { step: unit.id })
                .collect(),
        );
    }
    let store = vec![PlanAction::Dup { depth: 1 }, PlanAction::StoreLane(slot)];
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts,
        segment_actions: actions,
        edge_actions: BTreeMap::from([
            (EdgeId(0), store.clone()),
            (EdgeId(2), if repair { store } else { vec![] }),
        ]),
        spill_slots: vec![slot],
        lowering_bindings: BTreeMap::from([
            (SelectedStepId(0), LoweringBindings::default()),
            (SelectedStepId(1), LoweringBindings::default()),
        ]),
    };
    let boundary = |marker| AnnotatedOp::Boundary(marker);
    let spill = || {
        vec![
            AnnotatedOp::Instruction {
                instruction: Instruction::Dup1,
                unit: SelectedStepId(u32::MAX),
            },
            AnnotatedOp::Instruction {
                instruction: Instruction::LocStore(0.into()),
                unit: SelectedStepId(u32::MAX),
            },
        ]
    };
    let mut iteration = vec![
        boundary(Boundary::Edge(EdgeId(1))),
        boundary(Boundary::Segment(SegmentId(2))),
        boundary(Boundary::UnitStart(SelectedStepId(0))),
        push(1),
        inst(Instruction::Add),
        push(0),
        boundary(Boundary::UnitEnd(SelectedStepId(0))),
        boundary(Boundary::Edge(EdgeId(2))),
    ];
    if repair {
        iteration.extend(spill());
    }
    let mut code =
        vec![boundary(Boundary::Segment(SegmentId(0))), boundary(Boundary::Edge(EdgeId(0)))];
    code.extend(spill());
    code.extend(selected_loop(
        0,
        10,
        vec![boundary(Boundary::Segment(SegmentId(1)))],
        iteration,
        vec![boundary(Boundary::Edge(EdgeId(3)))],
    ));
    code.extend([
        boundary(Boundary::Segment(SegmentId(3))),
        boundary(Boundary::UnitStart(SelectedStepId(1))),
        boundary(Boundary::UnitEnd(SelectedStepId(1))),
        boundary(Boundary::Edge(EdgeId(4))),
    ]);
    (plan, frame, ops(code))
}

#[test]
fn selected_loop_zero_trip_and_backedge_are_independently_checked() {
    let (plan, frame, body) = loop_fixture(true);
    verify(&plan, &frame, body).unwrap();
}

#[test]
fn selected_loop_stale_carried_slot_is_rejected() {
    let (plan, frame, body) = loop_fixture(false);
    assert_eq!(
        verify(&plan, &frame, body).unwrap_err().category,
        DiagnosticCategory::InvalidEdgeTransfer
    );
}

struct NestedDefinition;

impl GeneratedDefinition for NestedDefinition {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(5)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = LoopDefinition.instantiate(parameters)?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        for (id, segment, index) in
            [(110, 100, 0), (111, 100, 1), (120, 101, 0), (140, 102, 0), (21, 6, 0)]
        {
            evidence.program.values.insert(
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(segment),
                        index,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        for (id, step, index) in [(60, 2, 0), (61, 2, 1), (130, 3, 0), (131, 3, 1)] {
            evidence.program.values.insert(
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(step),
                        index,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        let mut latch = evidence.program.segments[2].units.remove(0);
        latch.semantic_inputs = vec![ValueId(21)];
        latch.stack_inputs = vec![Atom::new(ValueId(21), 0)];
        evidence.program.segments.push(SelectedSegment {
            content_parameters: vec![],
            id: SegmentId(6),
            parameters: vec![ValueId(21)],
            units: vec![latch],
        });
        for (step, input, predicate, result, segment) in
            [(2, 20, 60, 61, 2), (3, 120, 130, 131, 101)]
        {
            let mut unit = evidence.program.segments[3].units[0].clone();
            unit.id = SelectedStepId(step);
            unit.source = SourceStepId(step);
            unit.semantic_inputs = vec![ValueId(input)];
            unit.stack_inputs = vec![Atom::new(ValueId(input), 0)];
            unit.outputs = vec![Atom::new(ValueId(predicate), 0), Atom::new(ValueId(result), 0)];
            if segment == 2 {
                evidence.program.segments[2].units.push(unit);
            } else {
                evidence.program.segments.push(SelectedSegment {
                    content_parameters: vec![],
                    id: SegmentId(segment),
                    parameters: vec![ValueId(input)],
                    units: vec![unit],
                });
            }
            evidence.units.insert(
                SelectedStepId(step),
                UnitEvidence {
                    aliases: LaneAliases::new(vec![(
                        Atom::new(ValueId(result), 0),
                        Atom::new(ValueId(input), 0),
                    )]),
                    ..Default::default()
                },
            );
            evidence.program.source_map.insert(
                SourceStepId(step),
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: step,
                    },
                    location: SourceLocation::default(),
                },
            );
        }
        evidence.program.segments.extend([
            SelectedSegment {
                content_parameters: vec![],
                id: SegmentId(100),
                parameters: vec![ValueId(110), ValueId(111)],
                units: vec![],
            },
            SelectedSegment {
                content_parameters: vec![],
                id: SegmentId(102),
                parameters: vec![ValueId(140)],
                units: vec![],
            },
        ]);
        evidence.program.edges[2].source = SegmentId(6);
        for (id, source, destination, kind, bindings) in [
            (5, 2, 100, EdgeKind::Forward, vec![(110, 60), (111, 61)]),
            (6, 100, 101, EdgeKind::True, vec![(120, 111)]),
            (7, 101, 100, EdgeKind::Backedge, vec![(110, 130), (111, 131)]),
            (8, 100, 102, EdgeKind::False, vec![(140, 111)]),
            (9, 102, 6, EdgeKind::Forward, vec![(21, 140)]),
        ] {
            evidence.program.edges.push(TransferEdge {
                content_bindings: vec![],
                id: EdgeId(id),
                source: SegmentId(source),
                destination: Some(SegmentId(destination)),
                kind,
                bindings: bindings
                    .into_iter()
                    .map(|(destination, source)| ParameterBinding {
                        destination: ValueId(destination),
                        source: ValueId(source),
                    })
                    .collect(),
            });
        }
        if let ControlTree::Sequence(parts) = &mut evidence.program.structure
            && let ControlTree::Loop { body, .. } = &mut parts[2]
        {
            **body = ControlTree::Sequence(vec![
                ControlTree::Segment(SegmentId(2)),
                ControlTree::Edge(EdgeId(5)),
                ControlTree::Loop {
                    id: ControlId(1),
                    condition: ControlId(11),
                    on_true: EdgeId(6),
                    on_false: EdgeId(8),
                    header: SegmentId(100),

                    before: Box::new(ControlTree::Segment(SegmentId(100))),
                    predicate: Atom::new(ValueId(110), 0),
                    body: alloc::boxed::Box::new(ControlTree::Sequence(vec![
                        ControlTree::Segment(SegmentId(101)),
                        ControlTree::Edge(EdgeId(7)),
                    ])),
                    backedges: vec![EdgeId(7)],
                },
                ControlTree::Segment(SegmentId(102)),
                ControlTree::Edge(EdgeId(9)),
                ControlTree::Segment(SegmentId(6)),
                ControlTree::Edge(EdgeId(2)),
            ]);
        }
        Ok(evidence)
    }
}

#[test]
fn nested_selected_loops_preserve_zero_trip_and_each_backedge_contract() {
    let (mut plan, frame, base) = loop_fixture(true);
    plan.selected = check_selection(
        NestedDefinition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&NestedDefinition),
    )
    .unwrap();
    for (segment, value) in [(2, 20), (6, 21), (100, 111), (101, 120), (102, 140)] {
        let selected = plan
            .selected
            .segments
            .iter()
            .find(|item| item.id == SegmentId(segment))
            .unwrap();
        plan.contracts.insert(
            selected.id,
            EntryContract {
                stack_top_first: selected
                    .parameters
                    .iter()
                    .map(|value| Atom::new(*value, 0))
                    .collect(),
                required_spill_slots: vec![(plan.spill_slots[0], Atom::new(ValueId(value), 0))],
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: TailPolicy::PreserveSuffix,
            },
        );
        plan.segment_actions.insert(
            selected.id,
            selected
                .units
                .iter()
                .map(|unit| PlanAction::RunLowering { step: unit.id })
                .collect(),
        );
    }
    for step in [2, 3] {
        plan.lowering_bindings.insert(SelectedStepId(step), LoweringBindings::default());
    }
    let mark = |boundary| AnnotatedOp::Boundary(boundary);
    let instruction = |unit, instruction| AnnotatedOp::Instruction {
        instruction,
        unit: SelectedStepId(unit),
    };
    let inner_taken = vec![
        mark(Boundary::Edge(EdgeId(6))),
        mark(Boundary::Segment(SegmentId(101))),
        mark(Boundary::UnitStart(SelectedStepId(3))),
        instruction(3, Primitive::Push(Felt::ZERO).instruction().unwrap()),
        mark(Boundary::UnitEnd(SelectedStepId(3))),
        mark(Boundary::Edge(EdgeId(7))),
    ];
    let mut iteration = vec![
        mark(Boundary::Edge(EdgeId(1))),
        mark(Boundary::Segment(SegmentId(2))),
        mark(Boundary::UnitStart(SelectedStepId(2))),
        instruction(2, Primitive::Push(Felt::ZERO).instruction().unwrap()),
        mark(Boundary::UnitEnd(SelectedStepId(2))),
        mark(Boundary::Edge(EdgeId(5))),
    ];
    iteration.extend(selected_loop(
        1,
        11,
        vec![mark(Boundary::Segment(SegmentId(100)))],
        inner_taken,
        vec![mark(Boundary::Edge(EdgeId(8)))],
    ));
    iteration.extend([
        mark(Boundary::Segment(SegmentId(102))),
        mark(Boundary::Edge(EdgeId(9))),
        mark(Boundary::Segment(SegmentId(6))),
        mark(Boundary::UnitStart(SelectedStepId(0))),
        push(1),
        inst(Instruction::Add),
        push(0),
        mark(Boundary::UnitEnd(SelectedStepId(0))),
        mark(Boundary::Edge(EdgeId(2))),
        instruction(u32::MAX, Instruction::Dup1),
        instruction(u32::MAX, Instruction::LocStore(0.into())),
    ]);
    let mut body = AnnotatedMasmBody::new();
    let mut replaced = false;
    let loop_code = selected_loop(
        0,
        10,
        vec![mark(Boundary::Segment(SegmentId(1)))],
        iteration,
        vec![mark(Boundary::Edge(EdgeId(3)))],
    );
    let mut skip = 0;
    for op in base.ops() {
        if matches!(op, AnnotatedOp::Boundary(Boundary::Control(ControlId(0)))) && !replaced {
            for item in &loop_code {
                body.push(item.clone());
            }
            replaced = true;
            skip = 2;
            continue;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        body.push(op.clone());
    }
    verify(&plan, &frame, body).unwrap();
}

struct HeaderDefinition;

impl GeneratedDefinition for HeaderDefinition {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(6)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = LoopDefinition.instantiate(parameters)?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        for id in [50, 51] {
            evidence.program.values.insert(
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(2),
                        index: id - 50,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        evidence.program.segments[1].units.push(SelectedLowering {
            publication: OutputPublication::Stack,
            address_results: vec![],
            id: SelectedStepId(2),
            kind: LoweringKind(0),
            source: SourceStepId(2),
            semantic_inputs: vec![ValueId(10), ValueId(11)],
            stack_inputs: vec![Atom::new(ValueId(10), 0), Atom::new(ValueId(11), 0)],
            stored_inputs: vec![],
            outputs: vec![Atom::new(ValueId(50), 0), Atom::new(ValueId(51), 0)],
            effects: LoweringEffects::default(),
            scratch: vec![ScratchRequirement {
                id: ScratchId(0),
                size: 1,
                alignment: 1,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Unit(SelectedStepId(2)),
                uses: vec![SelectedStepId(2)],
            }],
            scratch_effects: vec![ScratchEffect::Write(ScratchCellFact {
                object: ScratchId(0),
                offset: 0,
                content: ContentRequirement::Initialized,
            })],
        });
        evidence.units.insert(
            SelectedStepId(2),
            UnitEvidence {
                aliases: LaneAliases::new(vec![
                    (Atom::new(ValueId(50), 0), Atom::new(ValueId(10), 0)),
                    (Atom::new(ValueId(51), 0), Atom::new(ValueId(11), 0)),
                ]),
                ..Default::default()
            },
        );
        evidence.program.edges[1].bindings[0].source = ValueId(51);
        evidence.program.edges[3].bindings[0].source = ValueId(51);
        if let ControlTree::Sequence(parts) = &mut evidence.program.structure
            && let ControlTree::Loop { predicate, .. } = &mut parts[2]
        {
            *predicate = Atom::new(ValueId(50), 0);
        }
        evidence.program.source_map.insert(
            SourceStepId(2),
            SourceRecord {
                origin: SourceOrigin::Generated {
                    definition: self.id(),
                    stage: 2,
                },
                location: SourceLocation::default(),
            },
        );
        Ok(evidence)
    }
}

fn header_fixture() -> (ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let (mut plan, _, body) = loop_fixture(true);
    plan.selected = check_selection(
        HeaderDefinition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&HeaderDefinition),
    )
    .unwrap();
    plan.segment_actions.insert(
        SegmentId(1),
        vec![PlanAction::RunLowering {
            step: SelectedStepId(2),
        }],
    );
    let allocation = ScratchAllocation {
        offset: 4,
        size: 1,
        alignment: 1,
        lifetime: ScratchLifetime::Unit(SelectedStepId(2)),
    };
    plan.lowering_bindings.insert(
        SelectedStepId(2),
        LoweringBindings {
            input_order: crate::plan::InputOrder::Canonical,
            stored_inputs: BTreeMap::new(),
            scratch: BTreeMap::from([(ScratchId(0), allocation)]),
        },
    );
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(plan.spill_slots[0], 0)]),
        BTreeMap::from([(ScratchId(0), allocation)]),
        8,
    )
    .unwrap();
    let mut result = AnnotatedMasmBody::new();
    for op in body.ops() {
        if let AnnotatedOp::While { id, body } = op {
            let mut inner = AnnotatedMasmBody::new();
            for item in body.ops() {
                inner.push(item.clone());
                if matches!(item, AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1)))) {
                    inner.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(2))));
                    inner.push_instruction(Instruction::Dup1, SelectedStepId(2));
                    inner.push_instruction(Instruction::LocStore(4.into()), SelectedStepId(2));
                    inner.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(2))));
                }
            }
            result.push(AnnotatedOp::While {
                id: *id,
                body: inner,
            });
        } else {
            result.push(op.clone());
        }
    }
    (plan, frame, result)
}

/// Move an enclosing block's suffix across selected control without changing marker order.
fn move_header_suffix_into_then_arm(body: &AnnotatedMasmBody, count: usize) -> AnnotatedMasmBody {
    let mut result = AnnotatedMasmBody::new();
    for op in body.ops() {
        if let AnnotatedOp::While { id, body } = op {
            let mut header = body.ops().to_vec();
            let control =
                header.iter().position(|op| matches!(op, AnnotatedOp::If { .. })).unwrap();
            let moved: Vec<_> = header.drain(control - count..control).collect();
            let AnnotatedOp::If { then_body, .. } = &mut header[control - count] else {
                unreachable!();
            };
            let mut taken = moved;
            taken.extend_from_slice(then_body.ops());
            *then_body = ops(taken);
            result.push(AnnotatedOp::While {
                id: *id,
                body: ops(header),
            });
        } else {
            result.push(op.clone());
        }
    }
    result
}

fn assert_unowned_executable(
    plan: &ProcedurePlan,
    frame: &FinalFrameLayout,
    body: AnnotatedMasmBody,
) {
    let error = verify(plan, frame, body).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSelectedLowering);
    assert_eq!(error.message, "executable instruction lacks selected segment/edge");
}

#[test]
fn selected_then_arm_cannot_inherit_enclosing_segment_action() {
    let (mut plan, frame, body) = header_fixture();
    plan.segment_actions
        .get_mut(&SegmentId(1))
        .unwrap()
        .extend([PlanAction::PushLiteral(Felt::ZERO), PlanAction::Drop]);
    let mut original = AnnotatedMasmBody::new();
    for op in body.ops() {
        if let AnnotatedOp::While { id, body } = op {
            let mut header = AnnotatedMasmBody::new();
            for item in body.ops() {
                if matches!(item, AnnotatedOp::If { .. }) {
                    header.push_instruction(
                        Primitive::Push(Felt::ZERO).instruction().unwrap(),
                        SelectedStepId(u32::MAX),
                    );
                    header.push_instruction(Instruction::Drop, SelectedStepId(u32::MAX));
                }
                header.push(item.clone());
            }
            original.push(AnnotatedOp::While {
                id: *id,
                body: header,
            });
        } else {
            original.push(op.clone());
        }
    }
    verify(&plan, &frame, original.clone()).unwrap();
    assert_unowned_executable(&plan, &frame, move_header_suffix_into_then_arm(&original, 1));
}

#[test]
fn selected_then_arm_cannot_inherit_enclosing_segment_unit() {
    let (plan, frame, body) = header_fixture();
    verify(&plan, &frame, body.clone()).unwrap();
    assert_unowned_executable(&plan, &frame, move_header_suffix_into_then_arm(&body, 4));
}

#[test]
fn selected_while_body_cannot_inherit_enclosing_edge_action() {
    let (plan, frame, body) = loop_fixture(true);
    verify(&plan, &frame, body.clone()).unwrap();
    let mut code = body.ops().to_vec();
    let last_action = code
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::Control(ControlId(0)))))
        .unwrap()
        - 1;
    let moved = code.remove(last_action);
    let AnnotatedOp::While { body, .. } = &mut code[last_action + 2] else {
        unreachable!();
    };
    let mut header = vec![moved];
    header.extend_from_slice(body.ops());
    *body = ops(header);
    assert_unowned_executable(&plan, &frame, ops(code));
}

#[test]
fn nonempty_header_executes_inside_selected_while_and_matches_vm() -> Result<(), midenc_hir::Report>
{
    use miden_processor::{DefaultHost, FastProcessor, StackInputs};
    let (plan, frame, body) = header_fixture();
    verify(&plan, &frame, body.clone()).unwrap();
    fn render(body: &AnnotatedMasmBody) -> alloc::string::String {
        body.ops()
            .iter()
            .map(|op| match op {
                AnnotatedOp::Boundary(_) => alloc::string::String::new(),
                AnnotatedOp::Instruction { instruction, .. } => alloc::format!("{instruction} "),
                AnnotatedOp::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    alloc::format!("if.true {} else {} end ", render(then_body), render(else_body))
                }
                AnnotatedOp::While { body, .. } => {
                    alloc::format!("while.true {} end ", render(body))
                }
                _ => panic!("unsupported fixture syntax"),
            })
            .collect()
    }
    for (predicate, expected) in [(0, 7), (1, 8)] {
        let source = alloc::format!(
            "@locals(8) proc test {} loc_load.4 push.{expected} assert_eq end begin exec.test \
             push.{expected} assert_eq end",
            render(&body)
        );
        let package =
            miden_assembly::Assembler::default().assemble_program("checked_loop", source)?;
        let program = package.try_into_program()?;
        FastProcessor::new(
            StackInputs::new(&[Felt::new_unchecked(predicate), Felt::new_unchecked(7)]).unwrap(),
        )
        .execute_sync(&program, &mut DefaultHost::default())
        .map_err(|error| midenc_hir::Report::msg(error.to_string()))?;
    }
    Ok(())
}

#[test]
fn selected_while_cannot_be_replaced_with_repeat_or_moved_header() {
    let (plan, frame, body) = header_fixture();
    let mut repeat = AnnotatedMasmBody::new();
    for op in body.ops() {
        repeat.push(if let AnnotatedOp::While { id, body } = op {
            AnnotatedOp::Repeat {
                id: *id,
                count: 2,
                body: body.clone(),
            }
        } else {
            op.clone()
        });
    }
    assert_eq!(
        verify(&plan, &frame, repeat).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut moved = AnnotatedMasmBody::new();
    for op in body.ops() {
        if let AnnotatedOp::While { id, body } = op {
            for item in &body.ops()[..5] {
                moved.push(item.clone());
            }
            moved.push(AnnotatedOp::While {
                id: *id,
                body: ops(body.ops()[5..].to_vec()),
            });
        } else {
            moved.push(op.clone());
        }
    }
    assert_eq!(
        verify(&plan, &frame, moved).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn forged_synthetic_flags_are_rejected_before_physical_replay() {
    let (plan, frame, body) = loop_fixture(true);
    let mut invalid = AnnotatedMasmBody::new();
    let mut change = false;
    for op in body.ops() {
        if change {
            invalid.push(AnnotatedOp::Instruction {
                instruction: Primitive::Push(Felt::ZERO).instruction().unwrap(),
                unit: SelectedStepId(u32::MAX),
            });
            change = false;
        } else {
            invalid.push(op.clone());
            if matches!(op, AnnotatedOp::Boundary(Boundary::Control(ControlId(0)))) {
                change = true;
            }
        }
    }
    assert_eq!(
        verify(&plan, &frame, invalid).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

fn selected_loop(
    id: u32,
    condition: u32,
    mut header: Vec<AnnotatedOp>,
    mut taken: Vec<AnnotatedOp>,
    mut exit: Vec<AnnotatedOp>,
) -> Vec<AnnotatedOp> {
    let flag = |value| {
        vec![
            AnnotatedOp::Boundary(Boundary::Control(ControlId(id))),
            AnnotatedOp::Instruction {
                instruction: Primitive::Push(if value { Felt::ONE } else { Felt::ZERO })
                    .instruction()
                    .unwrap(),
                unit: SelectedStepId(u32::MAX),
            },
        ]
    };
    taken.extend(flag(true));
    exit.extend(flag(false));
    header.push(AnnotatedOp::If {
        id: ControlId(condition),
        then_body: ops(taken),
        else_body: ops(exit),
    });
    let mut result = flag(true);
    result.push(AnnotatedOp::While {
        id: ControlId(id),
        body: ops(header),
    });
    result
}
