use alloc::{boxed::Box, format, rc::Rc};

use midenc_hir::{
    AddressSpace, ArrayType, BuilderExt, Context, Op, OpPrinter, PointerType, SourceSpan,
    StructType, Type, Value, ValueRef,
    derive::operation,
    dialects::builtin::{BuiltinOpBuilder, Function, attributes::LocalVariableAttr},
    effects::{EffectOpInterface, MemoryEffect, MemoryEffectOpInterface},
    pass::{Nesting, PassManager},
    testing::{Test, parse_function_fixpoint},
};

use crate::{HirDialect, HirOpBuilder, transforms::Local2Reg};

#[test]
fn byte_local_addresses_infer_types_and_preserve_stores() {
    for allocate in [false, true] {
        let mut test = Test::new("byte_local", &[Type::U32], &[Type::U32]);
        let store;
        {
            let mut builder = test.function_builder();
            let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let local = builder.alloc_local(Type::U32);
            let addr = if allocate {
                let op =
                    builder.builder_mut().create::<crate::Alloca, _>(SourceSpan::UNKNOWN)(local)
                        .unwrap();
                op.borrow().result().as_value_ref()
            } else {
                builder.local_byte_address(local, SourceSpan::UNKNOWN).unwrap()
            };
            assert_eq!(addr.borrow().ty(), &Type::from(PointerType::new(Type::U32)));
            store = builder.store_local(local, input, SourceSpan::UNKNOWN).unwrap();
            // A direct load must not make this address-taken slot eligible for promotion either.
            let _direct = builder.load_local(local, SourceSpan::UNKNOWN).unwrap();
            let loaded = builder.load(addr, SourceSpan::UNKNOWN).unwrap();
            builder.ret([loaded], SourceSpan::UNKNOWN).unwrap();
        }
        test.apply_pass::<Local2Reg>(true).unwrap();
        assert!(store.as_operation_ref().parent().is_some());
    }
}

#[test]
fn alloca_builder_allocates_a_typed_local() {
    let mut test = Test::new("alloca_builder", &[], &[]);
    let mut builder = test.function_builder();
    let addr = builder.alloca(Type::U64, SourceSpan::UNKNOWN).unwrap();
    assert_eq!(addr.borrow().ty(), &Type::from(PointerType::new(Type::U64)));
    builder.ret([], SourceSpan::UNKNOWN).unwrap();
}

#[test]
fn parsed_local_address_types_are_inferred_from_attributes() {
    for (name, space) in [
        ("alloca", AddressSpace::Byte),
        ("local_byte_address", AddressSpace::Byte),
        ("local_address", AddressSpace::Element),
    ] {
        let context = Rc::new(Context::default());
        let source = format!(
            "builtin.function public extern(\"C\") @parsed() {{\n%0 = hir.{name} <{{ local = \
             #builtin.local_variable<7, u64> }}>;\nbuiltin.ret;\n}};"
        );
        let (function, _) = parse_function_fixpoint(&context, name, &source).unwrap();
        let mut saw_address = false;
        function.borrow().as_operation().postwalk_all(|op| {
            if op.name().name().as_str() == name {
                saw_address = true;
                assert_eq!(
                    op.results()[0].borrow().ty(),
                    &Type::from(PointerType::new_with_address_space(Type::U64, space))
                );
            }
        });
        assert!(saw_address);
    }
}

#[test]
fn gep_layout_and_result_types_share_byte_offsets() {
    let inner = Type::from(ArrayType::new(Type::U32, 3));
    let structure = StructType::new([Type::U8, inner]);
    let field_offset = structure.get(1).offset as usize;
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        for (pointee, indices, expected_offset, expected_type) in [
            (Type::from(ArrayType::new(Type::U32, 3)), alloc::vec![2], 8, Type::U32),
            (Type::from(ArrayType::new(Type::U128, 2)), alloc::vec![1], 16, Type::U128),
            (
                Type::from(ArrayType::new(Type::from(ArrayType::new(Type::U16, 3)), 2)),
                alloc::vec![1, 2],
                10,
                Type::U16,
            ),
            (
                Type::from(ArrayType::new(Type::from(StructType::new([Type::U32, Type::U8])), 2)),
                alloc::vec![1, 1],
                12,
                Type::U8,
            ),
            (Type::from(structure.clone()), alloc::vec![1, 2], field_offset + 8, Type::U32),
            (Type::from(structure.clone()), alloc::vec![0], 0, Type::U8),
            (Type::U64, alloc::vec![], 0, Type::U64),
        ] {
            let ptr = Type::from(PointerType::new_with_address_space(pointee, space));
            let mut test = Test::new("gep_layout", &[ptr], &[]);
            let mut builder = test.function_builder();
            let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let result = builder.getelementptr(input, &indices, SourceSpan::UNKNOWN).unwrap();
            assert_eq!(
                result.borrow().ty(),
                &Type::from(PointerType::new_with_address_space(expected_type.clone(), space,))
            );
            let op = result.borrow().get_defining_op().unwrap();
            let op = op.borrow();
            let gep = op.downcast_ref::<crate::GetElementPtr>().unwrap();
            assert_eq!(gep.get_element().unwrap(), (expected_offset, expected_type));
            assert_eq!(gep.get_offset().unwrap(), expected_offset);
        }
    }
}

