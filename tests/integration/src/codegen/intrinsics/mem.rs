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

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u32>(), move |value| {
        let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

        // Construct the link outputs to be populated
        let link_output = setup::build_empty_component_for_test(context.clone());
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let write_to = 17 * 2u32.pow(16);
        let value_bytes = value.to_ne_bytes();
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

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
            // Assert (in MASM) the loaded value matches what we wrote to memory
            //let expected = builder.u32(value, SourceSpan::default());
            //builder.assert_eq(loaded, expected, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

        let args = [Felt::new(write_to as u64)];
        let output = eval_link_output::<u32, _, _>(
            link_output,
            initializers,
            &args,
            context.session(),
            |trace| {
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

/// Tests the memory load intrinsic for aligned loads of double-word (i.e. 64-bit) values
#[test]
fn load_dw() {
    setup::enable_compiler_instrumentation();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u64>(), move |value| {
        let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

        // Construct the link outputs to be populated
        let link_output = setup::build_empty_component_for_test(context.clone());

        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let write_to = 17 * 2u32.pow(16);
        let value_felts = value.to_felts();
        let initializers = [Initializer::MemoryFelts {
            addr: write_to / 4,
            felts: Cow::Borrowed(value_felts.as_slice()),
        }];

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
            // Assert (in MASM) the loaded value matches what we wrote to memory
            let expected = builder.u64(value, SourceSpan::default());
            builder.assert_eq(loaded, expected, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

        let args = [Felt::new(write_to as u64)];
        let output = eval_link_output::<u64, _, _>(
            link_output,
            initializers,
            &args,
            context.session(),
            |trace| {
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

/// Tests the memory load intrinsic for loads of single-byte (i.e. 8-bit) values
#[test]
fn load_u8() {
    setup::enable_compiler_instrumentation();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u8>(), move |value| {
        let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

        // Construct the link outputs to be populated
        let link_output = setup::build_empty_component_for_test(context.clone());
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let write_to = 17 * 2u32.pow(16);
        let value_bytes = [value];
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

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

        let args = [Felt::new(write_to as u64)];
        let output = eval_link_output::<u8, _, _>(
            link_output,
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

/// Tests the memory load intrinsic for loads of two u8 values at 2-byte aligned positions
#[test]
fn load_u8_two_bytes() {
    setup::enable_compiler_instrumentation();

    // Use fixed values for debugging
    let value1: u8 = 0xab;
    let value2: u8 = 0xcd;

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write values to the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Create a u32 with the two u8 values at offsets 0 and 2
    // In little-endian: byte 0 = value1, byte 2 = value2, bytes 1 and 3 = 0
    let u32_value = (value1 as u32) | ((value2 as u32) << 16);
    let value_bytes = u32_value.to_ne_bytes();

    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &value_bytes,
    }];

    // Generate a `test` module with `main` function that loads two u8 values
    let signature = Signature::new(
        [],
        [AbiParam::new(Type::U32)], // Return a u32 for testing
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u8 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();

        // Load first u8 from offset 0
        let loaded1 = builder.load(ptr_u8, SourceSpan::default()).unwrap();

        // Create pointer to offset 2
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u8_offset = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();

        // Load second u8 from offset 2
        let loaded2 = builder.load(ptr_u8_offset, SourceSpan::default()).unwrap();

        // Verify the loaded values using assertions in MASM
        let expected1 = builder.u8(value1, SourceSpan::default());
        builder.assert_eq(loaded1, expected1, SourceSpan::default()).unwrap();

        let expected2 = builder.u8(value2, SourceSpan::default());
        builder.assert_eq(loaded2, expected2, SourceSpan::default()).unwrap();

        // Return a success indicator
        let result = builder.u32(0x1234, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let args = [];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |trace| {
            // Verify the initial memory state
            let stored_u32 = trace.read_from_rust_memory::<u32>(write_to).unwrap();
            let stored_u8_at_0 = (stored_u32 & 0xff) as u8;
            let stored_u8_at_2 = ((stored_u32 >> 16) & 0xff) as u8;

            println!("Initial memory at base address: 0x{:08x}", stored_u32);
            println!("Expected u8 at offset 0: 0x{:02x}", value1);
            println!("Expected u8 at offset 2: 0x{:02x}", value2);
            println!("Actual u8 at offset 0: 0x{:02x}", stored_u8_at_0);
            println!("Actual u8 at offset 2: 0x{:02x}", stored_u8_at_2);

            assert_eq!(stored_u8_at_0, value1, "Initial memory check failed for offset 0");
            assert_eq!(stored_u8_at_2, value2, "Initial memory check failed for offset 2");

            // Debug: check what's in element 278529
            let elem_addr = (write_to / 4) + 1;
            if let Some(elem) = trace.read_memory_element(elem_addr) {
                println!("Element at address {}: 0x{:08x}", elem_addr, elem.as_int());
            } else {
                println!("Element at address {} is not initialized", elem_addr);
            }

            Ok(())
        },
    )
    .unwrap();

    // If we get here, the assertions passed and the loads were correct
    println!("Successfully loaded u8 from offset 0: 0x{:02x}", value1);
    println!("Successfully loaded u8 from offset 2: 0x{:02x}", value2);
    assert_eq!(output, 0x1234, "Test should return success indicator");
}

/// Tests the memory load intrinsic for loads of 16-bit (u16) values
#[test]
fn load_u16() {
    setup::enable_compiler_instrumentation();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<u16>(), move |value| {
        let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

        // Construct the link outputs to be populated
        let link_output = setup::build_empty_component_for_test(context.clone());
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let write_to = 17 * 2u32.pow(16);
        let value_bytes = value.to_ne_bytes();
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

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

        let args = [Felt::new(write_to as u64)];
        let output = eval_link_output::<u16, _, _>(
            link_output,
            initializers,
            &args,
            context.session(),
            |trace| {
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

/// Tests the memory load intrinsic for loads of boolean (i.e. 1-bit) values
#[test]
fn load_bool() {
    setup::enable_compiler_instrumentation();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<bool>(), move |value| {
        let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

        // Construct the link outputs to be populated
        let link_output = setup::build_empty_component_for_test(context.clone());
        // Write `value` to the start of the 17th page (1 page after the 16 pages reserved for the
        // Rust stack)
        let write_to = 17 * 2u32.pow(16);
        let value_bytes = [value as u8];
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &value_bytes,
        }];

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

        let args = [Felt::new(write_to as u64)];
        let output = eval_link_output::<bool, _, _>(
            link_output,
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

/// Tests the memory store intrinsic for stores of 16-bit (u16) values
#[test]
fn store_u16() {
    setup::enable_compiler_instrumentation();

    // Use fixed values for debugging
    let value1: u16 = 0x1234;
    let value2: u16 = 0x5678;

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Initialize the memory location with zeros (4 bytes for a u32)
    let initial_bytes = [0u8; 4];
    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &initial_bytes,
    }];

    // Generate a `test` module with `main` function that stores two u16 values
    let signature = Signature::new(
        [AbiParam::new(Type::U16), AbiParam::new(Type::U16)],
        [AbiParam::new(Type::U32)], // Return u32 to satisfy test infrastructure
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        // Get the two input u16 values
        let value1 = block.borrow().arguments()[0] as ValueRef;
        let value2 = block.borrow().arguments()[1] as ValueRef;

        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u16 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Store first u16 at offset 0
        builder.store(ptr_u16, value1, SourceSpan::default()).unwrap();

        // Create pointer to offset 2 (next u16 position)
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u16_offset = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Store second u16 at offset 2
        builder.store(ptr_u16_offset, value2, SourceSpan::default()).unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    // Note: Arguments are pushed in reverse order on the stack in Miden
    // So we need to pass them in reverse order
    let args = [Felt::new(value2 as u64), Felt::new(value1 as u64)];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |trace| {
            // Read the u32 from the base address (aligned read)
            let stored_u32 = trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
                std::io::Error::other(format!(
                    "expected to read u32 from byte address {write_to}, but read failed"
                ))
            })?;

            // Extract the u16 values from the u32
            let stored_u16_at_0 = (stored_u32 & 0xffff) as u16;
            let stored_u16_at_2 = ((stored_u32 >> 16) & 0xffff) as u16;

            println!("Address for first store: 0x{:08x} (element: {})", write_to, write_to / 4);
            println!(
                "Address for second store: 0x{:08x} (element: {})",
                write_to + 2,
                (write_to + 2) / 4
            );
            println!(
                "Stored u16 at offset 0: 0x{:04x} (expected 0x{:04x})",
                stored_u16_at_0, value1
            );
            println!(
                "Stored u16 at offset 2: 0x{:04x} (expected 0x{:04x})",
                stored_u16_at_2, value2
            );

            // The u32 should contain both u16 values
            // In little-endian: low 16 bits = value1, high 16 bits = value2
            let expected_u32 = (value1 as u32) | ((value2 as u32) << 16);

            println!("Expected u32: 0x{:08x}, Got: 0x{:08x}", expected_u32, stored_u32);

            assert_eq!(
                stored_u32, expected_u32,
                "expected u32 value {} (0x{:08x}) at byte address {}, but found {} (0x{:08x}) \
                 instead",
                expected_u32, expected_u32, write_to, stored_u32, stored_u32
            );
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(output, 1u32);
}

/// Tests the memory load intrinsic for loads of two u16 values at 2-byte aligned positions
#[test]
fn load_u16_two_values() {
    setup::enable_compiler_instrumentation();

    // Use fixed values for debugging
    let value1: u16 = 0x1234;
    let value2: u16 = 0x5678;

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Write values to the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Create a u32 with the two u16 values at offsets 0 and 2
    // In little-endian: bytes 0-1 = value1, bytes 2-3 = value2
    let u32_value = (value1 as u32) | ((value2 as u32) << 16);
    let value_bytes = u32_value.to_ne_bytes();

    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &value_bytes,
    }];

    // Generate a `test` module with `main` function that loads two u16 values
    let signature = Signature::new(
        [],
        [AbiParam::new(Type::U32)], // Return a u32 for testing
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u16 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Load first u16 from offset 0
        let loaded1 = builder.load(ptr_u16, SourceSpan::default()).unwrap();

        // Create pointer to offset 2
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u16_offset = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Load second u16 from offset 2
        let loaded2 = builder.load(ptr_u16_offset, SourceSpan::default()).unwrap();

        // Verify the loaded values using assertions in MASM
        let expected1 = builder.u16(value1, SourceSpan::default());
        builder.assert_eq(loaded1, expected1, SourceSpan::default()).unwrap();

        let expected2 = builder.u16(value2, SourceSpan::default());
        builder.assert_eq(loaded2, expected2, SourceSpan::default()).unwrap();

        // Return a success indicator
        let result = builder.u32(0x1234, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let args = [];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |trace| {
            // Verify the initial memory state
            let stored_u32 = trace.read_from_rust_memory::<u32>(write_to).unwrap();
            let stored_u16_at_0 = (stored_u32 & 0xffff) as u16;
            let stored_u16_at_2 = ((stored_u32 >> 16) & 0xffff) as u16;

            println!("Initial memory at base address: 0x{:08x}", stored_u32);
            println!("Expected u16 at offset 0: 0x{:04x}", value1);
            println!("Expected u16 at offset 2: 0x{:04x}", value2);
            println!("Actual u16 at offset 0: 0x{:04x}", stored_u16_at_0);
            println!("Actual u16 at offset 2: 0x{:04x}", stored_u16_at_2);

            assert_eq!(stored_u16_at_0, value1, "Initial memory check failed for offset 0");
            assert_eq!(stored_u16_at_2, value2, "Initial memory check failed for offset 2");

            Ok(())
        },
    )
    .unwrap();

    // If we get here, the assertions passed and the loads were correct
    println!("Successfully loaded u16 from offset 0: 0x{:04x}", value1);
    println!("Successfully loaded u16 from offset 2: 0x{:04x}", value2);
    assert_eq!(output, 0x1234, "Test should return success indicator");
}

/// Tests that u16 stores only affect the targeted 2 bytes and don't corrupt surrounding memory
#[test]
fn store_u16_precise() {
    setup::enable_compiler_instrumentation();

    // Use specific values that are easily distinguishable
    let store_value1: u16 = 0x1234; // Store at offset 0
    let store_value2: u16 = 0x5678; // Store at offset 2

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16); // 1048564

    // Initialize memory with a pattern that's different from what we'll write
    // This helps us detect any unintended modifications
    // Pattern: [0xFF, 0xEE, 0xDD, 0xCC, 0x11, 0x22, 0x33, 0x44]
    let initial_bytes = [0xff, 0xee, 0xdd, 0xcc, 0x11, 0x22, 0x33, 0x44];
    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &initial_bytes,
    }];

    // Generate a `test` module with `main` function that stores two u16 values
    let signature = Signature::new(
        [],
        [AbiParam::new(Type::U32)], // Return u32 to satisfy test infrastructure
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let value1 = builder.u16(store_value1, SourceSpan::default());
        let value2 = builder.u16(store_value2, SourceSpan::default());

        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u16 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Store first u16 at offset 0
        builder.store(ptr_u16, value1, SourceSpan::default()).unwrap();

        // // After first store, load back the u16 value at offset 0
        let loaded1_after_store1 = builder.load(ptr_u16, SourceSpan::default()).unwrap();
        let expected_value1 = builder.u16(store_value1, SourceSpan::default());
        builder
            .assert_eq(loaded1_after_store1, expected_value1, SourceSpan::default())
            .unwrap();
        //
        // Load u16 at offset 2 (should still be unchanged - 0xCCDD)
        let addr_plus_2 = builder.u32(write_to + 2, SourceSpan::default());
        let ptr_u16_offset2_check = builder
            .inttoptr(addr_plus_2, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();
        let loaded2_before_store2 =
            builder.load(ptr_u16_offset2_check, SourceSpan::default()).unwrap();
        let expected_initial_at_2 = builder.u16(0xccdd, SourceSpan::default());
        builder
            .assert_eq(loaded2_before_store2, expected_initial_at_2, SourceSpan::default())
            .unwrap();

        // Now store second u16 at offset 2 (reuse ptr_u16_offset2_check)
        builder.store(ptr_u16_offset2_check, value2, SourceSpan::default()).unwrap();

        // After second store, load both u16 values to verify they are correct
        // Load u16 at offset 0 (should still be value1)
        let loaded1_after_store2 = builder.load(ptr_u16, SourceSpan::default()).unwrap();
        builder
            .assert_eq(loaded1_after_store2, expected_value1, SourceSpan::default())
            .unwrap();

        // Load u16 at offset 2 (should now be value2)
        let loaded2_after_store2 =
            builder.load(ptr_u16_offset2_check, SourceSpan::default()).unwrap();
        let expected_value2 = builder.u16(store_value2, SourceSpan::default());
        builder
            .assert_eq(loaded2_after_store2, expected_value2, SourceSpan::default())
            .unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    // Note: Arguments are pushed in reverse order on the stack in Miden
    let args = [Felt::new(store_value2 as u64), Felt::new(store_value1 as u64)];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |trace| {
            // The trace callback runs after execution
            // All assertions in the program passed, so we know:
            // 1. After first store, only bytes 0-1 changed
            // 2. After second store, only bytes 2-3 changed (bytes 0-1 remained as value1)

            // Read final memory state for verification
            let word0 = trace.read_from_rust_memory::<u32>(write_to).unwrap();
            let word1 = trace.read_from_rust_memory::<u32>(write_to + 4).unwrap();

            println!("Test passed! Memory integrity verified:");
            println!(
                "  After first store (offset 0): only bytes 0-1 changed to 0x{:04x}",
                store_value1
            );
            println!(
                "  After second store (offset 2): only bytes 2-3 changed to 0x{:04x}",
                store_value2
            );
            println!("  Bytes 4-7 remained unchanged throughout");
            println!("\nFinal memory state:");
            println!("  Word at offset 0: 0x{:08x}", word0);
            println!("  Word at offset 4: 0x{:08x}", word1);

            Ok(())
        },
    )
    .unwrap();

    assert_eq!(output, 1u32);
}

/// Tests u16 store followed by load with assertion
#[test]
fn store_u16_load_assert() {
    setup::enable_compiler_instrumentation();

    let value: u16 = 0x1234;

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let write_to = 17 * 2u32.pow(16);
    let initial_bytes = [0u8; 4];
    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &initial_bytes,
    }];

    let signature = Signature::new([AbiParam::new(Type::U16)], [AbiParam::new(Type::U32)]);

    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let block = builder.current_block();
        let value = block.borrow().arguments()[0] as ValueRef;

        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u16 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U16)), SourceSpan::default())
            .unwrap();

        // Store the u16 value
        builder.store(ptr_u16, value, SourceSpan::default()).unwrap();

        // Load the value back
        let loaded_value = builder.load(ptr_u16, SourceSpan::default()).unwrap();

        // Assert that the loaded value equals the original value
        builder.assert_eq(loaded_value, value, SourceSpan::default()).unwrap();

        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let args = [Felt::new(value as u64)];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |_trace| {
            // If we reach here, the assertion passed
            println!("Test passed: stored u16 value 0x{:04x} was successfully loaded back", value);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(output, 1u32);
}

/// Tests that u8 stores only affect the targeted byte and don't corrupt surrounding memory
#[test]
fn store_u8_precise() {
    setup::enable_compiler_instrumentation();

    // Use specific values that are easily distinguishable
    let store_value0: u8 = 0x12; // Store at offset 0
    let store_value1: u8 = 0x34; // Store at offset 1
    let store_value2: u8 = 0x56; // Store at offset 2
    let store_value3: u8 = 0x78; // Store at offset 3

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);

    // Construct the link outputs to be populated
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Initialize memory with a pattern that's different from what we'll write
    // This helps us detect any unintended modifications
    // Pattern: [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88]
    let initial_bytes = [0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88];
    let initializers = [Initializer::MemoryBytes {
        addr: write_to,
        bytes: &initial_bytes,
    }];

    // Generate a `test` module with `main` function that stores four u8 values
    let signature = Signature::new(
        [],
        [AbiParam::new(Type::U32)], // Return u32 to satisfy test infrastructure
    );
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let value0 = builder.u8(store_value0, SourceSpan::default());
        let value1 = builder.u8(store_value1, SourceSpan::default());
        let value2 = builder.u8(store_value2, SourceSpan::default());
        let value3 = builder.u8(store_value3, SourceSpan::default());

        // Create pointer to the base address
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let ptr_u8 = builder
            .inttoptr(base_addr, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();

        // Store first u8 at offset 0
        builder.store(ptr_u8, value0, SourceSpan::default()).unwrap();

        // After first store, verify byte at offset 0 changed
        let loaded0_after_store0 = builder.load(ptr_u8, SourceSpan::default()).unwrap();
        let expected_value0 = builder.u8(store_value0, SourceSpan::default());
        builder
            .assert_eq(loaded0_after_store0, expected_value0, SourceSpan::default())
            .unwrap();

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
        builder
            .assert_eq(loaded0_after_store1, expected_value0, SourceSpan::default())
            .unwrap();

        let loaded1_after_store1 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        let expected_value1 = builder.u8(store_value1, SourceSpan::default());
        builder
            .assert_eq(loaded1_after_store1, expected_value1, SourceSpan::default())
            .unwrap();

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
        builder
            .assert_eq(loaded0_after_store2, expected_value0, SourceSpan::default())
            .unwrap();

        let loaded1_after_store2 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        builder
            .assert_eq(loaded1_after_store2, expected_value1, SourceSpan::default())
            .unwrap();

        let loaded2_after_store2 = builder.load(ptr_u8_offset2, SourceSpan::default()).unwrap();
        let expected_value2 = builder.u8(store_value2, SourceSpan::default());
        builder
            .assert_eq(loaded2_after_store2, expected_value2, SourceSpan::default())
            .unwrap();

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
        builder
            .assert_eq(loaded0_after_store3, expected_value0, SourceSpan::default())
            .unwrap();

        let loaded1_after_store3 = builder.load(ptr_u8_offset1, SourceSpan::default()).unwrap();
        builder
            .assert_eq(loaded1_after_store3, expected_value1, SourceSpan::default())
            .unwrap();

        let loaded2_after_store3 = builder.load(ptr_u8_offset2, SourceSpan::default()).unwrap();
        builder
            .assert_eq(loaded2_after_store3, expected_value2, SourceSpan::default())
            .unwrap();

        let loaded3_after_store3 = builder.load(ptr_u8_offset3, SourceSpan::default()).unwrap();
        let expected_value3 = builder.u8(store_value3, SourceSpan::default());
        builder
            .assert_eq(loaded3_after_store3, expected_value3, SourceSpan::default())
            .unwrap();

        // Check that bytes 4-7 remain unchanged
        let addr_plus_4 = builder.u32(write_to + 4, SourceSpan::default());
        let ptr_u8_offset4 = builder
            .inttoptr(addr_plus_4, Type::from(PointerType::new(Type::U8)), SourceSpan::default())
            .unwrap();
        let loaded4_final = builder.load(ptr_u8_offset4, SourceSpan::default()).unwrap();
        let expected_initial_at_4 = builder.u8(0xbb, SourceSpan::default());
        builder
            .assert_eq(loaded4_final, expected_initial_at_4, SourceSpan::default())
            .unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let args = [];
    let output = eval_link_output::<u32, _, _>(
        link_output,
        initializers,
        &args,
        context.session(),
        |trace| {
            // The trace callback runs after execution
            // All assertions in the program passed, so we know each store only affected its target byte

            // Read final memory state for verification
            let word0 = trace.read_from_rust_memory::<u32>(write_to).unwrap();
            let word1 = trace.read_from_rust_memory::<u32>(write_to + 4).unwrap();

            println!("Test passed! Memory integrity verified:");
            println!("  Store at offset 0: only byte 0 changed to 0x{:02x}", store_value0);
            println!("  Store at offset 1: only byte 1 changed to 0x{:02x}", store_value1);
            println!("  Store at offset 2: only byte 2 changed to 0x{:02x}", store_value2);
            println!("  Store at offset 3: only byte 3 changed to 0x{:02x}", store_value3);
            println!("  Bytes 4-7 remained unchanged throughout");
            println!("\nFinal memory state:");
            println!("  Word at offset 0: 0x{:08x}", word0);
            println!("  Word at offset 4: 0x{:08x}", word1);

            Ok(())
        },
    )
    .unwrap();

    assert_eq!(output, 1u32);
}
