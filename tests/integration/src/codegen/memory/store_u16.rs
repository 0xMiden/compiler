use super::*;

/// Tests that u16 stores only affect the targeted 2 bytes and don't corrupt surrounding memory
#[test]
fn store_u16() {
    setup::enable_compiler_instrumentation();

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that stores two u16 values
    // Return u32 to satisfy test infrastructure
    let (package, context) = compile_test_module([Type::U16, Type::U16], [Type::U32], |builder| {
        let block = builder.current_block();
        let (value1, value2) = {
            let block_ref = block.borrow();
            let args = block_ref.arguments();
            (args[0] as ValueRef, args[1] as ValueRef)
        };

        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u16 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Store first u16 at offset 0
        builder.store(ptr_u16, value1, SourceSpan::default()).unwrap();

        // After first store, load back the u16 value at offset 0
        let loaded1_after_store1 = builder.load(ptr_u16, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded1_after_store1, value1, SourceSpan::default()).unwrap();

        // Load u16 at offset 2 (should still be unchanged - 0xCCDD)
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u16_offset2 = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();
        let loaded2_before_store2 = builder.load(ptr_u16_offset2, SourceSpan::default()).unwrap();
        let expected_initial_at_2 = builder.u16(0xccdd, SourceSpan::default());
        builder
            .assert_eq(loaded2_before_store2, expected_initial_at_2, SourceSpan::default())
            .unwrap();

        // Now store second u16 at offset 2
        builder.store(ptr_u16_offset2, value2, SourceSpan::default()).unwrap();

        // After second store, load both u16 values to verify they are correct
        // Load u16 at offset 0 (should still be value1)
        let loaded1_after_store2 = builder.load(ptr_u16, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded1_after_store2, value1, SourceSpan::default()).unwrap();

        // Load u16 at offset 2 (should now be value2)
        let loaded2_after_store2 = builder.load(ptr_u16_offset2, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded2_after_store2, value2, SourceSpan::default()).unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(
        &(any::<u16>(), any::<u16>()),
        move |(store_value1, store_value2)| {
            // Initialize memory with a pattern that's different from what we'll write
            // This helps us detect any unintended modifications
            // Pattern: [0xFF, 0xEE, 0xDD, 0xCC, 0x11, 0x22, 0x33, 0x44]
            let initial_bytes = [0xff, 0xee, 0xdd, 0xcc, 0x11, 0x22, 0x33, 0x44];
            let initializers = [Initializer::MemoryBytes {
                addr: write_to,
                bytes: &initial_bytes,
            }];

            // C calling convention: first argument on top of the stack
            let args = [
                Felt::new_unchecked(store_value1 as u64),
                Felt::new_unchecked(store_value2 as u64),
            ];
            let output = eval_package::<u32, _, _>(
                package.clone(),
                initializers,
                &args,
                context.session(),
                |trace| {
                    // The trace callback runs after execution
                    // All assertions in the program passed, so we know:
                    // 1. After first store, bytes 0-1 contain value1, bytes 2-3 are unchanged
                    // 2. After second store, bytes 2-3 contain value2, bytes 0-1 still contain value1

                    // Read final memory state for verification
                    // Since trace reader requires 4-byte alignment, read the full word and extract u16 values
                    let word0 = trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
                        TestCaseError::fail(format!("failed to read from byte address {write_to}"))
                    })?;

                    // Extract u16 values from the 32-bit word (little-endian)
                    let stored1 = (word0 & 0xffff) as u16;
                    let stored2 = ((word0 >> 16) & 0xffff) as u16;

                    prop_assert_eq!(
                        stored1,
                        store_value1,
                        "expected {} to have been written to byte address {}, but found {} there \
                         instead",
                        store_value1,
                        write_to,
                        stored1
                    );

                    prop_assert_eq!(
                        stored2,
                        store_value2,
                        "expected {} to have been written to byte address {}, but found {} there \
                         instead",
                        store_value2,
                        write_to + 2,
                        stored2
                    );

                    Ok(())
                },
            )?;

            prop_assert_eq!(output, 1u32);

            Ok(())
        },
    );

    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("FAILURE: {}\nMinimal failing case: {value:?}", reason.message());
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}