#[test]
fn invalid_gep_returns_diagnostics_without_panicking() {
    for (pointee, indices) in [
        (Type::from(ArrayType::new(Type::U32, 3)), alloc::vec![3]),
        (Type::from(StructType::new([Type::U32])), alloc::vec![1]),
        (Type::U32, alloc::vec![0]),
        (Type::from(ArrayType::new(Type::U32, 3)), alloc::vec![0, 0]),
        (Type::from(ArrayType::new(Type::U32, 0)), alloc::vec![0]),
        // Array strides, index multiplication, and accumulated offsets each need checked math.
        (
            Type::from(ArrayType::new(Type::from(ArrayType::new(Type::U8, usize::MAX / 2 + 1)), 3)),
            alloc::vec![2],
        ),
        (
            Type::from(ArrayType::new(Type::from(ArrayType::new(Type::U8, usize::MAX)), 2)),
            alloc::vec![1, 1],
        ),
        (
            Type::from(ArrayType::new(Type::from(ArrayType::new(Type::U32, usize::MAX)), 2)),
            alloc::vec![1],
        ),
    ] {
        let ptr = Type::from(PointerType::new(pointee));
        let mut test = Test::new("invalid_gep", &[ptr], &[]);
        let mut builder = test.function_builder();
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        assert!(builder.getelementptr(input, &indices, SourceSpan::UNKNOWN).is_err());
    }
}

#[test]
fn alloca_declares_local_and_result_allocation_while_addresses_are_pure() {
    let mut test = Test::new("address_effects", &[], &[]);
    let mut builder = test.function_builder();
    let local = builder.alloc_local(Type::U32);
    let allocation =
        builder.builder_mut().create::<crate::Alloca, _>(SourceSpan::UNKNOWN)(local).unwrap();
    let allocation = allocation.borrow();
    let effects = allocation.effects().collect::<alloc::vec::Vec<_>>();
    assert_eq!(effects.len(), 2);
    assert!(effects.iter().all(|effect| *effect.effect() == MemoryEffect::Allocate));
    assert!(effects.iter().any(|effect| effect.attribute().is_some()));
    assert!(
        effects
            .iter()
            .any(|effect| effect.value() == Some(allocation.result().as_value_ref()))
    );
    for address in [
        builder.local_address(local, SourceSpan::UNKNOWN).unwrap(),
        builder.local_byte_address(local, SourceSpan::UNKNOWN).unwrap(),
    ] {
        let op = address.borrow().get_defining_op().unwrap();
        assert!(op.borrow().is_memory_effect_free());
        assert!(
            op.borrow()
                .as_trait::<dyn MemoryEffectOpInterface>()
                .unwrap()
                .effects()
                .next()
                .is_none()
        );
    }
}

#[derive(
    midenc_hir::derive::EffectOpInterface,
    midenc_hir::derive::OpPrinter,
    midenc_hir::derive::OpParser,
)]
#[operation(
    name = "test_local_effect",
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct TestLocalEffect {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Free))]
    local: LocalVariableAttr,
}

