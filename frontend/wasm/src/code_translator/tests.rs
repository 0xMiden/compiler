use core::fmt::Write;
use std::rc::Rc;

use midenc_expect_test::expect_file;
use midenc_hir::{Op, Operation, WalkResult, dialects::builtin};

use crate::{WasmTranslationConfig, translate};

/// Check IR generated for a Wasm op(s).
/// Wrap Wasm ops in a function and check the IR generated for the entry block of that function.
fn check_op(wat_op: &str, expected_ir: midenc_expect_test::ExpectFile) {
    let ctx = midenc_hir::Context::default();
    let context = Rc::new(ctx);

    let wat = format!(
        r#"
        (module
            (memory (;0;) 16384)
            (global $MyGlobalVal (mut i32) i32.const 42)
            (func $test_wrapper
                {wat_op}
            )
            (export "test_wrapper" (func $test_wrapper))
        )"#,
    );
    let wasm = wat::parse_str(wat).unwrap();
    let output = translate(&wasm, &WasmTranslationConfig::default(), context.clone())
        .map_err(|e| {
            if let Some(labels) = e.labels() {
                for label in labels {
                    eprintln!("{}", label.label().unwrap());
                }
            }
            let report = midenc_session::diagnostics::PrintDiagnostic::new(e).to_string();
            eprintln!("{report}");
        })
        .unwrap();

    let component = output.component.borrow();
    let mut w = String::new();
    component
        .as_operation()
        .prewalk(|op: &Operation| {
            if let Some(_function) = op.downcast_ref::<builtin::Function>() {
                match writeln!(&mut w, "{op}") {
                    Ok(_) => WalkResult::Skip,
                    Err(err) => WalkResult::Break(err),
                }
            } else {
                WalkResult::Continue(())
            }
        })
        .into_result()
        .unwrap();

    expected_ir.assert_eq(&w);
}

/// Check IR generated for a complete Wasm module.
/// Unlike [check_op], prints every `builtin.module` wholesale, including module-level items such
/// as function tables, so tests can cover more than function bodies.
fn check_module(wat: &str, expected_ir: midenc_expect_test::ExpectFile) {
    let context = Rc::new(midenc_hir::Context::default());

    let wasm = wat::parse_str(wat).unwrap();
    let output = translate(&wasm, &WasmTranslationConfig::default(), context.clone())
        .map_err(|e| {
            if let Some(labels) = e.labels() {
                for label in labels {
                    eprintln!("{}", label.label().unwrap());
                }
            }
            let report = midenc_session::diagnostics::PrintDiagnostic::new(e).to_string();
            eprintln!("{report}");
        })
        .unwrap();

    let component = output.component.borrow();
    let mut w = String::new();
    component
        .as_operation()
        .prewalk(|op: &Operation| {
            if op.is::<builtin::Module>() {
                match writeln!(&mut w, "{op}") {
                    Ok(_) => WalkResult::Skip,
                    Err(err) => WalkResult::Break(err),
                }
            } else {
                WalkResult::Continue(())
            }
        })
        .into_result()
        .unwrap();

    expected_ir.assert_eq(&w);
}

/// Check that translating a complete Wasm module fails with an error containing `expected_msg`.
fn check_module_err(wat: &str, expected_msg: &str) {
    let context = Rc::new(midenc_hir::Context::default());
    let wasm = wat::parse_str(wat).unwrap();
    let msg = match translate(&wasm, &WasmTranslationConfig::default(), context) {
        Ok(_) => panic!("expected translation to fail"),
        Err(err) => format!("{err}"),
    };
    assert!(
        msg.contains(expected_msg),
        "expected error containing '{expected_msg}', got: {msg}"
    );
}

