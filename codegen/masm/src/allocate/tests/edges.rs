use alloc::{collections::BTreeMap, vec, vec::Vec};

use midenc_hir::Type;

use crate::{
    allocate::{edges::repair_edge, state::*},
    plan::{actions::*, *},
    verify::*,
};

pub(super) fn atom(id: u32) -> Atom {
    Atom::new(ValueId(id), 0)
}
pub(super) fn values(count: u32) -> BTreeMap<ValueId, SelectedValue> {
    (0..count)
        .map(|id| {
            (
                ValueId(id),
                SelectedValue {
                    id: ValueId(id),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: id,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            )
        })
        .collect()
}
pub(super) fn context(values: &BTreeMap<ValueId, SelectedValue>) -> AllocationContext<'_> {
    use crate::allocate::analysis::NextUses;
    static NO_NEXT_USES: NextUses = NextUses {
        entry: BTreeMap::new(),
        before: BTreeMap::new(),
        after: BTreeMap::new(),
        edge: BTreeMap::new(),
    };
    // Direct physical-state fixtures do not claim any graph-derived reconstruction eligibility.
    AllocationContext {
        input_symmetries: BTreeMap::new(),
        values,
        next_use: &NO_NEXT_USES,
        options: Default::default(),
        entry_reconstructions: BTreeMap::from([
            (SegmentId(0), Default::default()),
            (SegmentId(1), Default::default()),
        ]),
    }
}
pub(super) fn contract(stack: Vec<Atom>, slots: Vec<(SpillSlotId, Atom)>) -> EntryContract {
    EntryContract {
        stack_top_first: stack,
        required_spill_slots: slots,
        required_scratch: vec![],
        address_facts: vec![],
        aliases: LaneAliases::default(),
        tail: TailPolicy::PreserveSuffix,
    }
}
pub(super) fn state(stack: Vec<Atom>, slots: Vec<(SpillSlotId, Atom)>) -> AllocationState {
    AllocationState::from_contract(&contract(stack, slots)).unwrap()
}
pub(super) fn unit(stack: Vec<Atom>, stored: Vec<StoredInput>) -> SelectedLowering {
    SelectedLowering {
        publication: OutputPublication::Stack,
        address_results: vec![],
        id: SelectedStepId(0),
        kind: LoweringKind(0),
        source: SourceStepId(0),
        semantic_inputs: stack.iter().map(|a| a.value).collect(),
        stack_inputs: stack,
        stored_inputs: stored,
        outputs: vec![],
        effects: LoweringEffects::default(),
        scratch: vec![],
        scratch_effects: vec![],
    }
}

#[test]
fn cyclic_slots_and_stack_outputs_read_simultaneous_old_sources() {
    let values = values(4);
    let a = SpillSlotId::canonical(atom(0));
    let b = SpillSlotId::canonical(atom(1));
    let initial = state(vec![], vec![(a, atom(0)), (b, atom(1))]);
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![
            ParameterBinding {
                destination: ValueId(2),
                source: ValueId(1),
            },
            ParameterBinding {
                destination: ValueId(3),
                source: ValueId(0),
            },
        ],
        content_bindings: vec![],
    };
    let target = contract(vec![atom(2), atom(3), atom(2)], vec![(a, atom(2)), (b, atom(3))]);
    let proposal = repair_edge(&initial, &edge, &target, &context(&values)).unwrap();
    assert_eq!(
        proposal.state.stack_atoms().collect::<Vec<_>>(),
        vec![atom(2), atom(3), atom(2)]
    );
    assert_eq!(
        proposal.state.slots,
        BTreeMap::from([(a, StoredValue::Atom(atom(2))), (b, StoredValue::Atom(atom(3)))])
    );
    assert!(proposal.actions.len() < 16);
}

#[test]
fn satisfied_edge_preserves_occurrences_and_cached_slots_without_actions() {
    let values = values(20);
    let initial =
        state((0..20).map(atom).collect(), vec![(SpillSlotId::canonical(atom(0)), atom(0))]);
    let target =
        contract((0..20).map(atom).collect(), vec![(SpillSlotId::canonical(atom(0)), atom(0))]);
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![],
        content_bindings: vec![],
    };
    let result = repair_edge(&initial, &edge, &target, &context(&values)).unwrap();
    assert!(result.actions.is_empty());
    assert_eq!(result.state.stack, initial.stack);
}

