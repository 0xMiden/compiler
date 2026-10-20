use super::*;

struct Stages;

impl GeneratedDefinition for Stages {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(2)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(error("stages take no parameters"));
        }
        let mut evidence = Identity.instantiate(&[])?;
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let intermediate = Atom::new(ValueId(1), 0);
        let result = Atom::new(ValueId(2), 0);
        evidence.program.values.insert(
            ValueId(2),
            SelectedValue {
                id: ValueId(2),
                origin: ValueOrigin::Temporary {
                    step: SourceStepId(1),
                    index: 0,
                },
                layout: ValueLayout::new(Type::Felt).unwrap(),
                reconstruction: None,
            },
        );
        let first = &mut evidence.program.segments[0].units[0];
        first.effects.may_trap = true;
        let mut second = first.clone();
        second.id = SelectedStepId(1);
        second.source = SourceStepId(1);
        second.semantic_inputs = vec![ValueId(1)];
        second.stack_inputs = vec![intermediate];
        second.outputs = vec![result];
        evidence.program.segments[0].units.push(second);
        evidence.program.values.insert(
            ValueId(3),
            SelectedValue {
                id: ValueId(3),
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
            parameters: vec![ValueId(3)],
            units: vec![],
        });
        evidence.program.edges = vec![
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(0),
                source: SegmentId(0),
                destination: Some(SegmentId(1)),
                kind: EdgeKind::Forward,
                bindings: vec![ParameterBinding {
                    destination: ValueId(3),
                    source: ValueId(2),
                }],
            },
            TransferEdge {
                content_bindings: vec![],
                id: EdgeId(1),
                source: SegmentId(1),
                destination: None,
                kind: EdgeKind::Return,
                bindings: vec![ParameterBinding {
                    destination: ValueId(3),
                    source: ValueId(3),
                }],
            },
        ];
        evidence.program.structure = ControlTree::Sequence(vec![
            ControlTree::Segment(SegmentId(0)),
            ControlTree::Edge(EdgeId(0)),
            ControlTree::Segment(SegmentId(1)),
            ControlTree::Edge(EdgeId(1)),
        ]);
        evidence.program.source_map = BTreeMap::from([
            (
                SourceStepId(0),
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: 0,
                    },
                    location: SourceLocation::default(),
                },
            ),
            (
                SourceStepId(1),
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: 1,
                    },
                    location: SourceLocation::default(),
                },
            ),
        ]);
        evidence.units.insert(
            SelectedStepId(1),
            UnitEvidence {
                aliases: LaneAliases::new(vec![(result, intermediate)]),
                ..Default::default()
            },
        );
        Ok(evidence)
    }
}

