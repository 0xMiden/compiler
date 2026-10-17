use alloc::{collections::BTreeMap, vec, vec::Vec};

use midenc_hir::{ArrayType, Type};

use crate::{
    lower::{expand::*, units::*},
    plan::{actions::*, *},
};

#[test]
fn raw_layout_preserves_limb_order() {
    for ty in [Type::Felt, Type::U64, Type::U128] {
        let raw = ty.clone().to_raw_parts().unwrap();
        let layout = ValueLayout::new(ty).unwrap();
        assert_eq!(layout.raw_parts(), raw.as_slice());
    }
    assert!(ValueLayout::new(Type::Never).is_err());
    assert!(ValueLayout::new(Type::Unknown).is_err());
    assert!(ValueLayout::new(Type::from(ArrayType::new(Type::Felt, 5))).is_err());
    let layouts = [Type::U128, Type::U128, Type::U128, Type::U128, Type::U128]
        .into_iter()
        .map(|ty| ValueLayout::new(ty).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(TupleLayout::new(layouts).lanes(), 20);
    let whole = ValueId(7);
    let low = ValueId(8);
    let high = ValueId(9);
    let aliases = LaneAliases::new(vec![
        (Atom::new(low, 0), Atom::new(whole, 0)),
        (Atom::new(high, 0), Atom::new(whole, 1)),
    ]);
    assert_eq!(aliases.source(Atom::new(high, 0)), Some(Atom::new(whole, 1)));
    assert_eq!(aliases.source(Atom::new(low, 0)), Some(Atom::new(whole, 0)));
    let joined = LaneAliases::new(vec![
        (Atom::new(whole, 0), Atom::new(low, 0)),
        (Atom::new(whole, 1), Atom::new(high, 0)),
    ]);
    assert_eq!(joined.source(Atom::new(whole, 0)), Some(Atom::new(low, 0)));
    assert_eq!(joined.source(Atom::new(whole, 1)), Some(Atom::new(high, 0)));
}

#[test]
fn frame_refs_are_symbolic_until_layout() {
    let slot = SpillSlotId::canonical(Atom::new(ValueId(3), 1));
    let reference = FrameRef::Spill(slot);
    let frame = FinalFrameLayout::new(
        SourceLocalLayout {
            offsets: vec![0, 4],
            extent: 6,
        },
        BTreeMap::from([(slot, 6)]),
        BTreeMap::new(),
        8,
    )
    .unwrap();
    assert_eq!(frame.resolve(reference).unwrap(), 6);
    assert_eq!(frame.source().offsets, [0, 4]);
    assert!(
        frame
            .resolve(FrameRef::Spill(SpillSlotId::canonical(Atom::new(ValueId(4), 0))))
            .is_err()
    );
    assert!(
        FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            65_536
        )
        .is_err()
    );
    assert!(
        FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            65_532
        )
        .is_ok()
    );
}

#[test]
fn stored_input_is_not_stack_input() {
    let atom = Atom::new(ValueId(3), 0);
    let slot = SpillSlotId::canonical(atom);
    let unit = SelectedLowering {
        stored_inputs: vec![
            StoredInput {
                id: StoredInputId(0),
                value: StoredValue::Atom(atom),
            },
            StoredInput {
                id: StoredInputId(1),
                value: StoredValue::Atom(atom),
            },
            StoredInput {
                id: StoredInputId(2),
                value: StoredValue::Literal(miden_core::Felt::new_unchecked(0xffff_ffff_0000_0000)),
            },
        ],
        ..SelectedLowering::fixture(LoweringKind(0), SourceStepId(1))
    };
    assert!(unit.stack_inputs.is_empty());
    let literal_slot = SpillSlotId::temporary(unit.id, 0);
    let bindings = LoweringBindings {
        stored_inputs: BTreeMap::from([
            (StoredInputId(0), slot),
            (StoredInputId(1), slot),
            (StoredInputId(2), literal_slot),
        ]),
        scratch: BTreeMap::new(),
    };
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0), (literal_slot, 1)]),
        BTreeMap::new(),
        4,
    )
    .unwrap();
    let body = StoredInputLoads
        .expand(&unit, &bindings, &frame, &ExpansionEnv::default())
        .unwrap();
    assert_eq!(body.top_level_instructions().count(), 3);
    assert_eq!(
        body.top_level_instructions().cloned().collect::<Vec<_>>(),
        vec![
            crate::masm::Instruction::LocLoad(0.into()),
            crate::masm::Instruction::LocLoad(0.into()),
            crate::masm::Instruction::LocLoad(1.into()),
        ]
    );
    let mut incomplete = bindings.clone();
    incomplete.stored_inputs.remove(&StoredInputId(2));
    assert!(
        StoredInputLoads
            .expand(&unit, &incomplete, &frame, &ExpansionEnv::default())
            .is_err()
    );
}

struct StoredInputLoads;