#[test]
fn incremental_and_normalization_transfers_observe_the_same_old_sources_and_tail() {
    fn replay(
        stack: &mut Vec<Atom>,
        slots: &mut BTreeMap<SpillSlotId, Atom>,
        actions: &[PlanAction],
    ) {
        for action in actions {
            match *action {
                PlanAction::Dup { depth } => {
                    assert!(depth < 16);
                    stack.insert(0, stack[usize::from(depth)]);
                }
                PlanAction::Swap { depth } => {
                    assert!(depth < 16);
                    stack.swap(0, usize::from(depth));
                }
                PlanAction::SwapWord { word } => {
                    assert!((1..=3).contains(&word));
                    for lane in 0..4 {
                        stack.swap(lane, usize::from(word) * 4 + lane);
                    }
                }
                PlanAction::MoveUp { depth } => {
                    assert!(depth < 16);
                    let atom = stack.remove(usize::from(depth));
                    stack.insert(0, atom);
                }
                PlanAction::MoveDown { depth } => {
                    assert!(depth < 16);
                    let atom = stack.remove(0);
                    stack.insert(usize::from(depth), atom);
                }
                PlanAction::Drop => {
                    stack.remove(0);
                }
                PlanAction::StoreLane(slot) => {
                    slots.insert(slot, stack.remove(0));
                }
                PlanAction::LoadLane(slot) => stack.insert(0, slots[&slot]),
                _ => panic!("raw parameter fixture"),
            }
        }
    }
    let values = values(50);
    let context = context(&values);
    let a_slot = SpillSlotId::canonical(atom(0));
    let b_slot = SpillSlotId::canonical(atom(1));
    let edge = TransferEdge {
        id: EdgeId(7),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![
            ParameterBinding {
                destination: ValueId(40),
                source: ValueId(1),
            },
            ParameterBinding {
                destination: ValueId(41),
                source: ValueId(0),
            },
        ],
        content_bindings: vec![],
    };
    for depth in 0..23 {
        let initial =
            state((2..depth + 2).map(atom).collect(), vec![(a_slot, atom(0)), (b_slot, atom(1))]);
        let target = contract(
            vec![atom(40), atom(41), atom(40)],
            vec![(a_slot, atom(40)), (b_slot, atom(41))],
        );
        let incremental = repair_edge(&initial, &edge, &target, &context).unwrap();
        let fallback =
            crate::allocate::edges::normalize_edge(&initial, &edge, &target, &context).unwrap();
        let tail: Vec<_> = (1000..1016).map(atom).collect();
        for preparation in [&incremental, &fallback] {
            let mut actual: Vec<_> = initial.stack_atoms().chain(tail.iter().copied()).collect();
            let mut slots = BTreeMap::from([(a_slot, atom(0)), (b_slot, atom(1))]);
            replay(&mut actual, &mut slots, &preparation.actions);
            assert_eq!(
                actual,
                vec![atom(1), atom(0), atom(1)]
                    .into_iter()
                    .chain(tail.iter().copied())
                    .collect::<Vec<_>>()
            );
            assert_eq!(slots[&a_slot], atom(1));
            assert_eq!(slots[&b_slot], atom(0));
            assert_eq!(preparation.state.stack_atoms().collect::<Vec<_>>(), target.stack_top_first);
        }
    }
}

