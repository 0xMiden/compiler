use super::*;

fn scheduled_body(
    actions: &[PlanAction],
    unit_code: Vec<Instruction>,
    frame: &FinalFrameLayout,
) -> AnnotatedMasmBody {
    let mut result = AnnotatedMasmBody::new();
    result.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    for action in actions {
        if let PlanAction::RunLowering { step } = action {
            result.push(AnnotatedOp::Boundary(Boundary::UnitStart(*step)));
            for instruction in &unit_code {
                result.push_instruction(instruction.clone(), *step);
            }
            result.push(AnnotatedOp::Boundary(Boundary::UnitEnd(*step)));
            continue;
        }
        let instruction = match action {
            PlanAction::Dup { depth } => Primitive::Dup(*depth).instruction().unwrap(),
            PlanAction::PushLiteral(value) => Primitive::Push(*value).instruction().unwrap(),
            PlanAction::StoreLane(slot) => {
                Instruction::LocStore(frame.resolve(FrameRef::Spill(*slot)).unwrap().into())
            }
            PlanAction::LoadLane(slot) => {
                Instruction::LocLoad(frame.resolve(FrameRef::Spill(*slot)).unwrap().into())
            }
            PlanAction::Drop => Instruction::Drop,
            _ => panic!("unsupported fixture action"),
        };
        result.push_instruction(instruction, SelectedStepId(u32::MAX));
    }
    result.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    result
}
#[test]
fn raw_slot_roundtrip() {
    let mut plan = identity_plan();
    let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    plan.spill_slots.push(slot);
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let actions = vec![
        PlanAction::StoreLane(slot),
        PlanAction::LoadLane(slot),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    verify(&plan, &frame, scheduled_body(&actions, vec![], &frame)).unwrap();
}
#[test]
fn dead_store_overwrites_live_physical_cell() {
    let mut plan = identity_plan();
    let live = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    let dead = SpillSlotId::temporary(SelectedStepId(0), 0);
    plan.spill_slots = vec![live, dead];
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(live, 0), (dead, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let actions = vec![
        PlanAction::StoreLane(live),
        PlanAction::PushLiteral(Felt::ONE),
        PlanAction::StoreLane(dead),
        PlanAction::LoadLane(live),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    assert_eq!(
        verify(&plan, &frame, scheduled_body(&actions, vec![], &frame))
            .unwrap_err()
            .category,
        DiagnosticCategory::UnavailableValue
    );
}
#[test]
fn uninitialized_spill_and_wrong_physical_offset_are_rejected() {
    let mut plan = identity_plan();
    let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    plan.spill_slots.push(slot);
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let actions = vec![
        PlanAction::Drop,
        PlanAction::LoadLane(slot),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    assert!(verify(&plan, &frame, scheduled_body(&actions, vec![], &frame)).is_err());
    let actions = vec![
        PlanAction::Dup { depth: 0 },
        PlanAction::StoreLane(slot),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.segment_actions.insert(SegmentId(0), actions);
    let mut invalid = AnnotatedMasmBody::new();
    invalid.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    invalid.push_instruction(Instruction::Dup0, SelectedStepId(u32::MAX));
    invalid.push_instruction(Instruction::LocStore(1.into()), SelectedStepId(u32::MAX));
    invalid.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
    invalid.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    invalid.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    assert_eq!(
        verify(&plan, &frame, invalid).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn publication_cannot_bless_overwritten_stored_input() {
    let mut fixture = Fixture::new(1, 1);
    fixture.stored.push(StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(Fixture::input(0)),
    });
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    let mut plan = fixture.plan();
    let slot = SpillSlotId::canonical(Fixture::input(0));
    plan.spill_slots = vec![slot];
    plan.lowering_bindings
        .get_mut(&SelectedStepId(0))
        .unwrap()
        .stored_inputs
        .insert(StoredInputId(0), slot);
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let actions = vec![
        PlanAction::Dup { depth: 0 },
        PlanAction::StoreLane(slot),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
    ];
    plan.segment_actions.insert(SegmentId(0), actions.clone());
    verify(&plan, &frame, scheduled_body(&actions, vec![], &frame)).unwrap();
    let code = vec![
        Primitive::Push(Felt::ONE).instruction().unwrap(),
        Instruction::LocStore(0.into()),
    ];
    assert_eq!(
        verify(&plan, &frame, scheduled_body(&actions, code, &frame))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn compiler_address_arithmetic_cannot_turn_into_unknown_source_memory() {
    let mut fixture = Fixture::new(0, 1).scratch(1);
    fixture.effects.memory = SourceMemoryEffects::READ;
    let code = vec![
        push(17),
        inst(Instruction::LocStore(0.into())),
        inst(Instruction::Locaddr(0.into())),
        push(1),
        inst(Instruction::Add),
        inst(Instruction::MemLoad),
    ];
    assert_eq!(
        fixture.run(code).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn exact_frame_address_arithmetic_preserves_physical_identity() {
    let mut fixture = Fixture::new(0, 1).scratch(1);
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        },
        0,
    ));
    fixture
        .run(vec![
            push(17),
            inst(Instruction::LocStore(0.into())),
            inst(Instruction::Locaddr(0.into())),
            push(0),
            inst(Instruction::Add),
            inst(Instruction::MemLoad),
        ])
        .unwrap();
}

#[test]
fn final_source_local_layout_must_match_registered_source() {
    let changed = FinalFrameLayout::new(
        SourceLocalLayout {
            offsets: vec![0],
            extent: 1,
        },
        BTreeMap::new(),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    assert_eq!(
        verify(&identity_plan(), &changed, body(vec![])).unwrap_err().category,
        DiagnosticCategory::FrameCapacityExceeded
    );
}

#[test]
fn compiler_addresses_cannot_escape_to_source_memory_or_procedure_results() {
    let mut fixture = Fixture::new(0, 0).scratch(1);
    fixture.effects.memory = SourceMemoryEffects::WRITE;
    assert_eq!(
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                push(77),
                inst(Instruction::MemStore)
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    let fixture = Fixture::new(0, 1).scratch(1);
    assert_eq!(
        fixture.run(vec![inst(Instruction::Locaddr(0.into()))]).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
}

#[test]
fn source_local_addresses_may_escape_under_source_semantics() {
    let mut fixture = Fixture::new(0, 1);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 1,
    };
    fixture.run(vec![inst(Instruction::Locaddr(0.into()))]).unwrap();
    let mut store = Fixture::new(0, 0);
    store.locals = fixture.locals.clone();
    store.effects.memory = SourceMemoryEffects::WRITE;
    store
        .run(vec![
            inst(Instruction::Locaddr(0.into())),
            push(77),
            inst(Instruction::MemStore),
        ])
        .unwrap();
}

#[test]
fn mixed_source_memory_accesses_require_both_unit_capabilities() {
    for permission in [
        SourceMemoryEffects::READ_WRITE,
        SourceMemoryEffects::READ,
        SourceMemoryEffects::WRITE,
        SourceMemoryEffects::default(),
    ] {
        let mut fixture = Fixture::new(2, 0);
        fixture.effects.memory = permission;
        let result = fixture.run(vec![
            inst(Instruction::MemLoad),
            inst(Instruction::Swap1),
            inst(Instruction::MemStore),
        ]);
        assert_eq!(
            result.is_ok(),
            permission == SourceMemoryEffects::READ_WRITE,
            "{permission:?}: {result:?}"
        );
    }
}

#[test]
fn direct_source_local_accesses_use_compositional_unit_capabilities() {
    for permission in [
        SourceMemoryEffects::READ_WRITE,
        SourceMemoryEffects::READ,
        SourceMemoryEffects::WRITE,
    ] {
        let mut fixture = Fixture::new(0, 0);
        fixture.locals = SourceLocalLayout {
            extent: 2,
            offsets: vec![0, 1],
        };
        fixture.effects.memory = permission;
        let result = fixture.run(vec![
            inst(Instruction::LocLoad(0.into())),
            inst(Instruction::LocStore(1.into())),
        ]);
        assert_eq!(
            result.is_ok(),
            permission == SourceMemoryEffects::READ_WRITE,
            "{permission:?}: {result:?}"
        );
    }
}