#[test]
fn call_indirect() {
    check_module(
        r#"
        (module
            (type $binop (func (param i32 i32) (result i32)))
            (table 3 3 funcref)
            (elem (i32.const 1) func $add $mul)
            (memory (;0;) 16384)
            (func $add (type $binop) (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add)
            (func $mul (type $binop) (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.mul)
            (func $dispatch (param i32 i32 i32) (result i32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $binop))
            (export "dispatch" (func $dispatch))
        )"#,
        expect_file!["./expected/call_indirect.hir"],
    )
}

/// The final table image must honor Wasm initialization order: the whole-table `(ref.func ..)`
/// default is overwritten by later element segments, and an explicit `ref.null` entry clears a
/// previously initialized slot (so dispatching through it traps instead of calling a stale
/// function).
#[test]
fn call_indirect_ref_null_overwrites_earlier_entry() {
    check_module(
        r#"
        (module
            (type $binop (func (param i32 i32) (result i32)))
            (table 3 3 funcref (ref.func $add))
            (elem (i32.const 1) func $mul)
            (elem (i32.const 2) funcref (ref.null func))
            (memory (;0;) 16384)
            (func $add (type $binop) (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add)
            (func $mul (type $binop) (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.mul)
            (func $dispatch (param i32 i32 i32) (result i32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $binop))
            (export "dispatch" (func $dispatch))
        )"#,
        expect_file!["./expected/call_indirect_ref_null.hir"],
    )
}

/// A table with no statically-initialized entries still lowers: every dispatch through it fails
/// at runtime on the zero MAST root of a null slot, matching Wasm's uninitialized-element trap.
#[test]
fn call_indirect_all_null_table() {
    check_module(
        r#"
        (module
            (type $binop (func (param i32 i32) (result i32)))
            (table 2 2 funcref)
            (memory (;0;) 16384)
            (func $dispatch (param i32 i32 i32) (result i32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $binop))
            (export "dispatch" (func $dispatch))
        )"#,
        expect_file!["./expected/call_indirect_all_null.hir"],
    )
}

#[test]
fn call_indirect_rejects_oversized_table() {
    check_module_err(
        r#"
        (module
            (type $binop (func (param i32 i32) (result i32)))
            (table 2000000 2000000 funcref)
            (elem (i32.const 0) func $add)
            (memory (;0;) 16384)
            (func $add (type $binop) (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add)
            (func $dispatch (param i32 i32 i32) (result i32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $binop))
            (export "dispatch" (func $dispatch))
        )"#,
        "exceeds the supported maximum",
    )
}

#[test]
fn memory_grow() {
    check_op(
        r#"
            i32.const 1
            memory.grow
            drop
        "#,
        expect_file!["expected/memory_grow.hir"],
    )
}

/// The frontend metadata custom section emitted by `#[note_script]`, as WAT.
///
/// Marks `run` as the note-script entrypoint export; see
/// [midenc_frontend_wasm_metadata::WASM_FRONTEND_METADATA_CUSTOM_SECTION_NAME].
const NOTE_SCRIPT_METADATA_WAT: &str = r#"(@custom "rodata,miden_account_component_frontend" "{\"kind\":\"note_script\",\"method_path\":\"ProbeNote::run\",\"export_name\":\"run\"}")"#;

/// The `script_root` note intrinsic (behind the `get_entrypoint_root()` method generated by
/// `#[note]`) requires the frontend metadata emitted by `#[note_script]`; in its absence a call
/// to it must be rejected with an actionable diagnostic rather than compiled to a runtime trap
/// or a wrong digest.
#[test]
fn note_script_root_intrinsic_requires_note_script_metadata() {
    check_module_err(
        r#"
        (module
            (memory (;0;) 1)
            (table 1 1 funcref)
            (elem (i32.const 0) func $run)
            (func $run (param i32))
            (func $"intrinsics::note::script_root" (param i32 i32)
                unreachable)
            (func $probe (param i32)
                i32.const 0
                local.get 0
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
        )"#,
        "requires a `#[note_script]` entrypoint",
    )
}

/// The `script_root` note intrinsic resolves its entrypoint function reference at compile time:
/// a reference that is not a compile-time constant must be rejected with an actionable
/// diagnostic, since the slot it denotes cannot be identified (and repointed at the lifted
/// note-script export).
#[test]
fn note_script_root_intrinsic_requires_a_constant_function_reference() {
    check_module_err(
        &format!(
            r#"
        (module
            {NOTE_SCRIPT_METADATA_WAT}
            (memory (;0;) 1)
            (table 1 1 funcref)
            (elem (i32.const 0) func $run)
            (func $run (param i32))
            (func $"intrinsics::note::script_root" (param i32 i32)
                unreachable)
            (func $probe (param i32 i32)
                local.get 0
                local.get 1
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
        )"#
        ),
        "is not statically resolvable",
    )
}

/// A constant function reference that does not denote an initialized function-table slot cannot
/// yield a note script root and must be rejected with an actionable diagnostic.
#[test]
fn note_script_root_intrinsic_requires_a_resolvable_table_slot() {
    check_module_err(
        &format!(
            r#"
        (module
            {NOTE_SCRIPT_METADATA_WAT}
            (memory (;0;) 1)
            (table 1 1 funcref)
            (func $"intrinsics::note::script_root" (param i32 i32)
                unreachable)
            (func $probe (param i32)
                i32.const 0
                local.get 0
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
        )"#
        ),
        "does not resolve to a function",
    )
}

/// The happy path of the `script_root` note intrinsic: the constant entrypoint function
/// reference resolves to a function-table slot, whose entry is marked as naming the note
/// script (for export lifting to repoint), and the call lowers to a
/// `hir.function_table_root` of that slot plus stores of the digest to the result pointer.
#[test]
fn note_script_root_from_entrypoint_ref() {
    check_module(
        &format!(
            r#"
        (module
            {NOTE_SCRIPT_METADATA_WAT}
            (memory (;0;) 1)
            (table 2 2 funcref)
            (elem (i32.const 1) func $run)
            (func $run (param i32))
            (func $"intrinsics::note::script_root" (param i32 i32)
                unreachable)
            (func $probe (param i32)
                i32.const 1
                local.get 0
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
            (export "run" (func $run))
        )"#
        ),
        expect_file!["./expected/note_script_root_from_entrypoint_ref.hir"],
    )
}

#[test]
fn memory_size() {
    check_op(
        r#"
            memory.size
            drop
        "#,
        expect_file!["./expected/memory_size.hir"],
    )
}

#[test]
fn memory_copy() {
    check_op(
        r#"
            i32.const 20 ;; dst
            i32.const 10 ;; src
            i32.const 1  ;; len
            memory.copy
        "#,
        expect_file!["./expected/memory_copy.hir"],
    )
}

#[test]
fn i32_load8_u() {
    check_op(
        r#"
            i32.const 1024
            i32.load8_u
            drop
        "#,
        expect_file!["./expected/i32_load8_u.hir"],
    )
}

#[test]
fn i32_load16_u() {
    check_op(
        r#"
            i32.const 1024
            i32.load16_u
            drop
        "#,
        expect_file!["./expected/i32_load16_u.hir"],
    )
}

#[test]
fn i32_load8_s() {
    check_op(
        r#"
            i32.const 1024
            i32.load8_s
            drop
        "#,
        expect_file!["./expected/i32_load8_s.hir"],
    )
}

#[test]
fn i32_load16_s() {
    check_op(
        r#"
            i32.const 1024
            i32.load16_s
            drop
        "#,
        expect_file!["./expected/i32_load16_s.hir"],
    )
}

#[test]
fn i64_load8_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load8_u
            drop
        "#,
        expect_file!["./expected/i64_load8_u.hir"],
    )
}

#[test]
fn i64_load16_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load16_u
            drop
        "#,
        expect_file!["./expected/i64_load16_u.hir"],
    )
}

#[test]
fn i64_load8_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load8_s
            drop
        "#,
        expect_file!["./expected/i64_load8_s.hir"],
    )
}

#[test]
fn i64_load16_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load16_s
            drop
        "#,
        expect_file!["./expected/i64_load16_s.hir"],
    )
}

#[test]
fn i64_load32_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load32_s
            drop
        "#,
        expect_file!["./expected/i64_load32_s.hir"],
    )
}

