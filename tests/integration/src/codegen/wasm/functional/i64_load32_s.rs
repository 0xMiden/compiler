use super::*;

#[test]
fn i64_load32_s() {
    let span = SourceSpan::default();
    let mem_addr = 17 * 2u32.pow(16);

    let (package, context) = compile_test_module([Type::I32], [Type::I64], |builder| {
        let block = builder.current_block();
        let addr_i32 = block.borrow().arguments()[0] as ValueRef;
        let result = builder.i64_load32_s(addr_i32, None, span).unwrap();

        builder.ret(Some(result), span).unwrap();
    });

    // (value written to memory, expected result from i64.load32_s)
    let test_cases = [
        (
            0b0111_1111_1111_1111_1111_1111_1111_1111u32,
            0b0000_0000_0000_0000_0000_0000_0000_0000_0111_1111_1111_1111_1111_1111_1111_1111u64,
        ),
        (
            0b1000_0000_0000_0000_0000_0000_0000_0000u32,
            0b1111_1111_1111_1111_1111_1111_1111_1111_1000_0000_0000_0000_0000_0000_0000_0000u64,
        ),
        (
            0b1111_1111_1111_1111_1111_1111_1111_1111u32,
            0b1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111u64,
        ),
        (
            0b0000_0000_0000_0000_0000_0000_0000_0000u32,
            0b0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000u64,
        ),
        (
            0b1000_0000_0000_0000_0000_0000_0000_0001u32,
            0b1111_1111_1111_1111_1111_1111_1111_1111_1000_0000_0000_0000_0000_0000_0000_0001u64,
        ),
    ];

    for (mem_value, expected) in test_cases {
        assert_eq!(((mem_value as i32) as i64) as u64, expected, "invalid test case");

        let initializers = [Initializer::MemoryBytes {
            addr: mem_addr,
            bytes: &mem_value.to_le_bytes(),
        }];

        let output = eval_package::<u64, _, _>(
            &package,
            initializers,
            &[Felt::new(mem_addr as u64)],
            context.session(),
            |_trace| Ok(()),
        )
        .unwrap();

        assert_eq!(
            output, expected,
            "i64.load32_s failed for input 0b{:032b}: expected 0b{:064b}, got 0b{:064b}",
            mem_value, expected, output
        );
    }
}
