//! Tests for alignment-aware Wasm memory address preparation.

use std::panic::{AssertUnwindSafe, catch_unwind};

use miden_debug::DebugQuery;

use super::*;

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

    // The byte->element conversion must be done inline (a CSE pass may legally merge the two
    // conversions, hence no exact count), without falling back to the byte-space intrinsics.
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

/// The number of `assertz` instructions in the procedure `entrypoint` of `masm`.
fn assertz_count(masm: &str) -> usize {
    let start = masm.find("proc entrypoint").unwrap_or_else(|| panic!("no entrypoint: {masm}"));
    let entrypoint = &masm[start..];
    let end = entrypoint.find("\n    end").unwrap_or(entrypoint.len());
    entrypoint[..end].matches("assertz").count()
}

/// Memory holding `values` as little-endian `u32`s.
fn seeded(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_le_bytes()).collect()
}

/// Verifies that Wasm element-aligned i32 accesses through one base, at offsets 0 and 4, reach the
/// two cells and share one alignment check: the base is converted once and the offset added in
/// elements.
///
/// The base is read from its local four times in a row, so the reads are merged into one value
/// before the accesses: a read after an access is not merged with one before it, the access's
/// alignment check being a side effect.
#[test]
fn wasm_aligned_i32_accesses_through_one_base_share_one_alignment_check() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32 i32 i32) (result i32)
    (local i32)
    local.get 0
    local.get 0
    local.get 0
    local.get 0
    local.get 1
    i32.store offset=0 align=4
    local.get 2
    i32.store offset=4 align=4
    i32.load offset=4 align=4
    local.set 3
    i32.load offset=0 align=4
    local.get 3
    i32.add)"#,
    );
    assert_eq!(assertz_count(&masm), 1, "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");

    let (first, second) = (0x1234_5678u32, 0x0102_0304u32);
    let args = |base: u32| [Felt::from(base), Felt::from(first), Felt::from(second)];
    let result = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &args(MEMORY_ADDR),
        &test.session,
        |trace| {
            let stored: [u32; 2] = [
                trace
                    .read_from_rust_memory(MEMORY_ADDR)
                    .expect("the first cell was not written"),
                trace
                    .read_from_rust_memory(MEMORY_ADDR + 4)
                    .expect("the second cell was not written"),
            ];
            assert_eq!(stored, [first, second]);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, first + second, "{masm}");

    // The shared check still rejects a misaligned base.
    let misaligned = catch_unwind(AssertUnwindSafe(|| {
        let _ = eval_package::<u32, _, _>(
            package.clone(),
            None,
            &args(MEMORY_ADDR + 2),
            &test.session,
            |_| Ok(()),
        );
    }));
    assert!(misaligned.is_err(), "an alignment promise must still be checked at runtime");
}

/// Verifies that Wasm element-aligned i32 loads through one base (the parameter of an `if`) in
/// the two arms are each checked: alignment checks are only shared within a block.
#[test]
fn wasm_aligned_i32_loads_in_two_arms_are_checked_in_each_arm() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    if (param i32) (result i32)
      i32.load offset=0 align=4
    else
      i32.load offset=4 align=4
    end)"#,
    );
    assert_eq!(assertz_count(&masm), 2, "{masm}");

    let (first, second) = (0xaaaa_5555u32, 0x5a5a_a5a5u32);
    let bytes = seeded(&[first, second]);
    for (cond, expected) in [(1u32, first), (0, second)] {
        let result = eval_package::<u32, _, _>(
            package.clone(),
            [Initializer::MemoryBytes {
                addr: MEMORY_ADDR,
                bytes: &bytes,
            }],
            &[Felt::from(MEMORY_ADDR), Felt::from(cond)],
            &test.session,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(result, expected, "cond {cond}\n{masm}");
    }
}

/// Verifies that a Wasm `i32.load offset=2 align=2` from a base that is 2 modulo 4 reads the
/// element at the element-aligned effective address.
#[test]
fn wasm_two_aligned_load_with_offset_2_reads_the_effective_address() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    i32.load offset=2 align=2)"#,
    );

    const DECOY: u32 = 0xdead_beef;
    const EXPECTED: u32 = 0x1357_9bdf;
    let bytes = seeded(&[DECOY, EXPECTED, DECOY]);
    let result = eval_package::<u32, _, _>(
        package,
        [Initializer::MemoryBytes {
            addr: MEMORY_ADDR,
            bytes: &bytes,
        }],
        &[Felt::from(MEMORY_ADDR + 2)],
        &test.session,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, EXPECTED, "{masm}");
}

/// Verifies that a Wasm `i32.load offset=4 align=1`, whose offset is element-aligned but whose
/// promised alignment is below the element alignment, reads the four bytes at the unaligned
/// effective address from an odd base.
#[test]
fn wasm_one_aligned_load_with_offset_4_reads_from_an_odd_base() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    i32.load offset=4 align=1)"#,
    );

    let bytes: Vec<u8> = (1..=16).collect();
    let expected = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
    let result = eval_package::<u32, _, _>(
        package,
        [Initializer::MemoryBytes {
            addr: MEMORY_ADDR,
            bytes: &bytes,
        }],
        &[Felt::from(MEMORY_ADDR + 1)],
        &test.session,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, expected, "{masm}");
}
