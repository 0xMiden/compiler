//! Tests for alignment-aware Wasm memory address preparation.

use std::panic::{AssertUnwindSafe, catch_unwind};

use miden_debug::DebugQuery;

use super::*;
use crate::{
    CompilerTest,
    trap_helpers::{panic_message, trap_matches},
};

const MEMORY_ADDR: u32 = 17 * 2u32.pow(16);

/// Verifies that naturally aligned i32 accesses use element-space pointers and retain the trap.
#[test]
fn aligned_i32_memory_uses_element_addresses() {
    let span = SourceSpan::default();
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], |builder| {
            let (addr, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                (block.arguments()[0] as ValueRef, block.arguments()[1] as ValueRef)
            };
            let memarg = Some(WasmMemArg::new(4, 2));

            let store_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            builder.store(store_addr, value, span).unwrap();
            let load_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            let result = builder.load(load_addr, span).unwrap();
            builder.ret(Some(result), span).unwrap();
        });

    // The byte->element conversion must be done inline, without falling back to the byte-space
    // intrinsics.
    assert!(masm.contains("u32divmod"), "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::store_sw"), "{masm}");

    let value = 0x1234_5678;
    let result = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(MEMORY_ADDR), Felt::from(value)],
        context.session(),
        |trace| {
            // Independently verify the store hit the expected cell: a symmetric addressing bug
            // (e.g. an element-space pointer still carrying the byte address) would round-trip
            // through the load below while writing somewhere else entirely.
            let stored: u32 = trace
                .read_from_rust_memory(MEMORY_ADDR + 4)
                .expect("the target memory cell was not written");
            assert_eq!(stored, value);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, value);

    let misaligned = catch_unwind(AssertUnwindSafe(|| {
        let _ = eval_package::<u32, _, _>(
            package.clone(),
            None,
            &[Felt::from(MEMORY_ADDR + 1), Felt::from(value)],
            context.session(),
            |_| Ok(()),
        );
    }));
    assert!(misaligned.is_err(), "an alignment promise must still be checked at runtime");
}

/// Verifies the element path computes the correct element address: a load through an
/// element-space `u32` pointer must read exactly the seeded cell, not a decoy neighbor (as it
/// would if the static offset were ignored) nor a mis-scaled address.
#[test]
fn aligned_u32_load_reads_expected_element() {
    let span = SourceSpan::default();
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32], [Type::U32], |builder| {
            let addr = {
                let block = builder.current_block();
                let block = block.borrow();
                block.arguments()[0] as ValueRef
            };
            let memarg = Some(WasmMemArg::new(4, 2));

            let load_addr = prepare_addr(addr, &Type::U32, memarg, builder, span).unwrap();
            let result = builder.load(load_addr, span).unwrap();
            builder.ret(Some(result), span).unwrap();
        });

    assert!(!masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");

    // Seed the element at the effective address (MEMORY_ADDR + 4) and a decoy at MEMORY_ADDR
    // with distinct values, so an addressing bug cannot round-trip accidentally.
    const DECOY: u32 = 0xaaaa_5555;
    const EXPECTED: u32 = 0x5a5a_a5a5;
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&DECOY.to_le_bytes());
    bytes[4..].copy_from_slice(&EXPECTED.to_le_bytes());
    let initializers = [Initializer::MemoryBytes {
        addr: MEMORY_ADDR,
        bytes: &bytes,
    }];

    let result = eval_package::<u32, _, _>(
        package,
        initializers,
        &[Felt::from(MEMORY_ADDR)],
        context.session(),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, EXPECTED);
}

