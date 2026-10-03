use miden_core::Felt;
use miden_debug::{DebugQuery, Felt as TestFelt};
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};

use crate::{CompilerTestBuilder, testing::eval_package};

/// Byte address at which the entrypoint stores the pair.
const PAIR_ADDR: u32 = 64;

/// Two felts stored as one 64-bit integer, assembled with `extend`/`shl`/`or` the way LLVM's
/// IR-level passes can carry a pair of `f32`s, reach memory intact. Found in sub-project 3, where
/// the batch kernel trapped in the 64-bit `shl`/`or` on a felt outside the `u32` range; where the
/// assembled value feeds the store directly, as here, the backend stores the two halves
/// separately. A pair that reaches its store through a join is not covered by that rewrite.
#[test]
fn a_felt_pair_stored_as_one_merged_integer_reaches_memory_intact() {
    let wasm = wat::parse_str(format!(
        r#"(module
  (memory 1)
  (func $entrypoint (export "entrypoint") (param f32 f32) (result f32)
    i32.const {PAIR_ADDR}
    local.get 0
    i32.reinterpret_f32
    i64.extend_i32_u
    local.get 1
    i32.reinterpret_f32
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.or
    i64.store
    i32.const {PAIR_ADDR}
    f32.load offset=4))"#
    ))
    .unwrap();
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let mut test = builder.build();
    let package = test.compile_package();

    // Neither fits in 32 bits
    let lo = Felt::new_unchecked(u64::MAX - u64::from(u32::MAX));
    let hi = Felt::new_unchecked(1 << 40);
    let reloaded_hi = eval_package::<Felt, _, _>(package, [], &[lo, hi], &test.session, |trace| {
        // The word the pair begins
        let word: [TestFelt; 4] = trace
            .read_from_rust_memory(PAIR_ADDR)
            .expect("the pair should be readable from memory");
        assert_eq!([word[0].0, word[1].0], [lo, hi]);
        Ok(())
    })
    .unwrap();
    assert_eq!(reloaded_hi, hi);
}
