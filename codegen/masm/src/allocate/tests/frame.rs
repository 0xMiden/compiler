use alloc::{collections::BTreeMap, vec, vec::Vec};

use super::contracts::*;
use crate::{
    allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
    lower::expand::resolve_frame_bindings,
    plan::{actions::*, *},
    verify::verify,
};

fn slot(id: u32) -> SpillSlotId {
    SpillSlotId::temporary(SelectedStepId(0), id)
}

fn linear(units: Vec<SelectedLowering>) -> ProcedurePlan {
    let definition = source(
        vec![segment(0, &[], units)],
        vec![edge(0, 0, None, EdgeKind::Return, &[])],
        seq(vec![s(0), e(0)]),
        0,
    );
    allocate(checked(&definition), &AllocationOptions::default()).unwrap()
}

fn scalar(actions: Vec<PlanAction>, count: u32) -> ProcedurePlan {
    let mut plan = linear(vec![]);
    plan.spill_slots = (0..count).map(slot).collect();
    plan.segment_actions.insert(SegmentId(0), actions);
    plan
}

fn assign(plan: &ProcedurePlan) -> FinalFrameLayout {
    assign_frame(plan, &SourceLocalLayout::default()).unwrap()
}

fn scratch(
    id: u32,
    size: u32,
    alignment: u32,
    owner: Option<u32>,
    uses: &[u32],
) -> ScratchRequirement {
    ScratchRequirement {
        id: ScratchId(id),
        size,
        alignment,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: owner
            .map_or(ScratchLifetime::Spanning, |id| ScratchLifetime::Unit(SelectedStepId(id))),
        uses: uses.iter().copied().map(SelectedStepId).collect(),
    }
}

#[test]
fn dead_store_interferes_with_live_contents_and_stable_degree_coloring() {
    let plan = scalar(
        vec![
            PlanAction::StoreLane(slot(0)),
            PlanAction::StoreLane(slot(1)),
            PlanAction::LoadLane(slot(0)),
            PlanAction::StoreLane(slot(2)),
        ],
        3,
    );
    let frame = assign(&plan);
    assert_ne!(frame.spills()[&slot(0)], frame.spills()[&slot(1)]);
    assert_eq!(frame.spills()[&slot(0)], frame.spills()[&slot(2)]);
    assert_eq!(frame.spills()[&slot(0)], 0);
    assert_eq!(frame.resources().scalar_colors, 2);
    let mut reversed = plan.clone();
    reversed.spill_slots.reverse();
    assert_eq!(assign(&reversed).spills(), frame.spills());
}

#[test]
fn last_reload_releases_a_copy_even_while_its_atom_is_resident() {
    let mut identity = request(0, &[0], &[100]);
    identity.kind = LoweringKind(1);
    let mut definition = source(
        vec![segment(0, &[0], vec![identity])],
        vec![edge(0, 0, None, EdgeKind::Return, &[(100, 100)])],
        seq(vec![s(0), e(0)]),
        1,
    );
    definition.0.values.get_mut(&ValueId(100)).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![a(0)]));
    let mut plan = allocate(checked(&definition), &AllocationOptions::default()).unwrap();
    plan.spill_slots = vec![slot(0), slot(1)];
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::Dup { depth: 0 },
            PlanAction::StoreLane(slot(0)),
            PlanAction::LoadLane(slot(0)),
            PlanAction::Drop,
            PlanAction::PushLiteral(miden_core::Felt::ZERO),
            PlanAction::StoreLane(slot(1)),
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
        ],
    );
    let frame = assign(&plan);
    assert_eq!(frame.spills()[&slot(0)], frame.spills()[&slot(1)]);
    assert_eq!(frame.spills().len(), 2);
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    verify(&plan, &frame, body(&plan, &frame)).unwrap();
}