/// Verifies that naturally aligned felt accesses use element-space pointers and move whole field
/// elements (which need not fit in 32 bits).
#[test]
fn aligned_felt_memory_uses_element_addresses() {
    let span = SourceSpan::default();
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32, Type::Felt], [Type::Felt], |builder| {
            let (addr, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                (block.arguments()[0] as ValueRef, block.arguments()[1] as ValueRef)
            };
            let memarg = Some(WasmMemArg::new(4, 2));

            let store_addr = prepare_addr(addr, &Type::Felt, memarg, builder, span).unwrap();
            builder.store(store_addr, value, span).unwrap();
            let load_addr = prepare_addr(addr, &Type::Felt, memarg, builder, span).unwrap();
            let result = builder.load(load_addr, span).unwrap();
            builder.ret(Some(result), span).unwrap();
        });

    assert!(masm.contains("u32divmod"), "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::load_felt"), "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::store_felt"), "{masm}");

    // A value wider than 32 bits proves the access moves whole field elements.
    let value = Felt::new_unchecked(0x1234_5678_9abc);
    let result = eval_package::<Felt, _, _>(
        package,
        None,
        &[Felt::from(MEMORY_ADDR), value],
        context.session(),
        |trace| {
            let stored = trace
                .read_memory_element((MEMORY_ADDR + 4) / 4)
                .expect("the target memory cell was not written");
            assert_eq!(stored, value);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, value);
}

/// Verifies that accesses promising less than element alignment keep byte-space lowering while
/// still enforcing the promised (smaller) alignment at runtime.
#[test]
fn underaligned_i32_memory_keeps_byte_path_and_checks_alignment() {
    let span = SourceSpan::default();
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], |builder| {
            let (addr, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                (block.arguments()[0] as ValueRef, block.arguments()[1] as ValueRef)
            };
            let memarg = Some(WasmMemArg::new(2, 1));

            let store_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            builder.store(store_addr, value, span).unwrap();
            let load_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            let result = builder.load(load_addr, span).unwrap();
            builder.ret(Some(result), span).unwrap();
        });

    assert!(masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");
    assert!(masm.contains("exec.::intrinsics::mem::store_sw"), "{masm}");

    // The effective address MEMORY_ADDR + 2 honors the promised 2-byte alignment.
    let value = 0x0bad_f00d;
    let result = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(MEMORY_ADDR), Felt::from(value)],
        context.session(),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, value);

    // An odd base address violates the promised 2-byte alignment and must trap.
    let misaligned = catch_unwind(AssertUnwindSafe(|| {
        let _ = eval_package::<u32, _, _>(
            package.clone(),
            None,
            &[Felt::from(MEMORY_ADDR + 1), Felt::from(value)],
            context.session(),
            |_| Ok(()),
        );
    }));
    assert!(misaligned.is_err(), "a sub-element alignment promise must still be checked");
}

/// Verifies that potentially unaligned i32 accesses retain byte-space pointer lowering.
#[test]
fn unaligned_i32_memory_retains_byte_pointer_path() {
    let span = SourceSpan::default();
    let (masm, package, context) =
        compile_test_module_with_masm([Type::I32, Type::I32], [Type::I32], |builder| {
            let (addr, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                (block.arguments()[0] as ValueRef, block.arguments()[1] as ValueRef)
            };
            let memarg = Some(WasmMemArg::new(1, 0));

            let store_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            builder.store(store_addr, value, span).unwrap();
            let load_addr = prepare_addr(addr, &Type::I32, memarg, builder, span).unwrap();
            let result = builder.load(load_addr, span).unwrap();
            builder.ret(Some(result), span).unwrap();
        });

    assert!(masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");
    assert!(masm.contains("exec.::intrinsics::mem::store_sw"), "{masm}");

    let value = 0x89ab_cdef;
    let result = eval_package::<u32, _, _>(
        package,
        None,
        &[Felt::from(MEMORY_ADDR), Felt::from(value)],
        context.session(),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, value);
}

/// A Wasm function `(addr, v0, v1, v2) -> i32` that, in its entry block and through the same
/// `addr`, stores `v0`, `v1` and `v2` at element-aligned memarg offsets 0, 4 and 8, then loads
/// offset 8 and returns it.
///
/// Built from Wasm rather than with `compile_test_module_with_masm`: hand-built HIR skips the
/// rewrite pipeline, which is where accesses through one base come to share a conversion.
const SHARED_BASE_ACCESSES: &str = r#"(module
  (memory 1)
  (func (export "entrypoint") (param i32 i32 i32 i32) (result i32)
    local.get 0 local.get 1 i32.store offset=0 align=4
    local.get 0 local.get 2 i32.store offset=4 align=4
    local.get 0 local.get 3 i32.store offset=8 align=4
    local.get 0 i32.load offset=8 align=4))"#;

