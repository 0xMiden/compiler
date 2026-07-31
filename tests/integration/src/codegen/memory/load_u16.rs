use super::*;

/// Tests the memory load intrinsic for loads of 16-bit (u16) values
#[test]
fn load_u16() {
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes load for u16 when lowered to MASM
    // Compile once outside the test loop
    let (package, context) =
        compile_test_module([Type::from(PointerType::new(Type::U16))], [Type::U16], |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<u16>(), random_word_aligned_addr()),
        move |(value, write_to)| {
            let value_bytes = value.to_ne_bytes();
            let initializers = [Initializer::MemoryBytes {
                addr: write_to,
                bytes: &value_bytes,
            }];

            let args = [Felt::new_unchecked(write_to as u64)];
            let output = eval_package::<u16, _, _>(
                package.clone(),
                initializers,
                &args,
                context.session(),
                |trace| {
                    let stored = trace.read_from_rust_memory::<u16>(write_to).ok_or_else(|| {
                        TestCaseError::fail(format!(
                            "expected {value} to have been written to byte address {write_to}, \
                             but read from that address failed"
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

            prop_assert_eq!(output, value, "expected 0x{:x}; found 0x{:x}", value, output,);

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

macro_rules! define_unaligned_16bit_load_tests {
    (
        $run_fn:ident,
        $rust_ty:ty,
        $hir_ty:expr,
        $offset_1_test:ident,
        $offset_2_test:ident,
        $offset_3_test:ident
    ) => {
        #[doc = concat!(
                    "Runs a `",
                    stringify!($rust_ty),
                    "` load test from the specified unaligned byte offset."
                )]
        fn $run_fn(offset: u32) {
            setup::enable_compiler_instrumentation();

            let write_to = 17 * 2u32.pow(16);
            let read_from = write_to + offset;

            let (package, context) = compile_test_module(
                [Type::from(PointerType::new($hir_ty))],
                [$hir_ty],
                |builder| {
                    let block = builder.current_block();
                    let ptr = block.borrow().arguments()[0] as ValueRef;
                    let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
                    builder.ret(Some(loaded), SourceSpan::default()).unwrap();
                },
            );

            let config = proptest::test_runner::Config::with_cases(10);
            let res = TestRunner::new(config).run(&any::<$rust_ty>(), move |value| {
                let expected = value.to_le_bytes();
                let mut initial_bytes = [0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88];
                initial_bytes[offset as usize] = expected[0];
                initial_bytes[offset as usize + 1] = expected[1];
                let initializers = [Initializer::MemoryBytes {
                    addr: write_to,
                    bytes: &initial_bytes,
                }];

                let args = [Felt::new_unchecked(read_from as u64)];
                let output = eval_package::<$rust_ty, _, _>(
                    package.clone(),
                    initializers,
                    &args,
                    context.session(),
                    |_| Ok(()),
                )?;

                prop_assert_eq!(output, value, "expected 0x{:x}; found 0x{:x}", value, output,);

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
                    "Tests that loading a `",
                    stringify!($rust_ty),
                    "` from byte offset 1 stays within the current element."
                )]
        #[test]
        fn $offset_1_test() {
            $run_fn(1);
        }

        #[doc = concat!(
                    "Tests that loading a `",
                    stringify!($rust_ty),
                    "` from byte offset 2 stays within the current element."
                )]
        #[test]
        fn $offset_2_test() {
            $run_fn(2);
        }

        #[doc = concat!(
                    "Tests that loading a `",
                    stringify!($rust_ty),
                    "` from byte offset 3 correctly reconstructs the value across the next element \
                     boundary."
                )]
        #[test]
        fn $offset_3_test() {
            $run_fn(3);
        }
    };
}

define_unaligned_16bit_load_tests!(
    run_load_unaligned_u16,
    u16,
    Type::U16,
    load_unaligned_u16_offset_1,
    load_unaligned_u16_offset_2,
    load_unaligned_u16
);
define_unaligned_16bit_load_tests!(
    run_load_unaligned_i16,
    i16,
    Type::I16,
    load_unaligned_i16_offset_1,
    load_unaligned_i16_offset_2,
    load_unaligned_i16
);
