use midenc_hir::{
    AddressSpace, ArrayType, PointerType, SourceSpan, StructType, Type, TypeRepr,
    dialects::builtin::BuiltinOpBuilder, testing::Test,
};

use super::*;

fn access(ty: Type, space: AddressSpace, store: bool) -> Parsed {
    use crate::hir::HirOpBuilder;
    let mut inputs = vec![Type::from(PointerType::new_with_address_space(ty.clone(), space))];
    if store {
        inputs.push(ty.clone());
    }
    let outputs = if store { vec![] } else { vec![ty] };
    let mut test = Test::new("aggregate_pointer", &inputs, &outputs);
    {
        let mut builder = test.function_builder();
        let args = builder
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<Vec<_>>();
        if store {
            builder.store(args[0], args[1], SourceSpan::UNKNOWN).unwrap();
            builder.ret([], SourceSpan::UNKNOWN).unwrap();
        } else {
            let value = builder.load(args[0], SourceSpan::UNKNOWN).unwrap();
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn aggregate_pointer_partial_lanes_match_local_layout() {
    let ty = Type::from(StructType::new([Type::U8, Type::U32, Type::U8]));
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        let addr = if space == AddressSpace::Byte { 256 } else { 64 };
        execute_linked_memory(
            &access(ty.clone(), space, false),
            &[addr],
            &[0xa5, 0xa5a5a5a5, 0xa5],
            "push.2779096485 mem_store.64 push.2779096485 mem_store.65 push.2779096485 \
             mem_store.66",
            "",
        )
        .unwrap();
        execute_linked_memory(
            &access(ty.clone(), space, true),
            &[addr, 0x100000012, 0x34567890, 0x100000056],
            &[],
            "push.2779096485 mem_store.64 push.2779096485 mem_store.65 push.2779096485 \
             mem_store.66",
            "mem_load.64 push.2779096338 assert_eq mem_load.65 push.878082192 assert_eq \
             mem_load.66 push.2779096406 assert_eq",
        )
        .unwrap();
    }
}

#[test]
fn aggregate_pointer_packed_tails_cross_cells() {
    for len in [3, 7] {
        let ty = Type::from(ArrayType::new(Type::U8, len));
        for offset in 0..4 {
            let expected = if len == 3 {
                vec![0xa5a5a5]
            } else {
                vec![0xa5a5a5a5, 0xa5a5a5]
            };
            let initialize = "push.2779096485 mem_store.64 push.2779096485 mem_store.65 \
                              push.2779096485 mem_store.66";
            execute_linked_memory(
                &access(ty.clone(), AddressSpace::Byte, false),
                &[256 + offset],
                &expected,
                initialize,
                "",
            )
            .unwrap();
            let mut bytes = [0xa5u8; 12];
            bytes[offset..offset + len].fill(0);
            let observe = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .map(|(i, chunk)| {
                    alloc::format!(
                        "mem_load.{} push.{} assert_eq",
                        64 + i,
                        u32::from_le_bytes(*chunk)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let mut inputs = vec![256 + offset];
            inputs.extend(vec![0; expected.len()]);
            execute_linked_memory(
                &access(ty.clone(), AddressSpace::Byte, true),
                &inputs,
                &[],
                initialize,
                &observe,
            )
            .unwrap();
        }
    }
}

#[test]
fn aggregate_pointer_preserves_felt_and_rejects_split_layouts() {
    let ty = Type::from(StructType::new([Type::Felt, Type::U8]));
    let high = (1usize << 40) + 7;
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        let addr = if space == AddressSpace::Byte { 256 } else { 64 };
        for store in [false, true] {
            let f = access(ty.clone(), space, store);
            let inputs = if store {
                vec![addr, high, 0x12]
            } else {
                vec![addr]
            };
            let expected = if store { vec![] } else { vec![high, 0xa5] };
            execute_linked_memory(
                &f,
                &inputs,
                &expected,
                "push.1099511627783 mem_store.64 push.2779096485 mem_store.65",
                if store {
                    "mem_load.64 push.1099511627783 assert_eq mem_load.65 push.2779096338 assert_eq"
                } else {
                    ""
                },
            )
            .unwrap();
        }
    }
    let split = Type::from(StructType::new_with_repr(TypeRepr::packed(1), [Type::U8, Type::Felt]));
    for store in [false, true] {
        let f = access(split.clone(), AddressSpace::Byte, store);
        let error = select_function(&f.borrow(), &SelectEnv::default())
            .expect_err("split Felt must be rejected");
        assert!(error.to_string().contains("Felt"), "{error}");
    }
}

#[test]
fn aggregate_pointer_integer_lane_matrix_preserves_padding() {
    let cases = [
        (Type::from(ArrayType::new(Type::I1, 4)), vec![25]),
        (Type::from(ArrayType::new(Type::I1, 9)), vec![25, 25, 1]),
        (Type::from(StructType::new([Type::U8, Type::U32, Type::U8])), vec![8, 32, 8]),
        (
            Type::from(StructType::new_with_repr(
                TypeRepr::packed(1),
                [Type::U8, Type::U64, Type::U8],
            )),
            vec![32, 32, 16],
        ),
        (
            Type::from(StructType::new_with_repr(TypeRepr::align(16), [Type::U64, Type::U8])),
            vec![32, 32, 8],
        ),
        (Type::from(ArrayType::new(Type::U32, 4)), vec![32, 32, 32, 32]),
    ];
    // Explicit lane widths above describe the expected representation independently of ValueLayout.
    let cells = [0x11223344u32, 0x55667788, 0x99aabbcc, 0xddeeff00, 0x12345678, 0x9abcdef0];
    for (ty, widths) in cases {
        for space in [AddressSpace::Byte, AddressSpace::Element] {
            for offset in 0..if space == AddressSpace::Byte { 4 } else { 1 } {
                let addr = if space == AddressSpace::Byte {
                    256 + offset
                } else {
                    64
                };
                let bytes = cells.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
                let initialize = cells
                    .iter()
                    .enumerate()
                    .map(|(i, v)| alloc::format!("push.{v} mem_store.{}", 64 + i))
                    .collect::<Vec<_>>()
                    .join(" ");
                let expected = widths
                    .iter()
                    .enumerate()
                    .map(|(lane, bits)| {
                        let start = offset + lane * 4;
                        let word = u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap());
                        (word & (u32::MAX >> (32 - bits))) as usize
                    })
                    .collect::<Vec<_>>();
                execute_linked_memory(
                    &access(ty.clone(), space, false),
                    &[addr],
                    &expected,
                    &initialize,
                    "",
                )
                .unwrap();

                let mut updated = bytes.clone();
                let mut inputs = vec![addr];
                for (lane, bits) in widths.iter().enumerate() {
                    let value = 0x12345678usize + lane;
                    // Partial lanes deliberately contain bits above their declared width.
                    inputs.push(value);
                    for bit in 0..*bits {
                        let position = (offset + lane * 4) * 8 + bit;
                        let mask = 1 << (position % 8);
                        updated[position / 8] = (updated[position / 8] & !mask)
                            | (((value >> bit) as u8 & 1) << (position % 8));
                    }
                }
                let observe = updated
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        alloc::format!(
                            "mem_load.{} push.{} assert_eq",
                            64 + i,
                            u32::from_le_bytes(*v)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                execute_linked_memory(
                    &access(ty.clone(), space, true),
                    &inputs,
                    &[],
                    &initialize,
                    &observe,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn aggregate_pointer_felt_alignment_is_checked_even_for_small_values() {
    for ty in [
        Type::from(ArrayType::new(Type::Felt, 2)),
        Type::from(StructType::new_with_repr(TypeRepr::packed(1), [Type::Felt, Type::U8])),
    ] {
        for store in [false, true] {
            let f = access(ty.clone(), AddressSpace::Byte, store);
            let inputs = if store { vec![257, 0, 0] } else { vec![257] };
            let expected = if store { vec![] } else { vec![0, 0] };
            let error = execute_linked_memory(&f, &inputs, &expected, "", "").unwrap_err();
            let code = miden_core::mast::error_code_from_msg(
                "aggregate Felt access requires an element-aligned pointer",
            );
            assert!(error.to_string().contains(&code.to_string()), "{error}");
        }
    }
}

#[test]
fn aggregate_pointer_native_bounds_and_wide_addresses() {
    let ty = Type::from(ArrayType::new(Type::U32, 2));
    // Keep ordinary buffers away from live frame storage and the VM's frame-pointer cell.
    for base in [(1usize << 31) + 1024, u32::MAX as usize - 5] {
        for space in [AddressSpace::Byte, AddressSpace::Element] {
            for store in [false, true] {
                let f = access(ty.clone(), space, store);
                let env = unit_environment(&f);
                let addr = if space == AddressSpace::Byte {
                    base * 4
                } else {
                    base
                };
                let inputs = if store {
                    vec![addr, 11, 23]
                } else {
                    vec![addr]
                };
                let expected = if store { vec![] } else { vec![11, 23] };
                let initialize =
                    alloc::format!("push.11 mem_store.{base} push.23 mem_store.{}", base + 1);
                let observe = alloc::format!(
                    "mem_load.{base} push.11 assert_eq mem_load.{} push.23 assert_eq",
                    base + 1
                );
                execute_linked_memory_env(
                    &f,
                    &env,
                    &inputs,
                    &expected,
                    &initialize,
                    &observe,
                    |_| {},
                )
                .unwrap_or_else(|err| panic!("{base} {space:?} store={store}: {err}"));
            }
        }
    }
    for store in [false, true] {
        let f = access(ty.clone(), AddressSpace::Element, store);
        let inputs = if store {
            vec![u32::MAX as usize, 11, 23]
        } else {
            vec![u32::MAX as usize]
        };
        let expected = if store { vec![] } else { vec![0, 0] };
        assert!(execute_linked_memory(&f, &inputs, &expected, "", "").is_err());
    }
}

#[test]
fn aggregate_pointer_rejects_zero_and_oversized_values() {
    for ty in [
        Type::from(StructType::new([] as [Type; 0])),
        Type::from(ArrayType::new(Type::U32, 5)),
        Type::from(StructType::new_with_repr(TypeRepr::align(32), [Type::U8])),
    ] {
        for store in [false, true] {
            let f = access(ty.clone(), AddressSpace::Byte, store);
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
}

#[test]
fn aggregate_pointer_accepts_zero_sized_array_fields() {
    let empty = Type::from(StructType::new([] as [Type; 0]));
    let ty =
        Type::from(StructType::new([Type::U32, Type::from(ArrayType::new(empty, usize::MAX))]));
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        let addr = if space == AddressSpace::Byte { 256 } else { 64 };
        execute_linked_memory(
            &access(ty.clone(), space, false),
            &[addr],
            &[23],
            "push.23 mem_store.64",
            "",
        )
        .unwrap();
        execute_linked_memory(
            &access(ty.clone(), space, true),
            &[addr, 23],
            &[],
            "",
            "mem_load.64 push.23 assert_eq",
        )
        .unwrap();
    }
}

#[test]
fn aggregate_pointer_partial_access_uses_low32_of_full_field_cells() {
    let ty = Type::from(ArrayType::new(Type::U8, 3));
    let high = (1usize << 40) + 0xa5a5a5a5;
    for offset in 0..4 {
        let initialize = alloc::format!(
            "push.{high} mem_store.64 push.{high} mem_store.65 push.{high} mem_store.66"
        );
        execute_linked_memory(
            &access(ty.clone(), AddressSpace::Byte, false),
            &[256 + offset],
            &[0xa5a5a5],
            &initialize,
            "",
        )
        .unwrap();
        let mut bytes = [0xa5u8; 12];
        bytes[offset..offset + 3].fill(0);
        let observe = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let touched = i == 0 || (i == 1 && offset > 1);
                let value = if touched {
                    u32::from_le_bytes(*chunk) as usize
                } else {
                    high
                };
                alloc::format!("mem_load.{} push.{value} assert_eq", 64 + i)
            })
            .collect::<Vec<_>>()
            .join(" ");
        execute_linked_memory(
            &access(ty.clone(), AddressSpace::Byte, true),
            &[256 + offset, 0],
            &[],
            &initialize,
            &observe,
        )
        .unwrap();
    }
}

#[test]
fn aggregate_pointer_partial_tail_at_native_limit() {
    let ty = Type::from(ArrayType::new(Type::U8, 3));
    let env = SelectEnv {
        byte_addressing: crate::emit::addressing::ByteAddressing::Wide,
        ..Default::default()
    };
    let base = u32::MAX as usize * 4;
    for store in [false, true] {
        let f = access(ty.clone(), AddressSpace::Byte, store);
        let inputs = if store {
            vec![base + 1, 0x123456]
        } else {
            vec![base + 1]
        };
        let expected = if store { vec![] } else { vec![0xa5a5a5] };
        execute_linked_memory_env(
            &f,
            &env,
            &inputs,
            &expected,
            "push.2779096485 mem_store.4294967295",
            if store {
                "mem_load.4294967295 push.305419941 assert_eq"
            } else {
                ""
            },
            |_| {},
        )
        .unwrap();
        let inputs = if store {
            vec![base + 2, 0x123456]
        } else {
            vec![base + 2]
        };
        // Zeroed memory would return zero if the access incorrectly wrapped. Match that
        // result so a post-call value assertion cannot masquerade as the bounds failure.
        let expected = if store { vec![] } else { vec![0] };
        let error =
            execute_linked_memory_env(&f, &env, &inputs, &expected, "", "", |_| {}).unwrap_err();
        // The shared crossing helpers reject the u32 address carry with plain assertz.
        assert!(error.to_string().contains("assertion failed with error code: 0"), "{error}");
    }
}
