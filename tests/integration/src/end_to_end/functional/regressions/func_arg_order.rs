use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::eval_package};

#[ignore = "too fragile (depends on mem addrs), this bug is also covered by the test_hmerge test"]
#[test]
fn test_func_arg_order() {
    // This test reproduces the "swapped and frozen" function arguments issue
    // https://github.com/0xMiden/compiler/pull/576 discovered while working on hmerge VM op
    // The issue manifests in "intrinsic" function parameters being in the wrong order
    // (see assert_eq before the call and inside the function)
    // on the stack AND the their order is not changing when the parameters are
    // swapped at the call site (see expect_masm with the same file name, i.e. the MASM
    // do not change when she parameters are swapped).
    fn main_fn_template(digest_ptr_name: &str, result_ptr_name: &str) -> String {
        format!(
            r#"
    (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {{
        let digest1 = miden_stdlib_sys::Digest::new([f0, f1, f2, f3]);
        let digest2 = miden_stdlib_sys::Digest::new([f4, f5, f6, f7]);
        let digests = [digest1, digest2];
        let res = merge(digests);
        res.inner[0]
    }}

    #[inline]
    pub fn merge(digests: [Digest; 2]) -> Digest {{
        unsafe {{
            let digests_ptr = digests.as_ptr().addr() as u32;
            assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));

            let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
            let result_ptr = ret_area.as_mut_ptr().addr() as u32;
            assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));

            intrinsic({digest_ptr_name} as *const Felt, {result_ptr_name} as *mut Felt);

            Digest::from_word(ret_area.assume_init())
        }}
    }}

    #[unsafe(no_mangle)]
    fn intrinsic(digests_ptr: *const Felt, result_ptr: *mut Felt) {{
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));
    }}
        "#
        )
    }

    let config = WasmTranslationConfig::default();
    let main_fn_correct = main_fn_template("digests_ptr", "result_ptr");
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "func_arg_order",
        &main_fn_correct,
        config.clone(),
        [],
    );

    let args = [
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
    ];

    eval_package::<Felt, _, _>(&test.compile_package(), [], &args, &test.session, |_trace| Ok(()))
        .unwrap();
}
