use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;
use proptest::{
    prelude::*,
    test_runner::{TestError, TestRunner},
};

use crate::{CompilerTest, testing::eval_package};

#[test]
fn test_hmerge() {
    let main_fn = r#"
	        (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {
	            let digest1 = miden_stdlib_sys::Digest::new([f0, f1, f2, f3]);
	            let digest2 = miden_stdlib_sys::Digest::new([f4, f5, f6, f7]);
	            let digests = [digest1, digest2];
	            let res = miden_stdlib_sys::intrinsics::crypto::merge(digests);
	            res.inner[0]
	        }"#
	        .to_string();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("hmerge", &main_fn, config, []);

    let package = test.compile_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(16);
    let res = TestRunner::new(config).run(
        &any::<([miden_debug::Felt; 4], [miden_debug::Felt; 4])>(),
        move |(felts_in1, felts_in2)| {
            let raw_felts_in1: [Felt; 4] = [
                felts_in1[0].into(),
                felts_in1[1].into(),
                felts_in1[2].into(),
                felts_in1[3].into(),
            ];

            let raw_felts_in2: [Felt; 4] = [
                felts_in2[0].into(),
                felts_in2[1].into(),
                felts_in2[2].into(),
                felts_in2[3].into(),
            ];
            let digests_in =
                [miden_core::Word::from(raw_felts_in1), miden_core::Word::from(raw_felts_in2)];
            let digest_out = miden_core::crypto::hash::Poseidon2::merge(&digests_in);

            let felts_out: [miden_debug::Felt; 4] = [
                miden_debug::Felt(digest_out[0]),
                miden_debug::Felt(digest_out[1]),
                miden_debug::Felt(digest_out[2]),
                miden_debug::Felt(digest_out[3]),
            ];

            let args = [
                raw_felts_in1[0],
                raw_felts_in1[1],
                raw_felts_in1[2],
                raw_felts_in1[3],
                raw_felts_in2[0],
                raw_felts_in2[1],
                raw_felts_in2[2],
                raw_felts_in2[3],
            ];
            eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
                let res: [miden_debug::Felt; 4] = trace.parse_result().unwrap();
                prop_assert_eq!(res, felts_out);
                Ok(())
            })?;

            Ok(())
        },
    );

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {value:?}");
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}