fn rejects(mutate: impl FnOnce(&mut SelectedProgram)) {
    let mut candidate = Stages.instantiate(&[]).unwrap().program;
    check_selection(candidate.clone(), &SourceContext::Generated(&Stages)).unwrap();
    mutate(&mut candidate);
    assert_eq!(
        check_selection(candidate, &SourceContext::Generated(&Stages))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn omitted_effectful_unit() {
    rejects(|program| {
        program.segments[0].units.remove(0);
    });
}

#[test]
fn duplicate_phase() {
    rejects(|program| {
        let unit = program.segments[0].units[0].clone();
        program.segments[0].units.push(unit);
    });
}

#[test]
fn reordered_phase() {
    rejects(|program| program.segments[0].units.swap(0, 1));
}

#[test]
fn wrong_operand() {
    rejects(|program| program.segments[0].units[1].stack_inputs[0] = Atom::new(ValueId(0), 0));
}

#[test]
fn omitted_edge() {
    rejects(|program| program.edges.clear());
}

#[test]
fn omitted_capture_binding() {
    rejects(|program| program.edges[0].bindings.clear());
}

#[test]
fn forged_generated_stage_annotation() {
    rejects(|program| {
        program.source_map.get_mut(&SourceStepId(1)).unwrap().origin = SourceOrigin::Generated {
            definition: GeneratedDefinitionId(2),
            stage: 0,
        }
    });
}

#[test]
fn zero_instruction_alias_still_requires_checked_coverage() {
    rejects(|program| {
        program.source_map.remove(&SourceStepId(1));
    });
}

#[test]
fn unregistered_hir_source_fails_closed() {
    let candidate = Identity.instantiate(&[]).unwrap().program;
    let function = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @source() { builtin.ret; };",
    );
    let function = function.borrow();
    let env = crate::select::SelectEnv::default();
    assert!(
        check_selection(
            candidate,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
}

#[test]
fn source_parameters_are_validated_by_registered_definition() {
    rejects(|program| {
        program.origin = ProcedureOrigin::Generated {
            definition: GeneratedDefinitionId(2),
            parameters: vec![Felt::ONE],
        }
    });
}

#[test]
fn address_result_declarations_require_owned_scalar_outputs_and_spanning_ranges() {
    struct Addresses(u8);
    impl GeneratedDefinition for Addresses {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(109)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Identity.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            let unit = &mut evidence.program.segments[0].units[0];
            unit.scratch = vec![ScratchRequirement {
                id: ScratchId(0),
                size: 4,
                alignment: 4,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Spanning,
                uses: vec![unit.id],
            }];
            let mut fact = AddressFact {
                pointer: ValueId(1),
                object: ScratchId(0),
                offset: 1,
                accessible: 3,
            };
            match self.0 {
                0 => {}
                1 => fact.pointer = ValueId(0),
                2 => fact.object = ScratchId(9),
                3 => fact.offset = u32::MAX,
                4 => fact.accessible = 4,
                5 => unit.scratch[0].lifetime = ScratchLifetime::Unit(unit.id),
                6 => fact.pointer = ValueId(99),
                7 => unit.address_results.push(fact),
                8 => {
                    unit.outputs.push(Atom::new(ValueId(1), 1));
                    evidence.program.values.get_mut(&ValueId(1)).unwrap().layout =
                        ValueLayout::new(Type::U64).unwrap();
                }
                _ => unreachable!(),
            }
            unit.address_results.push(fact);
            Ok(evidence)
        }
    }
    for variant in 0..9 {
        let definition = Addresses(variant);
        let result = check_selection(
            definition.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&definition),
        );
        assert_eq!(result.is_ok(), variant == 0, "variant {variant}");
    }
}

#[test]
fn scratch_ownership_and_effect_domains_are_checked_before_replay() {
    let mut wrong_owner = Fixture::new(0, 0).scratch(1);
    wrong_owner.scratch[0].lifetime = ScratchLifetime::Unit(SelectedStepId(1));
    assert_eq!(
        check_selection(
            wrong_owner.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&wrong_owner)
        )
        .unwrap_err()
        .category,
        DiagnosticCategory::InvalidSelectedLowering
    );
    let mut undeclared = Fixture::new(0, 0);
    undeclared.scratch_effects.push(ScratchEffect::Read(ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content: ContentRequirement::Initialized,
    }));
    assert_eq!(
        check_selection(
            undeclared.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&undeclared)
        )
        .unwrap_err()
        .category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

#[test]
fn canonical_parameter_and_exact_atom_ownership_is_checked() {
    struct Invalid(u8);
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(101)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Identity.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            match self.0 {
                0 => {
                    evidence.program.values.get_mut(&ValueId(0)).unwrap().origin =
                        ValueOrigin::Parameter {
                            segment: SegmentId(9),
                            index: 0,
                        }
                }
                1 => {
                    evidence.program.values.get_mut(&ValueId(0)).unwrap().origin =
                        ValueOrigin::Parameter {
                            segment: SegmentId(0),
                            index: 1,
                        }
                }
                2 => evidence.program.segments[0].parameters.push(ValueId(0)),
                3 => evidence.program.segments[0].units[0].outputs.push(Atom::new(ValueId(1), 0)),
                4 => evidence.program.segments[0].units[0].outputs.push(Atom::new(ValueId(0), 0)),
                _ => unreachable!(),
            }
            Ok(evidence)
        }
    }
    for variant in 0..5 {
        let definition = Invalid(variant);
        let program = definition.instantiate(&[]).unwrap().program;
        assert!(
            check_selection(program, &SourceContext::Generated(&definition)).is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn canonical_control_identities_are_unique_across_nested_shapes() {
    struct Invalid;
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(102)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = super::loops::LoopDefinition.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            let ControlTree::Sequence(children) = &mut evidence.program.structure else {
                unreachable!()
            };
            let ControlTree::Loop { id, condition, .. } = &mut children[2] else {
                unreachable!()
            };
            *condition = *id;
            Ok(evidence)
        }
    }
    let program = Invalid.instantiate(&[]).unwrap().program;
    assert!(check_selection(program, &SourceContext::Generated(&Invalid)).is_err());
}

