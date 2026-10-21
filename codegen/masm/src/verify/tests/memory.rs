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

#[test]
fn stream_write_output_word_stores_preserve_exact_cells_and_untouched_neighbor() {
    for big_endian in [false, true] {
        let mut fixture = Fixture::new(5, 5).scratch(5);
        fixture.proof.aliases =
            LaneAliases::new((0..5).map(|i| (Fixture::output(i), Fixture::input(i))).collect());
        for cell in 0..5 {
            let output = if big_endian && cell < 4 {
                3 - cell
            } else {
                cell
            };
            fixture.proof.scratch_outputs.push((
                FrameRef::Scratch {
                    object: ScratchId(0),
                    offset: cell,
                },
                output,
            ));
        }
        let (store, load) = if big_endian {
            (Instruction::LocStoreWBe(0.into()), Instruction::LocLoadWBe(0.into()))
        } else {
            (Instruction::LocStoreWLe(0.into()), Instruction::LocLoadWLe(0.into()))
        };
        let code = vec![
            inst(Instruction::MovUp4),
            inst(Instruction::LocStore(4.into())),
            inst(store),
            inst(Instruction::DropW),
            inst(Instruction::LocLoad(4.into())),
            inst(Instruction::PadW),
            inst(load),
        ];
        // Recovered lanes must be the original four distinct inputs, independently of the
        // contents claimed by scratch_outputs; publication cannot bless a wrong store order.
        fixture.run(code.clone()).unwrap();
        let mut reversed_store = code.clone();
        reversed_store[2] = inst(if big_endian {
            Instruction::LocStoreWLe(0.into())
        } else {
            Instruction::LocStoreWBe(0.into())
        });
        assert!(fixture.run(reversed_store).is_err());
        let mut clobbered_neighbor = code.clone();
        clobbered_neighbor.insert(4, inst(Instruction::LocStore(4.into())));
        clobbered_neighbor.insert(4, push(0));
        assert!(fixture.run(clobbered_neighbor).is_err());
        let first = fixture.proof.scratch_outputs[0].1;
        fixture.proof.scratch_outputs[0].1 = fixture.proof.scratch_outputs[3].1;
        fixture.proof.scratch_outputs[3].1 = first;
        assert!(fixture.run(code).is_err());
    }
}

#[test]
fn stream_write_output_backed_indirect_store_rejects_compiler_address_escape() {
    let mut fixture = Fixture::new(0, 4).scratch(1);
    fixture.effects.memory = SourceMemoryEffects::WRITE;
    let code = vec![
        push(11),
        push(22),
        push(33),
        inst(Instruction::Locaddr(0.into())),
        push(400),
        inst(Instruction::MemStoreWLe),
    ];
    assert_eq!(
        fixture.run(code).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn stream_write_native_requires_both_source_permissions_and_preserves_compiler_boundary() {
    for permission in [
        SourceMemoryEffects::default(),
        SourceMemoryEffects::READ,
        SourceMemoryEffects::WRITE,
        SourceMemoryEffects::READ_WRITE,
    ] {
        let mut fixture = Fixture::new(14, 14);
        fixture.effects.memory = permission;
        fixture.effects.may_trap = true;
        let result = fixture.run(vec![inst(Instruction::CryptoStream)]);
        assert_eq!(
            result.is_ok(),
            permission == SourceMemoryEffects::READ_WRITE,
            "{permission:?}: {result:?}"
        );
    }
    for instruction in [Instruction::AdvPipe, Instruction::CryptoStream] {
        let width = if instruction == Instruction::AdvPipe {
            13
        } else {
            14
        };
        let mut fixture = Fixture::new(0, width).scratch(8);
        fixture.effects.memory = SourceMemoryEffects::READ_WRITE;
        fixture.effects.may_trap = true;
        let mut code = vec![inst(Instruction::Locaddr(0.into()))];
        code.extend((1..width).map(|_| push(1)));
        code.push(inst(instruction));
        // Native opaque computations cannot erase a compiler address or invent affine pointers.
        assert_eq!(fixture.run(code).unwrap_err().category, DiagnosticCategory::IllegalStackAccess);
    }
}

#[test]
fn stream_write_crypto_checks_complete_prewrite_read_initialization() {
    for initialized in [0, 7, 8] {
        let mut fixture = Fixture::new(0, 14).scratch(8);
        fixture.effects.memory = SourceMemoryEffects::READ_WRITE;
        fixture.effects.may_trap = true;
        let mut code = Vec::new();
        for offset in 0..initialized {
            code.extend([push(11), inst(Instruction::LocStore(offset.into()))]);
        }
        code.extend([push(400), inst(Instruction::Locaddr(0.into()))]);
        code.extend((0..12).map(|_| push(1)));
        code.push(inst(Instruction::CryptoStream));
        // Missing the eighth read cell cannot be hidden by opaque output computation or writes.
        assert_eq!(
            fixture.run(code).unwrap_err().category,
            if initialized < 8 {
                DiagnosticCategory::InvalidSpillSlotRead
            } else {
                DiagnosticCategory::IllegalStackAccess
            }
        );
    }
}

#[test]
fn stream_write_native_cannot_cross_source_local_into_compiler_storage() {
    for crypto in [false, true] {
        let n = if crypto { 14 } else { 13 };
        let mut fixture = Fixture::new(0, n).scratch(4);
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        fixture.effects.memory = SourceMemoryEffects::READ_WRITE;
        fixture.effects.may_trap = true;
        let mut code = vec![inst(Instruction::Locaddr(0.into()))];
        if crypto {
            code.push(push(400));
        }
        code.extend((0..12).map(|_| push(1)));
        code.push(inst(if crypto {
            Instruction::CryptoStream
        } else {
            Instruction::AdvPipe
        }));
        assert_eq!(
            fixture.run(code).unwrap_err().category,
            DiagnosticCategory::InvalidSpillSlotRead
        );
    }
}
