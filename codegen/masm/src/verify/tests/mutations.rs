use super::*;

#[test]
fn word_actions_require_exact_instructions_and_owned_words() {
    fn candidate(depth: u32, word: u8) -> (ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
        let mut fixture = Fixture::new(depth, depth);
        fixture.proof.aliases = LaneAliases::new(
            (0..depth)
                .map(|index| (Fixture::output(index), Fixture::input(index)))
                .collect(),
        );
        let mut plan = fixture.plan();
        plan.segment_actions.insert(
            SegmentId(0),
            vec![
                PlanAction::SwapWord { word },
                PlanAction::SwapWord { word },
                PlanAction::RunLowering {
                    step: SelectedStepId(0),
                },
            ],
        );
        let mut code = AnnotatedMasmBody::new();
        code.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        for _ in 0..2 {
            code.push_instruction(
                Primitive::SwapWord(word).instruction().unwrap(),
                SelectedStepId(u32::MAX),
            );
        }
        code.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
        code.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        code.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
        (plan, fixture.frame(), code)
    }
    for word in 1..=3 {
        let (plan, frame, code) = candidate(16, word);
        verify(&plan, &frame, code.clone()).unwrap();
        for wrong in [Instruction::Swap1, Primitive::SwapWord(word % 3 + 1).instruction().unwrap()]
        {
            let mut altered = code.clone();
            let AnnotatedOp::Instruction { instruction, .. } = &mut altered.ops_mut()[1] else {
                panic!()
            };
            *instruction = wrong;
            assert_eq!(
                verify(&plan, &frame, altered).unwrap_err().category,
                DiagnosticCategory::InvalidSelectedLowering
            );
        }
        for wrong in [PlanAction::SwapWord { word: word % 3 + 1 }, PlanAction::Swap { depth: 1 }] {
            let mut altered_plan = plan.clone();
            altered_plan.segment_actions.get_mut(&SegmentId(0)).unwrap()[0] = wrong;
            assert_eq!(
                verify(&altered_plan, &frame, code.clone()).unwrap_err().category,
                DiagnosticCategory::InvalidSelectedLowering
            );
        }
        let (plan, frame, code) = candidate(u32::from(word) * 4 + 3, word);
        assert_eq!(
            verify(&plan, &frame, code).unwrap_err().category,
            DiagnosticCategory::IllegalStackAccess
        );
    }
}

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
        DiagnosticCategory::InvalidSelectedLowering
    );
    // Declare trap permission so the independent owned-input check is reached.
    let mut division = Fixture::new(1, 1);
    division.effects.may_trap = true;
    division.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
    assert_eq!(
        division.run(vec![inst(Instruction::U32Div)]).unwrap_err().category,
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

#[test]
fn memory_selected_fill_loop_cleanup_access_and_call_mutations_reject() {
    let f = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @fill(%p: ptr<u64, byte>, %n: u32, %v: u64) { \
         hir.mem_set %p, %n, %v : (ptr<u64, byte>, u32, u64); builtin.ret; };",
    );
    let (plan, frame, code) = crate::select::tests::ready(&f, &crate::select::SelectEnv::default());
    verify(&plan, &frame, code.clone()).unwrap();
    for mutation in 0..5 {
        let mut bad = code.clone();
        let mut changed = 0;
        rewrite(&mut bad, &mut |op| {
            if mutation < 4
                && let AnnotatedOp::While { body, .. } = op
            {
                let index = body
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
                match mutation {
                    0 => {
                        let AnnotatedOp::Instruction { instruction, .. } =
                            &mut body.ops_mut()[index]
                        else {
                            unreachable!()
                        };
                        *instruction = Instruction::Exec(symbol("::intrinsics::mem::store_sw"));
                    }
                    1 => {
                        body.ops_mut().insert(0, inst(Instruction::Dup15));
                    }
                    2 => {
                        body.ops_mut().insert(index, inst(Instruction::Swap2));
                    }
                    _ => {
                        body.ops_mut().insert(
                            index,
                            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
                        );
                    }
                }
                changed += 1;
            }
        });
        if mutation == 4 {
            let index = bad
                .ops()
                .iter()
                .rposition(|op| {
                    matches!(
                        op,
                        AnnotatedOp::Instruction {
                            instruction: Instruction::Drop,
                            ..
                        }
                    )
                })
                .unwrap();
            bad.ops_mut().remove(index);
            changed += 1;
        }
        assert_eq!(changed, 1);
        assert!(verify(&plan, &frame, bad).is_err(), "mutation {mutation}");
    }
}

