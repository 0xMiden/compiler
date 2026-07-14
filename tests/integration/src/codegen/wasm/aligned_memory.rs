//! Tests for alignment-aware Wasm memory address preparation.

use std::panic::{AssertUnwindSafe, catch_unwind};

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

    assert_eq!(masm.matches("u32divmod").count(), 2, "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::load_sw"), "{masm}");
    assert!(!masm.contains("exec.::intrinsics::mem::store_sw"), "{masm}");

    let value = 0x1234_5678;
    let result = eval_package::<u32, _, _>(
        &package,
        None,
        &[Felt::from(MEMORY_ADDR), Felt::from(value)],
        context.session(),
        |_| Ok(()),
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
