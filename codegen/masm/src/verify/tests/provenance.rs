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
