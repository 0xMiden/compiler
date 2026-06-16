use super::*;

#[test]
fn store_u64_unaligned() {
    // Use the start of the 17th page (1 page after the 16 pages reserved for the Rust stack)
    let write_to = 17 * 2u32.pow(16);

    // Value which in turn will be little-endian bytes [ AA BB CC DD EE FF AB CD ] at addr.
    let write_val = 0xcdabffee_ddccbbaa_u64;

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
            .inttoptr(write_addr, Type::from(PointerType::new(Type::U64)), SourceSpan::default())
            .unwrap();

        // Store test value to pointer.
        let write_val = builder.u64(write_val, SourceSpan::default());
        builder.store(ptr, write_val, SourceSpan::default()).unwrap();

        // Return a constant to satisfy test infrastructure
        let result = builder.u32(1, SourceSpan::default());
        builder.ret(Some(result), SourceSpan::default()).unwrap();
    });

    let run_test = |offs: u32, expected0: u32, expected1: u32, expected2: u32, expected3: u32| {
        // Initialise memory with some known bytes.
        let initializers = [Initializer::MemoryBytes {
            addr: write_to,
            bytes: &[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
                0x17, 0x18,
            ],
        }];

        let output = eval_package::<u32, _, _>(
            &package,
            initializers,
            &[Felt::new_unchecked(offs as u64)],
            context.session(),
            |trace| {
                // Get the overwritten words.
                let word0 = trace.read_from_rust_memory::<u32>(write_to).unwrap();
                let word1 = trace.read_from_rust_memory::<u32>(write_to + 4).unwrap();
                let word2 = trace.read_from_rust_memory::<u32>(write_to + 8).unwrap();
                let word3 = trace.read_from_rust_memory::<u32>(write_to + 12).unwrap();

                eprintln!("word0: 0x{word0:0>8x}");
                eprintln!("word1: 0x{word1:0>8x}");
                eprintln!("word2: 0x{word2:0>8x}");
                eprintln!("word3: 0x{word3:0>8x}");

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

                assert_eq!(
                    word2, expected2,
                    "expected 3rd overwritten word to be {expected2}, got {word2}, with offset \
                     {offs}"
                );

                assert_eq!(
                    word3, expected3,
                    "expected 4th overwritten word to be {expected3}, got {word3}, with offset \
                     {offs}"
                );

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(output, 1);
    };

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes aa bb cc dd ee ff ab cd at offset 0:
    //   Expect     aa bb cc dd ee ff ab cd 11 12 13 14 15 16 17 18
    //   or         0xccbbaa01, 0xabffeedd, 0x141312cd, 0x18171615
    run_test(0, 0xddccbbaa, 0xcdabffee, 0x14131211, 0x18171615);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 1:
    //   Expect     01 aa bb cc dd ee ff ab cd 12 13 14 15 16 17 18
    //   or         0xccbbaa01, 0xabffeedd, 0x141312cd, 0x18171615
    run_test(1, 0xccbbaa01, 0xabffeedd, 0x141312cd, 0x18171615);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 2:
    //   Expect     01 02 aa bb cc dd ee ff ab cd 13 14 15 16 17 18
    //   or         0xbbaa0201, 0xffeeddcc, 0x1413cdab, 0x18171615
    run_test(2, 0xbbaa0201, 0xffeeddcc, 0x1413cdab, 0x18171615);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 3:
    //   Expect     01 02 03 aa bb cc dd ee ff ab cd 14 15 16 17 18
    //   or         0xaa030201, 0xeeddccbb, 0x14cdabff, 0x18171615
    run_test(3, 0xaa030201, 0xeeddccbb, 0x14cdabff, 0x18171615);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 4:
    //   Expect     01 02 03 04 aa bb cc dd ee ff ab cd 15 16 17 18
    //   or         0x04030201, 0xddccbbaa, 0xcdabffee, 0x18171615
    run_test(4, 0x04030201, 0xddccbbaa, 0xcdabffee, 0x18171615);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 5:
    //   Expect     01 02 03 04 05 aa bb cc dd ee ff ab cd 16 17 18
    //   or         0x04030201, 0xccbbaa05, 0xabffeedd, 0x181716cd
    run_test(5, 0x04030201, 0xccbbaa05, 0xabffeedd, 0x181716cd);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 6:
    //   Expect     01 02 03 04 05 06 aa bb cc dd ee ff ab cd 17 18
    //   or         0x04030201, 0xbbaa0605, 0xffeeddcc, 0x1817cdab
    run_test(6, 0x04030201, 0xbbaa0605, 0xffeeddcc, 0x1817cdab);

    // Overwrite    01 02 03 04 05 06 07 08-11 12 13 14 15 16 17 18
    //   with bytes    aa bb cc dd ee ff ab cd at offset 7:
    //   Expect     01 02 03 04 05 06 07 aa bb cc dd ee ff ab cd 18
    //   or         0x04030201, 0xaa070605, 0xeeddccbb, 0x18cdabff
    run_test(7, 0x04030201, 0xaa070605, 0xeeddccbb, 0x18cdabff);
}