/// The values stored at memarg offsets 0, 4 and 8 by [SHARED_BASE_ACCESSES].
const SHARED_BASE_VALUES: [u32; 3] = [0x1111_2222, 0x3333_4444, 0x5555_6666];

/// The failure message of a `u32` operation given an operand of 2^32 or more.
const U32_OVERFLOW_FAILURE: &str = "operation expected u32 values";

/// The failure message of an element-aligned access through a misaligned address.
const ALIGNMENT_FAILURE: &str = "pointer address does not meet minimum alignment for the type";

/// Compiles the Wasm module `wat`, which exports `entrypoint`, through the full pipeline.
fn compile_wat(wat: &str) -> CompilerTest {
    let wasm = wat::parse_str(wat).unwrap();
    CompilerTest::from_wasm(
        "shared_base",
        wasm,
        ["--test-harness".into(), "--entrypoint".into(), "shared_base::entrypoint".into()],
    )
}

/// The arguments of the entrypoint of a module compiled by [compile_wat].
fn felt_args(args: &[u32]) -> Vec<Felt> {
    args.iter().copied().map(Felt::from).collect()
}

/// Evaluates the entrypoint of `test` with `args`, handing `check` a reader of the final memory.
fn eval_wat(
    test: &mut CompilerTest,
    args: &[u32],
    check: impl Fn(&dyn Fn(u32) -> Option<u32>),
) -> u32 {
    let package = test.compile_package();
    eval_package::<u32, _, _>(package, None, &felt_args(args), &test.session, |trace| {
        check(&|addr| trace.read_from_rust_memory(addr));
        Ok(())
    })
    .unwrap()
}

/// Evaluates [SHARED_BASE_ACCESSES] at `addr`, handing `check` a reader of the final memory.
fn eval_shared_base_accesses(
    test: &mut CompilerTest,
    addr: u32,
    check: impl Fn(&dyn Fn(u32) -> Option<u32>),
) -> u32 {
    let [v0, v1, v2] = SHARED_BASE_VALUES;
    eval_wat(test, &[addr, v0, v1, v2], check)
}

/// Evaluates [SHARED_BASE_ACCESSES] at `addr`, which must fail, and returns the failure message.
fn shared_base_accesses_failure(addr: u32) -> String {
    let mut test = compile_wat(SHARED_BASE_ACCESSES);
    // Compiled outside `catch_unwind`, so that a compiler failure is not taken for a trap.
    let package = test.compile_package();
    let [v0, v1, v2] = SHARED_BASE_VALUES;
    let args = felt_args(&[addr, v0, v1, v2]);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        eval_package::<u32, _, _>(package, None, &args, &test.session, |_| Ok(()))
    }));
    match outcome {
        Err(panic) => panic_message(panic),
        Ok(Err(err)) => err.to_string(),
        Ok(Ok(result)) => panic!("expected a failure at {addr:#x}, got {result:#x}"),
    }
}

/// Returns the procedure `name` of `masm`, up to its column-0 `end`.
fn masm_procedure<'a>(masm: &'a str, name: &str) -> &'a str {
    let start = masm
        .find(&format!("proc {name}("))
        .unwrap_or_else(|| panic!("no procedure `{name}` in\n{masm}"));
    let body = &masm[start..];
    let end = body.find("\nend").unwrap_or(body.len());
    &body[..end]
}

