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
        &package,
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
            &package,
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
        &package,
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
        &package,
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
        &package,
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
            &package,
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
        &package,
        None,
        &[Felt::from(MEMORY_ADDR), Felt::from(value)],
        context.session(),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, value);
}
