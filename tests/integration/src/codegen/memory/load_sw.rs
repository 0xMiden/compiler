use super::*;

/// Tests the memory load intrinsic for aligned loads of single-word (i.e. 32-bit) values
#[test]
fn load_sw() {
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes `load_sw` when lowered to MASM
    // Compile once outside the test loop
    let (package, context) =
        compile_test_module([Type::from(PointerType::new(Type::U32))], [Type::U32], |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<u32>(), random_word_aligned_addr()),
        move |(value, write_to)| {
            let value_bytes = value.to_ne_bytes();
            let initializers = [Initializer::MemoryBytes {
                addr: write_to,
                bytes: &value_bytes,
            }];

            let args = [Felt::new_unchecked(write_to as u64)];
            let output = eval_package::<u32, _, _>(
                package.clone(),
                initializers,
                &args,
                context.session(),
                |trace| {
                    let stored = trace.read_from_rust_memory::<u32>(write_to).ok_or_else(|| {
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