#[test]
fn i64_load32_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load32_u
            drop
        "#,
        expect_file!["./expected/i64_load32_u.hir"],
    )
}

#[test]
fn i32_load() {
    check_op(
        r#"
            i32.const 1024
            i32.load
            drop
        "#,
        expect_file!["./expected/i32_load.hir"],
    )
}

#[test]
fn i64_load() {
    check_op(
        r#"
            i32.const 1024
            i64.load
            drop
        "#,
        expect_file!["./expected/i64_load.hir"],
    )
}

#[test]
fn i32_store() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store
        "#,
        expect_file!["./expected/i32_store.hir"],
    )
}

#[test]
fn i64_store() {
    check_op(
        r#"
            i32.const 1024
            i64.const 1
            i64.store
        "#,
        expect_file!["./expected/i64_store.hir"],
    )
}

#[test]
fn i32_store8() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store8
        "#,
        expect_file!["./expected/i32_store8.hir"],
    )
}

#[test]
fn i32_store16() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store16
        "#,
        expect_file!["./expected/i32_store16.hir"],
    )
}

#[test]
fn i64_store32() {
    check_op(
        r#"
            i32.const 1024
            i64.const 1
            i64.store32
        "#,
        expect_file!["./expected/i64_store32.hir"],
    )
}