#[test]
fn canonical_unit_publication_requires_every_lane_exactly_once() {
    struct Invalid(u8);
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(103)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Identity.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.units.get_mut(&SelectedStepId(0)).unwrap().aliases = LaneAliases::default();
            evidence.program.values.get_mut(&ValueId(1)).unwrap().layout =
                ValueLayout::new(Type::U64).unwrap();
            evidence.program.segments[0].units[0].outputs = match self.0 {
                0 => vec![Atom::new(ValueId(1), 0)],
                1 => vec![Atom::new(ValueId(1), 1)],
                2 => vec![Atom::new(ValueId(1), 0), Atom::new(ValueId(1), 0)],
                3 => vec![Atom::new(ValueId(1), 0), Atom::new(ValueId(1), 2)],
                _ => unreachable!(),
            };
            Ok(evidence)
        }
    }
    for variant in 0..4 {
        let definition = Invalid(variant);
        let program = definition.instantiate(&[]).unwrap().program;
        assert!(
            check_selection(program, &SourceContext::Generated(&definition)).is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn canonical_binding_arity_layout_and_ownership_are_checked() {
    struct Invalid(u8);
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(104)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Stages.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            match self.0 {
                0 => evidence.program.edges[0].bindings.clear(),
                1 => {
                    evidence.program.values.get_mut(&ValueId(3)).unwrap().layout =
                        ValueLayout::new(Type::U64).unwrap()
                }
                2 => evidence.program.edges[0].bindings[0].source = ValueId(999),
                3 => evidence.program.edges[0].bindings[0].destination = ValueId(0),
                4 => {
                    let binding = evidence.program.edges[0].bindings[0];
                    evidence.program.edges[0].bindings.push(binding);
                }
                5 => evidence.program.edges[0].source = SegmentId(1),
                _ => unreachable!(),
            }
            Ok(evidence)
        }
    }
    for variant in 0..6 {
        let definition = Invalid(variant);
        assert!(
            check_selection(
                definition.instantiate(&[]).unwrap().program,
                &SourceContext::Generated(&definition)
            )
            .is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn canonical_terminal_edges_cannot_have_normal_destinations() {
    struct Invalid(EdgeKind);
    impl GeneratedDefinition for Invalid {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(105)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Stages.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.edges[0].kind = self.0;
            Ok(evidence)
        }
    }
    for kind in [EdgeKind::Return, EdgeKind::Trap] {
        let definition = Invalid(kind);
        assert!(
            check_selection(
                definition.instantiate(&[]).unwrap().program,
                &SourceContext::Generated(&definition)
            )
            .is_err(),
            "{kind:?}"
        );
    }
}

/// Mutate the registered definition itself, so candidate/source equality is no defense.
struct AlternativeLabels {
    looped: bool,
    kinds: [EdgeKind; 2],
}

impl GeneratedDefinition for AlternativeLabels {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(40)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = if self.looped {
            super::loops::LoopDefinition.instantiate(&[])?
        } else {
            super::diamond::Diamond.instantiate(&[])?
        };
        evidence.program.origin = ProcedureOrigin::Generated {
            definition: self.id(),
            parameters: vec![],
        };
        let edges = if self.looped { [1, 3] } else { [0, 1] };
        for (index, kind) in edges.into_iter().zip(self.kinds) {
            evidence.program.edges[index].kind = kind;
        }
        Ok(evidence)
    }
}

#[test]
fn canonical_structured_alternatives_reject_swapped_and_duplicate_labels() {
    for looped in [false, true] {
        for kinds in [
            [EdgeKind::False, EdgeKind::True],
            [EdgeKind::True, EdgeKind::True],
            [EdgeKind::False, EdgeKind::False],
        ] {
            let definition = AlternativeLabels { looped, kinds };
            assert!(
                check_selection(
                    definition.instantiate(&[]).unwrap().program,
                    &SourceContext::Generated(&definition)
                )
                .is_err(),
                "loop={looped}, kinds={kinds:?}"
            );
        }
    }
}

#[test]
fn literal_true_swapped_label_cannot_hide_the_invalid_taken_repair() {
    struct TakenRepair;
    impl GeneratedDefinition for TakenRepair {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(50)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Fixture::new(0, 1).instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.abi = ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap();
            evidence.program.segments.extend((1..=2).map(|index| SelectedSegment {
                id: SegmentId(index),
                parameters: vec![],
                content_parameters: vec![],
                units: vec![],
            }));
            evidence.program.edges = (0..4)
                .map(|index| TransferEdge {
                    id: EdgeId(index),
                    source: SegmentId(if index < 2 { 0 } else { index - 1 }),
                    destination: if index < 2 {
                        Some(SegmentId(index + 1))
                    } else {
                        None
                    },
                    kind: [EdgeKind::False, EdgeKind::True, EdgeKind::Return, EdgeKind::Return]
                        [index as usize],
                    bindings: vec![],
                    content_bindings: vec![],
                })
                .collect();
            evidence.program.structure = ControlTree::Sequence(vec![
                ControlTree::Segment(SegmentId(0)),
                ControlTree::If {
                    id: ControlId(0),
                    predicate: Fixture::output(0),
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
            ]);
            Ok(evidence)
        }
    }
    // The real then arm consumes the predicate before Drop, leaving only the protected tail.
    let mut body = body(vec![Primitive::Push(Felt::ONE).instruction().unwrap()]);
    body = ops(body.ops()[..body.ops().len() - 1].to_vec());
    body.push(AnnotatedOp::If {
        id: ControlId(0),
        then_body: ops(vec![
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))),
            inst(Instruction::Drop),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(2))),
        ]),
        else_body: ops(vec![
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(1))),
            AnnotatedOp::Boundary(Boundary::Segment(SegmentId(2))),
            AnnotatedOp::Boundary(Boundary::Edge(EdgeId(3))),
        ]),
    });
    let result = check_selection(
        TakenRepair.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&TakenRepair),
    )
    .and_then(|selected| {
        let mut plan = Fixture::new(0, 0).plan();
        plan.selected = selected;
        plan.edge_actions.insert(EdgeId(0), vec![PlanAction::Drop]);
        for segment in [SegmentId(1), SegmentId(2)] {
            plan.contracts.insert(segment, plan.contracts[&SegmentId(0)].clone());
            plan.segment_actions.insert(segment, vec![]);
        }
        verify(&plan, &frame(), body)
    });
    let error = result.unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSelectedLowering);
    assert_eq!(error.message, "structured alternative disagrees with edge kind");
}

