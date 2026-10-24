use super::*;

#[derive(Clone, Copy)]
enum ProducerKind {
    Store,
    PossibleWrite,
    Borrow {
        definite: bool,
        output_relation: bool,
        conditional: bool,
    },
}
struct ProducerConsumer {
    producer: ProducerKind,
    read_before_write: bool,
    wrong_consumer: bool,
}
impl ProducerConsumer {
    fn reference() -> FrameRef {
        FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        }
    }

    fn fact(content: ContentRequirement) -> ScratchCellFact {
        ScratchCellFact {
            object: ScratchId(0),
            offset: 0,
            content,
        }
    }

    fn target() -> crate::masm::InvocationTarget {
        crate::masm::InvocationTarget::Symbol(crate::masm::Ident::new("scratch_producer").unwrap())
    }
}
impl GeneratedDefinition for ProducerConsumer {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(53)
    }

    fn instantiate(&self, _: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut evidence = Fixture::new(u32::from(self.wrong_consumer), 1).instantiate(&[])?;
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
            uses: vec![SelectedStepId(0), SelectedStepId(1)],
        };
        let producer = &mut evidence.program.segments[0].units[0];
        producer.semantic_inputs.clear();
        producer.stack_inputs.clear();
        producer.scratch = vec![requirement.clone()];
        producer.scratch_effects = vec![match self.producer {
            ProducerKind::Store => {
                ScratchEffect::Write(Self::fact(ContentRequirement::Content(ContentId(0))))
            }
            ProducerKind::PossibleWrite => ScratchEffect::PossibleWrite {
                object: ScratchId(0),
                offset: 0,
                size: 1,
            },
            ProducerKind::Borrow { definite, .. } => ScratchEffect::Borrow {
                object: ScratchId(0),
                offset: 0,
                size: 1,
                read: false,
                write: true,
                definite_write: definite,
                retained: false,
            },
        }];
        if self.read_before_write {
            producer
                .scratch_effects
                .push(ScratchEffect::Read(Self::fact(ContentRequirement::Initialized)));
        }
        let mut consumer = producer.clone();
        consumer.id = SelectedStepId(1);
        consumer.source = SourceStepId(1);
        consumer.outputs.clear();
        consumer.effects = LoweringEffects::default();
        consumer.scratch_effects = vec![ScratchEffect::Read(Self::fact(ContentRequirement::Atom(
            if self.wrong_consumer {
                Fixture::input(0)
            } else {
                Fixture::output(0)
            },
        )))];
        if let ProducerKind::Borrow {
            output_relation,
            conditional,
            ..
        } = self.producer
        {
            let abi = ProcedureAbi::new(CallingConvention::SameContext, 1, 1).unwrap();
            evidence.units.get_mut(&producer.id).unwrap().calls.push(CallSite {
                instruction: Instruction::Exec(Self::target()),
                abi,
                inputs: vec![CallArgument::Local],
                path: if conditional {
                    vec![CallPath::Then(ControlId(0))]
                } else {
                    vec![]
                },
                truncate: false,
                outputs: if output_relation {
                    vec![(Self::reference(), 0)]
                } else {
                    vec![]
                },
                publications: vec![(Fixture::output(0), 0)],
                selector: None,
            });
        }
        if !matches!(
            self.producer,
            ProducerKind::PossibleWrite
                | ProducerKind::Borrow {
                    conditional: true,
                    ..
                }
        ) {
            evidence.units.get_mut(&producer.id).unwrap().scratch_outputs =
                vec![(Self::reference(), 0)];
        }
        evidence.program.segments[0].units.push(consumer);
        evidence.units.insert(SelectedStepId(1), UnitEvidence::default());
        evidence.program.source_map.insert(
            SourceStepId(1),
            SourceRecord {
                origin: SourceOrigin::Generated {
                    definition: self.id(),
                    stage: 1,
                },
                location: SourceLocation::default(),
            },
        );
        Ok(evidence)
    }
}

