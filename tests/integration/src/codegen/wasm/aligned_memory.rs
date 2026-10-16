//! Tests for alignment-aware Wasm memory address preparation.

use std::panic::{AssertUnwindSafe, catch_unwind};

use miden_debug::DebugQuery;
use midenc_dialect_wasm::{BASE_FIRST_OVERFLOW_GUARD, MAX_BASE_FIRST_OFFSET};

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

/// The number of `assertz` instructions in the procedure `entrypoint` of `masm`, nested blocks
/// included.
fn assertz_count(masm: &str) -> usize {
    let start = masm.find("proc entrypoint").unwrap_or_else(|| panic!("no entrypoint: {masm}"));
    let entrypoint = &masm[start..];
    // The procedure's own `end` is the first one at column 0; nested blocks' are indented.
    let end = entrypoint.find("\nend").unwrap_or(entrypoint.len());
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

/// The first element address past the 2^32-byte guest address space.
const GUEST_ADDRESS_LIMIT: u32 = 1 << 30;

/// The element address of the heap metadata word (`heap_top`, `heap_size`, `heap_base`, magic),
/// placed just above the overflow guard.
const HEAP_INFO_ADDR: u32 = GUEST_ADDRESS_LIMIT + BASE_FIRST_OVERFLOW_GUARD;

/// Verifies that Wasm element-aligned stores whose `base + offset` leaves the 32-bit address space
/// land in the overflow guard and do not reach the heap metadata, so `memory.size` still reports
/// the heap size of a run through an in-range base.
///
/// The data segment makes the program initialize the heap, which `memory.size` checks.
#[test]
fn wasm_overflowing_aligned_store_misses_the_heap_metadata() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    i32.const 0x600DCAFE
    i32.store offset=20 align=4
    local.get 0
    i32.const 777
    i32.store offset=12 align=4
    memory.size)
(data (i32.const 1024) "\01")"#,
    );
    // The offsets are applied in elements after the conversion, with an unchecked add.
    assert!(!masm.contains("u32assert"), "{masm}");

    let in_range = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(MEMORY_ADDR)],
        &test.session,
        |_| Ok(()),
    )
    .unwrap();

    let overflowing = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(0xffff_fff8u32)],
        &test.session,
        |trace| {
            // The base is element 2^30 - 2, so the offsets of 5 and 3 elements land just past the
            // guest address space. The payloads differ from the heap magic (0xDEADBEEF), so an
            // intact magic cell is not mistaken for a store that landed on it.
            let cell = |addr: u32| trace.read_memory_element(addr).unwrap_or_default();
            assert_eq!(cell(GUEST_ADDRESS_LIMIT + 3), Felt::from(0x600d_cafeu32));
            assert_eq!(cell(GUEST_ADDRESS_LIMIT + 1), Felt::from(777u32));
            assert_eq!(cell(HEAP_INFO_ADDR + 3), Felt::from(0xdead_beefu32), "heap magic");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(overflowing, in_range, "the heap size was overwritten\n{masm}");
}

/// Verifies that a Wasm element-aligned access with an offset at [MAX_BASE_FIRST_OFFSET] adds the
/// offset to the base with an overflow check: it reaches the effective address from an in-range
/// base, and traps when `base + offset` leaves the 32-bit address space.
#[test]
fn wasm_aligned_store_with_a_large_offset_takes_the_checked_path() {
    assert_eq!(MAX_BASE_FIRST_OFFSET, 1 << 26);
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.store offset=67108864 align=4
    local.get 0
    i32.load offset=67108864 align=4)"#,
    );
    // The byte offset is added to the base with an overflow check, before the conversion.
    assert!(masm.contains("push.67108864\n    add\n    u32assert\n"), "{masm}");

    let value = 0x2468_ace0u32;
    let result = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(0u32), Felt::from(value)],
        &test.session,
        |trace| {
            let stored: u32 = trace
                .read_from_rust_memory(MAX_BASE_FIRST_OFFSET)
                .expect("the target memory cell was not written");
            assert_eq!(stored, value);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, value, "{masm}");

    let overflowing = catch_unwind(AssertUnwindSafe(|| {
        let _ = eval_package::<u32, _, _>(
            package.clone(),
            None,
            &[Felt::from(0xffff_fffcu32), Felt::from(value)],
            &test.session,
            |_| Ok(()),
        );
    }));
    assert!(overflowing.is_err(), "an overflowing `base + offset` must trap\n{masm}");
}

/// Verifies that Wasm element-aligned accesses with the largest offset below
/// [MAX_BASE_FIRST_OFFSET] convert the base first, sharing one alignment check, and reach the
/// effective address. A second run from the last aligned base, whose `base + offset` overflows,
/// lands on the highest guard element without trapping and leaves the heap metadata intact.
///
/// The base is read from its local twice before the accesses, so the reads are merged into one
/// value (see `wasm_aligned_i32_accesses_through_one_base_share_one_alignment_check`). The data
/// segment makes the program initialize the heap, so the second run has a magic cell to check.
#[test]
fn wasm_aligned_store_with_offset_below_the_bound_takes_the_fast_path() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (param i32 i32) (result i32)
    local.get 0
    local.get 0
    local.get 1
    i32.store offset=67108860 align=4
    i32.load offset=67108860 align=4)
(data (i32.const 1024) "\01")"#,
    );
    assert_eq!(assertz_count(&masm), 1, "{masm}");
    assert!(!masm.contains("u32assert"), "{masm}");

    let value = 0x1357_9bdfu32;
    let result = eval_package::<u32, _, _>(
        package.clone(),
        None,
        &[Felt::from(0u32), Felt::from(value)],
        &test.session,
        |trace| {
            let stored: u32 = trace
                .read_from_rust_memory(MAX_BASE_FIRST_OFFSET - 4)
                .expect("the target memory cell was not written");
            assert_eq!(stored, value);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, value, "{masm}");

    // From the last aligned base, element 2^30 - 1, the access lands on the highest guard element
    // it can reach, just below the heap metadata, without trapping: the highest base element
    // 2^30 - 1 plus the largest element offset (2^26 - 4) / 4 = 2^24 - 1. A literal, so that the
    // test pins the layout.
    let guard_end = 0x40ff_fffeu32;
    let result = eval_package::<u32, _, _>(
        package,
        None,
        &[Felt::from(0xffff_fffcu32), Felt::from(value)],
        &test.session,
        |trace| {
            let stored = trace
                .read_memory_element(guard_end)
                .expect("the guard memory cell was not written");
            assert_eq!(stored, Felt::from(value));
            let magic = trace.read_memory_element(HEAP_INFO_ADDR + 3).unwrap_or_default();
            assert_eq!(magic, Felt::from(0xdead_beefu32), "heap magic");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, value, "{masm}");
}