/// Verifies that element-aligned accesses through one base share a single byte->element
/// conversion, one overflow check of the base plus the largest offset and one alignment check,
/// and still write the expected cells.
#[test]
fn element_accesses_through_one_base_share_one_conversion() {
    let mut test = compile_wat(SHARED_BASE_ACCESSES);
    let masm = test.masm_src();
    let entry = masm_procedure(&masm, "entrypoint");
    assert_eq!(entry.matches("u32divmod").count(), 1, "{entry}");
    // The `u32assert` of the shared addition is the overflow check: `u32divmod` does not
    // constrain its dividend to 32 bits in a proof, so without it a prover could pick an element
    // address of 2^30 or more. It must stay exactly one, not "at least one".
    assert_eq!(entry.matches("u32assert").count(), 1, "{entry}");
    assert_eq!(entry.matches(ALIGNMENT_FAILURE).count(), 1, "{entry}");

    let result = eval_shared_base_accesses(&mut test, MEMORY_ADDR, |read| {
        for (index, expected) in SHARED_BASE_VALUES.into_iter().enumerate() {
            let addr = MEMORY_ADDR + 4 * index as u32;
            let stored = read(addr).expect("the target memory cell was not written");
            assert_eq!(stored, expected, "memory at {addr:#x}");
        }
    });
    assert_eq!(result, SHARED_BASE_VALUES[2]);
}

/// Verifies that the shared conversion reaches the last element below byte address 2^32: the
/// accesses at `0xFFFF_FFF4`, `0xFFFF_FFF8` and `0xFFFF_FFFC` are all valid.
#[test]
fn shared_conversion_reaches_the_last_byte_addressable_element() {
    let mut test = compile_wat(SHARED_BASE_ACCESSES);
    let result = eval_shared_base_accesses(&mut test, 0xffff_fff4, |read| {
        for (index, expected) in SHARED_BASE_VALUES.into_iter().enumerate() {
            let addr = 0xffff_fff4 + 4 * index as u32;
            let stored = read(addr).expect("the target memory cell was not written");
            assert_eq!(stored, expected, "memory at {addr:#x}");
        }
    });
    assert_eq!(result, SHARED_BASE_VALUES[2]);
}

/// Verifies that the shared conversion traps on the overflow of the base plus the largest offset
/// when only that offset overflows: at base `0xFFFF_FFF8` offsets 0 and 4 stay below 2^32, but
/// offset 8 does not.
#[test]
fn shared_conversion_traps_when_base_plus_offset_overflows() {
    let err = shared_base_accesses_failure(0xffff_fff8);
    assert!(err.contains(U32_OVERFLOW_FAILURE), "unexpected overflow failure: {err}");
}

/// Verifies that the shared conversion still checks the element alignment of the base.
#[test]
fn shared_conversion_keeps_the_alignment_check() {
    let err = shared_base_accesses_failure(MEMORY_ADDR + 2);
    assert!(trap_matches(&err, ALIGNMENT_FAILURE), "unexpected alignment failure: {err}");
}

/// A Wasm function `(addr0, addr1, v0, v1, v2) -> i32` that stores `v0` and `v1` at memarg
/// offsets 0 and 4 through local 0 holding `addr0`, sets local 0 to `addr1`, then stores `v2` at
/// offset 8 through local 0, loads it back and returns it.
const LOCAL_SET_SPLIT_ACCESSES: &str = r#"(module
  (memory 1)
  (func (export "entrypoint") (param i32 i32 i32 i32 i32) (result i32)
    local.get 0 local.get 2 i32.store offset=0 align=4
    local.get 0 local.get 3 i32.store offset=4 align=4
    local.get 1 local.set 0
    local.get 0 local.get 4 i32.store offset=8 align=4
    local.get 0 i32.load offset=8 align=4))"#;