#[test]
fn arbitrary_attribute_memory_effects_pin_locals() {
    let mut test = Test::new("attribute_effect", &[Type::U32], &[Type::U32]);
    let store;
    let ordinary_store;
    {
        let mut builder = test.function_builder();
        let local = builder.alloc_local(Type::U32);
        let ordinary = builder.alloc_local(Type::U32);
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        store = builder.store_local(local, input, SourceSpan::UNKNOWN).unwrap();
        ordinary_store = builder.store_local(ordinary, input, SourceSpan::UNKNOWN).unwrap();
        builder.builder_mut().create::<TestLocalEffect, _>(SourceSpan::UNKNOWN)(local).unwrap();
        let loaded = builder.load_local(ordinary, SourceSpan::UNKNOWN).unwrap();
        builder.ret([loaded], SourceSpan::UNKNOWN).unwrap();
    }
    test.apply_pass::<Local2Reg>(true).unwrap();
    assert!(store.as_operation_ref().parent().is_some());
    assert!(ordinary_store.as_operation_ref().parent().is_none());
}

#[test]
fn parsed_locals_are_pinned_by_addresses_and_effects() {
    for operation in ["alloca", "local_address", "local_byte_address"] {
        let context = Rc::new(Context::default());
        let source = format!(
            "builtin.function public extern(\"C\") @parsed(%0: u32) -> u32 {{\nhir.store_local %0 \
             <{{ local = #builtin.local_variable<7, u32> }}> : (u32);\n%1 = hir.{operation} <{{ \
             local = #builtin.local_variable<7, u32> }}>;\n%2 = hir.load %1;\nbuiltin.ret %2 : \
             (u32);\n}};",
        );
        let (function, _) = parse_function_fixpoint(&context, operation, &source).unwrap();
        let mut manager = PassManager::on::<Function>(context, Nesting::Implicit);
        manager.add_pass(Box::new(Local2Reg));
        manager.enable_verifier(true);
        manager.run(function.as_operation_ref()).unwrap();
        let mut stores = 0;
        function.borrow().as_operation().postwalk_all(|op| {
            if op.downcast_ref::<crate::StoreLocal>().is_some() {
                stores += 1;
            }
        });
        assert_eq!(stores, 1, "{operation} must preserve the parsed slot");
    }
}

#[test]
fn generic_builder_rejects_alloca_beside_functions_without_allocating_locals() {
    use midenc_hir::Builder;

    for in_module in [true, false] {
        for after in [false, true] {
            let mut test = if in_module {
                Test::default().in_module("alloca_container")
            } else {
                Test::default()
            };
            test.with_function("outside_alloca", &[], &[]);
            let function = test.function();
            let locals_before = function.borrow().num_locals();
            if after {
                test.builder_mut().set_insertion_point_after(function.as_operation_ref());
            } else {
                test.builder_mut().set_insertion_point_before(function.as_operation_ref());
            }
            assert!(test.builder_mut().alloca(Type::U32, SourceSpan::UNKNOWN).is_err());
            assert_eq!(function.borrow().num_locals(), locals_before);
        }
    }
}

#[test]
fn generic_builder_allocates_inside_function_blocks_and_before_operations() {
    use midenc_hir::Builder;

    let mut test = Test::new("generic_alloca", &[], &[]);
    let mut builder = test.function_builder();
    let ptr = builder.builder_mut().alloca(Type::U32, SourceSpan::UNKNOWN).unwrap();
    assert_eq!(ptr.borrow().ty(), &Type::from(PointerType::new(Type::U32)));
    let ret = builder.ret([], SourceSpan::UNKNOWN).unwrap();
    builder.builder_mut().set_insertion_point_before(ret.as_operation_ref());
    let ptr = builder.builder_mut().alloca(Type::U8, SourceSpan::UNKNOWN).unwrap();
    assert_eq!(ptr.borrow().ty(), &Type::from(PointerType::new(Type::U8)));
}

