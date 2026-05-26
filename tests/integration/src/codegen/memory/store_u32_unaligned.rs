use super::*;

#[test]
fn store_u32_unaligned() {
    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);
    let write_val = 0xddccbbaa_u32; // Little-endian bytes will be [AA BB CC DD].

    // Generate a `test` module with `main` function that stores to a u32 offset.
    // Return u32 to satisfy test infrastructure
    // Compile once outside the test loop
    let (package, context) = compile_test_module([Type::U32], [Type::U32], |builder| {
        let block = builder.current_block();
        let idx_val = block.borrow().arguments()[0] as ValueRef;

        // Set base pointer, add argument offset to it.
        let base_addr = builder.u32(write_to, SourceSpan::default());
        let write_addr = builder.add(base_addr, idx_val, SourceSpan::default()).unwrap();
        let ptr = builder
            .inttoptr(write_addr, Type::from(PointerType::new(Type::U32)), SourceSpan::default())
            .unwrap();

        // Store test value to pointer.
        let write_val = builder.u32(write_val, SourceSpan::default());
        builder.store(ptr, write_val, SourceSpan::default()).unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let run_test = |offs: u32, expected0: u32, expected1: u32| {
        // Initialise memory with some known bytes.
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        }];

        let output = eval_package::<u32, _, _>(
            &package,
            initializers,
            &[Felt::new_unchecked(offs as u64)],
            context.session(),
            |trace| {
                // Get the overwritten words.
                let word0 = trace.read_rust_memory::<u32>(write_to).unwrap();
                let word1 = trace.read_rust_memory::<u32>(write_to + 4).unwrap();

                eprintln!("word0: 0x{word0:0>8x}");
                eprintln!("word1: 0x{word1:0>8x}");

                assert_eq!(
                    word0, expected0,
                    "expected 1st overwritten word to be {expected0}, got {word0}, with offset \
                     {offs}"
                );

                assert_eq!(
                    word1, expected1,
                    "expected 2nd overwritten word to be {expected1}, got {word1}, with offset \
                     {offs}"
                );

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(output, 1);
    };

    // Overwrite 11 22 33 44 55 66 77 88 with bytes aa bb cc dd at offset 1:
    //  Expect 11 aa bb cc | dd 66 77 88
    //  or 0xccbbaa11 and 0x887766dd.
    run_test(1, 0xccbbaa11, 0x887766dd);

    // Overwrite 11 22 33 44 55 66 77 88 with bytes aa bb cc dd at offset 2:
    //  Expect 11 22 aa bb | cc dd 77 88
    //  or 0xbbaa2211 and 0x8877ddcc.
    run_test(2, 0xbbaa2211, 0x8877ddcc);

    // Overwrite 11 22 33 44 55 66 77 88 with bytes aa bb cc dd at offset 3:
    //  Expect 11 22 33 aa | bb cc dd 88
    //  or 0xaa332211 and 0x88ddccbb.
    run_test(3, 0xaa332211, 0x88ddccbb);
}
