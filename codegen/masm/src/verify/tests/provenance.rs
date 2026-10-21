use super::*;

pub(super) fn pointer_branch(other: AnnotatedOp) -> AnnotatedOp {
    AnnotatedOp::If {
        id: ControlId(0),
        then_body: ops(vec![inst(Instruction::Locaddr(0.into()))]),
        else_body: ops(vec![other]),
    }
}

#[test]
fn branch_compiler_origin_survives_distinct_and_mixed_pointer_joins() {
    for (source_extent, other) in [
        (0, inst(Instruction::Locaddr(1.into()))),
        (0, push(77)),
        (1, inst(Instruction::Locaddr(1.into()))),
    ] {
        let branch = pointer_branch(other);
        let mut fixture = Fixture::new(1, 1).scratch(2);
        if source_extent != 0 {
            fixture.locals = SourceLocalLayout {
                offsets: vec![0],
                extent: source_extent,
            };
        }
        assert_eq!(
            fixture.run(vec![branch.clone()]).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
        let mut load = fixture.clone();
        load.effects.memory = SourceMemoryEffects::READ;
        assert_eq!(
            load.run(vec![branch.clone(), inst(Instruction::MemLoad)]).unwrap_err().category,
            DiagnosticCategory::InvalidSpillSlotRead
        );
        let mut store = Fixture::new(1, 0).scratch(2);
        store.locals = fixture.locals.clone();
        store.effects.memory = SourceMemoryEffects::WRITE;
        assert_eq!(
            store
                .run(vec![branch, push(77), inst(Instruction::MemStore)])
                .unwrap_err()
                .category,
            DiagnosticCategory::InvalidSpillSlotRead
        );
    }
    let mut source = Fixture::new(1, 1);
    source.locals = SourceLocalLayout {
        offsets: vec![0, 1],
        extent: 2,
    };
    source.run(vec![pointer_branch(inst(Instruction::Locaddr(1.into())))]).unwrap();
    source.effects.memory = SourceMemoryEffects::READ;
    source
        .run(vec![
            pointer_branch(inst(Instruction::Locaddr(1.into()))),
            inst(Instruction::MemLoad),
        ])
        .unwrap();
}

#[test]
fn carried_compiler_origin_is_rechecked_after_backedge_taint() {
    for initial in [inst(Instruction::Locaddr(0.into())), push(0)] {
        let mut fixture = Fixture::new(0, 1).scratch(2);
        fixture.proof.loops.insert(
            ControlId(0),
            LoopSchema {
                stack: vec![LoopPosition::Carry(0)],
                cells: vec![],
            },
        );
        assert_eq!(
            fixture
                .run(vec![
                    initial,
                    AnnotatedOp::Repeat {
                        id: ControlId(0),
                        count: 2,
                        body: ops(vec![
                            inst(Instruction::Drop),
                            inst(Instruction::Locaddr(1.into()))
                        ]),
                    }
                ])
                .unwrap_err()
                .category,
            DiagnosticCategory::AbiMismatch
        );
    }
}

#[test]
fn carried_origin_fixed_point_rechecks_indirect_memory() {
    let mut fixture = Fixture::new(0, 1).scratch(2);
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    assert_eq!(
        fixture
            .run(vec![
                push(77),
                AnnotatedOp::Repeat {
                    id: ControlId(0),
                    count: 2,
                    body: ops(vec![
                        inst(Instruction::Dup0),
                        inst(Instruction::MemLoad),
                        inst(Instruction::Drop),
                        inst(Instruction::Drop),
                        inst(Instruction::Locaddr(1.into())),
                    ]),
                }
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

struct PointerTransfer(Fixture);

impl GeneratedDefinition for PointerTransfer {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(9)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = self.0.instantiate(parameters)?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        evidence.program.values.insert(
            ValueId(200),
            SelectedValue {
                id: ValueId(200),
                origin: ValueOrigin::Parameter {
                    segment: SegmentId(1),
                    index: 0,
                },
                layout: ValueLayout::new(Type::Felt).unwrap(),
                reconstruction: None,
            },
        );
        evidence.program.segments.push(SelectedSegment {
            content_parameters: vec![],
            id: SegmentId(1),
            parameters: vec![ValueId(200)],
            units: vec![],
        });
        // Replay visits segment 1 before its predecessor 2. Taint arriving later
        // must force another pass, even without an allocator-supplied address fact.
        let units = core::mem::take(&mut evidence.program.segments[0].units);
        evidence.program.segments.push(SelectedSegment {
            content_parameters: vec![],
            id: SegmentId(2),
            parameters: vec![],
            units,
        });
        evidence.program.edges[0] = TransferEdge {
            content_bindings: vec![],
            id: EdgeId(0),
            source: SegmentId(0),
            destination: Some(SegmentId(2)),
            kind: EdgeKind::Forward,
            bindings: vec![],
        };
        evidence.program.edges.push(TransferEdge {
            content_bindings: vec![],
            id: EdgeId(1),
            source: SegmentId(2),
            destination: Some(SegmentId(1)),
            kind: EdgeKind::Forward,
            bindings: vec![ParameterBinding {
                destination: ValueId(200),
                source: ValueId(100),
            }],
        });
        evidence.program.edges.push(TransferEdge {
            content_bindings: vec![],
            id: EdgeId(2),
            source: SegmentId(1),
            destination: None,
            kind: EdgeKind::Return,
            bindings: vec![ParameterBinding {
                destination: ValueId(200),
                source: ValueId(200),
            }],
        });
        evidence.program.structure = ControlTree::Sequence(vec![
            ControlTree::Segment(SegmentId(0)),
            ControlTree::Edge(EdgeId(0)),
            ControlTree::Segment(SegmentId(2)),
            ControlTree::Edge(EdgeId(1)),
            ControlTree::Segment(SegmentId(1)),
            ControlTree::Edge(EdgeId(2)),
        ]);
        Ok(evidence)
    }
}

#[test]
fn selected_contract_cannot_erase_origin_by_omitting_address_facts() {
    for compiler in [false, true] {
        let mut fixture = Fixture::new(0, 1).scratch(2);
        fixture.scratch[0].lifetime = ScratchLifetime::Spanning;
        if !compiler {
            fixture.locals = SourceLocalLayout {
                offsets: vec![0],
                extent: 1,
            };
        }
        let definition = PointerTransfer(fixture.clone());
        let mut plan = fixture.plan();
        plan.selected = check_selection(
            definition.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&definition),
        )
        .unwrap();
        let mut contract = plan.contracts[&SegmentId(0)].clone();
        contract.stack_top_first = vec![Atom::new(ValueId(200), 0)];
        plan.contracts.insert(SegmentId(1), contract);
        plan.contracts.insert(SegmentId(2), plan.contracts[&SegmentId(0)].clone());
        let actions = plan.segment_actions.insert(SegmentId(0), vec![]).unwrap();
        plan.segment_actions.insert(SegmentId(2), actions);
        plan.segment_actions.insert(SegmentId(1), vec![]);
        let code = ops(vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(2))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
            inst(Instruction::Locaddr(0.into())),
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(1))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(2))),
        ]);
        let result = verify(&plan, &fixture.frame(), code);
        if compiler {
            assert_eq!(result.unwrap_err().category, DiagnosticCategory::AbiMismatch);
        } else {
            result.unwrap();
        }
    }
}