#[test]
fn parsed_gep_infers_the_selected_pointee_and_rejects_invalid_paths() {
    let context = Rc::new(Context::default());
    let source = r#"builtin.function public extern("C") @parsed_gep(%0: ptr<u32, byte>) {
        %1 = hir.gep %0 <{ indices = #builtin.array_u32<> }>;
        builtin.ret;
    };"#;
    let (function, _) = parse_function_fixpoint(&context, "parsed_gep", source).unwrap();
    let mut saw_gep = false;
    function.borrow().as_operation().postwalk_all(|op| {
        if let Some(gep) = op.downcast_ref::<crate::GetElementPtr>() {
            saw_gep = true;
            assert_eq!(gep.get_element().unwrap(), (0, Type::U32));
            assert_eq!(gep.result().ty(), &Type::from(PointerType::new(Type::U32)));
        }
    });
    assert!(saw_gep);

    let invalid = source.replace("array_u32<>", "array_u32<0>");
    let config = midenc_hir::parse::ParserConfig::new(context);
    let error = midenc_hir::parse::parse::<Function>(
        config,
        midenc_hir::diagnostics::Uri::new("invalid_gep"),
        &invalid,
    )
    .err()
    .expect("invalid GEP is rejected");
    assert!(format!("{error:?}").contains("cannot index non-aggregate"));

    for indices in ["4294967296", "-1", "0,", "0 1"] {
        let malformed = source.replace("array_u32<>", &format!("array_u32<{indices}>"));
        let context = Rc::new(Context::default());
        let config = midenc_hir::parse::ParserConfig::new(context);
        assert!(
            midenc_hir::parse::parse::<Function>(
                config,
                midenc_hir::diagnostics::Uri::new("malformed_gep"),
                &malformed,
            )
            .is_err()
        );
    }
}

#[test]
fn gep_rejects_nonpointer_operands_and_indices_outside_u32() {
    let mut test = Test::new("gep_invalid_inputs", &[Type::U32], &[]);
    let mut builder = test.function_builder();
    let scalar = builder.entry_block().borrow().arguments()[0] as ValueRef;
    assert!(builder.getelementptr(scalar, &[], SourceSpan::UNKNOWN).is_err());
    let ptr = builder.alloca(Type::U32, SourceSpan::UNKNOWN).unwrap();
    assert!(builder.getelementptr(ptr, &[usize::MAX], SourceSpan::UNKNOWN).is_err());
}

#[test]
fn parsed_ordinary_locals_still_promote_without_function_local_table_entries() {
    let context = Rc::new(Context::default());
    let source = r#"builtin.function public extern("C") @parsed_ordinary(%0: u32) -> u32 {
        hir.store_local %0 <{ local = #builtin.local_variable<7, u32> }> : (u32);
        %1 = hir.load_local <{ local = #builtin.local_variable<7, u32> }>;
        builtin.ret %1 : (u32);
    };"#;
    let (function, _) = parse_function_fixpoint(&context, "parsed_ordinary", source).unwrap();
    let mut manager = PassManager::on::<Function>(context, Nesting::Implicit);
    manager.add_pass(Box::new(Local2Reg));
    manager.enable_verifier(true);
    manager.run(function.as_operation_ref()).unwrap();
    function.borrow().as_operation().postwalk_all(|op| {
        assert!(op.downcast_ref::<crate::LoadLocal>().is_none());
        assert!(op.downcast_ref::<crate::StoreLocal>().is_none());
    });
}

#[test]
fn nested_structured_control_flow_preserves_addressed_local_stores() {
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;

    let mut test = Test::new("nested_addresses", &[Type::U32, Type::I1], &[]);
    let store;
    {
        let mut builder = test.function_builder();
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let condition = builder.entry_block().borrow().arguments()[1] as ValueRef;
        let local = builder.alloc_local(Type::U32);
        store = builder.store_local(local, input, SourceSpan::UNKNOWN).unwrap();
        let entry = builder.current_block();
        let loop_op = builder.r#while([], &[], SourceSpan::UNKNOWN).unwrap();
        let before = loop_op.borrow().before().entry().as_block_ref();
        let after = loop_op.borrow().after().entry().as_block_ref();
        builder.switch_to_block(before);
        builder.condition(condition, [], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(after);
        let if_op = builder.r#if(condition, &[], SourceSpan::UNKNOWN).unwrap();
        let then_region = if_op.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let addr = builder.local_byte_address(local, SourceSpan::UNKNOWN).unwrap();
        builder.load(addr, SourceSpan::UNKNOWN).unwrap();
        builder.r#yield([], SourceSpan::UNKNOWN).unwrap();
        let else_region = if_op.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let addr = builder.local_address(local, SourceSpan::UNKNOWN).unwrap();
        builder.load(addr, SourceSpan::UNKNOWN).unwrap();
        builder.r#yield([], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(after);
        builder.r#yield([], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(entry);
        builder.ret([], SourceSpan::UNKNOWN).unwrap();
    }
    test.apply_pass::<Local2Reg>(true).unwrap();
    assert!(store.as_operation_ref().parent().is_some());
}