#[test]
fn memory_final_source_local_layout_mutation_rejects() {
    let mut f = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @address() -> ptr<u32, element> { %p = \
         hir.local_address <{ local = #builtin.local_variable<1, u32> }>; builtin.ret %p : \
         (ptr<u32, element>); };",
    );
    f.function.borrow_mut().alloc_local(midenc_hir::Type::U128);
    f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
    let (plan, frame, code) = crate::select::tests::ready(&f, &crate::select::SelectEnv::default());
    verify(&plan, &frame, code.clone()).unwrap();
    let bad = FinalFrameLayout::new(
        SourceLocalLayout {
            offsets: vec![0, 3],
            extent: 5,
        },
        frame.spills().clone(),
        frame.scratch().clone(),
        frame.aligned_size(),
    )
    .unwrap();
    assert_eq!(
        verify(&plan, &bad, code).unwrap_err().category,
        DiagnosticCategory::FrameCapacityExceeded
    );
}

#[test]
fn memory_source_local_access_cannot_enter_compiler_frame_padding() {
    let mut f = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @store(%v: u32) { hir.store_local %v <{ local = \
         #builtin.local_variable<0, u32> }> : (u32); builtin.ret; };",
    );
    f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
    let (plan, frame, mut code) =
        crate::select::tests::ready(&f, &crate::select::SelectEnv::default());
    let mut changed = 0;
    rewrite(&mut code, &mut |op| {
        if let AnnotatedOp::Instruction {
            instruction: i @ Instruction::Locaddr(_),
            ..
        } = op
        {
            *i = Instruction::Locaddr(1u16.into());
            changed += 1;
        }
    });
    assert_eq!(changed, 1);
    assert!(verify(&plan, &frame, code).is_err());
}

#[test]
fn variable_groups_require_exactly_one_following_anchor() {
    use crate::lower::expand::{Anchor, DebugDecorator};
    let variable = || {
        AnnotatedOp::Anchor(Anchor::Debug {
            id: DebugLocationId(0),
            decorator: DebugDecorator::Variable(crate::masm::DebugVarInfo::new(
                "visible",
                crate::masm::DebugVarLocation::Unavailable,
            )),
        })
    };
    let nop = || AnnotatedOp::Anchor(Anchor::Nop);
    let instruction = || AnnotatedOp::Instruction {
        instruction: Instruction::Nop,
        unit: SelectedStepId(0),
    };
    let inline = || {
        AnnotatedOp::Anchor(Anchor::Debug {
            id: DebugLocationId(1),
            decorator: DebugDecorator::InlineCallClear,
        })
    };
    let boundary = || AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0)));
    let inline_call = || {
        let location = miden_assembly::debuginfo::FileLineCol::new(
            miden_assembly::debuginfo::Uri::new("inline.hir"),
            miden_assembly::debuginfo::LineNumber::new(1).unwrap(),
            miden_assembly::debuginfo::ColumnNumber::new(1).unwrap(),
        );
        AnnotatedOp::Anchor(Anchor::Debug {
            id: DebugLocationId(2),
            decorator: DebugDecorator::InlineCall(crate::masm::DebugInlineCallInfo::new(
                "inlined",
                location.clone(),
                location,
            )),
        })
    };
    for ops in [
        vec![instruction(), variable(), nop()],
        vec![variable(), nop(), instruction(), variable(), nop()],
        vec![variable(), boundary(), variable(), boundary(), nop(), instruction()],
        vec![variable(), nop(), inline(), variable(), nop()],
        vec![variable(), nop(), inline_call(), variable(), nop()],
    ] {
        let mut body = AnnotatedMasmBody::new();
        body.ops_mut().extend(ops);
        super::super::check_anchors(&body).unwrap();
    }
    for ops in [
        vec![instruction(), variable()],
        vec![variable(), instruction()],
        vec![nop(), variable()],
        vec![variable(), nop(), nop()],
        vec![variable(), inline(), nop()],
        vec![variable(), inline_call(), nop()],
        vec![variable(), nop(), variable(), nop()],
        vec![instruction(), nop()],
        vec![variable(), instruction(), nop()],
    ] {
        let mut body = AnnotatedMasmBody::new();
        body.ops_mut().extend(ops);
        assert_eq!(
            super::super::check_anchors(&body).unwrap_err().category,
            DiagnosticCategory::InvalidSelectedLowering
        );
    }
}