/// Opaque source pointer arithmetic keeps its may-origin without requiring a local bounds proof.
#[test]
fn source_only_arithmetic_preserves_declared_memory_capability() {
    for arithmetic in [
        vec![push(4), inst(Instruction::Mul)],
        vec![inst(Instruction::MulImm(Felt::from(4u32).into()))],
        vec![inst(Instruction::U32Split), inst(Instruction::Drop)],
    ] {
        let mut fixture = Fixture::new(0, 0);
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 1,
        };
        fixture.effects.memory = SourceMemoryEffects::READ;
        let mut body = vec![inst(Instruction::Locaddr(0.into()))];
        body.extend(arithmetic);
        body.extend([inst(Instruction::MemLoad), inst(Instruction::Drop)]);
        fixture.run(body.clone()).unwrap();
        fixture.effects.memory = SourceMemoryEffects::default();
        assert!(fixture.run(body).is_err());
    }
}

#[test]
fn source_only_dynamic_add_and_sub_remain_opaque() {
    for instruction in [Instruction::Add, Instruction::Sub] {
        let mut fixture = Fixture::new(1, 0);
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 1,
        };
        fixture.effects.memory = SourceMemoryEffects::READ;
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                inst(instruction),
                inst(Instruction::MemLoad),
                inst(Instruction::Drop),
            ])
            .unwrap();
    }
}

#[test]
fn opaque_arithmetic_cannot_erase_compiler_or_mixed_origins() {
    for source_extent in [0, 1] {
        let mut fixture = Fixture::new(1, 0).scratch(2);
        fixture.locals = SourceLocalLayout {
            offsets: if source_extent == 0 { vec![] } else { vec![0] },
            extent: source_extent,
        };
        fixture.effects.memory = SourceMemoryEffects::READ;
        let body = vec![
            pointer_branch(inst(Instruction::Locaddr(1.into()))),
            push(4),
            inst(Instruction::Mul),
            inst(Instruction::MemLoad),
            inst(Instruction::Drop),
        ];
        assert_eq!(fixture.run(body).unwrap_err().category, DiagnosticCategory::IllegalStackAccess);
    }
}

#[test]
fn source_only_join_and_loop_progress_remain_source_memory() {
    let mut fixture = Fixture::new(1, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0, 1],
        extent: 2,
    };
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture
        .run(vec![
            pointer_branch(inst(Instruction::Locaddr(1.into()))),
            push(4),
            inst(Instruction::Mul),
            inst(Instruction::MemLoad),
            inst(Instruction::Drop),
        ])
        .unwrap();
    let mut fixture = Fixture::new(0, 0);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 1,
    };
    fixture.effects.memory = SourceMemoryEffects::READ;
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    fixture
        .run(vec![
            inst(Instruction::Locaddr(0.into())),
            AnnotatedOp::Repeat {
                id: ControlId(0),
                count: 2,
                body: ops(vec![push(4), inst(Instruction::Mul)]),
            },
            inst(Instruction::MemLoad),
            inst(Instruction::Drop),
        ])
        .unwrap();
}

#[test]
fn opaque_arithmetic_rechecks_late_compiler_loop_taint() {
    let mut fixture = Fixture::new(0, 0).scratch(2);
    fixture.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 1,
    };
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    assert_eq!(
        fixture
            .run(vec![
                inst(Instruction::Locaddr(0.into())),
                AnnotatedOp::Repeat {
                    id: ControlId(0),
                    count: 2,
                    body: ops(vec![
                        push(4),
                        inst(Instruction::Mul),
                        inst(Instruction::Drop),
                        inst(Instruction::Locaddr(1.into()))
                    ]),
                },
                inst(Instruction::Drop)
            ])
            .unwrap_err()
            .category,
        DiagnosticCategory::IllegalStackAccess
    );
}

