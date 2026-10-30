use super::*;

/// Reproduce the evaluator's native16/count3 example through the production emitter.
#[test]
fn native_u16_matches_evaluator() {
    setup::enable_compiler_instrumentation();
    for (base, count, initial, expected) in [
        (16, 3, vec![0, 0], vec![0xabcd_abcd, 0xabcd]),
        (16, 2, vec![0, (1u64 << 40) + 7], vec![0xabcd_abcd, (1u64 << 40) + 7]),
        (u32::MAX, 2, vec![0], vec![0xabcd_abcd]),
    ] {
        let ptr = element_ptr(Type::U16);
        let (package, context) = compile_test_module([ptr, Type::U32], [Type::U32], |builder| {
            let block = builder.current_block();
            let args = block.borrow();
            let dst = args.arguments()[0] as ValueRef;
            let count = args.arguments()[1] as ValueRef;
            drop(args);
            let span = SourceSpan::default();
            let value = builder.u16(0xabcd, span);
            builder.memset(dst, count, value, span).unwrap();
            builder.ret(Some(count), span).unwrap();
        });
        let init_base = base & !3;
        let initial = (0..base - init_base)
            .map(|_| Felt::ZERO)
            .chain(initial.into_iter().map(Felt::new_unchecked))
            .collect::<Vec<_>>();
        let result = eval_package::<u32, _, _>(
            package,
            [Initializer::MemoryFelts {
                addr: init_base,
                felts: initial.as_slice().into(),
            }],
            &[Felt::from(base), Felt::from(count)],
            context.session(),
            |trace| {
                for (i, value) in expected.iter().enumerate() {
                    prop_assert_eq!(
                        trace.read_memory_element(base + i as u32),
                        Some(Felt::new_unchecked(*value))
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, count);
    }
}

/// A `hir.mem_set` of five values through an element-address-space pointer writes them as
/// contiguous little-endian bytes from the start of the first cell, and a felt value as a full
/// cell each.
#[test]
fn native_fill_uses_contiguous_bytes() {
    setup::enable_compiler_instrumentation();
    let base = 0xc000_0010;
    for (value, expected) in [
        (Immediate::I1(true), vec![0x0101_0101, 1]),
        (Immediate::U8(0xab), vec![0xabab_abab, 0xab]),
        (Immediate::U16(0xabcd), vec![0xabcd_abcd, 0xabcd_abcd, 0xabcd]),
        (Immediate::U32(0x1234_5678), vec![0x1234_5678; 5]),
        (Immediate::U64(0x0123_4567_89ab_cdef), [0x89ab_cdef, 0x0123_4567].repeat(5)),
        (
            Immediate::U128(0x7654_3210_fedc_ba98_0123_4567_89ab_cdef),
            [0x89ab_cdef, 0x0123_4567, 0xfedc_ba98, 0x7654_3210].repeat(5),
        ),
        (
            Immediate::Felt(Felt::new_unchecked((1u64 << 40) + 7)),
            vec![(1u64 << 40) + 7; 5],
        ),
    ] {
        let ptr = element_ptr(value.ty());
        let (package, context) = compile_test_module([ptr], [Type::U32], |builder| {
            let ptr = builder.current_block().borrow().arguments()[0] as ValueRef;
            let span = SourceSpan::default();
            let value = builder.imm(value, span);
            let count = builder.u32(5, span);
            builder.memset(ptr, count, value, span).unwrap();
            builder.ret(Some(count), span).unwrap();
        });
        eval_package::<u32, _, _>(package, [], &[Felt::from(base)], context.session(), |trace| {
            for (i, value) in expected.iter().enumerate() {
                prop_assert_eq!(
                    trace.read_memory_element(base + i as u32),
                    Some(Felt::new_unchecked(*value))
                );
            }
            Ok(())
        })
        .unwrap();
    }
}

/// LocalAddress produces high native addresses; adjacent full-field locals stay untouched.
#[test]
fn local_fill_preserves_full_field_sentinel() {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_test_module([], [Type::Felt], |builder| {
        let span = SourceSpan::default();
        let local = builder.alloc_local(Type::U32);
        let neighbor = builder.alloc_local(Type::Felt);
        let ptr = builder.local_address(local, span).unwrap();
        let sentinel = builder.local_address(neighbor, span).unwrap();
        let high = builder.imm(Immediate::Felt(Felt::new_unchecked((1u64 << 40) + 7)), span);
        builder.store(sentinel, high, span).unwrap();
        let zero = builder.u32(0, span);
        builder.store(ptr, zero, span).unwrap();
        let whole_ptr = ptr;
        let ptr = builder.bitcast(ptr, element_ptr(Type::U16), span).unwrap();
        let value = builder.u16(0xabcd, span);
        let count = builder.u32(2, span);
        builder.memset(ptr, count, value, span).unwrap();
        let loaded = builder.load(whole_ptr, span).unwrap();
        let expected = builder.u32(0xabcd_abcd, span);
        builder.assert_eq(loaded, expected, span).unwrap();
        let result = builder.load(sentinel, span).unwrap();
        builder.ret(Some(result), span).unwrap();
    });
    let result =
        eval_package::<Felt, _, _>(package, [], &[], context.session(), |_| Ok(())).unwrap();
    assert_eq!(result, Felt::new_unchecked((1u64 << 40) + 7));
}

/// Packed parameters are existing one-lane source values, not scalar-evaluator stand-ins.
#[test]
fn packed_value_parameters_preserve_existing_domain() {
    use midenc_hir::{ArrayType, CallConv, EnumType, FunctionType, StructType, TypeRepr, Variant};
    let types = [
        (Type::from(ArrayType::new(Type::U32, 1)), 1),
        (Type::from(StructType::new_with_repr(TypeRepr::Transparent, [Type::U32])), 1),
        (
            Type::from(
                EnumType::new(
                    "PackedTag".into(),
                    Type::U32,
                    [Variant::c_like("One".into(), Some(1))],
                )
                .unwrap(),
            ),
            1,
        ),
        (Type::from(FunctionType::new(CallConv::Fast, [], [])), 1),
        (Type::from(ArrayType::new(Type::Felt, 1)), (1u64 << 40) + 7),
        (
            Type::from(StructType::new_with_repr(TypeRepr::Transparent, [Type::Felt])),
            (1u64 << 40) + 7,
        ),
    ];
    for (ty, value) in types {
        let ptr = element_ptr(ty.clone());
        let (package, context) = compile_test_module([ptr, ty.clone()], [Type::U32], |builder| {
            let block = builder.current_block();
            let args = block.borrow();
            let ptr = args.arguments()[0] as ValueRef;
            let value = args.arguments()[1] as ValueRef;
            drop(args);
            let span = SourceSpan::default();
            let count = builder.u32(2, span);
            builder.memset(ptr, count, value, span).unwrap();
            builder.ret(Some(count), span).unwrap();
        });
        let base = 0xc000_0010;
        let result = eval_package::<u32, _, _>(
            package,
            [],
            &[Felt::from(base), Felt::new_unchecked(value)],
            context.session(),
            |trace| {
                for addr in [base, base + 1] {
                    prop_assert_eq!(
                        trace.read_memory_element(addr),
                        Some(Felt::new_unchecked(value))
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, 2, "{ty}");
    }
}

#[test]
fn packed_fills_use_declared_bits_and_byte_stride() {
    use midenc_hir::{ArrayType, StructType, TypeRepr};
    let types = (1..=4)
        .map(|n| Type::from(ArrayType::new(Type::U8, n)))
        .chain((1..=4).map(|n| Type::from(ArrayType::new(Type::I1, n))))
        .chain([
            Type::from(StructType::new_with_repr(TypeRepr::packed(1), [Type::U8, Type::U16])),
            Type::from(ArrayType::new(Type::Felt, 1)),
        ]);
    for ty in types {
        let bits = ty.size_in_bits();
        let size = ty.size_in_bytes();
        let value = (1u64 << bits) - 1;
        for space in [AddressSpace::Byte, AddressSpace::Element] {
            let ptr = Type::from(PointerType::new_with_address_space(ty.clone(), space));
            let (package, context) =
                compile_test_module([ptr, ty.clone(), Type::U32], [Type::U32], |builder| {
                    let block = builder.current_block();
                    let args = block.borrow();
                    let ptr = args.arguments()[0] as ValueRef;
                    let value = args.arguments()[1] as ValueRef;
                    let count = args.arguments()[2] as ValueRef;
                    drop(args);
                    let span = SourceSpan::default();
                    builder.memset(ptr, count, value, span).unwrap();
                    builder.ret(Some(count), span).unwrap();
                });
            let base = if space == AddressSpace::Byte {
                16
            } else {
                0xc000_0010
            };
            for offset in 0..if space == AddressSpace::Byte { 4 } else { 1 } {
                for count in [0, 1, 3] {
                    let initial = [Felt::from(0x1212_1212u32); 4];
                    let mut expected = [0x12u8; 16];
                    for i in 0..count {
                        for bit in 0..bits {
                            let position = 8 * (offset + i * size) + bit;
                            expected[position / 8] |= 1 << (position % 8);
                        }
                    }
                    let ptr = if space == AddressSpace::Byte {
                        base * 4 + offset as u32
                    } else {
                        base
                    };
                    let result = eval_package::<u32, _, _>(
                        package.clone(),
                        [Initializer::MemoryFelts {
                            addr: base,
                            felts: initial.as_slice().into(),
                        }],
                        &[Felt::from(ptr), Felt::new_unchecked(value), Felt::from(count as u32)],
                        context.session(),
                        |trace| {
                            for (i, bytes) in expected.as_chunks::<4>().0.iter().enumerate() {
                                prop_assert_eq!(
                                    trace.read_memory_element(base + i as u32),
                                    Some(Felt::from(u32::from_le_bytes(*bytes)))
                                );
                            }
                            Ok(())
                        },
                    )
                    .unwrap();
                    assert_eq!(result, count as u32, "{ty}, {space}, offset={offset}");
                }
            }
        }
    }
}