#[test]
fn i32_const() {
    check_op(
        r#"
            i32.const 1
            drop
        "#,
        expect_file!["./expected/i32_const.hir"],
    )
}

#[test]
fn i64_const() {
    check_op(
        r#"
            i64.const 1
            drop
        "#,
        expect_file!["./expected/i64_const.hir"],
    )
}

#[test]
fn i32_popcnt() {
    check_op(
        r#"
            i32.const 1
            i32.popcnt
            drop
        "#,
        expect_file!["./expected/i32_popcnt.hir"],
    )
}

#[test]
fn i64_popcnt() {
    check_op(
        r#"
            i64.const 1
            i64.popcnt
            drop
        "#,
        expect_file!["./expected/i64_popcnt.hir"],
    )
}

#[test]
fn i32_clz() {
    check_op(
        r#"
            i32.const 1
            i32.clz
            drop
        "#,
        expect_file!["./expected/i32_clz.hir"],
    )
}

#[test]
fn i64_clz() {
    check_op(
        r#"
            i64.const 1
            i64.clz
            drop
        "#,
        expect_file!["./expected/i64_clz.hir"],
    )
}

#[test]
fn i32_ctz() {
    check_op(
        r#"
            i32.const 1
            i32.ctz
            drop
        "#,
        expect_file!["./expected/i32_ctz.hir"],
    )
}

#[test]
fn i64_ctz() {
    check_op(
        r#"
            i64.const 1
            i64.ctz
            drop
        "#,
        expect_file!["./expected/i64_ctz.hir"],
    )
}

#[test]
fn i32_extend8_s() {
    check_op(
        r#"
            i32.const 1
            i32.extend8_s
            drop
        "#,
        expect_file!["./expected/i32_extend8_s.hir"],
    )
}

#[test]
fn i32_extend16_s() {
    check_op(
        r#"
            i32.const 1
            i32.extend16_s
            drop
        "#,
        expect_file!["./expected/i32_extend16_s.hir"],
    )
}

#[test]
fn i64_extend8_s() {
    check_op(
        r#"
            i64.const 1
            i64.extend8_s
            drop
        "#,
        expect_file!["./expected/i64_extend8_s.hir"],
    )
}

#[test]
fn i64_extend16_s() {
    check_op(
        r#"
            i64.const 1
            i64.extend16_s
            drop
        "#,
        expect_file!["./expected/i64_extend16_s.hir"],
    )
}

#[test]
fn i64_extend32_s() {
    check_op(
        r#"
            i64.const 1
            i64.extend32_s
            drop
        "#,
        expect_file!["./expected/i64_extend32_s.hir"],
    )
}

#[test]
fn i64_extend_i32_s() {
    check_op(
        r#"
            i32.const 1
            i64.extend_i32_s
            drop
        "#,
        expect_file!["./expected/i64_extend_i32_s.hir"],
    )
}

#[test]
fn i64_extend_i32_u() {
    check_op(
        r#"
            i32.const 1
            i64.extend_i32_u
            drop
        "#,
        expect_file!["./expected/i64_extend_i32_u.hir"],
    )
}

#[test]
fn i32_wrap_i64() {
    check_op(
        r#"
            i64.const 1
            i32.wrap_i64
            drop
        "#,
        expect_file!["./expected/i32_wrap_i64.hir"],
    )
}