#[test]
fn system_event_observation_rejects_compiler_pointer_before_cleanup() {
    use miden_core::events::SystemEvent;
    let mut fixture = Fixture::new(0, 0).scratch(1);
    fixture.effects.may_trap = true;
    let event = Instruction::EmitImm(
        crate::masm::Immediate::Value(crate::masm::Span::unknown(
            SystemEvent::U32Clz.event_id().as_felt(),
        ))
        .into(),
    );
    let error = fixture
        .run(vec![inst(Instruction::Locaddr(0.into())), inst(event), inst(Instruction::Drop)])
        .unwrap_err();
    assert!(error.message.contains("observed"), "{error:?}");
}

fn system_event_instruction(event: miden_core::events::SystemEvent) -> Instruction {
    Instruction::EmitImm(
        crate::masm::Immediate::Value(crate::masm::Span::unknown(event.event_id().as_felt()))
            .into(),
    )
}

#[test]
fn system_event_every_observed_lane_rejects_exact_mixed_and_unresolved_compiler_origins() {
    for (event, n) in miden_core::events::SystemEvent::all().into_iter().filter_map(|event| {
        crate::events::SystemEventDescriptor::from_id(event.event_id().as_felt())
            .map(|d| (event, usize::from(d.payload_lanes)))
    }) {
        for position in 0..n {
            for origin in 0..4 {
                let mut fixture = Fixture::new(u32::from(origin != 0), 0).scratch(2);
                fixture.locals = SourceLocalLayout {
                    offsets: vec![0],
                    extent: 1,
                };
                fixture.effects.may_trap = true;
                // These permissions cannot authorize compiler observation.
                fixture.effects.memory = SourceMemoryEffects::READ_WRITE;
                let mut code = match origin {
                    0 => vec![inst(Instruction::Locaddr(1.into()))],
                    1 => vec![pointer_branch(inst(Instruction::Locaddr(1.into())))],
                    2 => vec![AnnotatedOp::If {
                        id: ControlId(0),
                        then_body: ops(vec![inst(Instruction::Locaddr(1.into()))]),
                        else_body: ops(vec![push(77)]),
                    }],
                    _ => vec![AnnotatedOp::If {
                        id: ControlId(0),
                        then_body: ops(vec![inst(Instruction::Locaddr(1.into()))]),
                        else_body: ops(vec![inst(Instruction::Locaddr(2.into()))]),
                    }],
                };
                code.extend((1..n).map(|i| push(101 + i as u64)));
                if n > 1 {
                    code.push(inst(Primitive::MoveUp((n - 1) as u8).instruction().unwrap()));
                }
                if position > 0 {
                    code.push(inst(Primitive::MoveDown(position as u8).instruction().unwrap()));
                }
                code.push(inst(system_event_instruction(event)));
                code.extend((0..n).map(|_| inst(Instruction::Drop)));
                let error = fixture.run(code).unwrap_err();
                assert!(
                    error.message.contains("observed"),
                    "{event:?} lane{position} origin{origin}: {error:?}"
                );
            }
        }
    }
}

#[test]
fn system_event_source_only_observation_and_unobserved_compiler_suffix_are_accepted() {
    for (event, n) in miden_core::events::SystemEvent::all().into_iter().filter_map(|event| {
        crate::events::SystemEventDescriptor::from_id(event.event_id().as_felt())
            .map(|d| (event, usize::from(d.payload_lanes)))
    }) {
        for position in 0..n {
            let mut fixture = Fixture::new(0, 0).scratch(1);
            fixture.locals = SourceLocalLayout {
                offsets: vec![0],
                extent: 1,
            };
            fixture.effects.may_trap = true;
            fixture.effects.memory = SourceMemoryEffects::READ;
            let mut code = vec![inst(Instruction::Locaddr(1.into()))];
            code.extend((0..n).rev().map(|i| {
                if i == position {
                    inst(Instruction::Locaddr(0.into()))
                } else {
                    push(if i == 5 { 0 } else { 101 + i as u64 })
                }
            }));
            code.push(inst(system_event_instruction(event)));
            code.extend((0..n + 1).map(|_| inst(Instruction::Drop)));
            fixture.run(code).unwrap();
        }
    }
}

#[test]
fn system_event_internal_loop_rechecks_late_compiler_taint_before_cleanup() {
    let mut fixture = Fixture::new(0, 0).scratch(1);
    fixture.effects.may_trap = true;
    fixture.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Carry(0)],
            cells: vec![],
        },
    );
    let error = fixture
        .run(vec![
            push(77),
            AnnotatedOp::Repeat {
                id: ControlId(0),
                count: 2,
                body: ops(vec![
                    inst(system_event_instruction(miden_core::events::SystemEvent::U32Clz)),
                    inst(Instruction::Drop),
                    inst(Instruction::Locaddr(0.into())),
                ]),
            },
            inst(Instruction::Drop),
        ])
        .unwrap_err();
    assert!(error.message.contains("observed"), "{error:?}");
}

