use super::*;

struct ContentDiamond;
impl GeneratedDefinition for ContentDiamond {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(43)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = super::diamond::Diamond.instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        for index in 0..3 {
            let segment = &mut evidence.program.segments[index + 1];
            segment.parameters.clear();
            let unit = &mut segment.units[0];
            unit.semantic_inputs.clear();
            unit.stack_inputs.clear();
            evidence.units.get_mut(&unit.id).unwrap().aliases = LaneAliases::default();
            let fact = ScratchCellFact {
                object: ScratchId(0),
                offset: 0,
                content: ContentRequirement::Content(ContentId(index as u32)),
            };
            unit.scratch_effects = vec![if index == 2 {
                ScratchEffect::Read(fact)
            } else {
                ScratchEffect::Write(fact)
            }];
            if index < 2 {
                unit.outputs.clear();
            }
        }
        evidence.program.segments[3].content_parameters = vec![ContentId(2), ContentId(3)];
        for index in 0..2 {
            let edge = &mut evidence.program.edges[index + 2];
            edge.bindings.clear();
            edge.content_bindings = [2, 3]
                .map(|destination| ContentBinding {
                    destination: ContentId(destination),
                    source: ContentId(index as u32),
                })
                .to_vec();
        }
        Ok(evidence)
    }
}
fn fixture() -> (ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let selected = check_selection(
        ContentDiamond.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&ContentDiamond),
    )
    .unwrap();
    let allocation = ScratchAllocation {
        offset: 0,
        size: 1,
        alignment: 1,
        lifetime: ScratchLifetime::Spanning,
    };
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::new(),
        BTreeMap::from([(ScratchId(0), allocation)]),
        4,
    )
    .unwrap();
    let contracts = selected
        .segments
        .iter()
        .map(|segment| {
            (
                segment.id,
                EntryContract {
                    stack_top_first: segment
                        .parameters
                        .iter()
                        .map(|value| Atom::new(*value, 0))
                        .collect(),
                    required_spill_slots: vec![],
                    required_scratch: segment
                        .content_parameters
                        .iter()
                        .map(|id| ScratchCellFact {
                            object: ScratchId(0),
                            offset: 0,
                            content: ContentRequirement::Content(*id),
                        })
                        .collect(),
                    address_facts: vec![],
                    aliases: LaneAliases::default(),
                    tail: TailPolicy::PreserveSuffix,
                },
            )
        })
        .collect();
    let segment_actions = selected
        .segments
        .iter()
        .map(|segment| {
            (
                segment.id,
                segment
                    .units
                    .iter()
                    .map(|unit| PlanAction::RunLowering { step: unit.id })
                    .collect(),
            )
        })
        .collect();
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts,
        segment_actions,
        edge_actions: BTreeMap::new(),
        spill_slots: vec![],
        semantic_storage_reads: Default::default(),
        lowering_bindings: (0..3)
            .map(|index| {
                (
                    SelectedStepId(index),
                    LoweringBindings {
                        input_order: crate::plan::InputOrder::Canonical,
                        stored_inputs: BTreeMap::new(),
                        scratch: BTreeMap::from([(ScratchId(0), allocation)]),
                    },
                )
            })
            .collect(),
    };
    let boundary = |marker| AnnotatedOp::Boundary(marker);
    let arm = |index, value| {
        let step = SelectedStepId(index);
        let mut result = ops(vec![
            boundary(Boundary::Edge(EdgeId(index))),
            boundary(Boundary::Segment(SegmentId(index + 1))),
            boundary(Boundary::UnitStart(step)),
        ]);
        result.push_instruction(
            Primitive::Push(Felt::new_unchecked(value)).instruction().unwrap(),
            step,
        );
        result.push_instruction(Instruction::LocStore(0.into()), step);
        result.push(boundary(Boundary::UnitEnd(step)));
        result.push(boundary(Boundary::Edge(EdgeId(index + 2))));
        result
    };
    let mut body = ops(vec![
        boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::If {
            id: ControlId(0),
            then_body: arm(0, 11),
            else_body: arm(1, 29),
        },
        boundary(Boundary::Segment(SegmentId(3))),
        boundary(Boundary::UnitStart(SelectedStepId(2))),
    ]);
    body.push_instruction(Instruction::LocLoad(0.into()), SelectedStepId(2));
    body.push(boundary(Boundary::UnitEnd(SelectedStepId(2))));
    body.push(boundary(Boundary::Edge(EdgeId(4))));
    (plan, frame, body)
}
#[test]
fn distinct_branch_contents_bind_repeated_join_parameters_without_ssa_demand() {
    let (plan, frame, body) = fixture();
    verify(&plan, &frame, body).unwrap();
}

