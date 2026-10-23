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
fn immediate_frame_arithmetic_preserves_native_cell_identity() {
    let mut fixture = Fixture::new(0, 1).scratch(2);
    fixture.proof.scratch_outputs.push((
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 1,
        },
        0,
    ));
    let prefix = [
        Primitive::Push(Felt::new_unchecked(17)).instruction().unwrap(),
        Instruction::LocStore(1.into()),
        Instruction::Locaddr(0.into()),
    ];
    for arithmetic in [
        vec![Primitive::Push(Felt::ONE).instruction().unwrap(), Instruction::Add],
        vec![Instruction::AddImm(Felt::ONE.into())],
    ] {
        let mut code = prefix.to_vec();
        code.extend(arithmetic);
        code.push(Instruction::MemLoad);
        let mut expanded = body(code);
        verify(&fixture.plan(), &fixture.frame(), expanded.clone()).unwrap();
        crate::opt::peephole::simplify_annotated(&mut expanded);
        assert!(
            expanded
                .top_level_instructions()
                .any(|instruction| { matches!(instruction, Instruction::AddImm(_)) })
        );
        verify(&fixture.plan(), &fixture.frame(), expanded).unwrap();
    }
}

#[test]
fn immediate_comparisons_clear_frame_provenance_like_native_comparisons() {
    for source_local in [false, true] {
        let mut fixture = Fixture::new(0, 1);
        if source_local {
            fixture.locals = SourceLocalLayout {
                offsets: vec![0],
                extent: 1,
            };
        } else {
            fixture = fixture.scratch(1);
        }
        for (native, immediate) in [
            (Instruction::Eq, Instruction::EqImm(Felt::ZERO.into())),
            (Instruction::Neq, Instruction::NeqImm(Felt::ZERO.into())),
        ] {
            for comparison in [
                vec![Primitive::Push(Felt::ZERO).instruction().unwrap(), native],
                vec![immediate],
            ] {
                let mut code = vec![Instruction::Locaddr(0.into())];
                code.extend(comparison);
                let mut expanded = body(code);
                verify(&fixture.plan(), &fixture.frame(), expanded.clone()).unwrap();
                crate::opt::peephole::simplify_annotated(&mut expanded);
                verify(&fixture.plan(), &fixture.frame(), expanded).unwrap();
            }
        }
    }
}