struct SystemEventGraphLoop {
    memory: bool,
}
impl GeneratedDefinition for SystemEventGraphLoop {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(if self.memory { 20 } else { 19 })
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = super::loops::LoopDefinition.instantiate(parameters)?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        for record in evidence.program.source_map.values_mut() {
            if let SourceOrigin::Generated { definition, .. } = &mut record.origin {
                *definition = self.id();
            }
        }
        let unit = &mut evidence.program.segments[2].units[0];
        unit.effects.may_trap = true;
        if self.memory {
            unit.effects.memory = SourceMemoryEffects::READ;
        }
        unit.scratch.push(ScratchRequirement {
            id: ScratchId(0),
            size: 1,
            alignment: 1,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![unit.id],
        });
        // Exit explicitly cleans up the carried pointer before returning an ordinary value.
        evidence.units.get_mut(&SelectedStepId(1)).unwrap().aliases = LaneAliases::default();
        Ok(evidence)
    }
}

#[test]
fn system_event_graph_backedge_rechecks_late_compiler_taint_before_cleanup() {
    system_event_graph_backedge_case(false, None);
}

#[test]
fn memory_system_event_graph_backedge_rechecks_late_compiler_bounds_before_cleanup() {
    system_event_graph_backedge_case(true, None);
}

fn system_event_graph_backedge_case(memory: bool, println_lane: Option<usize>) {
    let definition = SystemEventGraphLoop {
        memory: memory || println_lane.is_some(),
    };
    let (mut plan, _, body) = super::loops::loop_fixture(true);
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let slot = plan.spill_slots[0];
    let scratch = ScratchAllocation {
        offset: 1,
        size: 1,
        alignment: 1,
        lifetime: ScratchLifetime::Spanning,
    };
    plan.lowering_bindings
        .get_mut(&SelectedStepId(0))
        .unwrap()
        .scratch
        .insert(ScratchId(0), scratch);
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::from([(ScratchId(0), scratch)]),
        4,
    )
    .unwrap();
    fn replace(
        body: &AnnotatedMasmBody,
        memory: bool,
        println_lane: Option<usize>,
    ) -> AnnotatedMasmBody {
        let mut result = vec![];
        let mut unit = None;
        for op in body.ops() {
            match op {
                AnnotatedOp::Boundary(Boundary::UnitStart(id)) => {
                    unit = Some(*id);
                    result.push(op.clone());
                    if *id == SelectedStepId(0) {
                        if memory {
                            result.extend([
                                inst(Instruction::Dup0),
                                push(53),
                                push(37),
                                push(23),
                                push(11),
                                inst(system_event_instruction(
                                    miden_core::events::SystemEvent::MemToMap,
                                )),
                            ]);
                            result.extend((0..6).map(|_| inst(Instruction::Drop)));
                            result.extend([inst(Instruction::Locaddr(1.into())), push(0)]);
                        } else if let Some(lane) = println_lane {
                            result.extend([inst(Instruction::Dup0), push(0)]);
                            if lane == 0 {
                                result.push(inst(Instruction::Swap1));
                            }
                            result.extend([
                                inst(println_instruction()),
                                inst(Instruction::Drop),
                                inst(Instruction::Drop),
                                inst(Instruction::Drop),
                                inst(Instruction::Locaddr(1.into())),
                                push(0),
                            ]);
                        } else {
                            result.extend([
                                inst(Instruction::Dup0),
                                inst(system_event_instruction(
                                    miden_core::events::SystemEvent::U32Clz,
                                )),
                                inst(Instruction::Drop),
                                inst(Instruction::Drop),
                                inst(Instruction::Locaddr(1.into())),
                                push(0),
                            ]);
                        }
                    } else {
                        result.extend([
                            AnnotatedOp::Instruction {
                                instruction: Instruction::Drop,
                                unit: *id,
                            },
                            AnnotatedOp::Instruction {
                                instruction: Primitive::Push(Felt::from(77u32))
                                    .instruction()
                                    .unwrap(),
                                unit: *id,
                            },
                        ]);
                    }
                }
                AnnotatedOp::Boundary(Boundary::UnitEnd(_)) => {
                    unit = None;
                    result.push(op.clone());
                }
                AnnotatedOp::Instruction { .. } if unit.is_some() => {}
                AnnotatedOp::If {
                    id,
                    then_body,
                    else_body,
                } => result.push(AnnotatedOp::If {
                    id: *id,
                    then_body: replace(then_body, memory, println_lane),
                    else_body: replace(else_body, memory, println_lane),
                }),
                AnnotatedOp::While { id, body } => result.push(AnnotatedOp::While {
                    id: *id,
                    body: replace(body, memory, println_lane),
                }),
                other => result.push(other.clone()),
            }
        }
        ops(result)
    }
    let error = verify(&plan, &frame, replace(&body, memory, println_lane)).unwrap_err();
    assert!(error.message.contains("observed"), "{error:?}");
}

fn memory_system_event_range_code(start: u16, end: u16) -> Vec<AnnotatedOp> {
    vec![
        inst(Instruction::Locaddr(end.into())),
        inst(Instruction::Locaddr(start.into())),
        push(53),
        push(37),
        push(23),
        push(11),
        inst(system_event_instruction(miden_core::events::SystemEvent::MemToMap)),
    ]
}