#[test]
fn content_bindings_reject_missing_duplicate_and_undefined_sources() {
    struct InvalidContent(u8);
    impl GeneratedDefinition for InvalidContent {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(45)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = ContentDiamond.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            match self.0 {
                0 => {
                    evidence.program.edges[2].content_bindings.pop();
                }
                1 => {
                    evidence.program.edges[2].content_bindings[1].destination = ContentId(2);
                }
                2 => {
                    evidence.program.edges[2].content_bindings[0].source = ContentId(99);
                }
                3 => {
                    evidence.program.edges[2].content_bindings[0].source = ContentId(1);
                }
                _ => {
                    evidence.program.segments[3].content_parameters.push(ContentId(0));
                }
            }
            Ok(evidence)
        }
    }
    for mutation in 0..5 {
        let definition = InvalidContent(mutation);
        assert!(
            check_selection(
                definition.instantiate(&[]).unwrap().program,
                &SourceContext::Generated(&definition)
            )
            .is_err(),
            "mutation={mutation}"
        );
    }
}

#[test]
fn content_demand_cannot_be_erased_by_omitting_destination_facts() {
    let (mut plan, frame, body) = fixture();
    plan.contracts.get_mut(&SegmentId(3)).unwrap().required_scratch.clear();
    assert!(verify(&plan, &frame, body).is_err());
}

