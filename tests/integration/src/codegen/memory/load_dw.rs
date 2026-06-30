use super::*;

/// Tests the memory load intrinsic for aligned loads of double-word (i.e. 64-bit) values
#[test]
fn load_dw() {
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes `load_dw` when lowered to MASM
    // Compile once outside the test loop
    let (package, context) =
        compile_test_module([Type::from(PointerType::new(Type::U64))], [Type::U64], |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<u64>(), random_word_aligned_addr()),
        move |(value, write_to)| {
            // Felts must be written in little-endian order: lo at lower address.
            let value_felts = value.to_felts();
            let initializers = [Initializer::MemoryFelts {
                addr: write_to / 4,
                felts: Cow::Borrowed(&value_felts),
            }];

            let args = [Felt::new_unchecked(write_to as u64)];
            let output = eval_package::<u64, _, _>(
                &package,
                initializers,
                &args,
                context.session(),
                |trace| {
                    let lo = trace
                        .read_memory_element(write_to / 4)
                        .unwrap_or_default()
                        .as_canonical_u64();
                    let hi = trace
                        .read_memory_element((write_to / 4) + 1)
                        .unwrap_or_default()
                        .as_canonical_u64();

                    log::trace!(target: "executor", "hi = {hi} ({hi:0x})");
                    log::trace!(target: "executor", "lo = {lo} ({lo:0x})");

                    prop_assert_eq!(lo, value & 0xffffffff);
                    prop_assert_eq!(hi, value >> 32);

                    let stored = trace.read_from_rust_memory::<u64>(write_to).ok_or_else(|| {
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