#[test]
fn memory_system_event_exact_source_bounds_zero_extent_padding_and_strict_reuse() {
    for (extent, start, end, pass) in [
        (0, 0, 0, true),
        (0, 0, 1, false),
        (0, 1, 1, false),
        (3, 0, 3, true),
        (3, 3, 3, true),
        (3, 4, 4, false),
        (4, 0, 4, true),
        (4, 1, 3, true),
        (4, 4, 0, false),
        (4, 0, 5, false),
        (4, 5, 5, false),
    ] {
        let mut f = Fixture::new(0, 0).scratch(4);
        f.locals = SourceLocalLayout {
            offsets: if extent > 0 { vec![0] } else { vec![] },
            extent,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ_WRITE;
        let mut code = memory_system_event_range_code(start, end);
        code.extend((0..6).map(|_| inst(Instruction::Drop)));
        let result = f.run(code);
        assert_eq!(result.is_ok(), pass, "L{extent} [{start},{end}): {result:?}");
    }
    for lane in 0..4 {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        let mut code = memory_system_event_range_code(4, 4);
        code.splice(2 + lane..3 + lane, [inst(Instruction::Locaddr(4.into()))]);
        code.extend((0..6).map(|_| inst(Instruction::Drop)));
        assert!(f.run(code).unwrap_err().message.contains("observed"));
    }
    // The permitted endpoint remains compiler-origin after aliases are replayed.
    for reuse in 0..3 {
        let mut f = Fixture::new(0, u32::from(reuse == 0)).scratch(1);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ_WRITE;
        let mut code = memory_system_event_range_code(4, 4);
        code.extend((0..5).map(|_| inst(Instruction::Drop)));
        match reuse {
            0 => {}
            1 => code.extend([inst(Instruction::Locaddr(0.into())), inst(Instruction::MemStore)]),
            _ => code.extend([
                inst(system_event_instruction(miden_core::events::SystemEvent::U32Clz)),
                inst(Instruction::Drop),
            ]),
        }
        assert!(f.run(code).is_err(), "reuse{reuse}");
    }
}

#[test]
fn memory_system_event_exact_mixed_token_fixture_keeps_origins_and_requires_spatial_proof() {
    // Isolated token fixture: existing source joins normally abstract away exact addresses.
    // Seed an exact token with an additional source origin; no token implementation is changed.
    for (start, end, pass) in
        [(0, 4, true), (4, 4, true), (0, 5, false), (5, 5, false), (4, 0, false)]
    {
        let mut f = Fixture::new(0, 0).scratch(2);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let plan = f.plan();
        let frame = f.frame();
        let mut verifier = super::super::replay::Verifier::new(&plan, &frame).unwrap();
        use super::super::tokens::Identity;
        let source = verifier.tokens.intern(Identity::FrameAddress(0));
        let boundary = verifier.tokens.intern(Identity::FrameAddress(end));
        verifier.tokens.add_origins(boundary, verifier.tokens.origins(source));
        let origins = verifier.tokens.origins(boundary);
        assert_eq!(origins.compiler(), end >= 4);
        let mut body = AnnotatedMasmBody::new();
        body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
        for op in memory_system_event_range_code(start as u16, end as u16) {
            body.push(op);
        }
        for _ in 0..6 {
            body.push(inst(Instruction::Drop));
        }
        body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
        let result = verifier.execute(
            &body,
            super::super::stack::State::new(vec![], BTreeMap::new()),
            &mut super::super::replay::Cursor::default(),
        );
        assert_eq!(result.is_ok(), pass);
        assert_eq!(verifier.tokens.origins(boundary), origins);
        if let Ok(Some(state)) = result {
            assert!(state.cells.is_empty());
        }
    }
}

#[test]
fn memory_system_event_equal_unresolved_compiler_bounds_and_literal_coincidences_reject() {
    for mode in 0..3 {
        let mut f = Fixture::new(u32::from(mode == 0), 0).scratch(2);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let mut code = match mode {
            0 => {
                vec![pointer_branch(inst(Instruction::Locaddr(4.into()))), inst(Instruction::Dup0)]
            }
            1 => vec![inst(Instruction::Locaddr(4.into())), push(4)],
            _ => vec![push(0x1_0000_0000), inst(Instruction::Locaddr(4.into()))],
        };
        code.extend([
            push(53),
            push(37),
            push(23),
            push(11),
            inst(system_event_instruction(miden_core::events::SystemEvent::MemToMap)),
        ]);
        code.extend((0..6).map(|_| inst(Instruction::Drop)));
        assert!(f.run(code).unwrap_err().message.contains("observed bounds"), "mode{mode}");
    }
}

#[test]
fn memory_system_event_observations_precede_deferred_no_read_shortcuts() {
    use miden_core::events::SystemEvent::DeferredRegisterData;
    for payload in [
        [2, 0, 0, 0, 400, 0],
        [0, 0, 0, 0, 400, 1],
        [1, 7, 0, 0, 400, 1],
        [1, 0, 0, 0, 400, 2],
        [2, 0, 0, 0, 5, 1],
        [2, 0, 0, 0, 0xfffffff8, 1],
        [2, 0, 0, 0, 400, 131072],
    ] {
        for lane in 0..6 {
            let mut f = Fixture::new(0, 0).scratch(1);
            f.effects.may_trap = true;
            let mut code = payload
                .into_iter()
                .enumerate()
                .rev()
                .map(|(i, n)| {
                    if i == lane {
                        inst(Instruction::Locaddr(0.into()))
                    } else {
                        push(n)
                    }
                })
                .collect::<Vec<_>>();
            code.push(inst(system_event_instruction(DeferredRegisterData)));
            code.extend((0..6).map(|_| inst(Instruction::Drop)));
            assert!(
                f.run(code).unwrap_err().message.contains("observed"),
                "{payload:?} lane{lane}"
            );
        }
    }
}

#[test]
fn memory_system_event_internal_loop_rechecks_late_bounds_and_strict_keys() {
    for strict in [false, true] {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        f.proof.loops.insert(
            ControlId(0),
            LoopSchema {
                stack: vec![LoopPosition::Carry(0)],
                cells: vec![],
            },
        );
        let mut body = if strict {
            vec![push(0), push(0), push(53), push(37), push(23), inst(Instruction::Dup5)]
        } else {
            vec![inst(Instruction::Dup0), push(53), push(37), push(23), push(11)]
        };
        body.push(inst(system_event_instruction(miden_core::events::SystemEvent::MemToMap)));
        body.extend((0..if strict { 7 } else { 6 }).map(|_| inst(Instruction::Drop)));
        body.push(inst(Instruction::Locaddr(0.into())));
        let error = f
            .run(vec![
                push(77),
                AnnotatedOp::Repeat {
                    id: ControlId(0),
                    count: 2,
                    body: ops(body),
                },
                inst(Instruction::Drop),
            ])
            .unwrap_err();
        assert!(error.message.contains("observed"), "strict{strict}: {error:?}");
    }
}

#[test]
fn memory_system_event_permitted_endpoint_cannot_escape_through_unauthorized_call() {
    let mut f = Fixture::new(0, 0).scratch(1);
    f.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    f.effects.may_trap = true;
    let call = Instruction::Exec(crate::masm::InvocationTarget::Symbol(
        crate::masm::Ident::new("callee").unwrap(),
    ));
    f.proof.calls.push(CallSite {
        path: vec![],
        instruction: call.clone(),
        abi: ProcedureAbi::new(CallingConvention::SameContext, 1, 0).unwrap(),
        inputs: vec![CallArgument::Local],
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    });
    let mut code = memory_system_event_range_code(4, 4);
    code.extend((0..5).map(|_| inst(Instruction::Drop)));
    code.push(inst(call));
    assert_eq!(f.run(code).unwrap_err().category, DiagnosticCategory::AbiMismatch);
}

#[test]
fn memory_system_event_exact_invariant_boundary_is_reproved_on_each_internal_iteration() {
    let mut f = Fixture::new(0, 0).scratch(1);
    f.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    f.effects.may_trap = true;
    f.proof.loops.insert(
        ControlId(0),
        LoopSchema {
            stack: vec![LoopPosition::Invariant],
            cells: vec![],
        },
    );
    let mut body = vec![
        inst(Instruction::Dup0),
        inst(Instruction::Dup0),
        push(53),
        push(37),
        push(23),
        push(11),
        inst(system_event_instruction(miden_core::events::SystemEvent::MemToMap)),
    ];
    body.extend((0..6).map(|_| inst(Instruction::Drop)));
    f.run(vec![
        inst(Instruction::Locaddr(4.into())),
        AnnotatedOp::Repeat {
            id: ControlId(0),
            count: 2,
            body: ops(body),
        },
        inst(Instruction::Drop),
    ])
    .unwrap();
}

#[test]
fn memory_system_event_strict_keys_precede_every_source_range_shortcut() {
    for (start, end) in [(0, 0), (0x1_0000_0000, 9), (0, 0x1_0000_0000), (9, 2)] {
        for key in 0..4 {
            let mut f = Fixture::new(0, 0).scratch(1);
            f.effects.may_trap = true;
            let mut code = vec![push(end), push(start)];
            code.extend((0..4).rev().map(|i| {
                if i == key {
                    inst(Instruction::Locaddr(0.into()))
                } else {
                    push(11 + i)
                }
            }));
            code.push(inst(system_event_instruction(miden_core::events::SystemEvent::MemToMap)));
            code.extend((0..6).map(|_| inst(Instruction::Drop)));
            assert!(f.run(code).unwrap_err().message.contains("observed input"));
        }
    }
}

fn println_instruction() -> Instruction {
    Instruction::EmitImm(crate::Event::PrintLn.into())
}

#[test]
fn println_permissions_cover_possible_reads_and_every_canonical_no_read_shortcut() {
    for (pointer, length, reads) in [
        (Some(400), Some(1), true),
        (Some(403), Some(524288), true),
        (Some(4294967295), Some(2), true),
        (None, None, true),
        (None, Some(0), false),
        (Some(4294967296), None, false),
        (None, Some(524289), false),
        (Some(400), Some(0), false),
    ] {
        for permission in [false, true] {
            let mut f = Fixture::new(u32::from(pointer.is_none()) + u32::from(length.is_none()), 0);
            f.effects.may_trap = true;
            if permission {
                f.effects.memory = SourceMemoryEffects::READ;
            }
            let mut code = vec![];
            if let Some(length) = length {
                code.push(push(length));
            }
            if let Some(pointer) = pointer {
                code.push(push(pointer));
            } else if length.is_some() {
                code.push(inst(Instruction::Swap1));
            }
            code.extend([
                inst(println_instruction()),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
            ]);
            assert_eq!(
                f.run(code).is_ok(),
                !reads || permission,
                "{pointer:?}/{length:?} READ={permission}"
            );
        }
    }
}

#[test]
fn println_both_observations_precede_zero_invalid_pointer_and_cap_shortcuts() {
    for position in 0..2 {
        for other in [0, 524289, 4294967296] {
            for origin in 0..4 {
                let mut f = Fixture::new(u32::from(origin != 0), 0).scratch(2);
                f.locals = SourceLocalLayout {
                    offsets: vec![0],
                    extent: 1,
                };
                f.effects.may_trap = true;
                f.effects.memory = SourceMemoryEffects::READ;
                let mut code = match origin {
                    0 => vec![inst(Instruction::Locaddr(1.into()))],
                    1 => vec![pointer_branch(inst(Instruction::Locaddr(1.into())))],
                    2 => vec![AnnotatedOp::If {
                        id: ControlId(0),
                        then_body: ops(vec![inst(Instruction::Locaddr(1.into()))]),
                        else_body: ops(vec![push(77)]),
                    }],
                    _ => vec![AnnotatedOp::If {
                        id: ControlId(0),
                        then_body: ops(vec![inst(Instruction::Locaddr(1.into()))]),
                        else_body: ops(vec![inst(Instruction::Locaddr(2.into()))]),
                    }],
                };
                code.push(push(other));
                if position == 0 {
                    code.push(inst(Instruction::Swap1));
                }
                code.extend([
                    inst(println_instruction()),
                    inst(Instruction::Drop),
                    inst(Instruction::Drop),
                ]);
                let error = f.run(code).unwrap_err();
                assert!(
                    error.message.contains("observed"),
                    "lane{position} other{other} origin{origin}: {error:?}"
                );
            }
        }
    }
}

#[test]
fn println_source_coordinates_are_opaque_and_original_origins_survive() {
    use super::super::tokens::Identity;
    for offset in [0, 3] {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let plan = f.plan();
        let frame = f.frame();
        let mut verifier = super::super::replay::Verifier::new(&plan, &frame).unwrap();
        let pointer = verifier.tokens.intern(Identity::FrameAddress(offset));
        let origins = verifier.tokens.origins(pointer);
        let mut body = vec![
            push(524288),
            inst(Instruction::Locaddr((offset as u16).into())),
            inst(println_instruction()),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ];
        let mut wrapped = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        ];
        wrapped.append(&mut body);
        wrapped.extend([
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]);
        let result = verifier
            .execute(
                &ops(wrapped),
                super::super::stack::State::new(vec![], BTreeMap::new()),
                &mut super::super::replay::Cursor::default(),
            )
            .unwrap()
            .unwrap();
        assert!(result.cells.is_empty());
        assert_eq!(verifier.tokens.origins(pointer), origins);
        assert_eq!(verifier.tokens.frame_address(pointer), Some(offset));
    }
    // Source-only native address bits are legal strict observations in the length lane too.
    let mut source_length = Fixture::new(0, 0);
    source_length.locals = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    source_length.effects.may_trap = true;
    source_length.effects.memory = SourceMemoryEffects::READ;
    source_length
        .run(vec![
            inst(Instruction::Locaddr(3.into())),
            push(400),
            inst(println_instruction()),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ])
        .unwrap();
    // Unobserved compiler suffix is retained and cleaned without becoming a payload.
    let mut f = Fixture::new(0, 0).scratch(1);
    f.effects.may_trap = true;
    f.run(vec![
        inst(Instruction::Locaddr(0.into())),
        push(0),
        push(400),
        inst(println_instruction()),
        inst(Instruction::Drop),
        inst(Instruction::Drop),
        inst(Instruction::Drop),
    ])
    .unwrap();
}