#[test]
fn equality_assertions_check_actual_inputs_floor_and_trap_permission() {
    for with_message in [false, true] {
        let assertion = if with_message {
            Instruction::AssertEqWithError(alloc::sync::Arc::<str>::from("table tag").into())
        } else {
            Instruction::AssertEq
        };
        for (left, right) in [(7, 7), (7, 8)] {
            let mut fixture = Fixture::new(0, 0);
            fixture.effects.may_trap = true;
            let code = vec![
                Primitive::Push(Felt::new_unchecked(left)).instruction().unwrap(),
                Primitive::Push(Felt::new_unchecked(right)).instruction().unwrap(),
                assertion.clone(),
            ];
            verify(&fixture.plan(), &fixture.frame(), body(code.clone())).unwrap();
            fixture.effects.may_trap = false;
            assert!(verify(&fixture.plan(), &fixture.frame(), body(code)).is_err());
        }
        let mut fixture = Fixture::new(2, 0);
        fixture.effects.may_trap = true;
        verify(&fixture.plan(), &fixture.frame(), body(vec![assertion.clone()])).unwrap();
        let mut fixture = Fixture::new(1, 0);
        fixture.effects.may_trap = true;
        assert!(
            verify(&fixture.plan(), &fixture.frame(), body(vec![assertion])).is_err(),
            "cannot consume protected tail"
        );
    }
}

#[test]
fn registered_loop_header_must_be_the_before_subtree_entry_even_without_backedges() {
    struct BadHeader;
    impl GeneratedDefinition for BadHeader {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(201)
        }

        fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = super::loops::LoopDefinition.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            let edge = &mut evidence.program.edges[2];
            edge.kind = EdgeKind::Trap;
            edge.destination = None;
            edge.bindings.clear();
            let ControlTree::Sequence(children) = &mut evidence.program.structure else {
                unreachable!()
            };
            let ControlTree::Loop {
                header, backedges, ..
            } = &mut children[2]
            else {
                unreachable!()
            };
            *header = SegmentId(99);
            backedges.clear();
            Ok(evidence)
        }
    }
    let source = BadHeader;
    assert!(
        check_selection(
            source.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&source)
        )
        .is_err()
    );
}