#[test]
fn repeated_stores_kill_prior_contents_without_splitting_slot_identity() {
    let plan = scalar(
        vec![
            PlanAction::StoreLane(slot(0)),
            PlanAction::LoadLane(slot(0)),
            PlanAction::StoreLane(slot(1)),
            PlanAction::LoadLane(slot(1)),
            PlanAction::StoreLane(slot(0)),
            PlanAction::LoadLane(slot(0)),
        ],
        2,
    );
    assert_eq!(assign(&plan).spills()[&slot(0)], assign(&plan).spills()[&slot(1)]);
}

fn empty_diamond() -> Source {
    source(
        vec![
            segment(0, &[0, 1, 2], vec![]),
            segment(1, &[], vec![]),
            segment(2, &[], vec![]),
            segment(3, &[], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::True, &[]),
            edge(1, 0, Some(2), EdgeKind::False, &[]),
            edge(2, 1, Some(3), EdgeKind::Forward, &[]),
            edge(3, 2, Some(3), EdgeKind::Forward, &[]),
            edge(4, 3, None, EdgeKind::Return, &[(1, 1), (2, 2)]),
        ],
        seq(vec![
            s(0),
            ControlTree::If {
                id: ControlId(0),
                predicate: a(0),
                on_true: EdgeId(0),
                on_false: EdgeId(1),
                then_body: alloc::boxed::Box::new(seq(vec![s(1), e(2)])),
                else_body: alloc::boxed::Box::new(seq(vec![s(2), e(3)])),
            },
            s(3),
            e(4),
        ]),
        2,
    )
}

#[test]
fn branch_local_contents_reuse_but_prebranch_contents_coexist() {
    let definition = empty_diamond();
    let mut plan = allocate(checked(&definition), &AllocationOptions::default()).unwrap();
    plan.spill_slots = vec![slot(0), slot(1)];
    for (id, value) in [(0, 17), (1, 23)] {
        plan.segment_actions.insert(
            SegmentId(id + 1),
            vec![
                PlanAction::PushLiteral(miden_core::Felt::new_unchecked(value)),
                PlanAction::StoreLane(slot(id)),
                PlanAction::LoadLane(slot(id)),
                PlanAction::Drop,
            ],
        );
    }
    let local = assign(&plan);
    assert_eq!(local.spills()[&slot(0)], local.spills()[&slot(1)]);
    verify(&plan, &local, body(&plan, &local)).unwrap();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::Dup { depth: 1 },
            PlanAction::StoreLane(slot(0)),
            PlanAction::Dup { depth: 2 },
            PlanAction::StoreLane(slot(1)),
        ],
    );
    for id in 0..2 {
        plan.segment_actions
            .insert(SegmentId(id + 1), vec![PlanAction::LoadLane(slot(id)), PlanAction::Drop]);
        plan.contracts.get_mut(&SegmentId(id + 1)).unwrap().required_spill_slots =
            vec![(slot(id), a(id + 1))];
    }
    let coexisting = assign(&plan);
    assert_ne!(coexisting.spills()[&slot(0)], coexisting.spills()[&slot(1)]);
    verify(&plan, &coexisting, body(&plan, &coexisting)).unwrap();
}

#[test]
fn retained_cache_contracts_are_reads_including_resident_copies() {
    let definition = empty_diamond();
    let mut plan = allocate(checked(&definition), &AllocationOptions::default()).unwrap();
    plan.spill_slots = vec![slot(0), slot(1)];
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::Dup { depth: 1 },
            PlanAction::StoreLane(slot(0)),
            PlanAction::Dup { depth: 2 },
            PlanAction::StoreLane(slot(1)),
        ],
    );
    for id in 1..4 {
        let contract = plan.contracts.get_mut(&SegmentId(id)).unwrap();
        assert!(
            contract.stack_top_first.contains(&a(1)) && contract.stack_top_first.contains(&a(2))
        );
        contract.required_spill_slots = vec![(slot(0), a(1)), (slot(1), a(2))];
    }
    let frame = assign(&plan);
    assert_ne!(frame.spills()[&slot(0)], frame.spills()[&slot(1)]);
    verify(&plan, &frame, body(&plan, &frame)).unwrap();
}

