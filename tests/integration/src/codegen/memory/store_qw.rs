use super::*;

fn store_qw_unaligned_impl<T: QuadwordIO>(write_val: T) {
    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack).
    let write_to = 17 * 2u32.pow(16);

    // Signature of the test module:
    //
    // * takes one argument: `offset`
    // * returns `1` to indicate success
    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let block = builder.current_block();
        let idx_val = block.borrow().arguments()[0] as ValueRef;

        let base_addr = builder.u32(write_to, SourceSpan::default());
        let write_addr = builder.add(base_addr, idx_val, SourceSpan::default()).unwrap();
        let ptr = builder
            .inttoptr(
                write_addr,
                Type::from(PointerType::new(T::hir_type())),
                SourceSpan::default(),
            )
            .unwrap();

        let write_val = builder.imm(T::as_immediate(&write_val), SourceSpan::default());
        builder.store(ptr, write_val, SourceSpan::default()).unwrap();

        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let run_test = |offs: u32| {
        // Write known 20 bytes (5xu32) at `addr` before running the module.
        // Run the module with different offsets and afterwards verify storing 16 bytes inside the
        // module has modified memory as expected.
        let initial_bytes = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &initial_bytes,
        }];

        let mut expected_bytes = initial_bytes;
        let start = offs as usize;
        expected_bytes[start..start + 16].copy_from_slice(&write_val.to_le_bytes());
        let expected = [
            u32::from_le_bytes(expected_bytes[0..4].try_into().unwrap()),
            u32::from_le_bytes(expected_bytes[4..8].try_into().unwrap()),
            u32::from_le_bytes(expected_bytes[8..12].try_into().unwrap()),
            u32::from_le_bytes(expected_bytes[12..16].try_into().unwrap()),
            u32::from_le_bytes(expected_bytes[16..20].try_into().unwrap()),
        ];

        let output = eval_package::<u32, _, _>(
            &package,
            initializers,
            &[Felt::new(offs as u64)],
            context.session(),
            |trace| {
                let actual = [
                    trace.read_from_rust_memory::<u32>(write_to).unwrap(),
                    trace.read_from_rust_memory::<u32>(write_to + 4).unwrap(),
                    trace.read_from_rust_memory::<u32>(write_to + 8).unwrap(),
                    trace.read_from_rust_memory::<u32>(write_to + 12).unwrap(),
                    trace.read_from_rust_memory::<u32>(write_to + 16).unwrap(),
                ];

                for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
                    assert_eq!(
                        actual, expected,
                        "expected overwritten word {index} to be 0x{expected:0>8x}, got \
                         0x{actual:0>8x}, with offset {offs}"
                    );
                }

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(output, 1);
    };

    for offs in 0..=3 {
        run_test(offs);
    }
}

#[test]
fn store_qw_unaligned_i128() {
    store_qw_unaligned_impl(0x00112233_44556677_8899aabb_ccddeeff_i128);
}

#[test]
fn store_qw_unaligned_u128() {
    store_qw_unaligned_impl(0x00112233_44556677_8899aabb_ccddeeff_u128);
}