#[test]
fn i32_add() {
    check_op(
        r#"
            i32.const 3
            i32.const 1
            i32.add
            drop
        "#,
        expect_file!["./expected/i32_add.hir"],
    )
}

#[test]
fn i64_add() {
    check_op(
        r#"
            i64.const 3
            i64.const 1
            i64.add
            drop
        "#,
        expect_file!["./expected/i64_add.hir"],
    )
}

#[test]
fn i32_and() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.and
            drop
        "#,
        expect_file!["./expected/i32_and.hir"],
    )
}

#[test]
fn i64_and() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.and
            drop
        "#,
        expect_file!["./expected/i64_and.hir"],
    )
}

#[test]
fn i32_or() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.or
            drop
        "#,
        expect_file!["./expected/i32_or.hir"],
    )
}

#[test]
fn i64_or() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.or
            drop
        "#,
        expect_file!["./expected/i64_or.hir"],
    )
}

#[test]
fn i32_sub() {
    check_op(
        r#"
            i32.const 3
            i32.const 1
            i32.sub
            drop
        "#,
        expect_file!["./expected/i32_sub.hir"],
    )
}

#[test]
fn i64_sub() {
    check_op(
        r#"
            i64.const 3
            i64.const 1
            i64.sub
            drop
        "#,
        expect_file!["./expected/i64_sub.hir"],
    )
}

#[test]
fn i32_xor() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.xor
            drop
        "#,
        expect_file!["./expected/i32_xor.hir"],
    )
}

#[test]
fn i64_xor() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.xor
            drop
        "#,
        expect_file!["./expected/i64_xor.hir"],
    )
}

#[test]
fn i32_shl() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shl
            drop
        "#,
        expect_file!["./expected/i32_shl.hir"],
    )
}

#[test]
fn i64_shl() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shl
            drop
        "#,
        expect_file!["./expected/i64_shl.hir"],
    )
}

#[test]
fn i32_shr_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shr_u
            drop
        "#,
        expect_file!["./expected/i32_shr_u.hir"],
    )
}

#[test]
fn i64_shr_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shr_u
            drop
        "#,
        expect_file!["./expected/i64_shr_u.hir"],
    )
}

#[test]
fn i32_shr_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shr_s
            drop
        "#,
        expect_file!["./expected/i32_shr_s.hir"],
    )
}

#[test]
fn i64_shr_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shr_s
            drop
        "#,
        expect_file!["./expected/i64_shr_s.hir"],
    )
}

#[test]
fn i32_rotl() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rotl
            drop
        "#,
        expect_file!["./expected/i32_rotl.hir"],
    )
}

#[test]
fn i64_rotl() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rotl
            drop
        "#,
        expect_file!["./expected/i64_rotl.hir"],
    )
}

#[test]
fn i32_rotr() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rotr
            drop
        "#,
        expect_file!["./expected/i32_rotr.hir"],
    )
}

#[test]
fn i64_rotr() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rotr
            drop
        "#,
        expect_file!["./expected/i64_rotr.hir"],
    )
}

#[test]
fn i32_mul() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.mul
            drop
        "#,
        expect_file!["./expected/i32_mul.hir"],
    )
}

#[test]
fn i64_mul() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.mul
            drop
        "#,
        expect_file!["./expected/i64_mul.hir"],
    )
}

#[test]
fn i32_div_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.div_u
            drop
        "#,
        expect_file!["./expected/i32_div_u.hir"],
    )
}

#[test]
fn i64_div_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.div_u
            drop
        "#,
        expect_file!["./expected/i64_div_u.hir"],
    )
}

#[test]
fn i32_div_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.div_s
            drop
        "#,
        expect_file!["./expected/i32_div_s.hir"],
    )
}

#[test]
fn i64_div_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.div_s
            drop
        "#,
        expect_file!["./expected/i64_div_s.hir"],
    )
}

#[test]
fn i32_rem_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rem_u
            drop
        "#,
        expect_file!["./expected/i32_rem_u.hir"],
    )
}

#[test]
fn i64_rem_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rem_u
            drop
        "#,
        expect_file!["./expected/i64_rem_u.hir"],
    )
}