struct SwapDefinition;
impl GeneratedDefinition for SwapDefinition {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(402)
    }

    fn instantiate(&self, _: &[miden_core::Felt]) -> Result<DefinitionEvidence, VerificationError> {
        let mut values = values(4);
        for (id, index) in [(2, 0), (3, 1)] {
            values.get_mut(&ValueId(id)).unwrap().origin = ValueOrigin::Parameter {
                segment: SegmentId(1),
                index,
            };
        }
        let mut consumer = unit(
            vec![atom(2), atom(3), atom(2)],
            vec![
                StoredInput {
                    id: StoredInputId(0),
                    value: StoredValue::Atom(atom(2)),
                },
                StoredInput {
                    id: StoredInputId(1),
                    value: StoredValue::Atom(atom(3)),
                },
            ],
        );
        consumer.semantic_inputs = vec![ValueId(2), ValueId(3)];
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            units: BTreeMap::from([(SelectedStepId(0), UnitEvidence::default())]),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![
                    SelectedSegment {
                        id: SegmentId(0),
                        parameters: vec![ValueId(0), ValueId(1)],
                        content_parameters: vec![],
                        units: vec![],
                    },
                    SelectedSegment {
                        id: SegmentId(1),
                        parameters: vec![ValueId(2), ValueId(3)],
                        content_parameters: vec![],
                        units: vec![consumer],
                    },
                ],
                edges: vec![
                    TransferEdge {
                        id: EdgeId(0),
                        source: SegmentId(0),
                        destination: Some(SegmentId(1)),
                        kind: EdgeKind::Forward,
                        bindings: vec![
                            ParameterBinding {
                                destination: ValueId(2),
                                source: ValueId(1),
                            },
                            ParameterBinding {
                                destination: ValueId(3),
                                source: ValueId(0),
                            },
                        ],
                        content_bindings: vec![],
                    },
                    TransferEdge {
                        id: EdgeId(1),
                        source: SegmentId(1),
                        destination: None,
                        kind: EdgeKind::Return,
                        bindings: vec![],
                        content_bindings: vec![],
                    },
                ],
                source_map: BTreeMap::from([(
                    SourceStepId(0),
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: 0,
                        },
                        location: SourceLocation::default(),
                    },
                )]),
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                    ControlTree::Segment(SegmentId(1)),
                    ControlTree::Edge(EdgeId(1)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 2, 0).unwrap(),
            },
        })
    }
}

#[test]
fn simultaneous_slot_cycle_with_stack_readers_verifies_assembles_and_rejects_overwrite() {
    use super::normalization::{fixture_body, fixture_frame, verify_assemble_execute};
    use crate::lower::units::LoweringBindings;
    let selected = check_selection(
        SwapDefinition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&SwapDefinition),
    )
    .unwrap();
    let a = SpillSlotId::canonical(atom(0));
    let b = SpillSlotId::canonical(atom(1));
    let initial = state(vec![], vec![(a, atom(0)), (b, atom(1))]);
    let target = contract(vec![atom(2), atom(3), atom(2)], vec![(a, atom(2)), (b, atom(3))]);
    let proposal =
        repair_edge(&initial, &selected.edges[0], &target, &context(&selected.values)).unwrap();
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        spill_slots: proposal.slot_requests.iter().copied().collect(),
        contracts: BTreeMap::from([
            (SegmentId(0), contract(vec![atom(0), atom(1)], vec![])),
            (SegmentId(1), target),
        ]),
        segment_actions: BTreeMap::from([
            (SegmentId(0), vec![PlanAction::StoreLane(a), PlanAction::StoreLane(b)]),
            (
                SegmentId(1),
                vec![PlanAction::RunLowering {
                    step: SelectedStepId(0),
                }],
            ),
        ]),
        edge_actions: BTreeMap::from([(EdgeId(0), proposal.actions)]),
        semantic_storage_reads: Default::default(),
        lowering_bindings: BTreeMap::from([(
            SelectedStepId(0),
            LoweringBindings {
                input_order: crate::plan::InputOrder::Canonical,
                stored_inputs: BTreeMap::from([(StoredInputId(0), a), (StoredInputId(1), b)]),
                scratch: BTreeMap::new(),
            },
        )]),
        selected,
    };
    let frame = fixture_frame(&plan);
    verify_assemble_execute(&plan, &frame, &fixture_body(&plan, &frame), 2);
    let mut corrupt = plan.clone();
    let actions = corrupt.edge_actions.get_mut(&EdgeId(0)).unwrap();
    let snapshot = actions.iter_mut().find(|action| matches!(action, PlanAction::StoreLane(slot) if slot.owner == SlotOwner::Edge(EdgeId(0)))).unwrap();
    *snapshot = PlanAction::Drop;
    assert!(verify(&corrupt, &frame, fixture_body(&corrupt, &frame)).is_err());
    let physical_collision = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        plan.spill_slots.iter().copied().map(|slot| (slot, 0)).collect(),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    assert!(verify(&plan, &physical_collision, fixture_body(&plan, &physical_collision)).is_err());
}