/// Verifies that accesses through a local do not share a conversion across a `local.set` of that
/// local: sharing one would store `v2` at `addr0 + 8` rather than at `addr1 + 8`.
#[test]
fn local_set_splits_the_accesses_through_a_local() {
    let mut test = compile_wat(LOCAL_SET_SPLIT_ACCESSES);
    let masm = test.masm_src();
    let entry = masm_procedure(&masm, "entrypoint");
    assert_eq!(entry.matches("u32divmod").count(), 2, "{entry}");

    let [v0, v1, v2] = SHARED_BASE_VALUES;
    let addr0 = MEMORY_ADDR;
    let addr1 = MEMORY_ADDR + 64;
    let result = eval_wat(&mut test, &[addr0, addr1, v0, v1, v2], |read| {
        for (addr, expected) in [(addr0, v0), (addr0 + 4, v1), (addr1 + 8, v2)] {
            let stored = read(addr).expect("the target memory cell was not written");
            assert_eq!(stored, expected, "memory at {addr:#x}");
        }
        assert_ne!(read(addr0 + 8), Some(v2), "v2 was stored through the stale base");
    });
    assert_eq!(result, v2);
}

/// A Wasm function `(addr0, addr1, cond) -> i32` that stores 7 at offset 0 through local 0
/// holding `addr0`, sets local 0 to `addr1` in a branch taken when `cond` is not zero, then stores
/// 9 at offset 4 through local 0 and returns the value at `addr1 + 4`.
const LOCAL_SET_IN_BRANCH_ACCESSES: &str = r#"(module
  (memory 1)
  (func (export "entrypoint") (param i32 i32 i32) (result i32)
    local.get 0 i32.const 7 i32.store offset=0 align=4
    local.get 2 (if (then local.get 1 local.set 0))
    local.get 0 i32.const 9 i32.store offset=4 align=4
    local.get 1 i32.load offset=4 align=4))"#;

/// Verifies that a `local.set` of the base in a branch between two accesses through a local also
/// keeps them from sharing a conversion: sharing one would store 9 at `addr0 + 4`.
#[test]
fn local_set_in_a_branch_splits_the_accesses_through_a_local() {
    let mut test = compile_wat(LOCAL_SET_IN_BRANCH_ACCESSES);
    let addr0 = MEMORY_ADDR;
    let addr1 = MEMORY_ADDR + 64;
    let result = eval_wat(&mut test, &[addr0, addr1, 1], |read| {
        assert_eq!(read(addr0), Some(7), "memory at {addr0:#x}");
        assert_ne!(read(addr0 + 4), Some(9), "9 was stored through the stale base");
    });
    assert_eq!(result, 9);
}

/// Verifies that element-aligned accesses through a constant base materialized at each access
/// write the expected cells.
#[test]
fn element_accesses_through_a_constant_base() {
    let wat = format!(
        r#"(module
  (memory 1)
  (func (export "entrypoint") (param i32 i32 i32) (result i32)
    i32.const {MEMORY_ADDR} local.get 0 i32.store offset=0 align=4
    i32.const {MEMORY_ADDR} local.get 1 i32.store offset=4 align=4
    i32.const {MEMORY_ADDR} local.get 2 i32.store offset=8 align=4
    i32.const {MEMORY_ADDR} i32.load offset=8 align=4))"#
    );
    let mut test = compile_wat(&wat);
    let result = eval_wat(&mut test, &SHARED_BASE_VALUES, |read| {
        for (index, expected) in SHARED_BASE_VALUES.into_iter().enumerate() {
            let addr = MEMORY_ADDR + 4 * index as u32;
            let stored = read(addr).expect("the target memory cell was not written");
            assert_eq!(stored, expected, "memory at {addr:#x}");
        }
    });
    assert_eq!(result, SHARED_BASE_VALUES[2]);
}
