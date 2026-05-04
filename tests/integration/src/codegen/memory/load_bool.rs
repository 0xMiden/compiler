use super::*;

/// Tests the memory load intrinsic for loads of boolean (i.e. 1-bit) values
#[test]
fn load_bool() {
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes load for bool when lowered to MASM
    // Compile once outside the test loop
    let (package, context) =
        compile_test_module([Type::from(PointerType::new(Type::I1))], [Type::I1], |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<bool>(), random_word_aligned_addr()),
        move |(value, write_to)| {
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
                            "expected {value} to have been written to byte address {write_to}, \
                             but read from that address failed"
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

            prop_assert_eq!(output, value, "expected {}; found {}", output, value);

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