#[test]
fn canonical_slot_holds_other_needed_atom_on_an_edge() {
    let values = values(4);
    let slot = SpillSlotId::canonical(atom(0));
    let initial = state(vec![atom(0)], vec![(slot, atom(1))]);
    let edge = TransferEdge {
        id: EdgeId(3),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![
            ParameterBinding {
                destination: ValueId(2),
                source: ValueId(0),
            },
            ParameterBinding {
                destination: ValueId(3),
                source: ValueId(1),
            },
        ],
        content_bindings: vec![],
    };
    let target = contract(vec![atom(2), atom(3)], vec![(slot, atom(2))]);
    let proposal = repair_edge(&initial, &edge, &target, &context(&values)).unwrap();
    assert_eq!(proposal.state.stack_atoms().collect::<Vec<_>>(), vec![atom(2), atom(3)]);
    assert!(
        proposal
            .actions
            .iter()
            .filter(|a| matches!(a, PlanAction::StoreLane(_)))
            .count()
            >= 2,
        "old b must be saved before mandatory a write"
    );
}

#[test]
fn failed_edge_preparation_is_atomic_after_an_endangered_source_snapshot() {
    let values = values(3);
    let a = SpillSlotId::canonical(atom(0));
    let initial = state(vec![], vec![(a, atom(0))]);
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![],
        content_bindings: vec![],
    };
    let target = contract(vec![atom(0), atom(2)], vec![(a, atom(1))]);
    let snapshot = initial.clone();
    assert!(
        matches!(repair_edge(&initial, &edge, &target, &context(&values)), Err(AllocationError::Invariant(error)) if error.category == DiagnosticCategory::UnavailableValue)
    );
    assert_eq!(initial, snapshot);
}

#[test]
fn exact_wide_edge_removes_dead_owned_lanes_and_preserves_duplicate_multiplicity() {
    let values = values(30);
    let initial = state((0..30).map(atom).collect(), vec![]);
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![],
        content_bindings: vec![],
    };
    let desired: Vec<_> = (5..30).rev().map(atom).chain([atom(29), atom(29)]).collect();
    let proposal =
        repair_edge(&initial, &edge, &contract(desired.clone(), vec![]), &context(&values))
            .unwrap();
    assert_eq!(proposal.state.stack_atoms().collect::<Vec<_>>(), desired);
    assert!(proposal.state.slots.is_empty(), "retain exactly the contract facts");
    assert!(proposal.actions.len() < 90);
    assert_eq!(proposal.state.tail, TailPolicy::PreserveSuffix);
}

