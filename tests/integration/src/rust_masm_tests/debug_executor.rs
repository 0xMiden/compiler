use miden_core::Felt;

use crate::{CompilerTest, testing::eval_package};

#[test]
fn test_println() {
    let main_fn = "(a: u32, b: u32) -> u32 { a + b }";
    let mut test = CompilerTest::rust_fn_body(&main_fn, None);

    let package = test.compile_package();

    let args = [Felt::from(5u32), Felt::from(7u32)];
    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: Felt = trace.parse_result().unwrap();
        assert_eq!(result, Felt::from(12u32));
        Ok(())
    })
    .unwrap();
}