#[test]
fn immediate_arithmetic_retains_compiler_storage_restrictions() {
    let fixture = Fixture::new(0, 1).scratch(2);
    for immediate in [u64::from(u32::MAX), u64::from(u32::MAX) + 1] {
        let error = fixture
            .run(vec![
                inst(Instruction::Locaddr(1.into())),
                inst(Instruction::AddImm(Felt::new_unchecked(immediate).into())),
            ])
            .unwrap_err();
        assert_eq!(error.category, DiagnosticCategory::IllegalStackAccess);
    }
    assert_eq!(
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                inst(Instruction::AddImm(Felt::ONE.into())),
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::AbiMismatch
    );
    assert_eq!(
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                inst(Instruction::AddImm(Felt::ONE.into())),
                inst(Instruction::MemLoad),
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    assert_eq!(
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                inst(Instruction::MulImm(Felt::new_unchecked(2).into())),
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
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

/// Retain original inputs while storing them, so read-only checking has an independent identity
/// anchor. Reloads cannot bless a mutation by being compared only with themselves.
fn eval_circuit_compiler_fixture(
    read: u64,
    eval: u64,
    size: u32,
    initialized: u32,
) -> (Fixture, Vec<AnnotatedOp>) {
    let mut fixture = Fixture::new(8, 8).scratch(size);
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture.proof.aliases =
        LaneAliases::new((0..8).map(|i| (Fixture::output(i), Fixture::input(i))).collect());
    fixture.proof.scratch_outputs = (0..initialized)
        .map(|offset| {
            (
                FrameRef::Scratch {
                    object: ScratchId(0),
                    offset,
                },
                offset % 8,
            )
        })
        .collect();
    let mut code = Vec::new();
    for offset in 0..initialized {
        code.extend([
            inst(Primitive::Dup((offset % 8) as u8).instruction().unwrap()),
            inst(Instruction::LocStore((offset as u16).into())),
        ]);
    }
    code.extend([
        push(eval),
        push(read),
        inst(Instruction::Locaddr(0.into())),
        inst(Instruction::EvalCircuit),
        inst(Instruction::Drop),
        inst(Instruction::Drop),
        inst(Instruction::Drop),
    ]);
    for offset in 0..initialized {
        code.extend([
            inst(Primitive::Dup((offset % 8) as u8).instruction().unwrap()),
            inst(Instruction::LocLoad((offset as u16).into())),
            inst(Instruction::AssertEq),
        ]);
    }
    (fixture, code)
}

#[test]
fn eval_circuit_checks_complete_wide_compiler_footprint_and_unchanged_identities() {
    for (read, eval, width) in [(2, 4, 8), (128, 4, 260), (32764, 4, 65532)] {
        for initialized in [width - 1, width] {
            let (fixture, code) = eval_circuit_compiler_fixture(read, eval, width, initialized);
            let result = fixture.run(code);
            if initialized == width {
                result.unwrap();
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
                assert!(error.to_string().contains("not definitely initialized"), "{error}");
            }
        }
    }
    let (fixture, code) = eval_circuit_compiler_fixture(2, 4, 9, 9);
    fixture.run(code.clone()).unwrap();
    for offset in [0, 7, 8] {
        let mut clobbered = code.clone();
        let index = clobbered
            .iter()
            .position(|op| {
                matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: Instruction::EvalCircuit,
                        ..
                    }
                )
            })
            .unwrap()
            + 1;
        clobbered.splice(index..index, [push(0), inst(Instruction::LocStore(offset.into()))]);
        let error = fixture.run(clobbered).unwrap_err();
        assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
        assert!(error.to_string().contains("scratch/output"), "{error}");
    }

    let (fixture, code) = eval_circuit_compiler_fixture(128, 4, 256, 256);
    assert_eq!(
        fixture.run(code).unwrap_err().category,
        DiagnosticCategory::FrameCapacityExceeded
    );
}

#[test]
fn eval_circuit_source_local_runtime_counts_preserve_exact_base_and_permissions() {
    for permission in [SourceMemoryEffects::READ, SourceMemoryEffects::default()] {
        let mut fixture = Fixture::new(2, 0).scratch(8);
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 8,
        };
        fixture.effects.may_trap = true;
        fixture.effects.memory = permission;
        let code = vec![
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ];
        assert_eq!(fixture.run(code).is_ok(), permission == SourceMemoryEffects::READ);
    }
    let mut fixture = Fixture::new(0, 0).scratch(8);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    let error = fixture
        .run(vec![
            push(4),
            push(2),
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert!(error.to_string().contains("source access crosses"), "{error}");
}

#[test]
fn eval_circuit_unresolved_compiler_extent_or_address_cannot_use_source_permission() {
    let mut fixture = Fixture::new(2, 0).scratch(8);
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    let error = fixture
        .run(vec![
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap_err();
    assert!(error.to_string().contains("extent is not precisely resolved"), "{error}");
    // A previously source-local pointer advanced exactly into compiler storage remains compiler.
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    let error = fixture
        .run(vec![
            inst(Instruction::Locaddr(0.into())),
            push(4),
            inst(Instruction::Add),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap_err();
    assert!(error.to_string().contains("extent is not precisely resolved"), "{error}");
    // An unknown condition merges compiler and source origins without an exact address.
    let mut fixture = Fixture::new(1, 0).scratch(8);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    let error = fixture
        .run(vec![
            push(4),
            push(2),
            inst(Instruction::Locaddr(4.into())),
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::MovUp4),
            inst(Instruction::CDrop),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap_err();
    assert!(error.to_string().contains("address is not precisely resolved"), "{error}");
}

#[test]
fn eval_circuit_invalid_canonical_counts_trap_before_memory_without_skipping_publication() {
    for (read, eval) in [(0, 0), (1, 0), (2, 3), (32766, 4), (0xffff_ffff_0000_0000, 0x1_0000_0004)]
    {
        let mut fixture = Fixture::new(0, 0);
        fixture.effects.may_trap = true;
        fixture
            .run(vec![
                push(eval),
                push(read),
                push(1),
                inst(Instruction::EvalCircuit),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
            ])
            .unwrap();
        fixture.outputs = 1;
        assert!(
            fixture
                .run(vec![
                    push(eval),
                    push(read),
                    push(1),
                    inst(Instruction::EvalCircuit),
                    inst(Instruction::Drop),
                    inst(Instruction::Drop),
                    inst(Instruction::Drop)
                ])
                .is_err()
        );
    }
}

#[test]
fn eval_circuit_declared_object_extent_and_opaque_source_origin_remain_checked() {
    let (fixture, code) = eval_circuit_compiler_fixture(2, 4, 6, 6);
    let error = fixture.run(code).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert!(error.to_string().contains("undeclared compiler-storage"), "{error}");
    let mut fixture = Fixture::new(3, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 8,
    };
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture
        .run(vec![
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::Add),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap();
    let mut fixture = Fixture::new(0, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 8,
    };
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture
        .run(vec![
            push(4),
            push(2),
            inst(Instruction::Locaddr(0.into())),
            inst(Instruction::EvalCircuit),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap();
}

#[test]
fn eval_circuit_generated_initialized_compiler_circuit_executes_without_escaping_pointer() {
    use miden_core::prettier::PrettyPrint;
    let (fixture, code) = eval_circuit_compiler_fixture(2, 4, 8, 8);
    let procedure = fixture
        .run(code)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    let gate = |left: u64, right: u64, op: u64| (left + (right << 30) + (op << 60)) as usize;
    let input = [3, 7, 11, 13, gate(5, 5, 0), gate(5, 4, 2), gate(2, 2, 1), gate(1, 1, 0)];
    let source = format!(
        "{procedure} begin {} exec.selected {} end",
        crate::plan::tests::lowering_units::push_top(&input),
        crate::plan::tests::lowering_units::assert_top(&input)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("compiler_circuit", source)
        .unwrap()
        .try_into_program()
        .unwrap();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut crate::plan::tests::lowering_units::host())
        .unwrap();
}

fn memory_system_event(event: miden_core::events::SystemEvent) -> AnnotatedOp {
    inst(Instruction::EmitImm(
        crate::masm::Immediate::Value(crate::masm::Span::unknown(event.event_id().as_felt()))
            .into(),
    ))
}

fn memory_system_event_literals(payload: [Option<u64>; 6]) -> (Fixture, Vec<AnnotatedOp>) {
    let mut f = Fixture::new(payload.iter().filter(|v| v.is_none()).count() as u32, 0);
    f.effects.may_trap = true;
    let mut code = vec![];
    // Unknown fields are fresh ABI values; move each to its actual payload position.
    for (placed, value) in payload.into_iter().rev().enumerate() {
        if let Some(value) = value {
            code.push(push(value));
        } else if placed != 0 {
            code.push(inst(Primitive::MoveUp(placed as u8).instruction().unwrap()));
        }
    }
    (f, code)
}

#[test]
fn memory_system_event_source_ranges_require_read_only_when_possible() {
    use miden_core::events::SystemEvent::MemToMap;
    for (s, e, reads) in [
        (0, 0, false),
        (u32::MAX as u64 + 1, 9, false),
        (0, u32::MAX as u64 + 1, false),
        (9, 2, false),
        (0, 1, true),
        (0, 260, true),
        (0, 65533, true),
        (0, u32::MAX as u64, true),
    ] {
        for permission in [
            SourceMemoryEffects::default(),
            SourceMemoryEffects::WRITE,
            SourceMemoryEffects::READ,
        ] {
            let (mut f, mut code) = memory_system_event_literals([
                Some(11),
                Some(23),
                Some(37),
                Some(53),
                Some(s),
                Some(e),
            ]);
            f.effects.memory = permission;
            code.push(memory_system_event(MemToMap));
            code.extend((0..6).map(|_| inst(Instruction::Drop)));
            assert_eq!(
                f.run(code).is_ok(),
                !reads || permission == SourceMemoryEffects::READ,
                "{s}..{e} {permission:?}"
            );
        }
    }
    let mut f = Fixture::new(2, 0);
    f.effects.may_trap = true;
    f.effects.memory = SourceMemoryEffects::READ;
    let mut code = vec![push(53), push(37), push(23), push(11), memory_system_event(MemToMap)];
    code.extend((0..6).map(|_| inst(Instruction::Drop)));
    f.run(code).unwrap();
}

#[test]
fn memory_system_event_deferred_closed_no_read_predicates_and_possible_widths() {
    use miden_core::events::SystemEvent::DeferredRegisterData;
    // Hand-derived outcomes; None denotes a genuinely unknown ordinary ABI input.
    let cases = [
        ([None, None, None, None, None, Some(0)], false),
        ([None, None, None, None, None, Some(0x1_0000_0000)], false),
        ([None, None, None, None, None, Some(131072)], false),
        ([Some(0), None, None, None, None, None], false),
        ([Some(0), Some(7), Some(0), Some(0), None, Some(1)], false),
        ([Some(1), Some(7), None, None, None, None], false),
        ([Some(2), Some(0), Some(7), None, None, None], false),
        ([Some(1), None, None, None, None, Some(2)], false),
        ([None, None, None, None, Some(0x1_0000_0000), None], false),
        ([None, None, None, None, Some(5), None], false),
        ([Some(2), Some(0), Some(0), Some(0), Some(0xfffffff8), Some(1)], false),
        ([Some(2), Some(0), Some(0), Some(0), Some(0xfffffff4), Some(1)], true),
        ([Some(1), None, None, None, Some(400), Some(1)], true),
        ([Some(2), Some(0), Some(0), Some(0), Some(400), Some(131071)], true),
        ([Some(999), Some(0), Some(0), Some(0), Some(400), Some(1)], true),
        ([None, None, None, None, None, None], true),
    ];
    for (payload, reads) in cases {
        for permission in [
            SourceMemoryEffects::default(),
            SourceMemoryEffects::WRITE,
            SourceMemoryEffects::READ,
        ] {
            let (mut f, mut code) = memory_system_event_literals(payload);
            f.effects.memory = permission;
            code.push(memory_system_event(DeferredRegisterData));
            code.extend((0..6).map(|_| inst(Instruction::Drop)));
            assert_eq!(
                f.run(code).is_ok(),
                !reads || permission == SourceMemoryEffects::READ,
                "{payload:?} {permission:?}"
            );
        }
    }
}

#[test]
fn memory_system_event_deferred_checks_source_span_without_relative_alignment_guesses() {
    use miden_core::events::SystemEvent::DeferredRegisterData;
    for (offset, extent, n, pass) in [
        (1, 9, Some(1), true),
        (1, 8, Some(1), false),
        (0, 300, Some(33), true),
        (0, 300, Some(38), false),
        (0, 8, None, true),
    ] {
        let mut f = Fixture::new(u32::from(n.is_none()), 0).scratch(8);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let mut code = n.map(|n| vec![push(n)]).unwrap_or_default();
        code.extend([
            inst(Instruction::Locaddr(offset.into())),
            push(0),
            push(0),
            push(0),
            push(2),
            memory_system_event(DeferredRegisterData),
        ]);
        code.extend((0..6).map(|_| inst(Instruction::Drop)));
        assert_eq!(f.run(code).is_ok(), pass, "offset{offset} extent{extent} n{n:?}");
    }
}

#[test]
fn memory_system_event_wide_exact_source_range_has_no_loaded_cell_tokens() {
    for (extent, start, end) in [(300, 0, 300), (65528, 0, 65528)] {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let mut code = vec![
            inst(Instruction::Locaddr((end as u16).into())),
            inst(Instruction::Locaddr((start as u16).into())),
            push(53),
            push(37),
            push(23),
            push(11),
            memory_system_event(miden_core::events::SystemEvent::MemToMap),
        ];
        code.extend((0..6).map(|_| inst(Instruction::Drop)));
        f.run(code).unwrap();
    }
}

#[test]
fn memory_system_event_equal_ordinary_abi_bounds_need_no_fictitious_read() {
    let mut f = Fixture::new(1, 0);
    f.effects.may_trap = true;
    let mut code = vec![
        inst(Instruction::Dup0),
        push(53),
        push(37),
        push(23),
        push(11),
        memory_system_event(miden_core::events::SystemEvent::MemToMap),
    ];
    code.extend((0..6).map(|_| inst(Instruction::Drop)));
    f.run(code).unwrap();
}

#[test]
fn memory_system_event_maximum_deferred_width_cannot_narrow_to_a_local_extent() {
    let mut f = Fixture::new(0, 0).scratch(4);
    f.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 65528,
    };
    f.effects.may_trap = true;
    f.effects.memory = SourceMemoryEffects::READ;
    let mut code = vec![
        push(131071),
        inst(Instruction::Locaddr(0.into())),
        push(0),
        push(0),
        push(0),
        push(2),
        memory_system_event(miden_core::events::SystemEvent::DeferredRegisterData),
    ];
    code.extend((0..6).map(|_| inst(Instruction::Drop)));
    assert_eq!(f.run(code).unwrap_err().category, DiagnosticCategory::FrameCapacityExceeded);
}