#[test]
fn edge_scratch_addresses_and_aliases_require_simultaneous_old_facts() {
    let values = values(4);
    let mut initial = state(vec![atom(0), atom(1)], vec![]);
    initial.scratch = vec![
        ScratchCellFact {
            object: ScratchId(0),
            offset: 0,
            content: ContentRequirement::Atom(atom(1)),
        },
        ScratchCellFact {
            object: ScratchId(0),
            offset: 1,
            content: ContentRequirement::Content(ContentId(10)),
        },
    ];
    initial.addresses = vec![AddressFact {
        pointer: ValueId(0),
        object: ScratchId(0),
        offset: 0,
        accessible: 2,
    }];
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![
            ParameterBinding {
                destination: ValueId(2),
                source: ValueId(1),
            },
            ParameterBinding {
                destination: ValueId(3),
                source: ValueId(0),
            },
        ],
        content_bindings: vec![ContentBinding {
            destination: ContentId(20),
            source: ContentId(10),
        }],
    };
    let mut target = contract(vec![atom(2), atom(3)], vec![]);
    target.required_scratch = vec![
        ScratchCellFact {
            content: ContentRequirement::Atom(atom(2)),
            ..initial.scratch[0]
        },
        ScratchCellFact {
            content: ContentRequirement::Content(ContentId(20)),
            ..initial.scratch[1]
        },
    ];
    target.address_facts = vec![AddressFact {
        pointer: ValueId(3),
        ..initial.addresses[0]
    }];
    let proposal = repair_edge(&initial, &edge, &target, &context(&values)).unwrap();
    assert_eq!(proposal.state.scratch, target.required_scratch);
    assert_eq!(proposal.state.addresses, target.address_facts);
    target.aliases = LaneAliases::new(vec![(atom(2), atom(3))]);
    assert!(repair_edge(&initial, &edge, &target, &context(&values)).is_err());
    target.aliases = LaneAliases::default();
    target.required_scratch[0].content = ContentRequirement::Atom(atom(3));
    assert!(repair_edge(&initial, &edge, &target, &context(&values)).is_err());
    target.required_scratch.clear();
    target.address_facts[0].accessible = 3;
    assert!(repair_edge(&initial, &edge, &target, &context(&values)).is_err());
}

#[test]
fn edge_rebinding_checks_new_reconstruction_and_keeps_omitted_literal_parameters() {
    use alloc::collections::BTreeSet;

    use miden_core::Felt;
    let mut values = values(2);
    values.get_mut(&ValueId(0)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![Felt::new_unchecked(7)]));
    values.get_mut(&ValueId(1)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![Felt::new_unchecked(7)]));
    let initial = state(vec![atom(0)], vec![]);
    let edge = TransferEdge {
        id: EdgeId(0),
        source: SegmentId(0),
        destination: Some(SegmentId(1)),
        kind: EdgeKind::Forward,
        bindings: vec![ParameterBinding {
            destination: ValueId(1),
            source: ValueId(0),
        }],
        content_bindings: vec![],
    };
    let proposal =
        repair_edge(&initial, &edge, &contract(vec![], vec![]), &context(&values)).unwrap();
    let next = crate::allocate::normalize::prepare_unit(
        &proposal.state,
        &unit(vec![atom(1)], vec![]),
        &BTreeSet::new(),
        &context(&values),
    )
    .unwrap();
    assert_eq!(next.actions, vec![PlanAction::PushLiteral(Felt::new_unchecked(7))]);
    values.get_mut(&ValueId(1)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![Felt::new_unchecked(8)]));
    assert!(
        repair_edge(&initial, &edge, &contract(vec![atom(1)], vec![]), &context(&values)).is_err(),
        "new literal identity cannot rename a different old token"
    );
}

