se super::*;

/// Tests the memory load intrinsic for aligned and unaligned loads of quad-word values
fn load_qw_with_offset_impl<T: QuadwordIO>() {
    setup::enable_compiler_instrumentation();

    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack).
    let read_from = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that invokes `load_qw` when lowered to MASM.
    // The parameter is the offset to be applied to the base address (`read_from`).
    // Compile once outside the test loop.
    let (package, context) = compile_test_module([Type::U32], [T::hir_type()], |builder| {
        let block = builder.current_block();

        let offs = block.borrow().arguments()[0] as ValueRef;
        let base_addr = builder.u32(read_from, SourceSpan::default());
        let read_addr = builder.add(base_addr, offs, SourceSpan::default()).unwrap();
        let ptr = builder
            .inttoptr(read_addr, Type::from(PointerType::new(T::hir_type())), SourceSpan::default())
            .unwrap();
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();

        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    let initial_bytes = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    let run_test = |offs: u32| {
        let initializers = [Initializer::MemoryBytes {
            addr: read_from,
            bytes: &initial_bytes,
        }];

        let start = offs as usize;
        let expected = T::from_le_bytes(
            initial_bytes[start..start + 16].try_into().expect("expected a 16-byte window"),
        );

        let output = eval_package::<T, _, _>(
            &package,
            initializers,
            &[Felt::new_unchecked(offs as u64)],
            context.session(),
            |_trace| Ok(()),
        )
        .unwrap();

        assert_eq!(output, expected, "offset {offs}");
    };

    for offs in 0..=15 {
        run_test(offs);
    }
}

#[test]
fn load_qw_with_offset_i128() {
    load_qw_with_offset_impl::<i128>();
}

#[test]
fn load_qw_with_offset_u128() {
    load_qw_with_offset_impl::<u128>();
}

/// Tests the memory load intrinsic for aligned loads of quad-word (i.e. 128-bit) values
fn load_qw_impl<T>()
where
    T: QuadwordIO + Arbitrary + ToMidenRepr + 'static,
{
    setup::enable_compiler_instrumentation();

    // Generate a `test` module with `main` function that invokes `load_qw` when lowered to MASM
    // Compile once outside the test loop
    let (package, context) = compile_test_module(
        [Type::from(PointerType::new(T::hir_type()))],
        [T::hir_type()],
        |builder| {
            let block = builder.current_block();
            // Get the input pointer, and load the value at that address
            let ptr = block.borrow().arguments()[0] as ValueRef;
            let loaded = builder.load(ptr, SourceSpan::default()).unwrap();
            // Return the value so we can assert that the output of execution matches
            builder.ret(Some(loaded), SourceSpan::default()).unwrap();
        },
    );

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(
        &(any::<T>(), random_word_aligned_addr()),
        move |(value, write_to)| {
            // Felts must be written in little-endian order: lo at lower address.
            let value_felts = value.to_felts();
            let initializers = [Initializer::MemoryFelts {
                addr: write_to / 4,
                felts: Cow::Borrowed(&value_felts),
            }];

            let args = [Felt::new_unchecked(write_to as u64)];
            let output = eval_package::<T, _, _>(
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

                    let uvalue = u128::from_le_bytes(value.to_le_bytes());
                    prop_assert_eq!(e0, uvalue as u64 & 0xffffffff);
                    prop_assert_eq!(e1, (uvalue >> 32) as u64 & 0xffffffff);
                    prop_assert_eq!(e2, (uvalue >> 64) as u64 & 0xffffffff);
                    prop_assert_eq!(e3, (uvalue >> 96) as u64 & 0xffffffff);

                    let stored = trace.read_rust_memory::<T>(write_to).ok_or_else(|| {
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

            prop_assert_eq!(output, value, "expected 0x{:x?}; found 0x{:x?}", value, output,);

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

#[test]
fn load_qw_i128() {
    load_qw_impl::<i128>();
}

#[test]
fn load_qw_u128() {
    load_qw_impl::<u128>();
}
