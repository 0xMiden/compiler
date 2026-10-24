use super::*;

#[test]
fn allocator_equality_and_slot_validity_claims_cannot_establish_facts() {
    let mut fixture = Fixture::new(2, 1);
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    let mut plan = fixture.plan();
    plan.contracts.get_mut(&SegmentId(0)).unwrap().aliases =
        LaneAliases::new(vec![(Fixture::input(0), Fixture::input(1))]);
    assert_eq!(
        verify(&plan, &fixture.frame(), body(vec![Instruction::Drop]))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidEdgeTransfer
    );
    let mut plan = identity_plan();
    let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    plan.spill_slots = vec![slot];
    plan.contracts
        .get_mut(&SegmentId(0))
        .unwrap()
        .required_spill_slots
        .push((slot, Atom::new(ValueId(0), 0)));
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    assert_eq!(
        verify(&plan, &frame, body(vec![])).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn omitting_allocator_liveness_claims_does_not_make_lost_inputs_dead() {
    let mut plan = identity_plan();
    plan.contracts.get_mut(&SegmentId(0)).unwrap().required_spill_slots.clear();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::Drop,
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
        ],
    );
    let mut code = AnnotatedMasmBody::new();
    code.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    code.push_instruction(Instruction::Drop, SelectedStepId(u32::MAX));
    code.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
    code.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    code.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    assert_eq!(
        verify(&plan, &frame(), code).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
}

#[test]
fn actual_instruction_mutations_and_unsupported_effects_fail_closed() {
    assert_eq!(
        verify(&identity_plan(), &frame(), body(vec![Instruction::U32Div]))
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
    let mut fixture = Fixture::new(17, 17);
    fixture.proof.aliases = LaneAliases::new(
        (0..17).map(|index| (Fixture::output(index), Fixture::input(index))).collect(),
    );
    fixture.run(vec![]).unwrap();
    // A legal raw index can still reach the protected tail when owned state is short.
    assert_eq!(
        verify(&identity_plan(), &frame(), body(vec![Instruction::Dup15]))
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
}

use crate::{
    allocate::schedule::AllocationOptions,
    plan::tests::{
        lowering_units::{Case, Source, ready, rewrite, symbol},
        pipeline::{root_source, structured_ready},
    },
};

#[test]
fn memmove_cleanup_and_internal_access_are_rejected() {
    let source = Source {
        case: Case::Memmove,
        live: vec![],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    verify(&plan, &frame, body.clone()).unwrap();
    let mut cleanup = body.clone();
    let mut removed = 0;
    rewrite(&mut cleanup, &mut |op| {
        if let AnnotatedOp::If {
            id: ControlId(0),
            else_body,
            ..
        } = op
        {
            else_body.ops_mut().retain(|op| {
                let keep = !matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: Instruction::DropW,
                        ..
                    }
                );
                if !keep {
                    removed += 1;
                }
                keep
            });
        }
    });
    assert_eq!(removed, 1);
    assert_eq!(
        verify(&plan, &frame, cleanup).unwrap_err().category,
        DiagnosticCategory::IllegalStackAccess
    );
    let mut access = body;
    rewrite(&mut access, &mut |op| {
        if let AnnotatedOp::While {
            id: ControlId(2),
            body,
        } = op
        {
            let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[0] else {
                panic!()
            };
            *instruction = Instruction::Dup15;
        }
    });
    assert_eq!(
        verify(&plan, &frame, access).unwrap_err().category,
        DiagnosticCategory::IllegalStackAccess
    );
}

#[test]
fn pipeline_action_binding_frame_and_phase_mutations_fail_before_assembly() {
    let source = Source {
        case: Case::Fpi(16),
        live: vec![0, 6, 21],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    verify(&plan, &frame, body.clone()).unwrap();
    let mut action = plan.clone();
    action.segment_actions.get_mut(&SegmentId(0)).unwrap()[0] = PlanAction::Drop;
    assert_eq!(
        verify(&action, &frame, body.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut binding = plan.clone();
    let bindings = binding.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap();
    let wrong = bindings.stored_inputs[&StoredInputId(1)];
    bindings.stored_inputs.insert(StoredInputId(0), wrong);
    assert_eq!(
        verify(&binding, &frame, body.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    let slots = &plan.lowering_bindings[&SelectedStepId(0)].stored_inputs;
    let mut offsets = frame.spills().clone();
    offsets.insert(slots[&StoredInputId(1)], offsets[&slots[&StoredInputId(0)]]);
    let collision = FinalFrameLayout::new(
        frame.source().clone(),
        offsets,
        frame.scratch().clone(),
        frame.aligned_size(),
    )
    .unwrap();
    let definition = source.definition();
    let mut registry = crate::lower::units::LoweringRegistry::default();
    registry.register(definition.as_ref()).unwrap();
    let changed = crate::lower::expand::expand_procedure(
        &plan,
        &collision,
        &registry,
        &crate::lower::expand::ExpansionEnv::default(),
    )
    .unwrap();
    let error = verify(&plan, &collision, changed).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::UnavailableValue);
    assert_eq!(error.message, "destructive action lost last representation of live atom");
    let mut phase = body.clone();
    let start = phase
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitStart(_))))
        .unwrap();
    let end = phase
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
        .unwrap();
    phase.ops_mut().swap(start, end);
    assert_eq!(
        verify(&plan, &frame, phase).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut physical = body;
    let mut changed = false;
    rewrite(&mut physical, &mut |op| {
        if !changed
            && let AnnotatedOp::Instruction {
                instruction: Instruction::LocStore(offset),
                ..
            } = op
        {
            *offset = 0u16.into();
            changed = true;
        }
    });
    // Pick a distinct offset even if the first store happened to use cell zero.
    if let Some(AnnotatedOp::Instruction {
        instruction: Instruction::LocStore(offset),
        ..
    }) = physical.ops_mut().iter_mut().find(|op| {
        matches!(
            op,
            AnnotatedOp::Instruction {
                instruction: Instruction::LocStore(_),
                ..
            }
        )
    }) {
        *offset = (frame.aligned_size() as u16 - 1).into();
    }
    assert!(changed);
    assert_eq!(
        verify(&plan, &frame, physical).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn dynamic_root_binding_offset_and_selector_are_rejected() {
    let source = Source {
        case: Case::Context {
            invocation: Invocation::DynCall,
            arguments: 16,
            results: 16,
            root_argument: true,
        },
        live: vec![0],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    verify(&plan, &frame, body.clone()).unwrap();
    let mut missing = body.clone();
    let index = missing
        .ops()
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::DynCall,
                    ..
                }
            )
        })
        .unwrap();
    // Remove the selector producer while retaining the intervening debugger events.
    let selector_index = missing.ops()[..index]
        .iter()
        .rposition(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Locaddr(_),
                    ..
                }
            )
        })
        .unwrap();
    missing.ops_mut().remove(selector_index);
    assert_eq!(
        verify(&plan, &frame, missing).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
    let mut wrong = body;
    let allocation = frame.scratch()[&ScratchId(0)];
    rewrite(&mut wrong, &mut |op| {
        if let AnnotatedOp::Instruction {
            instruction: Instruction::Locaddr(offset),
            ..
        } = op
            && *offset == allocation.offset as u16
        {
            *offset = ((allocation.offset as u16) + 1).into();
        }
    });
    assert_eq!(
        verify(&plan, &frame, wrong).unwrap_err().category,
        DiagnosticCategory::IllegalStackAccess
    );
}

#[test]
fn fpi_swapped_prefix_buried_access_and_early_publication_are_rejected() {
    let source = Source {
        case: Case::Fpi(16),
        live: vec![],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    let first = frame
        .resolve(FrameRef::Spill(
            plan.lowering_bindings[&SelectedStepId(0)].stored_inputs[&StoredInputId(0)],
        ))
        .unwrap();
    let second = frame
        .resolve(FrameRef::Spill(
            plan.lowering_bindings[&SelectedStepId(0)].stored_inputs[&StoredInputId(1)],
        ))
        .unwrap();
    let mut swapped = body.clone();
    rewrite(&mut swapped, &mut |op| {
        if let AnnotatedOp::Instruction {
            instruction: Instruction::Locaddr(offset),
            unit: SelectedStepId(0),
        } = op
        {
            if *offset == first {
                *offset = second.into();
            } else if *offset == second {
                *offset = first.into();
            }
        }
    });
    assert_eq!(
        verify(&plan, &frame, swapped).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
    let call = body
        .ops()
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::Exec(_),
                    ..
                }
            )
        })
        .unwrap();
    let mut buried = body.clone();
    buried.ops_mut().insert(
        call,
        AnnotatedOp::Instruction {
            instruction: Instruction::MovUp15,
            unit: SelectedStepId(0),
        },
    );
    assert_eq!(
        verify(&plan, &frame, buried).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
    let mut early = body;
    early
        .ops_mut()
        .insert(call, AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    assert_eq!(
        verify(&plan, &frame, early).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn generated_root_missing_or_swapped_stages_fail_source_coverage() {
    let source = root_source(16, 16);
    let expected = source.instantiate(&[]).unwrap().program;
    for index in 0..4 {
        let mut missing = expected.clone();
        missing.segments[0].units.remove(index);
        assert_eq!(
            check_selection(missing, &SourceContext::Generated(&source))
                .unwrap_err()
                .category,
            DiagnosticCategory::InvalidSelectedLowering
        );
    }
    for index in 0..3 {
        let mut swapped = expected.clone();
        swapped.segments[0].units.swap(index, index + 1);
        assert_eq!(
            check_selection(swapped, &SourceContext::Generated(&source))
                .unwrap_err()
                .category,
            DiagnosticCategory::InvalidSelectedLowering
        );
    }
    let incompatible = crate::select::generated::GeneratedRoot {
        initializer: None,
        harness: None,
        entry: symbol("entry"),
        entry_abi: ProcedureAbi::new(CallingConvention::Context, 16, 16).unwrap(),
    };
    assert_eq!(
        incompatible.instantiate(&[]).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
}

#[test]
fn structured_scratch_alias_binding_and_edge_mutations_are_rejected() {
    let (plan, frame, body) = structured_ready(&AllocationOptions::default());
    verify(&plan, &frame, body.clone()).unwrap();
    let mut scratch = body.clone();
    let mut changes = 0;
    rewrite(&mut scratch, &mut |op| {
        if let AnnotatedOp::Instruction {
            instruction: Instruction::Locaddr(offset),
            unit: SelectedStepId(9),
        } = op
        {
            *offset = 0u16.into();
            changes += 1;
        }
    });
    assert_eq!(changes, 1);
    assert_eq!(
        verify(&plan, &frame, scratch).unwrap_err().category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
    let mut alias = body.clone();
    let mut count = 0;
    rewrite(&mut alias, &mut |op| {
        if let AnnotatedOp::If {
            id: ControlId(4),
            then_body,
            ..
        } = op
        {
            let end = then_body
                .ops()
                .iter()
                .position(|op| {
                    matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(6))))
                })
                .unwrap();
            then_body.ops_mut().splice(
                end..end,
                [
                    AnnotatedOp::Instruction {
                        instruction: Instruction::Drop,
                        unit: SelectedStepId(6),
                    },
                    AnnotatedOp::Instruction {
                        instruction: Primitive::Push(Felt::ONE).instruction().unwrap(),
                        unit: SelectedStepId(6),
                    },
                ],
            );
            count += 1;
        }
    });
    assert_eq!(count, 1);
    assert_eq!(
        verify(&plan, &frame, alias).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
    let mut edge = body;
    let mut count = 0;
    rewrite(&mut edge, &mut |op| {
        if let AnnotatedOp::Boundary(Boundary::Edge(id)) = op
            && *id == EdgeId(10)
        {
            *id = EdgeId(11);
            count += 1;
        }
    });
    assert_eq!(count, 1);
    assert_eq!(
        verify(&plan, &frame, edge).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn canonical_and_debug_anchors_cannot_hide_actions_or_move_evidence() {
    use crate::lower::expand::{Anchor, DebugDecorator, ExpansionEnv, expand_procedure};
    let mut plan =
        crate::allocate::allocate(empty_program(), &AllocationOptions::default()).unwrap();
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .push(PlanAction::DebugLocation(DebugLocationId(1)));
    let frame = crate::allocate::slots::assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    let env = ExpansionEnv {
        debug: BTreeMap::from([(DebugLocationId(1), DebugDecorator::InlineCallClear)]),
        ..Default::default()
    };
    let mut body =
        expand_procedure(&plan, &frame, &crate::lower::units::LoweringRegistry::default(), &env)
            .unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    verify(&plan, &frame, body.clone()).unwrap();
    let mut missing = body.clone();
    missing
        .ops_mut()
        .retain(|op| !matches!(op, AnnotatedOp::Anchor(Anchor::Debug { .. })));
    assert_eq!(
        verify(&plan, &frame, missing).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut wrong = body.clone();
    rewrite(&mut wrong, &mut |op| {
        if let AnnotatedOp::Anchor(Anchor::Debug { id, .. }) = op {
            *id = DebugLocationId(2);
        }
    });
    assert_eq!(
        verify(&plan, &frame, wrong).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut duplicate = body.clone();
    duplicate.push(AnnotatedOp::Anchor(Anchor::Nop));
    assert_eq!(
        verify(&plan, &frame, duplicate).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut executable = body;
    executable.push_instruction(Instruction::Nop, SelectedStepId(u32::MAX));
    assert_eq!(
        verify(&plan, &frame, executable).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn wide_result_equalities_and_scratch_relations_check_positions_above_255() {
    let mut fixture = Fixture::new(0, 258).scratch(1);
    let reference = FrameRef::Scratch {
        object: ScratchId(0),
        offset: 0,
    };
    let high: u32 = 256;
    fixture.proof.equal_outputs.push((0, high));
    fixture.proof.scratch_outputs.push((reference, high));
    let mut code = vec![push(99), inst(Instruction::LocStore(0.into()))];
    code.extend((0..258).rev().map(|i| push(if i == 0 || i == 256 { 99 } else { 1000 + i })));
    fixture.run(code.clone()).unwrap();
    for (wrong, category) in [
        (255, DiagnosticCategory::UnavailableValue),
        (u32::MAX, DiagnosticCategory::InvalidSelectedLowering),
    ] {
        fixture.proof.equal_outputs[0] = (0, wrong);
        assert_eq!(fixture.run(code.clone()).unwrap_err().category, category);
        fixture.proof.equal_outputs[0] = (wrong, 0);
        assert_eq!(fixture.run(code.clone()).unwrap_err().category, category);
    }
    fixture.proof.equal_outputs[0] = (0, high);
    for (wrong, category) in [
        (255, DiagnosticCategory::InvalidSpillSlotRead),
        (u32::MAX, DiagnosticCategory::InvalidSelectedLowering),
    ] {
        fixture.proof.scratch_outputs[0].1 = wrong;
        assert_eq!(fixture.run(code.clone()).unwrap_err().category, category);
    }
}

#[test]
fn wide_call_publications_and_borrowed_outputs_check_positions_above_255() {
    let mut fixture = Fixture::new(0, 258).scratch(1);
    let reference = FrameRef::Scratch {
        object: ScratchId(0),
        offset: 0,
    };
    let high: u32 = 256;
    let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 258).unwrap();
    let instruction = Instruction::Exec(symbol("wide_callee"));
    fixture.effects.call = Some(abi.invocation(Invocation::Exec).unwrap());
    fixture.scratch_effects.push(ScratchEffect::Borrow {
        object: ScratchId(0),
        offset: 0,
        size: 1,
        read: false,
        write: true,
        definite_write: true,
        retained: false,
    });
    fixture.proof.scratch_outputs.push((reference, high));
    fixture.proof.calls.push(CallSite {
        instruction: instruction.clone(),
        abi,
        inputs: vec![CallArgument::Local],
        path: vec![],
        truncate: false,
        outputs: vec![(reference, high)],
        publications: (0..258).map(|i| (Fixture::output(i), i)).collect(),
        selector: None,
    });
    let code = vec![inst(Instruction::Locaddr(0.into())), inst(instruction)];
    fixture.run(code.clone()).unwrap();
    for (wrong, category) in [
        (255, DiagnosticCategory::AbiMismatch),
        (u32::MAX, DiagnosticCategory::InvalidSelectedLowering),
    ] {
        fixture.proof.calls[0].publications[256].1 = wrong;
        assert_eq!(fixture.run(code.clone()).unwrap_err().category, category);
    }
    fixture.proof.calls[0].publications[256].1 = high;
    for (wrong, category) in [
        (255, DiagnosticCategory::InvalidSpillSlotRead),
        (u32::MAX, DiagnosticCategory::InvalidSelectedLowering),
    ] {
        fixture.proof.calls[0].outputs[0].1 = wrong;
        assert_eq!(fixture.run(code.clone()).unwrap_err().category, category);
    }
}