struct ReconstructedEdge {
    alias: bool,
}
impl GeneratedDefinition for ReconstructedEdge {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(403)
    }

    fn instantiate(&self, _: &[miden_core::Felt]) -> Result<DefinitionEvidence, VerificationError> {
        use miden_core::Felt;
        let mut values = values(1 + u32::from(self.alias));
        values.get_mut(&ValueId(0)).unwrap().origin = ValueOrigin::Temporary {
            step: SourceStepId(0),
            index: 0,
        };
        values.get_mut(&ValueId(0)).unwrap().reconstruction =
            Some(Reconstruction::Literal(vec![Felt::new_unchecked(7)]));
        if self.alias {
            let parameter = values.get_mut(&ValueId(1)).unwrap();
            parameter.origin = ValueOrigin::Parameter {
                segment: SegmentId(1),
                index: 0,
            };
            parameter.reconstruction = Some(Reconstruction::Alias(vec![atom(0)]));
        }
        let mut producer = unit(vec![], vec![]);
        producer.outputs = vec![atom(0)];
        let mut consumer = unit(vec![atom(u32::from(self.alias))], vec![]);
        consumer.id = SelectedStepId(1);
        consumer.source = SourceStepId(1);
        let edges = vec![
            TransferEdge {
                id: EdgeId(0),
                source: SegmentId(0),
                destination: Some(SegmentId(1)),
                kind: EdgeKind::Forward,
                bindings: if self.alias {
                    vec![ParameterBinding {
                        destination: ValueId(1),
                        source: ValueId(0),
                    }]
                } else {
                    vec![]
                },
                content_bindings: vec![],
            },
            TransferEdge {
                id: EdgeId(1),
                source: SegmentId(1),
                destination: None,
                kind: EdgeKind::Return,
                bindings: vec![],
                content_bindings: vec![],
            },
        ];
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            units: BTreeMap::from([
                (SelectedStepId(0), UnitEvidence::default()),
                (SelectedStepId(1), UnitEvidence::default()),
            ]),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![
                    SelectedSegment {
                        id: SegmentId(0),
                        parameters: vec![],
                        content_parameters: vec![],
                        units: vec![producer],
                    },
                    SelectedSegment {
                        id: SegmentId(1),
                        parameters: if self.alias { vec![ValueId(1)] } else { vec![] },
                        content_parameters: vec![],
                        units: vec![consumer],
                    },
                ],
                edges,
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                    ControlTree::Segment(SegmentId(1)),
                    ControlTree::Edge(EdgeId(1)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
                source_map: (0..2)
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
                    .collect(),
            },
        })
    }
}

fn reconstructed_edge_survives_empty_contract(alias: bool) {
    use alloc::collections::BTreeSet;

    use super::normalization::{append_actions, fixture_frame, verify_assemble_execute};
    use crate::{
        allocate::normalize::prepare_unit,
        lower::{expand::*, units::LoweringBindings},
    };
    let definition = ReconstructedEdge { alias };
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let analysis = crate::allocate::analysis::analyze(&selected);
    let context = AllocationContext::new(&selected, &analysis);
    let transfer = repair_edge(
        &state(vec![atom(0)], vec![]),
        &selected.edges[0],
        &contract(vec![], vec![]),
        &context,
    )
    .unwrap();
    assert!(transfer.state.stack.is_empty());
    assert!(transfer.state.slots.is_empty());
    let consumer = &selected.segments[1].units[0];
    let preparation = prepare_unit(&transfer.state, consumer, &BTreeSet::new(), &context).unwrap();
    assert_eq!(
        preparation.actions,
        vec![PlanAction::PushLiteral(miden_core::Felt::new_unchecked(7))]
    );
    let fresh =
        AllocationState::at_entry(&contract(vec![], vec![]), SegmentId(1), &context).unwrap();
    let independently_prepared =
        prepare_unit(&fresh, consumer, &BTreeSet::new(), &context).unwrap();
    assert_eq!(
        independently_prepared.actions, preparation.actions,
        "planning a segment once from its contract must not depend on predecessor state"
    );
    let mut actions = preparation.actions;
    actions.push(PlanAction::RunLowering { step: consumer.id });
    let mut bindings = preparation.bindings;
    bindings.insert(SelectedStepId(0), LoweringBindings::default());
    let plan = ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts: BTreeMap::from([
            (SegmentId(0), contract(vec![], vec![])),
            (SegmentId(1), contract(vec![], vec![])),
        ]),
        segment_actions: BTreeMap::from([
            (
                SegmentId(0),
                vec![PlanAction::RunLowering {
                    step: SelectedStepId(0),
                }],
            ),
            (SegmentId(1), actions),
        ]),
        edge_actions: BTreeMap::from([(EdgeId(0), transfer.actions)]),
        spill_slots: vec![],
        semantic_storage_reads: Default::default(),
        lowering_bindings: bindings,
    };
    let frame = fixture_frame(&plan);
    let mut body = AnnotatedMasmBody::new();
    body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
    body.push_instruction(
        Primitive::Push(miden_core::Felt::new_unchecked(7)).instruction().unwrap(),
        SelectedStepId(0),
    );
    body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    append_actions(&mut body, &plan.edge_actions[&EdgeId(0)], &plan, &frame);
    body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(1))));
    append_actions(&mut body, &plan.segment_actions[&SegmentId(1)], &plan, &frame);
    body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(1))));
    verify_assemble_execute(&plan, &frame, &body, 0);
    let mut invalid = AnnotatedMasmBody::new();
    for op in body.ops() {
        invalid.push(
            if matches!(
                op,
                AnnotatedOp::Instruction {
                    unit: SelectedStepId(0),
                    ..
                }
            ) {
                AnnotatedOp::Instruction {
                    unit: SelectedStepId(0),
                    instruction: Primitive::Push(miden_core::Felt::new_unchecked(8))
                        .instruction()
                        .unwrap(),
                }
            } else {
                op.clone()
            },
        );
    }
    assert!(
        verify(&plan, &frame, invalid).is_err(),
        "a reconstruction declaration cannot bless the wrong publication"
    );
}