#[test]
fn println_internal_loop_rechecks_late_compiler_taint_in_both_payload_lanes() {
    for position in 0..2 {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        f.proof.loops.insert(
            ControlId(0),
            LoopSchema {
                stack: vec![LoopPosition::Carry(0)],
                cells: vec![],
            },
        );
        let mut body = vec![push(0)];
        if position == 0 {
            body.push(inst(Instruction::Swap1));
        }
        body.extend([
            inst(println_instruction()),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
            inst(Instruction::Locaddr(0.into())),
        ]);
        let error = f
            .run(vec![
                push(0),
                AnnotatedOp::Repeat {
                    id: ControlId(0),
                    count: 2,
                    body: ops(body),
                },
                inst(Instruction::Drop),
            ])
            .unwrap_err();
        assert!(error.message.contains("observed"), "{error:?}");
    }
}

#[test]
fn println_graph_backedge_rechecks_late_compiler_taint_in_both_payload_lanes() {
    for lane in 0..2 {
        system_event_graph_backedge_case(false, Some(lane));
    }
}

#[test]
fn println_exact_mixed_token_origins_cannot_escape_no_read_in_either_lane() {
    use super::super::tokens::Identity;
    for lane in 0..2 {
        let mut f = Fixture::new(0, 0).scratch(1);
        f.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        f.effects.may_trap = true;
        f.effects.memory = SourceMemoryEffects::READ;
        let plan = f.plan();
        let frame = f.frame();
        let mut verifier = super::super::replay::Verifier::new(&plan, &frame).unwrap();
        // Isolated token fixture: source joins normally abstract exact identity. Keep
        // an exact source address while explicitly adding a compiler may-origin.
        let source = verifier.tokens.intern(Identity::FrameAddress(0));
        let compiler = verifier.tokens.intern(Identity::FrameAddress(4));
        verifier.tokens.add_origins(source, verifier.tokens.origins(compiler));
        let origins = verifier.tokens.origins(source);
        assert!(origins.compiler());
        let mut code = vec![
            inst(Instruction::Locaddr(0.into())),
            push(if lane == 0 { 0 } else { 4294967296 }),
        ];
        if lane == 0 {
            code.push(inst(Instruction::Swap1));
        }
        code.extend([
            inst(println_instruction()),
            inst(Instruction::Drop),
            inst(Instruction::Drop),
        ]);
        let mut wrapped = vec![
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
            AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
        ];
        wrapped.extend(code);
        wrapped.extend([
            AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
        ]);
        let error = verifier
            .execute(
                &ops(wrapped),
                super::super::stack::State::new(vec![], BTreeMap::new()),
                &mut super::super::replay::Cursor::default(),
            )
            .unwrap_err();
        assert!(error.message.contains("observed"), "{error:?}");
        assert_eq!(verifier.tokens.origins(source), origins);
        assert_eq!(verifier.tokens.frame_address(source), Some(0));
    }
}