#[test]
fn invariant_loop_copy_and_changing_header_contents_use_backedge_fixed_point() {
    let definition = loop_source();
    let mut plan = allocate(checked(&definition), &AllocationOptions::default()).unwrap();
    plan.spill_slots = vec![slot(0), slot(1), slot(2)];
    for actions in plan.segment_actions.values_mut() {
        actions.clear();
    }
    for actions in plan.edge_actions.values_mut() {
        actions.clear();
    }
    for contract in plan.contracts.values_mut() {
        contract.required_spill_slots.clear();
    }
    plan.contracts.get_mut(&SegmentId(1)).unwrap().required_spill_slots =
        vec![(slot(0), a(1)), (slot(1), a(10))];
    plan.segment_actions.insert(
        SegmentId(2),
        vec![PlanAction::StoreLane(slot(2)), PlanAction::LoadLane(slot(1))],
    );
    plan.edge_actions.insert(EdgeId(3), vec![PlanAction::StoreLane(slot(1))]);
    let frame = assign(&plan);
    assert_ne!(frame.spills()[&slot(0)], frame.spills()[&slot(1)]);
    assert_ne!(frame.spills()[&slot(0)], frame.spills()[&slot(2)]);
    assert_ne!(frame.spills()[&slot(1)], frame.spills()[&slot(2)]);
    // Killing the changing contents before the dead temporary permits their reuse.
    plan.segment_actions.insert(
        SegmentId(2),
        vec![PlanAction::LoadLane(slot(1)), PlanAction::StoreLane(slot(2))],
    );
    plan.contracts
        .get_mut(&SegmentId(1))
        .unwrap()
        .required_spill_slots
        .retain(|(id, _)| *id != slot(1));
    assert_eq!(assign(&plan).spills()[&slot(1)], assign(&plan).spills()[&slot(2)]);
}

#[test]
fn snapshot_lifetime_includes_final_stack_output_reads() {
    let plan = scalar(
        vec![
            PlanAction::PushLiteral(miden_core::Felt::new_unchecked(17)),
            PlanAction::StoreLane(slot(0)),
            PlanAction::PushLiteral(miden_core::Felt::new_unchecked(23)),
            PlanAction::StoreLane(slot(1)),
            PlanAction::LoadLane(slot(0)),
            PlanAction::StoreLane(slot(2)),
            PlanAction::LoadLane(slot(1)),
            PlanAction::StoreLane(slot(0)),
            PlanAction::LoadLane(slot(2)),
            PlanAction::StoreLane(slot(1)),
            PlanAction::LoadLane(slot(2)),
            PlanAction::Drop,
        ],
        3,
    );
    let frame = assign(&plan);
    assert_ne!(frame.spills()[&slot(2)], frame.spills()[&slot(0)]);
    assert_ne!(frame.spills()[&slot(2)], frame.spills()[&slot(1)]);
    super::normalization::verify_assemble_execute(&plan, &frame, &body(&plan, &frame), 0);
}

#[test]
fn stored_bindings_reserve_whole_unit_against_each_other_and_live_through_cells() {
    let mut unit = request(0, &[], &[]);
    unit.stored_inputs = (0..2)
        .map(|id| StoredInput {
            id: StoredInputId(id),
            value: StoredValue::Literal(miden_core::Felt::ONE),
        })
        .collect();
    let mut plan = linear(vec![unit]);
    let bindings = plan.lowering_bindings.get_mut(&SelectedStepId(0)).unwrap();
    bindings.stored_inputs =
        BTreeMap::from([(StoredInputId(0), slot(0)), (StoredInputId(1), slot(1))]);
    plan.spill_slots = (0..3).map(slot).collect();
    plan.segment_actions.insert(
        SegmentId(0),
        vec![
            PlanAction::RunLowering {
                step: SelectedStepId(0),
            },
            PlanAction::LoadLane(slot(2)),
        ],
    );
    let frame = assign(&plan);
    assert_eq!(
        frame
            .spills()
            .values()
            .copied()
            .collect::<alloc::collections::BTreeSet<_>>()
            .len(),
        3
    );
}