#[test]
fn dominating_literal_survives_an_empty_edge_without_parameter_bindings() {
    reconstructed_edge_survives_empty_contract(false);
}

#[test]
fn omitted_alias_parameter_retains_its_dominating_literal_dependency() {
    reconstructed_edge_survives_empty_contract(true);
}

struct ReconstructionLoop;
impl GeneratedDefinition for ReconstructionLoop {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(404)
    }

    fn instantiate(&self, _: &[miden_core::Felt]) -> Result<DefinitionEvidence, VerificationError> {
        use alloc::boxed::Box;

        use miden_core::Felt;
        let mut values = values(5);
        for (id, source, index) in [(0, 0, 0), (2, 2, 0), (3, 1, 0)] {
            values.get_mut(&ValueId(id)).unwrap().origin = ValueOrigin::Temporary {
                step: SourceStepId(source),
                index,
            };
        }
        values.get_mut(&ValueId(1)).unwrap().origin = ValueOrigin::Parameter {
            segment: SegmentId(1),
            index: 0,
        };
        values.get_mut(&ValueId(4)).unwrap().origin = ValueOrigin::Parameter {
            segment: SegmentId(0),
            index: 0,
        };
        for (id, literal) in [(0, 7), (3, 9)] {
            values.get_mut(&ValueId(id)).unwrap().reconstruction =
                Some(Reconstruction::Literal(vec![Felt::new_unchecked(literal)]));
        }
        for id in [1, 2] {
            values.get_mut(&ValueId(id)).unwrap().reconstruction =
                Some(Reconstruction::Alias(vec![atom(0)]));
        }
        let mut producer = unit(vec![], vec![]);
        producer.outputs = vec![atom(0)];
        let mut header = unit(vec![], vec![]);
        header.id = SelectedStepId(1);
        header.source = SourceStepId(1);
        header.outputs = vec![atom(3)];
        let mut body = unit(vec![atom(0)], vec![]);
        body.id = SelectedStepId(2);
        body.source = SourceStepId(2);
        body.outputs = vec![atom(2)];
        let binding = ParameterBinding {
            destination: ValueId(1),
            source: ValueId(0),
        };
        let edges = vec![
            TransferEdge {
                id: EdgeId(0),
                source: SegmentId(0),
                destination: Some(SegmentId(1)),
                kind: EdgeKind::Forward,
                bindings: vec![binding],
                content_bindings: vec![],
            },
            TransferEdge {
                id: EdgeId(1),
                source: SegmentId(1),
                destination: Some(SegmentId(2)),
                kind: EdgeKind::True,
                bindings: vec![],
                content_bindings: vec![],
            },
            TransferEdge {
                id: EdgeId(2),
                source: SegmentId(1),
                destination: Some(SegmentId(3)),
                kind: EdgeKind::False,
                bindings: vec![],
                content_bindings: vec![],
            },
            TransferEdge {
                id: EdgeId(3),
                source: SegmentId(2),
                destination: Some(SegmentId(1)),
                kind: EdgeKind::Backedge,
                bindings: vec![binding],
                content_bindings: vec![],
            },
            TransferEdge {
                id: EdgeId(4),
                source: SegmentId(3),
                destination: None,
                kind: EdgeKind::Return,
                bindings: vec![],
                content_bindings: vec![],
            },
        ];
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            units: BTreeMap::from([
                (SelectedStepId(0), UnitEvidence::default()),
                (SelectedStepId(1), UnitEvidence::default()),
                (
                    SelectedStepId(2),
                    UnitEvidence {
                        aliases: LaneAliases::new(vec![(atom(2), atom(0))]),
                        ..UnitEvidence::default()
                    },
                ),
            ]),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![
                    SelectedSegment {
                        id: SegmentId(0),
                        parameters: vec![ValueId(4)],
                        content_parameters: vec![],
                        units: vec![producer],
                    },
                    SelectedSegment {
                        id: SegmentId(1),
                        parameters: vec![ValueId(1)],
                        content_parameters: vec![],
                        units: vec![header],
                    },
                    SelectedSegment {
                        id: SegmentId(2),
                        parameters: vec![],
                        content_parameters: vec![],
                        units: vec![body],
                    },
                    SelectedSegment {
                        id: SegmentId(3),
                        parameters: vec![],
                        content_parameters: vec![],
                        units: vec![],
                    },
                ],
                edges,
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                    ControlTree::Loop {
                        id: ControlId(0),
                        condition: ControlId(1),
                        on_true: EdgeId(1),
                        on_false: EdgeId(2),
                        header: SegmentId(1),

                        before: Box::new(ControlTree::Segment(SegmentId(1))),
                        predicate: atom(4),
                        body: Box::new(ControlTree::Sequence(vec![
                            ControlTree::Segment(SegmentId(2)),
                            ControlTree::Edge(EdgeId(3)),
                        ])),
                        backedges: vec![EdgeId(3)],
                    },
                    ControlTree::Segment(SegmentId(3)),
                    ControlTree::Edge(EdgeId(4)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 1, 0).unwrap(),
                source_map: (0..3)
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
                    .collect(),
            },
        })
    }
}

