use super::*;

/// Casting an element pointer to a smaller pointee still extracts only that pointee's bits.
#[test]
fn element_subword_load_extracts_low_bits() {
    setup::enable_compiler_instrumentation();

    for (ty, initial, expected) in [
        (Type::U8, 0x1234, 0x34u32),
        (Type::I8, 0x1280, 0x80),
        (Type::U16, 0x1234_5678, 0x5678),
        (Type::I16, 0x1234_8000, 0x8000),
        (Type::I1, 0x1235, 1),
    ] {
        let (package, context) =
            compile_test_module([element_ptr(Type::U32)], [ty.clone()], |builder| {
                let span = SourceSpan::default();
                let ptr = builder.current_block().borrow().arguments()[0] as ValueRef;
                let value = builder.u32(initial, span);
                builder.store(ptr, value, span).unwrap();
                let ptr = builder.bitcast(ptr, element_ptr(ty.clone()), span).unwrap();
                let loaded = builder.load(ptr, span).unwrap();
                builder.ret(Some(loaded), span).unwrap();
            });

        // Inspect the raw element so conversion to a Rust subword type cannot hide excess bits.
        let output = eval_package::<Felt, _, _>(
            package,
            [],
            &[Felt::new_unchecked(0x10_0000)],
            context.session(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(output, Felt::from(expected), "loading {ty}");
    }
}

/// A store through a cast element pointer must preserve the rest of the original u32 cell.
#[test]
fn element_subword_store_preserves_high_bits() {
    setup::enable_compiler_instrumentation();

    for (value, initial, expected) in [
        (Immediate::U8(0x56), 0x1234, 0x1256),
        (Immediate::I8(-1), 0x1234, 0x12ff),
        (Immediate::U16(0x5678), 0x1234_abcd, 0x1234_5678),
        (Immediate::I16(-1), 0x1234_abcd, 0x1234_ffff),
        (Immediate::I1(false), 0x1235, 0x1234),
    ] {
        let (package, context) =
            compile_test_module([element_ptr(Type::U32)], [Type::U32], |builder| {
                let span = SourceSpan::default();
                let ptr = builder.current_block().borrow().arguments()[0] as ValueRef;
                let initial = builder.u32(initial, span);
                builder.store(ptr, initial, span).unwrap();
                let value = builder.imm(value, span);
                let subword_ty = element_ptr(value.borrow().ty().clone());
                let subword_ptr = builder.bitcast(ptr, subword_ty, span).unwrap();
                builder.store(subword_ptr, value, span).unwrap();
                let loaded = builder.load(ptr, span).unwrap();
                builder.ret(Some(loaded), span).unwrap();
            });

        let output = eval_package::<u32, _, _>(
            package,
            [],
            &[Felt::new_unchecked(0x10_0000)],
            context.session(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(output, expected, "storing {value}");
    }
}