#[test]
fn arithmetic_traps_require_declared_permission() {
    use Instruction::*;
    for (instruction, inputs) in [
        (Div, vec![2, 14]),
        (Inv, vec![1]),
        (Not, vec![1]),
        (And, vec![1, 0]),
        (Or, vec![1, 0]),
        (Xor, vec![1, 0]),
        (Pow2, vec![4]),
        (Exp, vec![3, 5]),
        (ILog2, vec![31]),
        (U32Assert, vec![31]),
        (U32Assert2, vec![31, 32]),
        (U32AssertW, vec![31, 32, 33, 34]),
        (U32WrappingAdd, vec![2, 3]),
        (U32OverflowingSub, vec![2, 3]),
        (U32WideningMul, vec![2, 3]),
        (U32Div, vec![2, 3]),
        (U32Not, vec![2]),
        (U32And, vec![2, 3]),
        (U32Or, vec![2, 3]),
        (U32Shl, vec![2, 3]),
        (U32Rotr, vec![2, 3]),
        (U32Popcnt, vec![2]),
        (U32Clz, vec![2]),
        (U32Clo, vec![2]),
        (U32Ctz, vec![2]),
        (U32Cto, vec![2]),
        (Ext2Div, vec![1, 0, 2, 3]),
        (Ext2Inv, vec![1, 0]),
        (AssertEqw, vec![2, 3, 5, 7, 2, 3, 5, 7]),
        (CDrop, vec![1, 2, 3]),
        (CDropW, vec![0, 2, 3, 5, 7, 11, 13, 17, 19]),
    ] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert!(effect.may_trap, "{instruction}");
        let mut code = inputs.into_iter().rev().map(push).collect::<Vec<_>>();
        code.push(inst(instruction.clone()));
        code.extend((0..effect.outputs.len()).map(|_| inst(Drop)));
        let mut fixture = Fixture::new(0, 0);
        fixture.effects.may_trap = true;
        fixture.run(code.clone()).unwrap();
        fixture.effects.may_trap = false;
        assert_eq!(
            fixture.run(code).unwrap_err().message,
            "undeclared trapping instruction",
            "{instruction}"
        );
    }
}

#[test]
fn conditional_drop_preserves_known_and_equal_unknown_candidate_identities() {
    for predicate in [0, 1] {
        let mut fixture = Fixture::new(2, 1);
        fixture.effects.may_trap = true;
        fixture.proof.aliases = LaneAliases::new(vec![(
            Fixture::output(0),
            Fixture::input(if predicate == 1 { 0 } else { 1 }),
        )]);
        fixture.run(vec![push(predicate), inst(Instruction::CDrop)]).unwrap();
        fixture.proof.aliases = LaneAliases::new(vec![(
            Fixture::output(0),
            Fixture::input(if predicate == 1 { 1 } else { 0 }),
        )]);
        assert!(fixture.run(vec![push(predicate), inst(Instruction::CDrop)]).is_err());
    }
    // Preserve equality when either arm chooses the very same input token.
    let mut fixture = Fixture::new(2, 1);
    fixture.effects.may_trap = true;
    fixture.proof.aliases = LaneAliases::new(vec![(Fixture::output(0), Fixture::input(1))]);
    fixture
        .run(vec![
            inst(Instruction::Dup1),
            inst(Instruction::MovDn2),
            inst(Instruction::CDrop),
        ])
        .unwrap();
}

#[test]
fn conditional_drop_unknown_candidates_retain_compiler_origins() {
    let mut fixture = Fixture::new(1, 1).scratch(2);
    fixture.effects.may_trap = true;
    fixture.effects.memory = SourceMemoryEffects::READ;
    for candidates in [
        vec![inst(Instruction::Locaddr(0.into())), push(77)],
        vec![push(77), inst(Instruction::Locaddr(0.into()))],
    ] {
        let mut code = candidates;
        code.extend([
            inst(Instruction::MovUp2),
            inst(Instruction::CDrop),
            inst(Instruction::MemLoad),
        ]);
        assert_eq!(
            fixture.run(code).unwrap_err().category,
            DiagnosticCategory::InvalidSpillSlotRead
        );
    }
}

#[test]
fn arithmetic_target_rules_never_consume_padding_as_owned_input() {
    for instruction in [
        Instruction::U32AssertW,
        Instruction::Eqw,
        Instruction::CDropW,
        Instruction::Ext2Mul,
    ] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        let mut fixture = Fixture::new(u32::from(effect.consumed) - 1, effect.outputs.len() as u32);
        fixture.effects.may_trap = true;
        assert_eq!(
            fixture.run(vec![inst(instruction)]).unwrap_err().category,
            DiagnosticCategory::IllegalStackAccess
        );
    }
}

#[test]
fn nonboolean_conditional_predicate_has_no_normal_continuation() {
    let mut fixture = Fixture::new(0, 0);
    fixture.effects.may_trap = true;
    // The invalid Drop cannot be reached after the known nonboolean predicate traps.
    fixture
        .run(vec![
            push(7),
            push(9),
            push(2),
            inst(Instruction::CDrop),
            inst(Instruction::DropW),
        ])
        .unwrap();
    fixture.effects.may_trap = false;
    assert_eq!(
        fixture
            .run(vec![push(7), push(9), push(2), inst(Instruction::CDrop)])
            .unwrap_err()
            .message,
        "undeclared trapping instruction"
    );
}