#[test]
fn backedge_retains_current_invariants_but_discards_body_and_header_unit_reconstructions() {
    use alloc::collections::BTreeSet;

    use crate::allocate::normalize::prepare_unit;
    let selected = check_selection(
        ReconstructionLoop.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&ReconstructionLoop),
    )
    .unwrap();
    let predicate_slot = SpillSlotId::canonical(atom(4));
    let target = contract(vec![], vec![(predicate_slot, atom(4))]);
    let mut initial = state(
        vec![atom(0), atom(2), atom(3)],
        vec![(SpillSlotId::canonical(atom(2)), atom(2)), (predicate_slot, atom(4))],
    );
    initial.aliases = LaneAliases::new(vec![(atom(2), atom(0))]);
    initial.available_values.insert(ValueId(1));
    let analysis = crate::allocate::analysis::analyze(&selected);
    let context = AllocationContext::new(&selected, &analysis);
    let transfer = repair_edge(&initial, &selected.edges[3], &target, &context).unwrap();
    assert_eq!(
        transfer.state.available_values,
        BTreeSet::from([ValueId(0), ValueId(1), ValueId(4)])
    );
    assert!(transfer.state.stack.is_empty());
    assert_eq!(
        transfer.state.slots,
        BTreeMap::from([(predicate_slot, StoredValue::Atom(atom(4)))])
    );
    assert!(transfer.state.aliases.pairs().is_empty());
    let fresh = AllocationState::at_entry(&target, SegmentId(1), &context).unwrap();
    assert_eq!(fresh.available_values, transfer.state.available_values);
    let rebuilt =
        prepare_unit(&transfer.state, &unit(vec![atom(1)], vec![]), &BTreeSet::new(), &context)
            .unwrap();
    assert_eq!(
        rebuilt.actions,
        vec![PlanAction::PushLiteral(miden_core::Felt::new_unchecked(7))]
    );
    for stale in [atom(2), atom(3)] {
        assert!(
            matches!(prepare_unit(&transfer.state, &unit(vec![stale], vec![]), &BTreeSet::new(), &context), Err(AllocationError::Invariant(error)) if error.category == DiagnosticCategory::UnavailableValue)
        );
    }
}
