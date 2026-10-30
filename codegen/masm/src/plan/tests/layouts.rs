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
    assert!(ValueLayout::new(Type::from(ArrayType::new(Type::U8, 0))).is_err());
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
        input_order: crate::plan::InputOrder::Canonical,
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
        _: &ExpansionEnv,
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
    assert_eq!(
        rendered,
        "IllegalStackAccess: indexed access exceeds target window step=s4 depth=16 expected=v8.1 \
         actual=v2.0"
    );
    let plan = ProcedurePlan::fixture(vec![PlanAction::LoadLane(forward[0]), PlanAction::Drop]);
    assert_eq!(
        plan.to_string(),
        "procedure ProcedureAbi { convention: SameContext, arguments: 0, results: 0 }\nsegment \
         b0\n  operand-traffic loads=1 stores=0\n  load v8.1\n  drop\n"
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

#[test]
fn resolving_scratch_preserves_stored_bindings_and_validates_the_final_assignment() {
    let mut unit = SelectedLowering::fixture(LoweringKind(0), SourceStepId(1));
    unit.scratch = vec![ScratchRequirement {
        id: ScratchId(3),
        size: 4,
        alignment: 4,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Unit(unit.id),
        uses: vec![unit.id],
    }];
    let slot = SpillSlotId::canonical(Atom::new(ValueId(7), 0));
    unit.stored_inputs = vec![StoredInput {
        id: StoredInputId(9),
        value: StoredValue::Literal(miden_core::Felt::ONE),
    }];
    let binding = LoweringBindings {
        input_order: crate::plan::InputOrder::Reversed,
        stored_inputs: BTreeMap::from([(StoredInputId(9), slot)]),
        scratch: BTreeMap::new(),
    };
    let assigned = ScratchAllocation {
        offset: 4,
        size: 4,
        alignment: 4,
        lifetime: ScratchLifetime::Unit(unit.id),
    };
    let frame = FinalFrameLayout::new(
        SourceLocalLayout::default(),
        BTreeMap::from([(slot, 0)]),
        BTreeMap::from([(ScratchId(3), assigned)]),
        8,
    )
    .unwrap();
    let resolved = binding.resolve_scratch(&unit, &frame).unwrap();
    assert_eq!(resolved.stored_inputs, binding.stored_inputs);
    assert_eq!(resolved.input_order, crate::plan::InputOrder::Reversed);
    assert!(binding.scratch.is_empty());
    assert_eq!(resolved.scratch, BTreeMap::from([(ScratchId(3), assigned)]));
    assert!(binding.validate(&unit, &frame).is_err());
    unit.scratch[0].size = 5;
    assert!(binding.resolve_scratch(&unit, &frame).is_err());
}

#[test]
fn oversized_aggregate_layout_is_rejected_before_raw_decomposition() {
    // Upstream splitting of a wide member followed by more fields can underflow while
    // calculating remaining offsets. None of that decomposition is needed for a >4-lane value.
    let ty = Type::from(midenc_hir::StructType::new([Type::U128, Type::U64, Type::U32]));
    assert!(ValueLayout::new(ty).unwrap_err().to_string().contains("one to four"));
}

#[test]
fn extreme_array_layout_is_rejected_without_size_overflow() {
    let ty = Type::from(midenc_hir::ArrayType::new(Type::U128, usize::MAX));
    assert!(ValueLayout::new(ty).unwrap_err().to_string().contains("one to four"));
}

#[test]
fn small_aggregate_leading_wide_field_preserves_four_lanes() {
    let ty = Type::from(midenc_hir::StructType::new([Type::U64, Type::U32, Type::U32]));
    let byte_limb = Type::from(ArrayType::new(Type::U8, 4));
    assert_eq!(
        ValueLayout::new(ty).unwrap().raw_parts(),
        &[byte_limb.clone(), byte_limb, Type::U32, Type::U32]
    );
}

#[test]
fn small_aggregate_two_wide_fields_preserve_four_lanes() {
    let ty = Type::from(midenc_hir::StructType::new([Type::U64, Type::U64]));
    let byte_limb = Type::from(ArrayType::new(Type::U8, 4));
    assert_eq!(ValueLayout::new(ty).unwrap().raw_parts(), &vec![byte_limb; 4]);
}

#[test]
fn aggregate_adapter_preserves_unaffected_raw_types() {
    use midenc_hir::{PointerType, StructType, TypeRepr};
    let pointer = Type::from(PointerType::new(Type::U8));
    for ty in [
        Type::from(ArrayType::new(Type::U8, 9)),
        Type::from(ArrayType::new(Type::U16, 5)),
        Type::from(ArrayType::new(Type::U32, 3)),
        Type::from(StructType::new([pointer.clone(), Type::U8, Type::I32])),
        Type::from(StructType::new([Type::U16, Type::U16, Type::U32])),
        Type::from(StructType::new_with_repr(TypeRepr::packed(1), [pointer, Type::U8, Type::I32])),
    ] {
        let expected = ty.clone().to_raw_parts().unwrap();
        assert_eq!(ValueLayout::new(ty.clone()).unwrap().raw_parts(), expected.as_slice(), "{ty}");
    }
}

#[test]
fn aggregate_adapter_projects_packed_padded_and_nested_fields() {
    use midenc_hir::{StructType, TypeRepr};
    let bytes = |count| Type::from(ArrayType::new(Type::U8, count));
    let packed =
        |fields: Vec<Type>| Type::from(StructType::new_with_repr(TypeRepr::packed(1), fields));
    for (ty, expected) in [
        (
            packed(vec![Type::U8, Type::U64, Type::U8]),
            vec![packed(vec![Type::U8, bytes(3)]), bytes(4), packed(vec![Type::U8, Type::U8])],
        ),
        (
            Type::from(StructType::new([Type::U8, Type::U64, Type::U8])),
            vec![Type::U8, bytes(4), bytes(4), Type::U8],
        ),
        (
            Type::from(StructType::new([Type::U8, Type::U32, Type::U32])),
            vec![Type::U8, Type::U32, Type::U32],
        ),
        (
            Type::from(StructType::new([
                Type::U32,
                Type::from(StructType::new([Type::U64, Type::U32])),
            ])),
            vec![Type::U32, bytes(4), bytes(4), Type::U32],
        ),
        (
            Type::from(StructType::new_with_repr(TypeRepr::align(16), [Type::U64, Type::U8])),
            vec![bytes(4), bytes(4), Type::U8],
        ),
        (
            Type::from(StructType::new([
                Type::U8,
                Type::from(StructType::new_with_repr(TypeRepr::align(8), [Type::U32])),
            ])),
            vec![Type::U8, bytes(4), Type::U32],
        ),
    ] {
        assert_eq!(ValueLayout::new(ty.clone()).unwrap().raw_parts(), &expected, "{ty}");
    }
}

#[test]
fn aggregate_adapter_array_crossings_keep_all_five_elements_in_order() {
    use midenc_hir::{StructType, TypeRepr};
    let packed =
        |fields: Vec<Type>| Type::from(StructType::new_with_repr(TypeRepr::packed(1), fields));
    let element = packed(vec![Type::U8, Type::U16]);
    let ty = Type::from(ArrayType::new(element.clone(), 5));
    let expected = vec![
        packed(vec![element.clone(), Type::U8]),
        packed(vec![Type::U16, packed(vec![Type::U8, Type::U8])]),
        packed(vec![Type::U8, element.clone()]),
        element,
    ];
    assert_eq!(ValueLayout::new(ty).unwrap().raw_parts(), &expected);
    let wide_array = Type::from(ArrayType::new(Type::U64, 2));
    assert_eq!(
        ValueLayout::new(wide_array).unwrap().raw_parts(),
        &vec![Type::from(ArrayType::new(Type::U8, 4)); 4]
    );
}

#[test]
fn aggregate_adapter_nested_fragment_fits_its_projected_interval() {
    use midenc_hir::{StructType, TypeRepr};
    let packed =
        |fields: Vec<Type>| Type::from(StructType::new_with_repr(TypeRepr::packed(1), fields));
    let inner = Type::from(StructType::new([Type::U16, Type::U8, Type::U32]));
    let ty = packed(vec![Type::U8, inner]);
    assert_eq!(ty.size_in_bytes(), 9);
    let layout = ValueLayout::new(ty).unwrap();
    assert_eq!(
        layout.raw_parts(),
        &[
            packed(vec![Type::U8, packed(vec![Type::U16, Type::U8])]),
            // Original byte four is inner padding; bytes five through seven begin the u32.
            packed(vec![Type::U8, Type::from(ArrayType::new(Type::U8, 3))]),
            Type::U8,
        ]
    );
    assert_eq!(
        layout.raw_parts().iter().map(Type::size_in_bytes).collect::<Vec<_>>(),
        [4, 4, 1]
    );
}

#[test]
fn aggregate_adapter_bounded_recursive_corpus_keeps_every_lane_nonempty_and_bounded() {
    use midenc_hir::{StructType, TypeRepr};
    let mut generation = vec![Type::U8, Type::U16, Type::U32, Type::U64, Type::U128];
    let mut checked = 0;
    for depth in 0..3 {
        let mut next = Vec::new();
        for ty in generation {
            let layout = ValueLayout::new(ty.clone()).unwrap();
            assert!((1..=4).contains(&layout.lanes()), "{ty:?}");
            assert!(
                layout.raw_parts().iter().all(|part| (1..=4).contains(&part.size_in_bytes())),
                "{ty:?}: {:?}",
                layout.raw_parts()
            );
            checked += 1;
            if depth == 2 {
                continue;
            }
            for scalar in [Type::U8, Type::U16, Type::U32] {
                for fields in [[scalar.clone(), ty.clone()], [ty.clone(), scalar]] {
                    for repr in [TypeRepr::Default, TypeRepr::packed(1), TypeRepr::align(8)] {
                        let candidate = Type::from(StructType::new_with_repr(repr, fields.clone()));
                        if candidate.size_in_bytes() <= 16 {
                            next.push(candidate);
                        }
                    }
                }
            }
            for count in [2, 3, 5] {
                let candidate = Type::from(ArrayType::new(ty.clone(), count));
                if candidate.size_in_bytes() <= 16 {
                    next.push(candidate);
                }
            }
        }
        generation = next;
    }
    assert!(checked > 1000, "recursive corpus unexpectedly shrank to {checked} cases");
}
