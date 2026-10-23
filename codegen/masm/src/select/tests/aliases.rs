use super::*;
#[test]
fn real_hir_aliases_preserve_distinct_limb_order() {
    for (input, limb, count) in [("u64", "u32", 2), ("u128", "u64", 2), ("u128", "u32", 4)] {
        let split_types = vec![limb; count].join(", ");
        let limbs = (0..count).map(|i| alloc::format!("%v{i}")).collect::<Vec<_>>().join(", ");
        let text = alloc::format!(
            "builtin.function public extern(\"C\") @aliases(%input: {input}) -> {input} {{ \
             {limbs} = arith.split %input : ({input}) -> ({split_types}); %output = arith.join \
             {limbs} <{{ ty = #builtin.type<{input}> }}>; builtin.ret %output : ({input}); }};"
        );
        let f = parse(&text);
        let selected = checked(&f, &SelectEnv::default());
        assert_eq!(selected.segments[0].units.len(), 2);
        let lanes = if input == "u64" { 2 } else { 4 };
        let values = (1..=lanes).map(|i| 100 + i).collect::<Vec<_>>();
        execute(&f, &SelectEnv::default(), &values, &values);
        // Returning just the first named limb must select the high bits.
        let text = alloc::format!(
            "builtin.function public extern(\"C\") @high(%input: {input}) -> {limb} {{ {limbs} = \
             arith.split %input : ({input}) -> ({split_types}); builtin.ret %v0 : ({limb}); }};"
        );
        let f = parse(&text);
        let width = if limb == "u64" { 2 } else { 1 };
        execute(&f, &SelectEnv::default(), &values, &values[values.len() - width..]);
    }
    let f = parse(include_str!("fixtures/aliases.hir"));
    execute(&f, &SelectEnv::default(), &[1, 2, 3, 4], &[1, 2, 3, 4]);
}
#[test]
fn transparent_casts_have_checked_alias_dependencies() {
    let f = parse(include_str!("fixtures/transparent-casts.hir"));
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units.len(), 3);
    for unit in &selected.segments[0].units {
        assert_eq!(unit.semantic_inputs.len(), 1);
        assert!(
            unit.stack_inputs.is_empty(),
            "transparent aliases do not request a physical prefix"
        );
    }
    execute(&f, &SelectEnv::default(), &[123], &[123]);
    let function = f.borrow();
    let env = SelectEnv::default();
    let mut bad = select_function(&function, &env).unwrap();
    bad.segments[0].units[0].semantic_inputs.clear();
    assert!(
        check_selection(
            bad,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
}

#[test]
fn malformed_alias_groupings_and_candidate_lane_maps_are_rejected() {
    use midenc_hir::{Type, Value};
    for operation_index in [0, 1] {
        let f = parse(include_str!("fixtures/aliases.hir"));
        let op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .iter()
            .nth(operation_index)
            .unwrap()
            .as_operation_ref();
        let mut result = op.borrow().results().all()[0];
        result.borrow_mut().set_type(Type::U64);
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
    let f = parse(include_str!("fixtures/aliases.hir"));
    let function = f.borrow();
    let env = SelectEnv::default();
    let mut selected = select_function(&function, &env).unwrap();
    let aliases = selected
        .values
        .values_mut()
        .find_map(|v| match &mut v.reconstruction {
            Some(Reconstruction::Alias(lanes)) if lanes.len() > 1 => Some(lanes),
            _ => None,
        })
        .unwrap();
    aliases.reverse();
    assert!(
        check_selection(
            selected,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
}

#[test]
fn aliases_require_exact_declared_types_groupings_and_properties() {
    use midenc_hir::{Type, Value, dialects::builtin::attributes::TypeAttr};
    let split = "builtin.function public extern(\"C\") @split(%a: u64) { %hi,%lo=arith.split %a : \
                 (u64) -> (u32,u32); builtin.ret; };";
    let join = "builtin.function public extern(\"C\") @join(%a: u32,%b: u32,%c: u32,%d: u32) { \
                %unused=arith.join %a,%b,%c,%d <{ ty=#builtin.type<u128> }>; builtin.ret; };";
    for (text, ty) in [(split, Type::U16), (join, Type::I128)] {
        let f = parse(text);
        let op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let mut result = op.borrow().results().all()[0];
        result.borrow_mut().set_type(ty.clone());
        assert_malformed_source_rejected(&f, &SelectEnv::default());
        assert_eq!(result.borrow().ty(), &ty);
    }
    for (text, property, ty) in [(split, "limb_ty", Type::U16), (join, "ty", Type::I128)] {
        let f = parse(text);
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let attr = f.context.create_attribute::<TypeAttr, _>(ty);
        op.borrow_mut().set_property(property, attr.as_attribute_ref()).unwrap();
        if property == "limb_ty" {
            // Agreement alone is insufficient: a U64 split into two U16 limbs is invalid
            // even though every actual result agrees with its declared limb property.
            for mut result in op.borrow().results().all().iter().copied() {
                result.borrow_mut().set_type(Type::U16);
            }
        }
        assert_malformed_source_rejected(&f, &SelectEnv::default());
    }
    let f = parse(
        "builtin.function public extern(\"C\") @join(%a: u32,%small: u16) { %unused=arith.join \
         %a,%a,%a,%a <{ty=#builtin.type<u128>}>; builtin.ret; };",
    );
    let mut op = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .front()
        .get()
        .unwrap()
        .as_operation_ref();
    let small = f.borrow().entry_block().borrow().arguments()[1] as midenc_hir::ValueRef;
    op.borrow_mut().set_operands([small; 4]);
    assert_malformed_source_rejected(&f, &SelectEnv::default());
    for (name, input, target, actual) in [
        ("hir.ptr_to_int", "ptr<u32, byte>", "u32", Type::I32),
        (
            "hir.int_to_ptr",
            "u32",
            "ptr<u32, byte>",
            midenc_hir::PointerType::new(Type::I32).into(),
        ),
        ("hir.bitcast", "u32", "i32", Type::U32),
    ] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @cast(%a: {input}) {{ %unused={name} %a \
             <{{ty=#builtin.type<{target}>}}>; builtin.ret; }};"
        ));
        let op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let mut result = op.borrow().results().all()[0];
        result.borrow_mut().set_type(actual.clone());
        assert_malformed_source_rejected(&f, &SelectEnv::default());
        assert_eq!(result.borrow().ty(), &actual);
    }
}

#[test]
fn buried_transparent_alias_does_not_prepare_an_operand_prefix() {
    use crate::plan::actions::PlanAction;

    let f = parse(
        "builtin.function public extern(\"C\") @buried(%keep: felt, %source: u32) -> felt { \
         %alias = hir.bitcast %source <{ ty = #builtin.type<felt> }>; %result = arith.sub %keep, \
         %alias <{ overflow = #builtin.overflow<wrapping> }>; builtin.ret %result : (felt); };",
    );
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert!(matches!(
        plan.segment_actions[&SegmentId(0)][0],
        PlanAction::RunLowering {
            step: SelectedStepId(0)
        }
    ));
    verify::verify(&plan, &frame, body).unwrap();
    execute(&f, &SelectEnv::default(), &[100, 7], &[93]);
}

#[test]
fn aliases_below_the_addressable_window_preserve_source_and_suffix() {
    use crate::plan::actions::PlanAction;

    for width in [16, 17, 24] {
        let params = (0..width).map(|i| format!("%p{i}: u32")).collect::<Vec<_>>().join(", ");
        let text = format!(
            "builtin.function public extern(\"C\") @deep({params}) -> felt {{
            %alias = hir.bitcast %p{} <{{ ty = #builtin.type<felt> }}>;
            builtin.ret %alias : (felt); }};",
            width - 1
        );
        let f = parse(&text);
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert!(matches!(
            plan.segment_actions[&SegmentId(0)].as_slice(),
            [PlanAction::RunLowering {
                step: SelectedStepId(0)
            }]
        ));
        verify::verify(&plan, &frame, body).unwrap();
        let inputs = (0..width).map(|i| 101 + i).collect::<Vec<_>>();
        execute(&f, &SelectEnv::default(), &inputs, &[100 + width]);
    }
}

#[test]
fn alias_publications_keep_complete_lanes_across_joins_and_loop_backedges() {
    let f = parse(
        "builtin.function public extern(\"C\") @join(%p: i1, %a: u64, %b: u64) -> (u64, u64) {
        %selected = scf.if %p then {
            %hi, %lo = arith.split %a : (u64) -> (u32, u32);
            %joined = arith.join %hi, %lo <{ty=#builtin.type<u64>}>;
            scf.yield %joined : (u64);
        } else {
            %hi, %lo = arith.split %b : (u64) -> (u32, u32);
            %joined = arith.join %hi, %lo <{ty=#builtin.type<u64>}>;
            scf.yield %joined : (u64);
        } : (i1) -> (u64);
        builtin.ret %selected, %a : (u64, u64); };",
    );
    execute(&f, &SelectEnv::default(), &[1, 11, 22, 33, 44], &[11, 22, 11, 22]);
    execute(&f, &SelectEnv::default(), &[0, 11, 22, 33, 44], &[33, 44, 11, 22]);

    let f = parse(
        "builtin.function public extern(\"C\") @loop(%n: u32, %a: u64) -> u64 {
        %done, %result = scf.while %n, %a before {
            ^before(%count: u32, %value: u64):
            %zero = arith.constant 0 : u32;
            %more = arith.neq %count, %zero;
            scf.condition %more, %count, %value : (i1, u32, u64);
        } after {
            ^after(%count: u32, %value: u64):
            %hi, %lo = arith.split %value : (u64) -> (u32, u32);
            %next = arith.join %lo, %hi <{ty=#builtin.type<u64>}>;
            %one = arith.constant 1 : u32;
            %remaining = arith.sub %count, %one <{overflow=#builtin.overflow<wrapping>}>;
            scf.yield %remaining, %next : (u32, u64);
        } : (u32, u64) -> (u32, u64);
        builtin.ret %result : (u64); };",
    );
    for count in 0..4 {
        let expected = if count % 2 == 0 { [11, 22] } else { [22, 11] };
        execute(&f, &SelectEnv::default(), &[count, 11, 22], &expected);
    }
}
