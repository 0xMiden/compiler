use super::*;

pub(super) struct Diamond;
impl GeneratedDefinition for Diamond {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(3)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Fixture::new(1, 1).instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let predicate = ValueId(0);
        let parameter = ValueId(200);
        evidence.program.values.insert(
            parameter,
            SelectedValue {
                id: parameter,
                origin: ValueOrigin::Parameter {
                    segment: SegmentId(3),
                    index: 0,
                },
                layout: ValueLayout::new(Type::Felt).unwrap(),
                reconstruction: None,
            },
        );
        let requirement = ScratchRequirement {
            id: ScratchId(0),
            size: 1,
            alignment: 1,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![SelectedStepId(0), SelectedStepId(1), SelectedStepId(2)],
        };
        let fact = ScratchCellFact {
            object: ScratchId(0),
            offset: 0,
            content: ContentRequirement::Content(ContentId(0)),
        };
        evidence.program.segments = vec![SelectedSegment {
            content_parameters: vec![],
            id: SegmentId(0),
            parameters: vec![predicate],
            units: vec![],
        }];
        evidence.units.clear();
        evidence.program.source_map.clear();
        for index in 0..3 {
            let output = ValueId(100 + index);
            evidence.program.values.insert(
                output,
                SelectedValue {
                    id: output,
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(index),
                        index: 0,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
            let input = if index == 2 {
                vec![Atom::new(parameter, 0)]
            } else {
                vec![]
            };
            let unit = SelectedLowering {
                address_results: vec![],
                id: SelectedStepId(index),
                kind: LoweringKind(0),
                source: SourceStepId(index),
                semantic_inputs: if index == 2 { vec![parameter] } else { vec![] },
                stack_inputs: input,
                stored_inputs: vec![],
                outputs: vec![Atom::new(output, 0)],
                effects: LoweringEffects::default(),
                scratch: vec![requirement.clone()],
                scratch_effects: vec![if index == 2 {
                    ScratchEffect::Read(ScratchCellFact {
                        content: ContentRequirement::Content(ContentId(2)),
                        ..fact
                    })
                } else {
                    ScratchEffect::Write(ScratchCellFact {
                        content: ContentRequirement::Content(ContentId(index)),
                        ..fact
                    })
                }],
            };
            evidence.program.segments.push(SelectedSegment {
                content_parameters: vec![],
                id: SegmentId(index + 1),
                parameters: if index == 2 { vec![parameter] } else { vec![] },
                units: vec![unit],
            });
            evidence.units.insert(
                SelectedStepId(index),
                UnitEvidence {
                    aliases: if index == 2 {
                        LaneAliases::new(vec![(Atom::new(output, 0), Atom::new(parameter, 0))])
                    } else {
                        LaneAliases::default()
                    },
                    ..Default::default()
                },
            );
            evidence.program.source_map.insert(
                SourceStepId(index),
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: index,
                    },
                    location: SourceLocation::default(),
                },
            );
        }
        evidence.program.edges = vec![
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(0),
                source: SegmentId(0),
                destination: Some(SegmentId(1)),
                kind: EdgeKind::True,
                bindings: vec![],
            },
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(1),
                source: SegmentId(0),
                destination: Some(SegmentId(2)),
                kind: EdgeKind::False,
                bindings: vec![],
            },
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(2),
                source: SegmentId(1),
                destination: Some(SegmentId(3)),
                kind: EdgeKind::Forward,
                bindings: vec![ParameterBinding {
                    destination: parameter,
                    source: ValueId(100),
                }],
            },
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(3),
                source: SegmentId(2),
                destination: Some(SegmentId(3)),
                kind: EdgeKind::Forward,
                bindings: vec![ParameterBinding {
                    destination: parameter,
                    source: ValueId(101),
                }],
            },
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(4),
                source: SegmentId(3),
                destination: None,
                kind: EdgeKind::Return,
                bindings: vec![ParameterBinding {
                    destination: ValueId(102),
                    source: ValueId(102),
                }],
            },
        ];
        evidence.program.segments[3].content_parameters = vec![ContentId(2)];
        for index in 0..2 {
            evidence.program.edges[index + 2].content_bindings = vec![ContentBinding {
                destination: ContentId(2),
                source: ContentId(index as u32),
            }];
        }
        evidence.program.structure = ControlTree::Sequence(vec![
            ControlTree::Segment(SegmentId(0)),
            ControlTree::If {
                id: ControlId(0),
                predicate: Atom::new(predicate, 0),
                on_true: EdgeId(0),
                then_body: alloc::boxed::Box::new(ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(1)),
                    ControlTree::Edge(EdgeId(2)),
                ])),
                on_false: EdgeId(1),
                else_body: alloc::boxed::Box::new(ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(2)),
                    ControlTree::Edge(EdgeId(3)),
                ])),
            },
            ControlTree::Segment(SegmentId(3)),
            ControlTree::Edge(EdgeId(4)),
        ]);
        Ok(evidence)
    }
}
#[test]
fn spanning_content_definitions_merge_with_physical_cells_at_selected_join() {
    let selected = check_selection(
        Diamond.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&Diamond),
    )
    .unwrap();
    let allocation = ScratchAllocation {
        offset: 0,
        size: 1,
        alignment: 1,
        lifetime: ScratchLifetime::Spanning,
    };
    let mut contracts = BTreeMap::new();
    let mut actions = BTreeMap::new();
    for segment in &selected.segments {
        contracts.insert(
            segment.id,
            EntryContract {
                stack_top_first: segment
                    .parameters
                    .iter()
                    .map(|value| Atom::new(*value, 0))
                    .collect(),
                required_spill_slots: vec![],
                required_scratch: if segment.id == SegmentId(3) {
                    vec![
                        ScratchCellFact {
                            object: ScratchId(0),
                            offset: 0,
                            content: ContentRequirement::Content(ContentId(2)),
                        },
                        ScratchCellFact {
                            object: ScratchId(0),
                            offset: 0,
                            content: ContentRequirement::Atom(Atom::new(ValueId(200), 0)),
                        },
                    ]
                } else {
                    vec![]
                },
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
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts,
        segment_actions: actions,
        edge_actions: BTreeMap::new(),
        spill_slots: vec![],
        lowering_bindings: (0..3)
            .map(|index| {
                (
                    SelectedStepId(index),
                    LoweringBindings {
                        stored_inputs: BTreeMap::new(),
                        scratch: BTreeMap::from([(ScratchId(0), allocation)]),
                    },
                )
            })
            .collect(),
    };
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::new(),
        BTreeMap::from([(ScratchId(0), allocation)]),
        4,
    )
    .unwrap();
    let boundary = |marker| AnnotatedOp::Boundary(marker);
    let arm = |index: u32, value: u64| {
        let mut code = vec![
            boundary(Boundary::Edge(EdgeId(index))),
            boundary(Boundary::Segment(SegmentId(index + 1))),
            boundary(Boundary::UnitStart(SelectedStepId(index))),
        ];
        for instruction in [
            Primitive::Push(Felt::new_unchecked(value)).instruction().unwrap(),
            Instruction::Dup0,
            Instruction::LocStore(0.into()),
        ] {
            code.push(AnnotatedOp::Instruction {
                instruction,
                unit: SelectedStepId(index),
            });
        }
        code.extend([
            boundary(Boundary::UnitEnd(SelectedStepId(index))),
            boundary(Boundary::Edge(EdgeId(index + 2))),
        ]);
        ops(code)
    };
    let body = ops(vec![
        boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::If {
            id: ControlId(0),
            then_body: arm(0, 11),
            else_body: arm(1, 29),
        },
        boundary(Boundary::Segment(SegmentId(3))),
        boundary(Boundary::UnitStart(SelectedStepId(2))),
        boundary(Boundary::UnitEnd(SelectedStepId(2))),
        boundary(Boundary::Edge(EdgeId(4))),
    ]);
    verify(&plan, &frame, body.clone()).unwrap();
    let mut invalid = AnnotatedMasmBody::new();
    for (index, op) in body.ops().iter().enumerate() {
        invalid.push(op.clone());
        if index == 0 {
            invalid.push_instruction(Instruction::Drop, SelectedStepId(u32::MAX));
            invalid.push_instruction(
                Primitive::Push(Felt::ZERO).instruction().unwrap(),
                SelectedStepId(u32::MAX),
            );
        }
    }
    let mut plan = plan;
    plan.segment_actions
        .insert(SegmentId(0), vec![PlanAction::Drop, PlanAction::PushLiteral(Felt::ZERO)]);
    assert_eq!(
        verify(&plan, &frame, invalid).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
}
