use miden_core::{Felt, StarkField};
use miden_debug::ToMidenRepr;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::eval_package};

#[test]
fn test_felt_construction_edge_cases() {
    // Return `0` zero as sentinel value for failure of constructing the Felt.
    // This avoids having to deal with panics.
    let main_fn = r#"(x: u64) -> Felt {
        Felt::new(x).unwrap_or(Felt::new(0u64).unwrap())
    }
    "#;
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "felt_constructor_edge_case",
        main_fn,
        config,
        None,
    );
    let package = test.compile_package();

    // (input, expected_out) with `expected_out = None` if Felt construction is expected to fail.
    let cases: Vec<(u64, Option<u64>)> = Vec::from([
        (1, Some(1)),
        (Felt::MODULUS - 1, Some(Felt::MODULUS - 1)),
        // failure as these values do not fit into Felt
        (Felt::MODULUS, None),
        (Felt::MODULUS + 1, None),
        (u64::MAX, None),
    ]);

    for (input, expected_out) in cases {
        let mut args = Vec::<midenc_hir::Felt>::default();
        input.push_to_operand_stack(&mut args);
        eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
            let expected_out = match expected_out {
                Some(out) => {
                    assert!(
                        out != 0,
                        "don't explicitly expect 0, it is the sentinel value for error"
                    );
                    out
                }
                None => 0,
            };

            let res: Felt = trace.parse_result().unwrap();
            println!("input: {input}");
            assert_eq!(res.as_int(), expected_out);
            Ok(())
        })
        .unwrap();
    }
}
