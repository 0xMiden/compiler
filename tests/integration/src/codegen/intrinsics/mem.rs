use std::borrow::Cow;

use midenc_debug::ToMidenRepr;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::BuiltinOpBuilder, AbiParam, Felt, PointerType, Signature, SourceSpan, Type,
    ValueRef,
};
use proptest::{
    prelude::any,
    prop_assert_eq,
    test_runner::{TestCaseError, TestError, TestRunner},
};

use crate::testing::*;

/// Tests the memory load intrinsic for aligned loads of single-word (i.e. 32-bit) values
#[test]
fn load_sw() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write address to use
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes `load_sw` when lowered to MASM
    let signature = Signature::new(
        [AbiParam::new(Type::from(PointerType::new(Type::U32)))],
        [AbiParam::new(Type::U32)],
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the input pointer, and load the value at that address
        let ptr = block.borrow().arguments()[0] as ValueRef;
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u32>(), move |value| {
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let value_bytes = value.to_ne_bytes();
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

        let args = [Felt::new(write_to as u64)];
        let output =
            eval_package::<u32, _, _>(&package, initializers, &args, context.session(), |trace| {
                let stored = trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
                    TestCaseError::fail(format!(
                        "expected {value} to have been written to byte address {write_to}, but \
                         read from that address failed"
                    ))
                })?;
                prop_assert_eq!(
                    stored,
                    value,
                    "expected {} to have been written to byte address {}, but found {} there \
                     instead",
                    value,
                    write_to,
                    stored
                );
                Ok(())
            })?;

        prop_assert_eq!(output, value);

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests the memory load intrinsic for aligned loads of double-word (i.e. 64-bit) values
#[test]
fn load_dw() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write address to use
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes `load_dw` when lowered to MASM
    let signature = Signature::new(
        [AbiParam::new(Type::from(PointerType::new(Type::U64)))],
        [AbiParam::new(Type::U64)],
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the input pointer, and load the value at that address
        let ptr = block.borrow().arguments()[0] as ValueRef;
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u64>(), move |value| {
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let value_felts = value.to_felts();
        let initializers = [Initializer::MemoryFelts {
            addr: write_to / 4,
            felts: Cow::Borrowed(value_felts.as_slice()),
        }];

        let args = [Felt::new(write_to as u64)];
        let output =
            eval_package::<u64, _, _>(&package, initializers, &args, context.session(), |trace| {
                let hi =
                    trace.read_memory_element(write_to / 4).unwrap_or_default().as_int() as u32;
                let lo = trace.read_memory_element((write_to / 4) + 1).unwrap_or_default().as_int()
                    as u32;
                log::trace!(target: "executor", "hi = {hi} ({hi:0x})");
                log::trace!(target: "executor", "lo = {lo} ({lo:0x})");
                let stored = trace.read_from_rust_memory::<u64>(write_to).ok_or_else(|| {
                    TestCaseError::fail(format!(
                        "expected {value} to have been written to byte address {write_to}, but \
                         read from that address failed"
                    ))
                })?;
                prop_assert_eq!(
                    stored,
                    value,
                    "expected {} to have been written to byte address {}, but found {} there \
                     instead",
                    value,
                    write_to,
                    stored
                );
                Ok(())
            })?;

        prop_assert_eq!(output, value);

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests the memory load intrinsic for loads of single-byte (i.e. 8-bit) values
#[test]
fn load_u8() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write address to use
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes load for u8 when lowered to MASM
    let signature = Signature::new(
        [AbiParam::new(Type::from(PointerType::new(Type::U8)))],
        [AbiParam::new(Type::U8)],
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the input pointer, and load the value at that address
        let ptr = block.borrow().arguments()[0] as ValueRef;
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u8>(), move |value| {
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let value_bytes = [value];
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

        let args = [Felt::new(write_to as u64)];
        let output =
            eval_package::<u8, _, _>(&package, initializers, &args, context.session(), |trace| {
                let stored = trace.read_from_rust_memory::<u8>(write_to).ok_or_else(|| {
                    TestCaseError::fail(format!(
                        "expected {value} to have been written to byte address {write_to}, but \
                         read from that address failed"
                    ))
                })?;
                prop_assert_eq!(
                    stored,
                    value,
                    "expected {} to have been written to byte address {}, but found {} there \
                     instead",
                    value,
                    write_to,
                    stored
                );
                Ok(())
            })?;

        prop_assert_eq!(output, value);

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests the memory load intrinsic for loads of 16-bit (u16) values
#[test]
fn load_u16() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write address to use
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes load for u16 when lowered to MASM
    let signature = Signature::new(
        [AbiParam::new(Type::from(PointerType::new(Type::U16)))],
        [AbiParam::new(Type::U16)],
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the input pointer, and load the value at that address
        let ptr = block.borrow().arguments()[0] as ValueRef;
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u16>(), move |value| {
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let value_bytes = value.to_ne_bytes();
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

        let args = [Felt::new(write_to as u64)];
        let output =
            eval_package::<u16, _, _>(&package, initializers, &args, context.session(), |trace| {
                let stored = trace.read_from_rust_memory::<u16>(write_to).ok_or_else(|| {
                    TestCaseError::fail(format!(
                        "expected {value} to have been written to byte address {write_to}, but \
                         read from that address failed"
                    ))
                })?;
                prop_assert_eq!(
                    stored,
                    value,
                    "expected {} to have been written to byte address {}, but found {} there \
                     instead",
                    value,
                    write_to,
                    stored
                );
                Ok(())
            })?;

        prop_assert_eq!(output, value);

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests the memory load intrinsic for loads of boolean (i.e. 1-bit) values
#[test]
fn load_bool() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write address to use
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes load for bool when lowered to MASM
    let signature = Signature::new(
        [AbiParam::new(Type::from(PointerType::new(Type::I1)))],
        [AbiParam::new(Type::I1)],
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the input pointer, and load the value at that address
        let ptr = block.borrow().arguments()[0] as ValueRef;
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<bool>(), move |value| {
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let value_bytes = [value as u8];
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

        let args = [Felt::new(write_to as u64)];
        let output = eval_package::<bool, _, _>(
            &package,
            initializers,
            &args,
            context.session(),
            |trace| {
                let stored = trace.read_from_rust_memory::<u8>(write_to).ok_or_else(|| {
                    TestCaseError::fail(format!(
                        "expected {value} to have been written to byte address {write_to}, but \
                         read from that address failed"
                    ))
                })?;
                let stored_bool = stored != 0;
                prop_assert_eq!(
                    stored_bool,
                    value,
                    "expected {} to have been written to byte address {}, but found {} there \
                     instead",
                    value,
                    write_to,
                    stored_bool
                );
                Ok(())
            },
        )?;

        prop_assert_eq!(output, value);

        Ok(())
    });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests that u16 stores only affect the targeted 2 bytes and don't corrupt surrounding memory
#[test]
fn store_u16() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that stores two u16 values
    let signature = Signature::new(
        [AbiParam::new(Type::U16), AbiParam::new(Type::U16)],
        [AbiParam::new(Type::U32)], // Return u32 to satisfy test infrastructure
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
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

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

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

            // Note: Arguments are pushed in reverse order on the stack in Miden
            let args = [Felt::new(store_value2 as u64), Felt::new(store_value1 as u64)];
            let output = eval_package::<u32, _, _>(
                &package,
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
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

/// Tests that u8 stores only affect the targeted byte and don't corrupt surrounding memory
#[test]
fn store_u8() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that stores four u8 values
    let signature = Signature::new(
        [
            AbiParam::new(Type::U8),
            AbiParam::new(Type::U8),
            AbiParam::new(Type::U8),
            AbiParam::new(Type::U8),
        ],
        [AbiParam::new(Type::U32)], // Return u32 to satisfy test infrastructure
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
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

    // Compile once outside the test loop
    let package = compile_link_output_to_package(link_output).unwrap();

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

            // Note: Arguments are pushed in reverse order on the stack in Miden
            let args = [
                Felt::new(store_value3 as u64),
                Felt::new(store_value2 as u64),
                Felt::new(store_value1 as u64),
                Felt::new(store_value0 as u64),
            ];
            let output = eval_package::<u32, _, _>(
                &package,
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
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}