/// Corruptions preserve the surrounding selected phases and executable stack shape.
#[derive(Clone, Copy)]
enum Corruption {
    None,
    Output,
    Cell,
    SkipCall,
}
fn run(
    definition: &ProducerConsumer,
    corruption: Corruption,
) -> Result<VerifiedProcedure, VerificationError> {
    let mut plan = Fixture::new(u32::from(definition.wrong_consumer), 1).plan();
    plan.selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(definition),
    )?;
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
    )?;
    for step in 0..2 {
        plan.lowering_bindings.insert(
            SelectedStepId(step),
            LoweringBindings {
                input_order: crate::plan::InputOrder::Canonical,
                stored_inputs: BTreeMap::new(),
                scratch: BTreeMap::from([(ScratchId(0), allocation)]),
            },
        );
    }
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
            PlanAction::RunLowering {
                step: SelectedStepId(1),
            },
        ],
    );
    let mut code = vec![
        AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))),
        AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))),
    ];
    if definition.read_before_write {
        code.extend([inst(Instruction::LocLoad(0.into())), inst(Instruction::Drop)]);
    }
    match definition.producer {
        ProducerKind::Store => {
            code.extend([push(17), inst(Instruction::Dup0), inst(Instruction::LocStore(0.into()))])
        }
        ProducerKind::PossibleWrite => code.push(push(17)),
        ProducerKind::Borrow { conditional, .. } => {
            let call = vec![
                inst(Instruction::Locaddr(0.into())),
                inst(Instruction::Exec(ProducerConsumer::target())),
            ];
            if conditional {
                code.push(push(u64::from(!matches!(corruption, Corruption::SkipCall))));
                code.push(AnnotatedOp::If {
                    id: ControlId(0),
                    then_body: ops(call),
                    else_body: ops(vec![push(17)]),
                });
            } else {
                code.extend(call);
            }
        }
    }
    match corruption {
        Corruption::None | Corruption::SkipCall => {}
        Corruption::Output => code.extend([inst(Instruction::Drop), push(19)]),
        Corruption::Cell => code.extend([push(19), inst(Instruction::LocStore(0.into()))]),
    }
    code.extend([
        AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))),
        AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(1))),
    ]);
    code.push(AnnotatedOp::Instruction {
        instruction: Instruction::LocLoad(0.into()),
        unit: SelectedStepId(1),
    });
    code.push(AnnotatedOp::Instruction {
        instruction: Instruction::Drop,
        unit: SelectedStepId(1),
    });
    code.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(1))));
    if definition.wrong_consumer {
        plan.segment_actions
            .get_mut(&SegmentId(0))
            .unwrap()
            .extend([PlanAction::Swap { depth: 1 }, PlanAction::Drop]);
        code.extend([inst(Instruction::Swap1), inst(Instruction::Drop)]);
    }
    code.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    verify(&plan, &frame, ops(code))
}

#[test]
fn definite_write_checks_outgoing_atom_identity_without_requiring_it_on_entry() {
    let definition = ProducerConsumer {
        producer: ProducerKind::Store,
        read_before_write: false,
        wrong_consumer: false,
    };
    run(&definition, Corruption::None).unwrap();
}

#[test]
fn definite_write_still_checks_output_cell_and_consumer_identities() {
    let definition = ProducerConsumer {
        producer: ProducerKind::Store,
        read_before_write: false,
        wrong_consumer: false,
    };
    for corruption in [Corruption::Output, Corruption::Cell] {
        let error = run(&definition, corruption).unwrap_err();
        assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
        assert_eq!(error.message, "scratch/output equality was not established");
    }
    let wrong = ProducerConsumer {
        wrong_consumer: true,
        ..definition
    };
    let error = run(&wrong, Corruption::None).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert_eq!(error.message, "physical scratch content differs from required identity");
}

#[test]
fn explicit_read_before_definite_write_still_requires_incoming_initialization() {
    let definition = ProducerConsumer {
        producer: ProducerKind::Store,
        read_before_write: true,
        wrong_consumer: false,
    };
    let error = run(&definition, Corruption::None).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert_eq!(error.message, "physical cell is not definitely initialized");
}

#[test]
fn definite_borrowed_output_establishes_the_following_atom_demand() {
    let definition = ProducerConsumer {
        producer: ProducerKind::Borrow {
            definite: true,
            output_relation: true,
            conditional: false,
        },
        read_before_write: false,
        wrong_consumer: false,
    };
    run(&definition, Corruption::None).unwrap();
}

#[test]
fn possible_writes_do_not_initialize_or_supply_unproved_borrowed_output_identity() {
    for producer in [
        ProducerKind::PossibleWrite,
        ProducerKind::Borrow {
            definite: false,
            output_relation: false,
            conditional: false,
        },
        ProducerKind::Borrow {
            definite: true,
            output_relation: false,
            conditional: false,
        },
    ] {
        let definition = ProducerConsumer {
            producer,
            read_before_write: false,
            wrong_consumer: false,
        };
        let error = run(&definition, Corruption::None).unwrap_err();
        assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
        let expected = if matches!(producer, ProducerKind::Borrow { definite: true, .. }) {
            "scratch/output equality was not established"
        } else {
            "physical cell is not definitely initialized"
        };
        assert_eq!(error.message, expected);
    }
}

#[test]
fn conditional_definite_borrow_does_not_initialize_the_no_call_path() {
    let definition = ProducerConsumer {
        producer: ProducerKind::Borrow {
            definite: true,
            output_relation: true,
            conditional: true,
        },
        read_before_write: false,
        wrong_consumer: false,
    };
    run(&definition, Corruption::None).unwrap();
    let error = run(&definition, Corruption::SkipCall).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::InvalidSpillSlotRead);
    assert_eq!(error.message, "physical cell is not definitely initialized");
}
