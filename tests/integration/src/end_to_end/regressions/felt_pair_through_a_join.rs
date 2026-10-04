use std::sync::Arc;

use miden_core::Felt;
use miden_debug::{DebugQuery, Felt as TestFelt};
use miden_mast_package::Package;
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};

use crate::{CompilerTest, CompilerTestBuilder, testing::eval_package};

/// Byte address of the pair the entrypoints store or read.
const PAIR_ADDR: u32 = 64;

/// Byte address at which the unpacking entrypoints store the low half they took from the pair.
const LOW_HALF_ADDR: u32 = 128;

/// Byte address at which `a_felt_pair_used_whole_and_taken_apart_is_intact` stores the pair whole.
const COPY_ADDR: u32 = 192;

/// Four felts, none of which fits in 32 bits: the pair each branch takes.
fn pairs() -> [[Felt; 2]; 2] {
    [
        [
            Felt::new_unchecked(u64::MAX - u64::from(u32::MAX)),
            Felt::new_unchecked(1 << 40),
        ],
        [Felt::new_unchecked(1 << 63), Felt::new_unchecked((1 << 32) + 5)],
    ]
}

/// Compile `wat`, whose `entrypoint` takes the branch to follow (nonzero for the first pair) and
/// the two pairs, as four `f32`s.
fn compile(wat: String) -> (CompilerTest, Arc<Package>) {
    let wasm = wat::parse_str(wat).unwrap();
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let mut test = builder.build();
    let package = test.compile_package();
    (test, package)
}

/// The entrypoint's arguments for the branch that takes pair `index`.
fn args(index: usize) -> [Felt; 5] {
    let [[a, b], [c, d]] = pairs();
    [Felt::new_unchecked(u64::from(index == 0)), a, b, c, d]
}

/// The first two elements of the word at the byte address `addr`.
fn read_pair(trace: &miden_debug::ExecutionTrace, addr: u32) -> [Felt; 2] {
    let word: [TestFelt; 4] = trace
        .read_from_rust_memory(addr)
        .expect("the pair should be readable from memory");
    [word[0].0, word[1].0]
}

/// Two felts assembled into one 64-bit integer with `extend`/`shl`/`or` in each of two branches,
/// the integer carried out of the `if` and stored, reach memory intact. This is the form
/// sub-project 3 hit, where two rebuilt `Word`s met at a branch: the assembled value reaches the
/// store through a join, so splitting the store does not apply. The backend instead joins each
/// pair as two limbs, which emits no 64-bit operation.
#[test]
fn a_felt_pair_assembled_in_two_branches_reaches_memory_intact() {
    let assemble = |lo: u32, hi: u32| {
        format!(
            "local.get {lo}
      i32.reinterpret_f32
      i64.extend_i32_u
      local.get {hi}
      i32.reinterpret_f32
      i64.extend_i32_u
      i64.const 32
      i64.shl
      i64.or"
        )
    };
    let (first, second) = (assemble(1, 2), assemble(3, 4));
    let (test, package) = compile(format!(
        r#"(module
  (memory 1)
  (func $entrypoint (export "entrypoint") (param i32 f32 f32 f32 f32) (result f32)
    i32.const {PAIR_ADDR}
    local.get 0
    if (result i64)
      {first}
    else
      {second}
    end
    i64.store
    i32.const {PAIR_ADDR}
    f32.load offset=4))"#
    ));

    for (index, [lo, hi]) in pairs().into_iter().enumerate() {
        let reloaded_hi =
            eval_package::<Felt, _, _>(package.clone(), [], &args(index), &test.session, |trace| {
                assert_eq!(read_pair(trace, PAIR_ADDR), [lo, hi]);
                Ok(())
            })
            .unwrap();
        assert_eq!(reloaded_hi, hi);
    }
}

/// Two felts read as one 64-bit integer in each of two branches, the integer carried out of the
/// `if` and taken apart with `i32.wrap_i64` and `i64.shr_u` by 32, come back intact. The integer
/// reaches its uses through a join, so splitting the load does not apply. The backend instead
/// takes both halves from a split of the integer into two limbs, which emits no 64-bit `shr`.
#[test]
fn a_felt_pair_that_arrives_through_a_join_is_taken_apart_intact() {
    let (test, package) = compile(format!(
        r#"(module
  (memory 1)
  (func $entrypoint (export "entrypoint") (param i32 f32 f32 f32 f32) (result f32)
    (local i64)
    i32.const {PAIR_ADDR}
    local.get 1
    f32.store
    i32.const {PAIR_ADDR}
    local.get 2
    f32.store offset=4
    i32.const {PAIR_ADDR}
    local.get 3
    f32.store offset=8
    i32.const {PAIR_ADDR}
    local.get 4
    f32.store offset=12
    local.get 0
    if (result i64)
      i32.const {PAIR_ADDR}
      i64.load
    else
      i32.const {PAIR_ADDR}
      i64.load offset=8
    end
    local.set 5
    i32.const {LOW_HALF_ADDR}
    local.get 5
    i32.wrap_i64
    f32.reinterpret_i32
    f32.store
    local.get 5
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    f32.reinterpret_i32))"#
    ));

    for (index, [lo, hi]) in pairs().into_iter().enumerate() {
        let read_hi =
            eval_package::<Felt, _, _>(package.clone(), [], &args(index), &test.session, |trace| {
                assert_eq!(read_pair(trace, LOW_HALF_ADDR)[0], lo);
                Ok(())
            })
            .unwrap();
        assert_eq!(read_hi, hi);
    }
}

/// A felt pair that arrives through a join, is stored whole and is also taken apart, is intact
/// both ways: the high half comes from a split of the integer, made where that half is taken,
/// and the low half's `i32.wrap_i64` and the store still get the integer whole.
#[test]
fn a_felt_pair_used_whole_and_taken_apart_is_intact() {
    let (test, package) = compile(format!(
        r#"(module
  (memory 1)
  (func $entrypoint (export "entrypoint") (param i32 f32 f32 f32 f32) (result f32)
    (local i64)
    i32.const {PAIR_ADDR}
    local.get 1
    f32.store
    i32.const {PAIR_ADDR}
    local.get 2
    f32.store offset=4
    i32.const {PAIR_ADDR}
    local.get 3
    f32.store offset=8
    i32.const {PAIR_ADDR}
    local.get 4
    f32.store offset=12
    local.get 0
    if (result i64)
      i32.const {PAIR_ADDR}
      i64.load
    else
      i32.const {PAIR_ADDR}
      i64.load offset=8
    end
    local.set 5
    i32.const {LOW_HALF_ADDR}
    local.get 5
    i32.wrap_i64
    f32.reinterpret_i32
    f32.store
    i32.const {COPY_ADDR}
    local.get 5
    i64.store
    local.get 5
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    f32.reinterpret_i32))"#
    ));

    for (index, [lo, hi]) in pairs().into_iter().enumerate() {
        let read_hi =
            eval_package::<Felt, _, _>(package.clone(), [], &args(index), &test.session, |trace| {
                assert_eq!(read_pair(trace, LOW_HALF_ADDR)[0], lo);
                assert_eq!(read_pair(trace, COPY_ADDR), [lo, hi]);
                Ok(())
            })
            .unwrap();
        assert_eq!(read_hi, hi);
    }
}
