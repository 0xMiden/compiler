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
