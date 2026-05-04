use super::*;

#[test]
fn load_u64_unaligned() {
    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Generate a `test` module with `main` function that loads from `write_to` + a passed offset.
    // Compile once outside the test loop
    let (package, context) = compile_test_module([Type::U32], [Type::U64], |builder| {
        let block = builder.current_block();

        // Get the offset, add it to the base address and load the 64bit value there.
        let offs = block.borrow().arguments()[0] as ValueRef;
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let read_addr = builder.add(base_addr, offs, SourceSpan::default()).unwrap();
        let ptr = builder
            .inttoptr(read_addr, Type::from(PointerType::new(Type::U64)), SourceSpan::default())
            .unwrap();
        let loaded = builder.load(ptr, SourceSpan::default()).unwrap();

        // Return the value so we can assert that the output of execution matches
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    });

    let run_test = |offs: u32, expected: u64| {
        // Initialise memory with some known bytes.
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
                0x17, 0x18,
            ],
        }];

        let output = eval_package::<u64, _, _>(
            &package,
            initializers,
            &[Felt::new(offs as u64)],
            context.session(),
            |trace| {
                //
                let stack = trace.outputs();
                let hi: u64 = stack.get_element(0).unwrap().as_canonical_u64();
                let lo: u64 = stack.get_element(1).unwrap().as_canonical_u64();

                eprintln!("hi limb = 0x{hi:08x}");
                eprintln!("lo limb = 0x{lo:08x}");

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(output, expected);
    };

    run_test(0, 0x0807060504030201_u64);
    run_test(1, 0x1108070605040302_u64);
    run_test(2, 0x1211080706050403_u64);
    run_test(3, 0x1312110807060504_u64);
    run_test(4, 0x1413121108070605_u64);
    run_test(5, 0x1514131211080706_u64);
    run_test(6, 0x1615141312110807_u64);
    run_test(7, 0x1716151413121108_u64);
}
