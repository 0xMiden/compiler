use super::*;
use crate::{
    allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
    lower::expand::{expand_allocation_action, resolve_frame_bindings},
};

fn object(id: u32, owner: u32) -> ScratchRequirement {
    ScratchRequirement {
        id: ScratchId(id),
        size: 1,
        alignment: 1,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Unit(SelectedStepId(owner)),
        uses: vec![SelectedStepId(owner)],
    }
}

struct Phases;
impl GeneratedDefinition for Phases {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(470)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Fixture::new(0, 0).instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let mut second = evidence.program.segments[0].units[0].clone();
        evidence.program.segments[0].units[0].scratch = vec![object(0, 0)];
        second.id = SelectedStepId(1);
        second.source = SourceStepId(1);
        second.scratch = vec![object(1, 1)];
        evidence.program.segments[0].units.push(second);
        evidence.units.insert(SelectedStepId(1), UnitEvidence::default());
        evidence.program.source_map = (0..2)
            .map(|id| {
                (
                    SourceStepId(id),
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: id,
                        },
                        location: SourceLocation::default(),
                    },
                )
            })
            .collect();
        Ok(evidence)
    }
}

fn phases() -> (ProcedurePlan, FinalFrameLayout) {
    let selected = check_selection(
        Phases.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&Phases),
    )
    .unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    (plan, frame)
}

fn phase_body(frame: &FinalFrameLayout, initialize_second: bool) -> AnnotatedMasmBody {
    let mut body = AnnotatedMasmBody::new();
    body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    for id in 0..2 {
        body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(id))));
        let offset = frame
            .resolve(FrameRef::Scratch {
                object: ScratchId(id),
                offset: 0,
            })
            .unwrap();
        if id == 0 || initialize_second {
            body.push_instruction(
                Primitive::Push(Felt::new_unchecked(17 + u64::from(id))).instruction().unwrap(),
                SelectedStepId(id),
            );
            body.push_instruction(Instruction::LocStore(offset.into()), SelectedStepId(id));
        }
        body.push_instruction(Instruction::LocLoad(offset.into()), SelectedStepId(id));
        body.push_instruction(Instruction::Drop, SelectedStepId(id));
        body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(id))));
    }
    body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    body
}