impl LoweringDefinition for StoredInputLoads {
    fn kind(&self) -> LoweringKind {
        LoweringKind(0)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, midenc_hir::Report> {
        bindings.validate(unit, frame)?;
        let mut body = AnnotatedMasmBody::new();
        for input in &unit.stored_inputs {
            body.push_instruction(
                crate::masm::Instruction::LocLoad(
                    frame.resolve(FrameRef::Spill(bindings.stored_inputs[&input.id]))?.into(),
                ),
                unit.id,
            );
        }
        Ok(body)
    }
}

#[test]
fn stable_ids_and_diagnostics_do_not_depend_on_trial_order() {
    let a = Atom::new(ValueId(8), 1);
    let b = Atom::new(ValueId(2), 0);
    let forward = [a, b].map(SpillSlotId::canonical);
    let reverse = [b, a].map(SpillSlotId::canonical);
    assert_eq!(forward, [reverse[1], reverse[0]]);
    let error = PlanError {
        category: DiagnosticCategory::IllegalStackAccess,
        origin: SourceLocation {
            span: crate::masm::SourceSpan::UNKNOWN,
            inline_chain: vec![crate::masm::SourceSpan::UNKNOWN],
        },
        step: Some(SelectedStepId(4)),
        depth: Some(16),
        expected: Some(a),
        actual: Some(b),
        message: "indexed access exceeds target window".into(),
    };
    let rendered = error.to_string();
    assert!(rendered.contains("IllegalStackAccess"));
    assert!(rendered.contains("depth=16"));
    assert!(rendered.contains("v8.1"));
    assert!(rendered.contains("v2.0"));
    assert!(rendered.contains("inline"));
    let plan = ProcedurePlan::fixture(vec![PlanAction::LoadLane(forward[0]), PlanAction::Drop]);
    assert_eq!(
        plan.to_string(),
        "procedure ProcedureAbi { convention: SameContext, arguments: 0, results: 0 }\nsegment \
         b0\n  load v8.1\n  drop\n"
    );
}

#[test]
fn assigned_frame_rejects_overlaps_misalignment_and_arithmetic_overflow() {
    let slot = SpillSlotId::canonical(Atom::new(ValueId(1), 0));
    let scratch = |offset, size, alignment, lifetime| ScratchAllocation {
        offset,
        size,
        alignment,
        lifetime,
    };
    let source = SourceLocalLayout {
        offsets: vec![0],
        extent: 4,
    };
    for allocation in [
        scratch(4, 4, 3, ScratchLifetime::Spanning),
        scratch(5, 4, 4, ScratchLifetime::Spanning),
        scratch(u32::MAX, 4, 1, ScratchLifetime::Spanning),
        scratch(0, 4, 4, ScratchLifetime::Spanning),
    ] {
        assert!(
            FinalFrameLayout::new(
                source.clone(),
                BTreeMap::new(),
                BTreeMap::from([(ScratchId(0), allocation)]),
                16
            )
            .is_err()
        );
    }
    assert!(
        FinalFrameLayout::new(
            source.clone(),
            BTreeMap::from([(slot, 4)]),
            BTreeMap::from([(ScratchId(0), scratch(4, 4, 4, ScratchLifetime::Spanning))]),
            8
        )
        .is_err()
    );
    assert!(
        FinalFrameLayout::new(
            source.clone(),
            BTreeMap::new(),
            BTreeMap::from([
                (ScratchId(0), scratch(4, 4, 4, ScratchLifetime::Spanning)),
                (ScratchId(1), scratch(4, 4, 4, ScratchLifetime::Spanning))
            ]),
            8
        )
        .is_err()
    );
    // Distinct unit-local reservations deliberately overlay the reusable workspace.
    let frame = FinalFrameLayout::new(
        source,
        BTreeMap::new(),
        BTreeMap::from([
            (ScratchId(0), scratch(4, 4, 4, ScratchLifetime::Unit(SelectedStepId(0)))),
            (ScratchId(1), scratch(4, 4, 4, ScratchLifetime::Unit(SelectedStepId(1)))),
        ]),
        8,
    )
    .unwrap();
    assert_eq!(
        frame
            .resolve(FrameRef::Scratch {
                object: ScratchId(1),
                offset: 3
            })
            .unwrap(),
        7
    );
    assert!(
        frame
            .resolve(FrameRef::Scratch {
                object: ScratchId(1),
                offset: 4
            })
            .is_err()
    );
}

#[test]
fn binding_domains_and_registered_definitions_fail_closed() {
    let unit = SelectedLowering::fixture(LoweringKind(0), SourceStepId(1));
    let frame =
        FinalFrameLayout::new(SourceLocalLayout::default(), BTreeMap::new(), BTreeMap::new(), 0)
            .unwrap();
    let mut registry = LoweringRegistry::default();
    assert!(registry.get(unit.kind).is_err());
    registry.register(&StoredInputLoads).unwrap();
    assert!(registry.register(&StoredInputLoads).is_err());
    assert!(
        registry
            .get(unit.kind)
            .unwrap()
            .expand(&unit, &LoweringBindings::default(), &frame, &ExpansionEnv::default())
            .is_ok()
    );
    let mut extra = LoweringBindings::default();
    extra
        .stored_inputs
        .insert(StoredInputId(99), SpillSlotId::canonical(Atom::new(ValueId(7), 0)));
    assert!(extra.validate(&unit, &frame).is_err());
}