#[test]
fn variable_anchor_positions_and_debug_identities_are_both_verified() {
    use crate::lower::expand::{Anchor, DebugDecorator, ExpansionEnv, expand_procedure};
    let mut plan =
        crate::allocate::allocate(empty_program(), &AllocationOptions::default()).unwrap();
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .push(PlanAction::DebugLocation(DebugLocationId(1)));
    let frame = crate::allocate::slots::assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    let env = ExpansionEnv {
        debug: BTreeMap::from([(
            DebugLocationId(1),
            DebugDecorator::Variable(crate::masm::DebugVarInfo::new(
                "v",
                crate::masm::DebugVarLocation::Unavailable,
            )),
        )]),
    };
    let mut body =
        expand_procedure(&plan, &frame, &crate::lower::units::LoweringRegistry::default(), &env)
            .unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    verify(&plan, &frame, body.clone()).unwrap();
    let mut missing = body.clone();
    missing.ops_mut().retain(|op| !matches!(op, AnnotatedOp::Anchor(Anchor::Nop)));
    assert!(verify(&plan, &frame, missing).is_err());
    let mut wrong = body.clone();
    rewrite(&mut wrong, &mut |op| {
        if let AnnotatedOp::Anchor(Anchor::Debug { id, .. }) = op {
            *id = DebugLocationId(2);
        }
    });
    assert!(verify(&plan, &frame, wrong).is_err());
    let mut moved = body.clone();
    let position = moved
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Anchor(Anchor::Debug { .. })))
        .unwrap();
    let debug = moved.ops_mut().remove(position);
    let nop = moved.ops_mut().remove(position);
    assert!(matches!(nop, AnnotatedOp::Anchor(Anchor::Nop)));
    moved.push(debug);
    moved.push(nop);
    // The whole group stays canonical; only its position in the planned action stream changes.
    super::super::check_anchors(&moved).unwrap();
    assert_eq!(
        verify(&plan, &frame, moved).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut disguised = body;
    let nop = disguised
        .ops_mut()
        .iter_mut()
        .find(|op| matches!(op, AnnotatedOp::Anchor(Anchor::Nop)))
        .unwrap();
    *nop = AnnotatedOp::Instruction {
        instruction: Instruction::Nop,
        unit: SelectedStepId(u32::MAX),
    };
    assert!(verify(&plan, &frame, disguised).is_err());
}

#[test]
fn repeat_variable_debug_anchor_has_valid_package_rows() {
    use alloc::sync::Arc;

    use miden_assembly::{Assembler, DefaultSourceManager, Path};

    use crate::{
        lower::expand::{Anchor, DebugDecorator},
        masm,
    };
    let variable = masm::DebugVarInfo::new("repeat_visible", masm::DebugVarLocation::Stack(0));
    let mut child = AnnotatedMasmBody::new();
    child.push(AnnotatedOp::Anchor(Anchor::Debug {
        id: DebugLocationId(0),
        decorator: DebugDecorator::Variable(variable.clone()),
    }));
    let mut body = AnnotatedMasmBody::new();
    body.push(AnnotatedOp::Repeat {
        id: ControlId(0),
        count: 2,
        body: child,
    });
    crate::opt::peephole::simplify_annotated(&mut body);
    super::super::check_anchors(&body).unwrap();
    let AnnotatedOp::Repeat { body: child, .. } = &mut body.ops_mut()[0] else {
        panic!("repeat");
    };
    assert_eq!(child.ops().len(), 2);
    assert!(matches!(child.ops()[1], AnnotatedOp::Anchor(Anchor::Nop)));
    child.ops_mut().pop();
    assert!(super::super::check_anchors(&body).is_err());

    // Assembler oracle for the target positional rule, independent of source plan reachability.
    // Raw lowering repeats cannot carry planned observations inside an active lowering unit.
    let span = masm::SourceSpan::UNKNOWN;
    let procedure = masm::Procedure::new(
        span,
        masm::Visibility::Public,
        masm::ProcedureName::new("repeat_debug").unwrap(),
        0,
        masm::Block::new(
            span,
            vec![masm::Op::Repeat {
                span,
                count: 2u32.into(),
                body: masm::Block::new(
                    span,
                    vec![
                        masm::Op::Inst(masm::Span::unknown(Instruction::DebugVar(variable))),
                        masm::Op::Inst(masm::Span::unknown(Instruction::Nop)),
                    ],
                ),
            }],
        ),
    );
    let manager = Arc::new(DefaultSourceManager::default());
    let mut module = masm::Module::parser(Some(masm::ModuleKind::Library))
        .parse_str(Some(Path::new("::repeat_debug")), "", manager.clone())
        .unwrap();
    module.define_procedure(procedure, manager.clone()).unwrap();
    let package = Assembler::new(manager)
        .assemble_library("repeat_debug", module, core::iter::empty::<&str>())
        .unwrap();
    let debug = package.debug_info().unwrap().unwrap();
    let rows = debug.nodes().iter().flat_map(|node| &node.debug_vars).collect::<Vec<_>>();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|row| debug.get_string(row.name_idx).unwrap().as_ref() == "repeat_visible"
                && row.value_location == masm::DebugVarLocation::Stack(0))
    );
}