#[test]
fn unit_workspace_overlays_successive_units_and_separates_nested_helper_objects() {
    let mut first = request(0, &[], &[]);
    first.scratch = vec![scratch(9, 4, 4, Some(0), &[0]), scratch(1, 3, 2, Some(0), &[0])];
    let mut second = request(1, &[], &[]);
    second.scratch = vec![scratch(2, 4, 4, Some(1), &[1])];
    let mut plan = linear(vec![first, second]);
    let frame = assign(&plan);
    assert_eq!(frame.workspace().alignment, 4);
    assert_eq!(frame.workspace().size, 8);
    assert_eq!(frame.scratch()[&ScratchId(1)].offset, 0);
    assert_eq!(frame.scratch()[&ScratchId(9)].offset, 4);
    assert_eq!(frame.scratch()[&ScratchId(2)].offset, 0);
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    assert_eq!(plan.lowering_bindings[&SelectedStepId(0)].scratch.len(), 2);
    assert_eq!(
        plan.lowering_bindings[&SelectedStepId(1)].scratch[&ScratchId(2)],
        frame.scratch()[&ScratchId(2)]
    );
    let empty = assign(&linear(vec![]));
    assert_eq!((empty.workspace().size, empty.workspace().alignment), (0, 1));
}

#[test]
fn two_four_cell_objects_in_successive_units_share_one_aligned_workspace() {
    let units = (0..2)
        .map(|id| {
            let mut u = request(id, &[], &[]);
            u.scratch = vec![scratch(id, 4, 4, Some(id), &[id])];
            u
        })
        .collect();
    let frame = assign(&linear(units));
    assert_eq!(frame.aligned_size(), 4);
    assert_eq!(frame.scratch()[&ScratchId(0)].offset, frame.scratch()[&ScratchId(1)].offset);
}

#[test]
fn spanning_objects_keep_dedicated_ranges_even_in_successive_units() {
    let units = (0..2)
        .map(|id| {
            let mut unit = request(id, &[], &[]);
            unit.scratch = vec![scratch(id, 4, 4, None, &[id])];
            unit
        })
        .collect();
    let frame = assign(&linear(units));
    assert_eq!(frame.scratch()[&ScratchId(0)].offset, 0);
    assert_eq!(frame.scratch()[&ScratchId(1)].offset, 4);
    assert_eq!(frame.resources().spanning_scratch, 8);
    assert_eq!((frame.workspace().size, frame.workspace().alignment), (0, 1));
    assert_eq!(frame.aligned_size(), 8);
}

#[test]
fn source_scalar_spanning_workspace_and_padding_have_checked_order() {
    let mut unit = request(0, &[], &[]);
    unit.scratch = vec![scratch(0, 3, 4, None, &[0]), scratch(1, 1, 8, Some(0), &[0])];
    let mut plan = linear(vec![unit]);
    plan.spill_slots = vec![slot(0)];
    let source = SourceLocalLayout {
        offsets: vec![0, 4],
        extent: 6,
    };
    let frame = assign_frame(&plan, &source).unwrap();
    assert_eq!(frame.source(), &source);
    assert_eq!(frame.spills()[&slot(0)], 6);
    assert_eq!(frame.scratch()[&ScratchId(0)].offset, 8);
    assert_eq!(frame.workspace().offset, 16);
    assert_eq!(frame.scratch()[&ScratchId(1)].offset, 16);
    assert_eq!(frame.aligned_size(), 20);
    let debug = crate::emitter::FrameLayout::from(&frame);
    assert_eq!(
        debug.element_offset(1),
        Some(u32::from(frame.resolve(FrameRef::SourceLocal(1)).unwrap()))
    );
    assert_eq!(debug.aligned_size, 20);
    assert_eq!(frame.resources().source_locals, 6);
    assert_eq!(frame.resources().spanning_scratch, 3);
    assert_eq!(frame.resources().workspace, 1);
    assert_eq!(frame.resources().padding, 9);
}