#[test]
fn new_workspace_owner_does_not_inherit_initialization_or_require_runtime_clearing() {
    let (plan, frame) = phases();
    assert_eq!(frame.scratch()[&ScratchId(0)].offset, frame.scratch()[&ScratchId(1)].offset);
    assert_eq!(
        verify(&plan, &frame, phase_body(&frame, false)).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    verify(&plan, &frame, phase_body(&frame, true)).unwrap();
}

#[test]
fn final_scratch_binding_domain_and_ranges_are_checked() {
    let (plan, frame) = phases();
    for mutation in 0..3 {
        let mut altered = plan.clone();
        let binding = altered.lowering_bindings.get_mut(&SelectedStepId(1)).unwrap();
        match mutation {
            0 => {
                binding.scratch.get_mut(&ScratchId(1)).unwrap().offset += 1;
            }
            1 => {
                binding.scratch.remove(&ScratchId(1));
            }
            _ => {
                binding.scratch.insert(ScratchId(9), frame.scratch()[&ScratchId(1)]);
            }
        }
        assert!(verify(&altered, &frame, phase_body(&frame, true)).is_err());
    }
    let mut extra = frame.scratch().clone();
    extra.insert(
        ScratchId(9),
        ScratchAllocation {
            offset: 1,
            ..frame.scratch()[&ScratchId(1)]
        },
    );
    let extra_frame = FinalFrameLayout::new(
        frame.source().clone(),
        frame.spills().clone(),
        extra,
        frame.aligned_size(),
    )
    .unwrap();
    assert!(verify(&plan, &extra_frame, phase_body(&frame, true)).is_err());
    for offset in [1, 4] {
        let mut invalid = AnnotatedMasmBody::new();
        for op in phase_body(&frame, true).ops() {
            invalid.push(match op {
                AnnotatedOp::Instruction {
                    instruction: Instruction::LocLoad(_),
                    unit,
                } => AnnotatedOp::Instruction {
                    instruction: Instruction::LocLoad(offset.into()),
                    unit: *unit,
                },
                other => other.clone(),
            });
        }
        assert_eq!(
            verify(&plan, &frame, invalid).unwrap_err().category,
            if offset == 4 {
                DiagnosticCategory::FrameCapacityExceeded
            } else {
                DiagnosticCategory::InvalidSpillSlotRead
            }
        );
    }
}

#[test]
fn failed_frame_binding_resolution_does_not_partially_publish_bindings() {
    let (mut plan, frame) = phases();
    plan.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().scratch.clear();
    plan.lowering_bindings.remove(&SelectedStepId(1));
    assert!(resolve_frame_bindings(&mut plan, &frame).is_err());
    assert!(plan.lowering_bindings[&SelectedStepId(0)].scratch.is_empty());
    assert!(!plan.lowering_bindings.contains_key(&SelectedStepId(1)));
}

#[test]
fn final_map_collision_replays_real_writes_even_with_unchanged_abstract_facts() {
    let mut plan = Fixture::new(0, 0).plan();
    let a = SpillSlotId::temporary(SelectedStepId(0), 0);
    let b = SpillSlotId::temporary(SelectedStepId(0), 1);
    let actions = vec![
        PlanAction::PushLiteral(Felt::new_unchecked(17)),
        PlanAction::StoreLane(a),
        PlanAction::PushLiteral(Felt::new_unchecked(23)),
        PlanAction::StoreLane(b),
        PlanAction::LoadLane(a),
        PlanAction::Drop,
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.spill_slots = vec![a, b];
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    assert_ne!(frame.spills()[&a], frame.spills()[&b]);
    let expand = |frame| {
        let mut body = AnnotatedMasmBody::new();
        body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        for action in &actions {
            if matches!(action, PlanAction::RunLowering { .. }) {
                body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
                body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
            } else if let Some(inst) = expand_allocation_action(action, frame).unwrap() {
                body.push_instruction(inst, SelectedStepId(u32::MAX));
            }
        }
        body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
        body
    };
    verify(&plan, &frame, expand(&frame)).unwrap();
    let collision = FinalFrameLayout::new(
        frame.source().clone(),
        BTreeMap::from([(a, 0), (b, 0)]),
        BTreeMap::new(),
        frame.aligned_size(),
    )
    .unwrap();
    assert_eq!(
        verify(&plan, &collision, expand(&collision)).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

fn borrow_fixture(write: bool, definite: bool) -> (Fixture, Vec<AnnotatedOp>) {
    let mut fixture = Fixture::new(1, 1).scratch(1);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 0).unwrap();
    fixture.effects.call = Some(abi.invocation(Invocation::Exec).unwrap());
    fixture.scratch_effects = vec![ScratchEffect::Borrow {
        object: ScratchId(0),
        offset: 0,
        size: 1,
        read: true,
        write,
        definite_write: definite,
        retained: false,
    }];
    let target = crate::masm::InvocationTarget::Symbol(crate::masm::Ident::new("callee").unwrap());
    fixture.proof.calls.push(CallSite {
        instruction: Instruction::Exec(target.clone()),
        abi,
        inputs: vec![CallArgument::Local],
        path: vec![],
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    });
    let code = vec![
        inst(Instruction::Dup0),
        inst(Instruction::LocStore(0.into())),
        inst(Instruction::Locaddr(0.into())),
        inst(Instruction::Exec(target)),
    ];
    (fixture, code)
}

#[test]
fn readonly_borrow_preserves_exact_token_and_possible_write_invalidates_it() {
    let (readonly, code) = borrow_fixture(false, false);
    readonly.run(code).unwrap();
    for definite in [false, true] {
        let (writer, code) = borrow_fixture(true, definite);
        assert_eq!(
            writer.run(code).unwrap_err().category,
            DiagnosticCategory::InvalidSpillSlotRead
        );
    }
}

#[test]
fn possible_borrow_write_preserves_initialization_but_cannot_create_it() {
    let (mut fixture, mut code) = borrow_fixture(true, false);
    fixture.proof.scratch_outputs.clear();
    code.extend([inst(Instruction::LocLoad(0.into())), inst(Instruction::Drop)]);
    fixture.run(code.clone()).unwrap();
    // Omit the initializing duplicate/store and the borrow's read requirement.
    code.drain(..2);
    let ScratchEffect::Borrow { read, .. } = &mut fixture.scratch_effects[0] else {
        unreachable!()
    };
    *read = false;
    assert_eq!(
        fixture.run(code.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    let ScratchEffect::Borrow { definite_write, .. } = &mut fixture.scratch_effects[0] else {
        unreachable!()
    };
    *definite_write = true;
    fixture.run(code).unwrap();
}

#[test]
fn unknown_retained_and_out_of_range_borrows_fail() {
    let (fixture, code) = borrow_fixture(false, false);
    for mutation in 0..4 {
        let mut fixture = fixture.clone();
        match mutation {
            0 => fixture.scratch_effects.clear(),
            1 => {
                let ScratchEffect::Borrow { retained, .. } = &mut fixture.scratch_effects[0] else {
                    unreachable!()
                };
                *retained = true;
            }
            2 => {
                let ScratchEffect::Borrow { size, .. } = &mut fixture.scratch_effects[0] else {
                    unreachable!()
                };
                *size = 2;
            }
            _ => {
                let ScratchEffect::Borrow { offset, .. } = &mut fixture.scratch_effects[0] else {
                    unreachable!()
                };
                *offset = u32::MAX;
            }
        }
        let candidate = fixture.instantiate(&[]).unwrap().program;
        let result = check_selection(candidate, &SourceContext::Generated(&fixture));
        if mutation >= 2 {
            assert!(result.is_err());
        } else {
            assert!(fixture.run(code.clone()).is_err());
        }
    }
}

#[test]
fn possible_write_summary_has_the_same_initialization_rules_as_borrow() {
    let (mut fixture, mut code) = borrow_fixture(false, false);
    fixture.proof.scratch_outputs.clear();
    fixture.scratch_effects.push(ScratchEffect::PossibleWrite {
        object: ScratchId(0),
        offset: 0,
        size: 1,
    });
    code.extend([inst(Instruction::LocLoad(0.into())), inst(Instruction::Drop)]);
    fixture.run(code.clone()).unwrap();
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    assert_eq!(
        fixture.run(code.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    fixture.proof.scratch_outputs.clear();
    code.drain(..2);
    let ScratchEffect::Borrow { read, .. } = &mut fixture.scratch_effects[0] else {
        unreachable!()
    };
    *read = false;
    assert_eq!(
        fixture.run(code).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

struct LocalLoop;
impl GeneratedDefinition for LocalLoop {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(471)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = super::loops::LoopDefinition.instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        for unit in evidence.program.segments.iter_mut().flat_map(|segment| &mut segment.units) {
            unit.scratch = vec![object(unit.id.0, unit.id.0)];
        }
        for (id, source) in &mut evidence.program.source_map {
            source.origin = SourceOrigin::Generated {
                definition: self.id(),
                stage: id.0,
            };
        }
        Ok(evidence)
    }
}

#[test]
fn repeated_selected_loop_units_cannot_inherit_workspace_initialization() {
    let selected = check_selection(
        LocalLoop.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&LocalLoop),
    )
    .unwrap();
    let (mut plan, _, original) = super::loops::loop_fixture(true);
    plan.selected = selected;
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    assert!(frame.spills().values().all(|offset| *offset == 0));
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    fn inject(
        body: &AnnotatedMasmBody,
        frame: &FinalFrameLayout,
        initialize: bool,
    ) -> AnnotatedMasmBody {
        let mut out = AnnotatedMasmBody::new();
        for op in body.ops() {
            match op {
                AnnotatedOp::Boundary(Boundary::UnitStart(id)) => {
                    out.push(op.clone());
                    let offset = frame
                        .resolve(FrameRef::Scratch {
                            object: ScratchId(id.0),
                            offset: 0,
                        })
                        .unwrap();
                    if id.0 == 0 || initialize {
                        out.push_instruction(
                            Primitive::Push(Felt::ONE).instruction().unwrap(),
                            *id,
                        );
                        out.push_instruction(Instruction::LocStore(offset.into()), *id);
                    }
                    out.push_instruction(Instruction::LocLoad(offset.into()), *id);
                    out.push_instruction(Instruction::Drop, *id);
                }
                AnnotatedOp::If {
                    id,
                    then_body,
                    else_body,
                } => out.push(AnnotatedOp::If {
                    id: *id,
                    then_body: inject(then_body, frame, initialize),
                    else_body: inject(else_body, frame, initialize),
                }),
                AnnotatedOp::While { id, body } => out.push(AnnotatedOp::While {
                    id: *id,
                    body: inject(body, frame, initialize),
                }),
                _ => out.push(op.clone()),
            }
        }
        out
    }
    verify(&plan, &frame, inject(&original, &frame, true)).unwrap();
    assert_eq!(
        verify(&plan, &frame, inject(&original, &frame, false)).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

struct RootPhases;
impl GeneratedDefinition for RootPhases {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(472)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Phases.instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let root = ScratchRequirement {
            id: ScratchId(0),
            size: 4,
            alignment: 4,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![SelectedStepId(0), SelectedStepId(1)],
        };
        for unit in &mut evidence.program.segments[0].units {
            unit.scratch = vec![root.clone()];
            unit.scratch_effects = (0..4)
                .map(|lane| {
                    let fact = ScratchCellFact {
                        object: ScratchId(0),
                        offset: lane,
                        content: ContentRequirement::Content(ContentId(lane)),
                    };
                    if unit.id.0 == 0 {
                        ScratchEffect::Write(fact)
                    } else {
                        ScratchEffect::Read(fact)
                    }
                })
                .collect();
        }
        let abi = ProcedureAbi::new(CallingConvention::SameContext, 0, 0)?;
        evidence.program.segments[0].units[1].effects.call =
            Some(abi.invocation(Invocation::DynExec)?);
        evidence.units.get_mut(&SelectedStepId(1)).unwrap().calls.push(CallSite {
            instruction: Instruction::DynExec,
            abi,
            inputs: vec![],
            path: vec![],
            truncate: false,
            outputs: vec![],
            publications: vec![],
            selector: Some((
                FrameRef::Scratch {
                    object: ScratchId(0),
                    offset: 0,
                },
                [11, 13, 17, 19]
                    .map(|value| StoredValue::Literal(Felt::new_unchecked(value)))
                    .to_vec(),
            )),
        });
        for (id, source) in &mut evidence.program.source_map {
            source.origin = SourceOrigin::Generated {
                definition: self.id(),
                stage: id.0,
            };
        }
        Ok(evidence)
    }
}

#[test]
fn spanning_dynamic_root_requires_exact_contents_across_phases() {
    let selected = check_selection(
        RootPhases.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&RootPhases),
    )
    .unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let make = |stale: bool, bad_address: bool| {
        let mut body = AnnotatedMasmBody::new();
        body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
        for (lane, value) in [11, 13, 17, 19].into_iter().enumerate() {
            let value = if stale && lane == 3 { 97 } else { value };
            body.push_instruction(
                Primitive::Push(Felt::new_unchecked(value)).instruction().unwrap(),
                SelectedStepId(0),
            );
            body.push_instruction(
                Instruction::LocStore(
                    frame
                        .resolve(FrameRef::Scratch {
                            object: ScratchId(0),
                            offset: lane as u32,
                        })
                        .unwrap()
                        .into(),
                ),
                SelectedStepId(0),
            );
        }
        body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(1))));
        body.push_instruction(
            Instruction::Locaddr(
                frame
                    .resolve(FrameRef::Scratch {
                        object: ScratchId(0),
                        offset: u32::from(bad_address),
                    })
                    .unwrap()
                    .into(),
            ),
            SelectedStepId(1),
        );
        body.push_instruction(Instruction::DynExec, SelectedStepId(1));
        body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(1))));
        body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
        body
    };
    verify(&plan, &frame, make(false, false)).unwrap();
    for (stale, bad_address) in [(true, false), (false, true)] {
        assert_eq!(
            verify(&plan, &frame, make(stale, bad_address)).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
    }
}