#[test]
fn i32_rem_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rem_s
            drop
        "#,
        expect_file!["./expected/i32_rem_s.hir"],
    )
}

#[test]
fn i64_rem_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rem_s
            drop
        "#,
        expect_file!["./expected/i64_rem_s.hir"],
    )
}

#[test]
fn i32_lt_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.lt_u
            drop
        "#,
        expect_file!["./expected/i32_lt_u.hir"],
    )
}

#[test]
fn i64_lt_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.lt_u
            drop
        "#,
        expect_file!("./expected/i64_lt_u.hir"),
    )
}

#[test]
fn i32_lt_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.lt_s
            drop
        "#,
        expect_file!("./expected/i32_lt_s.hir"),
    )
}

#[test]
fn i64_lt_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.lt_s
            drop
        "#,
        expect_file!("./expected/i64_lt_s.hir"),
    )
}

#[test]
fn i32_le_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.le_u
            drop
        "#,
        expect_file!("./expected/i32_le_u.hir"),
    )
}

#[test]
fn i64_le_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.le_u
            drop
        "#,
        expect_file!("./expected/i64_le_u.hir"),
    )
}

#[test]
fn i32_le_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.le_s
            drop
        "#,
        expect_file!("./expected/i32_le_s.hir"),
    )
}

#[test]
fn i64_le_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.le_s
            drop
        "#,
        expect_file!("./expected/i64_le_s.hir"),
    )
}

#[test]
fn i32_gt_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.gt_u
            drop
        "#,
        expect_file!("./expected/i32_gt_u.hir"),
    )
}

#[test]
fn i64_gt_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.gt_u
            drop
        "#,
        expect_file!("./expected/i64_gt_u.hir"),
    )
}

#[test]
fn i32_gt_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.gt_s
            drop
        "#,
        expect_file!("./expected/i32_gt_s.hir"),
    )
}

#[test]
fn i64_gt_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.gt_s
            drop
        "#,
        expect_file!("./expected/i64_gt_s.hir"),
    )
}

#[test]
fn i32_ge_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ge_u
            drop
        "#,
        expect_file!("./expected/i32_ge_u.hir"),
    )
}

#[test]
fn i64_ge_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ge_u
            drop
        "#,
        expect_file!("./expected/i64_ge_u.hir"),
    )
}

#[test]
fn i32_ge_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ge_s
            drop
        "#,
        expect_file!("./expected/i32_ge_s.hir"),
    )
}

#[test]
fn i64_ge_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ge_s
            drop
        "#,
        expect_file!("./expected/i64_ge_s.hir"),
    )
}

#[test]
fn i32_eqz() {
    check_op(
        r#"
            i32.const 2
            i32.eqz
            drop
        "#,
        expect_file!("./expected/i32_eqz.hir"),
    )
}

#[test]
fn i64_eqz() {
    check_op(
        r#"
            i64.const 2
            i64.eqz
            drop
        "#,
        expect_file!("./expected/i64_eqz.hir"),
    )
}

#[test]
fn i32_eq() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.eq
            drop
        "#,
        expect_file!("./expected/i32_eq.hir"),
    )
}

#[test]
fn i64_eq() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.eq
            drop
        "#,
        expect_file!("./expected/i64_eq.hir"),
    )
}

#[test]
fn i32_ne() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ne
            drop
        "#,
        expect_file!("./expected/i32_ne.hir"),
    )
}

#[test]
fn i64_ne() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ne
            drop
        "#,
        expect_file!("./expected/i64_ne.hir"),
    )
}

#[test]
fn select_i32() {
    check_op(
        r#"
            i64.const 3
            i64.const 7
            i32.const 1
            select
            drop
        "#,
        expect_file!("./expected/select_i32.hir"),
    )
}

#[test]
fn if_else() {
    check_op(
        r#"
        i32.const 2
        if (result i32)
            i32.const 3
        else
            i32.const 5
        end
        drop
    "#,
        expect_file!("./expected/if_else.hir"),
    )
}

#[test]
fn globals() {
    check_op(
        r#"

        global.get $MyGlobalVal
        i32.const 9
        i32.add
        global.set $MyGlobalVal
    "#,
        expect_file!("./expected/globals.hir"),
    )
}