#[test]
fn generic_event_full_state_observation_preserves_tokens_origins_cells_and_padding() {
    use super::super::{
        replay::{ActiveUnit, Cursor, Verifier},
        stack::State,
        tokens::Identity,
    };
    for dynamic in [false, true] {
        let mut fixture = Fixture::new(u32::from(dynamic), u32::from(dynamic));
        fixture.effects.may_trap = true;
        fixture.locals = SourceLocalLayout {
            offsets: vec![0],
            extent: 4,
        };
        fixture.proof.generated_event = Some(if dynamic {
            GeneratedEvent::Dynamic(Fixture::input(0))
        } else {
            GeneratedEvent::Immediate
        });
        if dynamic {
            fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(0))]);
        }
        let mut plan = fixture.plan();
        plan.spill_slots = vec![SpillSlotId::temporary(SelectedStepId(0), 0)];
        let frame = FinalFrameLayout::new(
            fixture.locals.clone(),
            BTreeMap::from([(SpillSlotId::temporary(SelectedStepId(0), 0), 4)]),
            BTreeMap::new(),
            8,
        )
        .unwrap();
        let mut verifier = Verifier::new(&plan, &frame).unwrap();
        let compiler = verifier.tokens.intern(Identity::FrameAddress(4));
        let source = verifier.tokens.intern(Identity::FrameAddress(0));
        let mixed = verifier.tokens.intern(Identity::Join(0, ControlId(0), vec![compiler, source]));
        let origins = [verifier.tokens.origins(compiler), verifier.tokens.origins(mixed)];
        let mut state =
            State::new(vec![compiler, mixed], BTreeMap::from([(Fixture::input(0), compiler)]));
        state.cells.insert(4, mixed);
        state.slots.insert(SpillSlotId::temporary(SelectedStepId(0), 0), mixed);
        let before = state.clone();
        let mut cursor = Cursor {
            active: Some(ActiveUnit {
                id: SelectedStepId(0),
                incoming: state.clone(),
                readonly: BTreeMap::new(),
                call: 0,
                path: vec![],
            }),
            ..Default::default()
        };
        let event = if dynamic {
            Instruction::Emit
        } else {
            Instruction::EmitImm(crate::Event::PrintLn.into())
        };
        verifier.instruction(&event, &mut state, &mut cursor).unwrap();
        assert_eq!(state.stack, before.stack);
        assert_eq!(state.cells, before.cells);
        assert_eq!(state.names, before.names);
        assert_eq!(state.slots, before.slots);
        assert_eq!(state.written, before.written);
        assert_eq!(state.padding, before.padding);
        assert_eq!([verifier.tokens.origins(compiler), verifier.tokens.origins(mixed)], origins);
        assert_eq!(verifier.tokens.frame_address(compiler), Some(4));
        let mut edge = plan.selected.edges[0].clone();
        edge.bindings.clear();
        assert_eq!(
            verifier.check_return(&state, &edge).unwrap_err().category,
            DiagnosticCategory::AbiMismatch
        );
        // Full-state observation does not grant a source store permission, including after
        // cleanup of another observed pointer. Original compiler provenance remains intact.
        state.stack = vec![source, compiler];
        assert!(verifier.instruction(&Instruction::MemStore, &mut state, &mut cursor).is_err());
        state.stack.clear();
        assert!(verifier.tokens.origins(compiler).compiler());
        assert!(verifier.tokens.origins(mixed).compiler());
        // Following opaque advice has no numerical disjointness proof; unknown addresses
        // continue to rely on the source-memory/caller-local ABI, without declassification.
        let advice = verifier.tokens.fresh();
        assert!(!verifier.tokens.origins(advice).has_frame());
    }
}

#[test]
fn generic_event_authority_does_not_transfer_to_typed_or_ordinary_units() {
    let mut fixture = Fixture::new(0, 0).scratch(1);
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ_WRITE;
    for event in [
        Instruction::Emit,
        Instruction::EmitImm(crate::Event::PrintLn.into()),
        Instruction::EmitImm(crate::masm::EventImmediate::Immediate(
            miden_core::events::SystemEvent::HasMapKey.event_id().as_felt().into(),
        )),
    ] {
        let error = fixture
            .run(vec![
                push(0),
                push(0),
                push(0),
                inst(Instruction::Locaddr(0.into())),
                inst(event),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
                inst(Instruction::Drop),
            ])
            .unwrap_err();
        assert!(
            matches!(
                error.category,
                DiagnosticCategory::IllegalStackAccess
                    | DiagnosticCategory::InvalidSelectedLowering
            ),
            "{error:?}"
        );
    }
}