macro_rules! define_unaligned_16bit_store_tests {
    (
        $run_fn:ident,
        $rust_ty:ty,
        $hir_ty:expr,
        $to_felt:expr,
        $offset_1_test:ident,
        $offset_2_test:ident,
        $offset_3_test:ident
    ) => {
        #[doc = concat!(
                    "Runs a `",
                    stringify!($rust_ty),
                    "` store test at the specified unaligned byte offset."
                )]
        fn $run_fn(offset: u32) {
            setup::enable_compiler_instrumentation();

            let write_to = 17 * 2u32.pow(16);
            let store_to = write_to + offset;

            let (package, context) = compile_test_module([$hir_ty], [Type::U32], |builder| {
                let block = builder.current_block();
                let value = block.borrow().arguments()[0] as ValueRef;

                let addr = builder.u32(store_to, SourceSpan::default());
                let ptr = builder
                    .inttoptr(addr, Type::from(PointerType::new($hir_ty)), SourceSpan::default())
                    .unwrap();

                builder.store(ptr, value, SourceSpan::default()).unwrap();

                let result = builder.u32(1, SourceSpan::default());
                builder.ret(Some(result), SourceSpan::default()).unwrap();
            });

            let config = proptest::test_runner::Config::with_cases(32);
            let res = TestRunner::new(config).run(&any::<$rust_ty>(), move |store_value| {
                let initial_bytes = [0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88];
                let initializers = [Initializer::MemoryBytes {
                    addr: write_to,
                    bytes: &initial_bytes,
                }];

                let args = [($to_felt)(store_value)];
                let output = eval_package::<u32, _, _>(
                    package.clone(),
                    initializers,
                    &args,
                    context.session(),
                    |trace| {
                        let expected = store_value.to_le_bytes();
                        let mut expected_bytes = initial_bytes;
                        expected_bytes[offset as usize] = expected[0];
                        expected_bytes[offset as usize + 1] = expected[1];

                        let word0 =
                            trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
                                TestCaseError::fail(format!(
                                    "failed to read from byte address {write_to}"
                                ))
                            })?;
                        let word1 =
                            trace.read_from_rust_memory::<u32>(write_to + 4).ok_or_else(|| {
                                TestCaseError::fail(format!(
                                    "failed to read from byte address {}",
                                    write_to + 4
                                ))
                            })?;
                        let observed_bytes = [
                            (word0 & 0xff) as u8,
                            ((word0 >> 8) & 0xff) as u8,
                            ((word0 >> 16) & 0xff) as u8,
                            ((word0 >> 24) & 0xff) as u8,
                            (word1 & 0xff) as u8,
                            ((word1 >> 8) & 0xff) as u8,
                            ((word1 >> 16) & 0xff) as u8,
                            ((word1 >> 24) & 0xff) as u8,
                        ];

                        for (index, (stored, expected_byte)) in
                            observed_bytes.into_iter().zip(expected_bytes).enumerate()
                        {
                            prop_assert_eq!(
                                stored,
                                expected_byte,
                                "unexpected byte at address {}",
                                write_to + index as u32
                            );
                        }

                        Ok(())
                    },
                )?;

                prop_assert_eq!(output, 1u32);
                Ok(())
            });

            match res {
                Err(TestError::Fail(reason, value)) => {
                    panic!("FAILURE: {}\nMinimal failing case: {value:?}", reason.message());
                }
                Ok(_) => (),
                _ => panic!("Unexpected test result: {res:?}"),
            }
        }

        #[doc = concat!(
                    "Tests that storing a `",
                    stringify!($rust_ty),
                    "` at byte offset 1 updates only the target bytes."
                )]
        #[test]
        fn $offset_1_test() {
            $run_fn(1);
        }

        #[doc = concat!(
                    "Tests that storing a `",
                    stringify!($rust_ty),
                    "` at byte offset 2 updates only the target bytes."
                )]
        #[test]
        fn $offset_2_test() {
            $run_fn(2);
        }

        #[doc = concat!(
                    "Tests that storing a `",
                    stringify!($rust_ty),
                    "` at byte offset 3 updates only the target bytes across the element boundary."
                )]
        #[test]
        fn $offset_3_test() {
            $run_fn(3);
        }
    };
}

define_unaligned_16bit_store_tests!(
    run_store_unaligned_u16,
    u16,
    Type::U16,
    |store_value: u16| Felt::new_unchecked(store_value as u64),
    store_unaligned_u16_offset_1,
    store_unaligned_u16_offset_2,
    store_unaligned_u16
);
define_unaligned_16bit_store_tests!(
    run_store_unaligned_i16,
    i16,
    Type::I16,
    |store_value: i16| Felt::new_unchecked(store_value as u16 as u64),
    store_unaligned_i16_offset_1,
    store_unaligned_i16_offset_2,
    store_unaligned_i16
);