#[test]
fn capacity_checks_precede_narrowing_and_boundary_size_assembles() {
    let plan = linear(vec![]);
    let source = SourceLocalLayout {
        offsets: vec![0, 65_531],
        extent: 65_532,
    };
    let frame = assign_frame(&plan, &source).unwrap();
    assert_eq!(frame.aligned_size(), MAX_FRAME_ELEMENTS);
    let code = alloc::format!(
        "@locals({}) proc boundary locaddr.65531 drop end begin exec.boundary end",
        frame.aligned_size()
    );
    miden_assembly::Assembler::default()
        .assemble_program("frame_boundary", code)
        .unwrap();
    for extent in [65_533, u32::MAX] {
        let error = assign_frame(
            &plan,
            &SourceLocalLayout {
                offsets: vec![0],
                extent,
            },
        )
        .unwrap_err();
        let crate::allocate::state::AllocationError::Invariant(error) = error else {
            panic!("expected frame diagnostic")
        };
        assert_eq!(error.category, DiagnosticCategory::FrameCapacityExceeded);
        for contribution in ["source", "scalar", "spanning", "workspace", "padding"] {
            assert!(error.message.contains(contribution), "{}", error.message);
        }
    }
}

#[test]
fn scratch_descriptors_reject_invalid_alignment_conflicting_identity_and_wrong_use_sets() {
    use crate::verify::{SourceContext, check_selection};
    let build = |units| {
        source(
            vec![segment(0, &[], units)],
            vec![edge(0, 0, None, EdgeKind::Return, &[])],
            seq(vec![s(0), e(0)]),
            0,
        )
    };
    let shared = scratch(0, 4, 4, None, &[0, 1]);
    let mut first = request(0, &[], &[]);
    first.scratch = vec![shared.clone()];
    let mut second = request(1, &[], &[]);
    second.scratch = vec![shared];
    checked(&build(vec![first.clone(), second.clone()]));
    let mut reversed = second.clone();
    reversed.scratch[0].uses.reverse();
    checked(&build(vec![first.clone(), reversed]));
    for mutation in 0..6 {
        let mut units = vec![first.clone(), second.clone()];
        match mutation {
            0 => units[0].scratch[0].alignment = 0,
            1 => units[0].scratch[0].alignment = 3,
            2 => units[1].scratch[0].size = 8,
            3 => {
                units[0].scratch[0].uses.push(SelectedStepId(7));
                units[1].scratch[0].uses.push(SelectedStepId(7));
            }
            4 => {
                units[0].scratch[0].uses.push(SelectedStepId(0));
                units[1].scratch[0].uses.push(SelectedStepId(0));
            }
            _ => {
                let duplicate = units[0].scratch[0].clone();
                units[0].scratch.push(duplicate);
            }
        }
        let definition = build(units);
        assert!(
            check_selection(definition.0.clone(), &SourceContext::Generated(&definition)).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn overflow_in_spanning_or_workspace_is_reported_before_operand_narrowing() {
    for spanning in [false, true] {
        let mut unit = request(0, &[], &[]);
        unit.scratch = vec![
            scratch(0, u32::MAX, 1, if spanning { None } else { Some(0) }, &[0]),
            scratch(1, 4, 4, Some(0), &[0]),
        ];
        let plan = linear(vec![unit]);
        let error = assign_frame(&plan, &SourceLocalLayout::default()).unwrap_err();
        let crate::allocate::state::AllocationError::Invariant(error) = error else {
            panic!("frame diagnostic")
        };
        assert_eq!(error.category, DiagnosticCategory::FrameCapacityExceeded);
        assert!(error.message.contains("workspace="));
    }
}

#[test]
fn descending_degree_precedes_stable_slot_order() {
    let plan = scalar(
        vec![
            PlanAction::StoreLane(slot(2)),
            PlanAction::StoreLane(slot(0)),
            PlanAction::LoadLane(slot(0)),
            PlanAction::StoreLane(slot(1)),
            PlanAction::LoadLane(slot(1)),
            PlanAction::LoadLane(slot(2)),
        ],
        4,
    );
    let frame = assign(&plan);
    assert_eq!(frame.spills()[&slot(2)], 0);
    assert_eq!(frame.spills()[&slot(0)], 1);
    assert_eq!(frame.spills()[&slot(1)], 1);
    assert_eq!(frame.spills()[&slot(3)], 0);
}

/// A source is entirely in storage while a dead store precedes complete alias publication.
/// The two-lane case later reloads only lane one, so lane zero has no physical read.
fn spill_only_alias_plan(width: u8) -> ProcedurePlan {
    let outputs: Vec<_> = (100..100 + u32::from(width)).collect();
    let mut alias = request(0, &[], &outputs);
    alias.kind = LoweringKind(1);
    alias.semantic_inputs = vec![ValueId(0)];
    alias.publication = OutputPublication::Aliases;
    let mut units = vec![alias.clone()];
    if width == 2 {
        units.push(request(1, &[101], &[]));
    }
    let mut definition = source(
        vec![segment(0, &[0], units)],
        vec![edge(0, 0, None, EdgeKind::Return, &[])],
        seq(vec![s(0), e(0)]),
        0,
    );
    if width == 2 {
        definition.0.values.get_mut(&ValueId(0)).unwrap().layout =
            ValueLayout::new(midenc_hir::Type::U64).unwrap();
        definition.0.abi = ProcedureAbi::new(CallingConvention::SameContext, 2, 0).unwrap();
    }
    for (lane, output) in outputs.iter().enumerate() {
        definition.0.values.get_mut(&ValueId(*output)).unwrap().reconstruction =
            Some(Reconstruction::Alias(vec![Atom::new(ValueId(0), lane as u8)]));
    }
    let mut plan = allocate(checked(&definition), &AllocationOptions::default()).unwrap();
    let state = super::edges::state(
        vec![],
        (0..width)
            .map(|lane| (slot(u32::from(lane)), Atom::new(ValueId(0), lane)))
            .collect(),
    );
    let context = super::edges::context(&definition.0.values);
    let reads = context.semantic_storage_reads(&state, &alias).unwrap();
    assert_eq!(reads, (0..u32::from(width)).map(slot).collect::<Vec<_>>());
    plan.semantic_storage_reads.insert(alias.id, reads);
    plan.spill_slots = (0..=u32::from(width)).map(slot).collect();
    plan.edge_actions.insert(EdgeId(0), vec![]);
    let mut actions: Vec<_> =
        (0..u32::from(width)).map(|id| PlanAction::StoreLane(slot(id))).collect();
    actions.extend([
        PlanAction::PushLiteral(miden_core::Felt::ZERO),
        PlanAction::StoreLane(slot(u32::from(width))),
        PlanAction::RunLowering { step: alias.id },
    ]);
    if width == 2 {
        actions.extend([
            PlanAction::LoadLane(slot(1)),
            PlanAction::RunLowering {
                step: SelectedStepId(1),
            },
        ]);
    }
    plan.segment_actions.insert(SegmentId(0), actions);
    plan
}

#[test]
fn alias_publication_preserves_spill_only_source() {
    for width in [1, 2] {
        let mut plan = spill_only_alias_plan(width);
        let distinct = FinalFrameLayout::new(
            SourceLocalLayout::default(),
            plan.spill_slots
                .iter()
                .enumerate()
                .map(|(offset, slot)| (*slot, offset as u32))
                .collect(),
            BTreeMap::new(),
            4,
        )
        .unwrap();
        verify(&plan, &distinct, body(&plan, &distinct)).unwrap();
        let frame = assign(&plan);
        for id in 0..u32::from(width) {
            assert_ne!(frame.spills()[&slot(id)], frame.spills()[&slot(u32::from(width))]);
        }
        verify(&plan, &frame, body(&plan, &frame)).unwrap();
        // Allocation metadata cannot authorize overwritten raw representations.
        let collision = FinalFrameLayout::new(
            SourceLocalLayout::default(),
            plan.spill_slots
                .iter()
                .enumerate()
                .map(|(offset, slot)| {
                    (
                        *slot,
                        if offset == usize::from(width) {
                            0
                        } else {
                            offset as u32
                        },
                    )
                })
                .collect(),
            BTreeMap::new(),
            4,
        )
        .unwrap();
        assert_eq!(
            verify(&plan, &collision, body(&plan, &collision)).unwrap_err().category,
            DiagnosticCategory::UnavailableValue,
        );
        plan.semantic_storage_reads.clear();
        assert_eq!(
            verify(&plan, &collision, body(&plan, &collision)).unwrap_err().category,
            DiagnosticCategory::UnavailableValue,
        );
        plan.semantic_storage_reads
            .insert(SelectedStepId(0), vec![slot(u32::from(width))]);
        assert_eq!(
            verify(&plan, &collision, body(&plan, &collision)).unwrap_err().category,
            DiagnosticCategory::UnavailableValue,
        );
    }
}

#[test]
fn allocation_freezes_semantic_reads_after_preparation() {
    use crate::allocate::schedule::SchedulingTier;
    let mut alias = request(1, &[], &[100]);
    alias.kind = LoweringKind(1);
    alias.semantic_inputs = vec![ValueId(0)];
    alias.publication = OutputPublication::Aliases;
    let mut definition = source(
        vec![segment(0, &[0, 1], vec![request(0, &[1], &[]), alias])],
        vec![edge(0, 0, None, EdgeKind::Return, &[(100, 100)])],
        seq(vec![s(0), e(0)]),
        1,
    );
    definition.0.values.get_mut(&ValueId(100)).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![a(0)]));
    let options = AllocationOptions {
        force_tier: Some(SchedulingTier::Normalization),
        ..Default::default()
    };
    let mut plan = allocate(checked(&definition), &options).unwrap();
    assert_eq!(
        plan.semantic_storage_reads[&SelectedStepId(1)],
        vec![SpillSlotId::canonical(a(0))]
    );
    assert!(!plan.semantic_storage_reads.contains_key(&SelectedStepId(0)));
    let frame = assign(&plan);
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    verify(&plan, &frame, body(&plan, &frame)).unwrap();
}

#[test]
fn semantic_reads_reuse_resident_literals_and_checked_equal_backing() {
    use super::edges::{context, state, values};
    let mut values = values(4);
    values.get_mut(&ValueId(2)).unwrap().reconstruction = Some(Reconstruction::Alias(vec![a(1)]));
    values.get_mut(&ValueId(3)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![miden_core::Felt::ONE]));
    let context = context(&values);
    let mut state = state(vec![a(0)], vec![(slot(0), a(1)), (slot(1), a(1)), (slot(2), a(0))]);
    state.available_values.extend([ValueId(2), ValueId(3)]);
    state.aliases = LaneAliases::new(vec![(a(0), a(1))]);
    let unit = request(0, &[2, 3], &[]);
    assert!(context.semantic_storage_reads(&state, &unit).unwrap().is_empty());
    state.stack.clear();
    assert_eq!(context.semantic_storage_reads(&state, &unit).unwrap(), vec![slot(0)]);
    state.slots.clear();
    assert!(context.semantic_storage_reads(&state, &unit).is_err());
}

#[test]
fn semantic_reads_reject_undeclared_backing_slots() {
    let mut plan = spill_only_alias_plan(1);
    plan.semantic_storage_reads.insert(SelectedStepId(0), vec![slot(99)]);
    let error = assign_frame(&plan, &SourceLocalLayout::default()).unwrap_err();
    let crate::allocate::state::AllocationError::Invariant(error) = error else {
        panic!("frame diagnostic")
    };
    assert!(error.message.contains("undeclared scalar slot"));
}
