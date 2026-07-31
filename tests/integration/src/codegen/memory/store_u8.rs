use super::*;

/// Tests that u8 stores only affect the targeted byte and don't corrupt surrounding memory
#[test]
fn store_u8() {
    setup::enable_compiler_instrumentation();

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that stores four u8 values
    // Return u32 to satisfy test infrastructure
    let params = [Type::U8, Type::U8, Type::U8, Type::U8];
    let (package, context) = compile_test_module(params, [Type::U32], |builder| {
        let block = builder.current_block();
        let (value0, value1, value2, value3) = {
            let block_ref = block.borrow();
            let args = block_ref.arguments();
            (
                args[0] as ValueRef,
                args[1] as ValueRef,
                args[2] as ValueRef,
                args[3] as ValueRef,
            )
        };

        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u8 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();

        // Store first u8 at offset 0
        builder.store(ptr_u8, value0, SourceSpan::default()).unwrap();

        // After first store, verify byte at offset 0 changed
        let loaded0_after_store0 = builder.load(ptr_u8, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded0_after_store0, value0, SourceSpan::default()).unwrap();

        // Verify other bytes remain unchanged
        // Check byte at offset 1 (should still be 0xEE)
        let addr_plus_1 = builder.u32(write_to + 1, SourceSpan::default());
        let ptr_u8_offset1 = builder
            .inttoptr(addr_plus_1, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();
        let loaded1_before_store1 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        let expected_initial_at_1 = builder.u8(0xee, SourceSpan::default());
        builder
            .assert_eq(loaded1_before_store1, expected_initial_at_1, SourceSpan::default())
            .unwrap();

        // Store second u8 at offset 1
        builder.store(ptr_u8_offset1, value1, SourceSpan::default()).unwrap();

        // After second store, verify both bytes have correct values
        let loaded0_after_store1 = builder.load(ptr_u8, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded0_after_store1, value0, SourceSpan::default()).unwrap();

        let loaded1_after_store1 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded1_after_store1, value1, SourceSpan::default()).unwrap();

        // Check byte at offset 2 (should still be 0xDD)
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u8_offset2 = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();
        let loaded2_before_store2 = builder.load(ptr_u8_offset2, SourceSpan::default()).unwrap();
        let expected_initial_at_2 = builder.u8(0xdd, SourceSpan::default());
        builder
            .assert_eq(loaded2_before_store2, expected_initial_at_2, SourceSpan::default())
            .unwrap();

        // Store third u8 at offset 2
        builder.store(ptr_u8_offset2, value2, SourceSpan::default()).unwrap();

        // After third store, verify first three bytes have correct values
        let loaded0_after_store2 = builder.load(ptr_u8, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded0_after_store2, value0, SourceSpan::default()).unwrap();

        let loaded1_after_store2 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded1_after_store2, value1, SourceSpan::default()).unwrap();

        let loaded2_after_store2 = builder.load(ptr_u8_offset2, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded2_after_store2, value2, SourceSpan::default()).unwrap();

        // Check byte at offset 3 (should still be 0xCC)
        let addr_plus_3 = builder.u32(write_to + 3, SourceSpan::default());
        let ptr_u8_offset3 = builder
            .inttoptr(addr_plus_3, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();
        let loaded3_before_store3 = builder.load(ptr_u8_offset3, SourceSpan::default()).unwrap();
        let expected_initial_at_3 = builder.u8(0xcc, SourceSpan::default());
        builder
            .assert_eq(loaded3_before_store3, expected_initial_at_3, SourceSpan::default())
            .unwrap();

        // Store fourth u8 at offset 3
        builder.store(ptr_u8_offset3, value3, SourceSpan::default()).unwrap();

        // After fourth store, verify all four bytes have correct values
        let loaded0_after_store3 = builder.load(ptr_u8, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded0_after_store3, value0, SourceSpan::default()).unwrap();

        let loaded1_after_store3 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded1_after_store3, value1, SourceSpan::default()).unwrap();

        let loaded2_after_store3 = builder.load(ptr_u8_offset2, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded2_after_store3, value2, SourceSpan::default()).unwrap();

        let loaded3_after_store3 = builder.load(ptr_u8_offset3, SourceSpan::default()).unwrap();
        builder.assert_eq(loaded3_after_store3, value3, SourceSpan::default()).unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(
        &(any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>()),
        move |(store_value0, store_value1, store_value2, store_value3)| {
            // Initialize memory with a pattern that's different from what we'll write
            // This helps us detect any unintended modifications
            // Pattern: [0xFF, 0xEE, 0xDD, 0xCC] for the first word only
            let initial_bytes = [0xff, 0xee, 0xdd, 0xcc];
            let initializers = [Initializer::MemoryBytes {
                addr: write_to,
                bytes: &initial_bytes,
            }];

            // C calling convention: first argument on top of the stack
            let args = [
                Felt::new_unchecked(store_value0 as u64),
                Felt::new_unchecked(store_value1 as u64),
                Felt::new_unchecked(store_value2 as u64),
                Felt::new_unchecked(store_value3 as u64),
            ];
            let output = eval_package::<u32, _, _>(
                package.clone(),
                initializers,
                &args,
                context.session(),
                |trace| {
                    // The trace callback runs after execution
                    // All assertions in the program passed, so we know each store only affected its target byte

                    // Read final memory state for verification
                    let word0 = trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
                        TestCaseError::fail(format!("failed to read from byte address {write_to}"))
                    })?;

                    // Extract u8 values from the 32-bit word (little-endian)
                    let stored0 = (word0 & 0xff) as u8;
                    let stored1 = ((word0 >> 8) & 0xff) as u8;
                    let stored2 = ((word0 >> 16) & 0xff) as u8;
                    let stored3 = ((word0 >> 24) & 0xff) as u8;

                    prop_assert_eq!(
                        stored0,
                        store_value0,
                        "expected {} to have been written to byte address {}, but found {} there \
                         instead",
                        store_value0,
                        write_to,
                        stored0
                    );

                    prop_assert_eq!(
                        stored1,
                        store_value1,
                        "expected {} to have been written to byte address {}, but found {} there \
                         instead",
                        store_value1,
                        write_to + 1,
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

                    prop_assert_eq!(
                        stored3,
                        store_value3,
                        "expected {} to have been written to byte address {}, but found {} there \
                         instead",
                        store_value3,
                        write_to + 3,
                        stored3
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
