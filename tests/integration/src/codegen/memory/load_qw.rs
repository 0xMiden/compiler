use super::*;

/// Tests the memory load intrinsic for aligned and unaligned loads of quad-word values
#[test]
fn load_qw_with_offset() {
    setup::enable_compiler_instrumentation();

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack).
    let read_from = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes `load_qw` when lowered to MASM.
    // The parameter is the offset to be applied to the base address (`read_from`).
    // Compile once outside the test loop.
    let (package, context) = compile_test_module([Type::U32], [Type::I128], |builder| {
        let block = builder.current_block();

        let offs = block.borrow().arguments()[0] as ValueRef;
        let base_addr = builder.u32(read_from, SourceSpan::default());
        let read_addr = builder.add(base_addr, offs, SourceSpan::default()).unwrap();
        let ptr = builder
            .inttoptr(read_addr, Type::from(PointerType::new(Type::I128)), SourceSpan::default())
            .unwrap();
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();

        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    let initial_bytes = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14,
    ];

    let run_test = |offs: u32| {
        let initializers = [Initializer::MemoryBytes {
            addr: read_from,
            bytes: &initial_bytes,
        }];

        let start = offs as usize;
        let expected = i128::from_le_bytes(
            initial_bytes[start..start + 16].try_into().expect("expected a 16-byte window"),
        );

        let output = eval_package::<i128, _, _>(
            &package,
            initializers,
            &[Felt::new(offs as u64)],
            context.session(),
            |_trace| Ok(()),
        )
        .unwrap();

        assert_eq!(output, expected, "offset {offs}");
    };

    for offs in 0..=3 {
        run_test(offs);
    }
}

/// Tests the memory load intrinsic for aligned loads of quad-word (i.e. 128-bit) values
#[test]
fn load_qw() {
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes `load_qw` when lowered to MASM
    // Compile once outside the test loop
    let (package, context) =
        compile_test_module([Type::from(PointerType::new(Type::I128))], [Type::I128], |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        });

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<i128>(), random_word_aligned_addr()),
        move |(value, write_to)| {
            // Felts must be written in little-endian order: lo at lower address.
            let value_felts = value.to_felts();
            let initializers = [Initializer::MemoryFelts {
                addr: write_to / 4,
                felts: Cow::Borrowed(&value_felts),
            }];

            let args = [Felt::new(write_to as u64)];
            let output = eval_package::<i128, _, _>(
                &package,
                initializers,
                &args,
                context.session(),
                |trace| {
                    let base_addr = write_to / 4;
                    let e0 =
                        trace.read_memory_element(base_addr).unwrap_or_default().as_canonical_u64();
                    let e1 = trace
                        .read_memory_element(base_addr + 1)
                        .unwrap_or_default()
                        .as_canonical_u64();
                    let e2 = trace
                        .read_memory_element(base_addr + 2)
                        .unwrap_or_default()
                        .as_canonical_u64();
                    let e3 = trace
                        .read_memory_element(base_addr + 3)
                        .unwrap_or_default()
                        .as_canonical_u64();

                    log::trace!(target: "executor", "e0 = {e0} ({e0:0x})");
                    log::trace!(target: "executor", "e1 = {e1} ({e1:0x})");
                    log::trace!(target: "executor", "e2 = {e2} ({e2:0x})");
                    log::trace!(target: "executor", "e3 = {e3} ({e3:0x})");

                    let uvalue = value as u128;
                    prop_assert_eq!(e0, uvalue as u64 & 0xffffffff);
                    prop_assert_eq!(e1, (uvalue >> 32) as u64 & 0xffffffff);
                    prop_assert_eq!(e2, (uvalue >> 64) as u64 & 0xffffffff);
                    prop_assert_eq!(e3, (uvalue >> 96) as u64 & 0xffffffff);

                    let stored =
                        trace.read_from_rust_memory::<i128>(write_to).ok_or_else(|| {
                            TestCaseError::fail(format!(
                                "expected {value} to have been written to byte address \
                                 {write_to}, but read from that address failed"
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