struct ContentLoop {
    changing: bool,
    stale: bool,
    zero_trip: bool,
}
impl GeneratedDefinition for ContentLoop {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(46)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = super::loops::LoopDefinition.instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let requirement = ScratchRequirement {
            id: ScratchId(0),
            size: 1,
            alignment: 1,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![SelectedStepId(0), SelectedStepId(1), SelectedStepId(2)],
        };
        let fact = |id| ScratchCellFact {
            object: ScratchId(0),
            offset: 0,
            content: ContentRequirement::Content(ContentId(id)),
        };
        let init = SelectedLowering {
            publication: OutputPublication::Stack,
            address_results: vec![],
            id: SelectedStepId(2),
            kind: LoweringKind(0),
            source: SourceStepId(2),
            semantic_inputs: vec![],
            stack_inputs: vec![],
            stored_inputs: vec![],
            outputs: vec![],
            effects: LoweringEffects::default(),
            scratch: vec![requirement.clone()],
            scratch_effects: vec![ScratchEffect::Write(fact(0))],
        };
        let mut init = init;
        if self.zero_trip {
            init.semantic_inputs = vec![ValueId(0)];
            init.stack_inputs = vec![Atom::new(ValueId(0), 0)];
            init.outputs = vec![Atom::new(ValueId(50), 0)];
            evidence.program.values.insert(
                ValueId(50),
                SelectedValue {
                    id: ValueId(50),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(2),
                        index: 0,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: Some(Reconstruction::Literal(vec![Felt::ZERO])),
                },
            );
            evidence.program.edges[0].bindings[0].source = ValueId(50);
        }
        evidence.program.segments[0].units.push(init);
        evidence.units.insert(SelectedStepId(2), UnitEvidence::default());
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
        for (segment, content) in [(1, 10), (2, 20), (3, 40)] {
            evidence.program.segments[segment].content_parameters = vec![ContentId(content)];
        }
        let body = &mut evidence.program.segments[2].units[0];
        body.scratch = vec![requirement.clone()];
        body.scratch_effects = vec![ScratchEffect::Read(fact(20))];
        if self.changing {
            body.scratch_effects.push(ScratchEffect::Write(fact(30)));
        }
        let exit = &mut evidence.program.segments[3].units[0];
        exit.scratch = vec![requirement];
        exit.scratch_effects = vec![ScratchEffect::Read(fact(40))];
        for (edge, destination, source) in [
            (0, 10, 0),
            (1, 20, 10),
            (2, 10, if self.changing && !self.stale { 30 } else { 20 }),
            (3, 40, 10),
        ] {
            evidence.program.edges[edge].content_bindings = vec![ContentBinding {
                destination: ContentId(destination),
                source: ContentId(source),
            }];
        }
        Ok(evidence)
    }
}
fn content_loop(
    changing: bool,
    stale: bool,
    omit_init: bool,
    zero_trip: bool,
) -> Result<VerifiedProcedure, VerificationError> {
    let (mut plan, _, body) = super::loops::loop_fixture(true);
    let definition = ContentLoop {
        changing,
        stale,
        zero_trip,
    };
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let allocation = ScratchAllocation {
        offset: 1,
        size: 1,
        alignment: 1,
        lifetime: ScratchLifetime::Spanning,
    };
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(plan.spill_slots[0], 0)]),
        BTreeMap::from([(ScratchId(0), allocation)]),
        4,
    )
    .unwrap();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![PlanAction::RunLowering {
            step: SelectedStepId(2),
        }],
    );
    for (segment, content) in [(1, 10), (2, 20), (3, 40)] {
        plan.contracts.get_mut(&SegmentId(segment)).unwrap().required_scratch =
            vec![ScratchCellFact {
                object: ScratchId(0),
                offset: 0,
                content: ContentRequirement::Content(ContentId(content)),
            }];
    }
    for step in 0..3 {
        plan.lowering_bindings.insert(
            SelectedStepId(step),
            LoweringBindings {
                input_order: crate::plan::InputOrder::Canonical,
                stored_inputs: BTreeMap::new(),
                scratch: BTreeMap::from([(ScratchId(0), allocation)]),
            },
        );
    }
    fn rewrite(
        body: &AnnotatedMasmBody,
        changing: bool,
        omit_init: bool,
        zero_trip: bool,
    ) -> AnnotatedMasmBody {
        let mut result = AnnotatedMasmBody::new();
        for op in body.ops() {
            match op {
                AnnotatedOp::While { id, body } => result.push(AnnotatedOp::While {
                    id: *id,
                    body: rewrite(body, changing, omit_init, zero_trip),
                }),
                AnnotatedOp::If {
                    id,
                    then_body,
                    else_body,
                } => result.push(AnnotatedOp::If {
                    id: *id,
                    then_body: rewrite(then_body, changing, omit_init, zero_trip),
                    else_body: rewrite(else_body, changing, omit_init, zero_trip),
                }),
                _ => result.push(op.clone()),
            }
            if matches!(op, AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0)))) {
                result.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(2))));
                if zero_trip {
                    result.push_instruction(Instruction::Drop, SelectedStepId(2));
                }
                if !omit_init {
                    result.push_instruction(
                        Primitive::Push(Felt::new_unchecked(11)).instruction().unwrap(),
                        SelectedStepId(2),
                    );
                    result.push_instruction(Instruction::LocStore(1.into()), SelectedStepId(2));
                }
                if zero_trip {
                    result.push_instruction(
                        Primitive::Push(Felt::ZERO).instruction().unwrap(),
                        SelectedStepId(2),
                    );
                }
                result.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(2))));
            }
            if changing
                && matches!(op, AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))))
            {
                result.push_instruction(Instruction::Dup0, SelectedStepId(0));
                result.push_instruction(Instruction::LocStore(1.into()), SelectedStepId(0));
            }
        }
        result
    }
    verify(&plan, &frame, rewrite(&body, changing, omit_init, zero_trip))
}
#[test]
fn content_loop_checks_changing_invariant_and_zero_trip_entries() {
    for changing in [false, true] {
        for zero_trip in [false, true] {
            content_loop(changing, false, false, zero_trip).unwrap();
        }
    }
}
#[test]
fn content_loop_rejects_stale_backedge_and_missing_zero_trip_initialization() {
    assert!(content_loop(true, true, false, false).is_err());
    for changing in [false, true] {
        for zero_trip in [false, true] {
            assert!(content_loop(changing, false, true, zero_trip).is_err());
        }
    }
}