#[test]
fn semantic_alias_publication_rejects_physical_mutation_and_lost_inputs() {
    let plan = super::reconstruction::semantic_alias_plan();
    for instructions in [
        vec![],
        vec![Instruction::Swap1],
        vec![Instruction::Drop],
        vec![Instruction::Push(crate::masm::PushValue::from(7u32).into())],
        vec![Instruction::Drop, Instruction::Push(crate::masm::PushValue::from(7u32).into())],
    ] {
        let should_pass = instructions.is_empty();
        let mut code = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        ];
        code.extend(instructions.into_iter().map(inst));
        code.extend([
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]);
        assert_eq!(verify(&plan, &frame(), ops(code)).is_ok(), should_pass);
    }
    let mut plan = plan;
    plan.segment_actions.get_mut(&SegmentId(0)).unwrap().insert(0, PlanAction::Drop);
    assert!(
        verify(
            &plan,
            &frame(),
            ops(vec![
                AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
                inst(Instruction::Drop),
                AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
                AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
                AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            ])
        )
        .is_err()
    );
}

#[test]
fn commutative_permissions_require_complete_equal_operands() {
    struct Source(u8);
    impl GeneratedDefinition for Source {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(1)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Fixture::new(2, 0).instantiate(&[])?;
            evidence.units.get_mut(&SelectedStepId(0)).unwrap().input_symmetry =
                InputSymmetry::CommutativePair {
                    lanes_per_operand: match self.0 {
                        1 => 0,
                        2 | 7 | 8 => 2,
                        _ => 1,
                    },
                };
            match self.0 {
                3 => {
                    evidence.program.values.get_mut(&ValueId(1)).unwrap().layout =
                        ValueLayout::new(Type::U32).unwrap()
                }
                4 => {
                    evidence.program.segments[0].units[0].semantic_inputs.pop().map(|_| ()).unwrap()
                }
                5 => evidence.program.segments[0].units[0].publication = OutputPublication::Aliases,
                6 => evidence.program.segments[0].units[0].stack_inputs[1] = Fixture::input(0),
                7 | 8 => {
                    for value in evidence.program.values.values_mut() {
                        value.layout = ValueLayout::new(Type::U64).unwrap();
                    }
                    evidence.program.abi =
                        ProcedureAbi::new(CallingConvention::SameContext, 4, 0).unwrap();
                    evidence.program.segments[0].units[0].stack_inputs = vec![
                        Atom::new(ValueId(0), 0),
                        Atom::new(ValueId(0), 1),
                        Atom::new(ValueId(1), 0),
                        Atom::new(ValueId(1), 1),
                    ];
                    if self.0 == 8 {
                        evidence.program.segments[0].units[0].stack_inputs.swap(0, 1);
                    }
                }
                _ => {}
            }
            Ok(evidence)
        }
    }
    for case in 0..=8 {
        let source = Source(case);
        let result = check_selection(
            source.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&source),
        );
        if matches!(case, 0 | 7) {
            result.unwrap();
        } else {
            assert_eq!(
                result.unwrap_err().category,
                DiagnosticCategory::InvalidSelectedLowering,
                "case {case}"
            );
        }
    }
}

#[test]
fn commutative_order_requires_permission_and_the_actual_prefix() {
    let mut fixture = Fixture::new(2, 1);
    let code = body(vec![Instruction::Add]);
    let mut ordered = fixture.plan();
    verify(&ordered, &fixture.frame(), code.clone()).unwrap();
    ordered.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().input_order =
        InputOrder::Reversed;
    assert_eq!(
        verify(&ordered, &fixture.frame(), code.clone()).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    fixture.proof.input_symmetry = InputSymmetry::CommutativePair {
        lanes_per_operand: 1,
    };
    let mut plan = fixture.plan();
    plan.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().input_order = InputOrder::Reversed;
    assert_eq!(
        verify(&plan, &fixture.frame(), code.clone()).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .insert(0, PlanAction::Swap { depth: 1 });
    let mut reversed = code;
    reversed.ops_mut().insert(
        1,
        AnnotatedOp::Instruction {
            instruction: Instruction::Swap1,
            unit: SelectedStepId(u32::MAX),
        },
    );
    verify(&plan, &fixture.frame(), reversed.clone()).unwrap();
    plan.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap().input_order = InputOrder::Canonical;
    assert_eq!(
        verify(&plan, &fixture.frame(), reversed).unwrap_err().category,
        DiagnosticCategory::UnavailableValue
    );
}
