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
    check_module_with_config(wat, &WasmTranslationConfig::default(), expected_ir)
}

/// Like [check_module], but with a caller-supplied translation config — used by the tests that
/// resolve a linker stub against a linked package.
fn check_module_with_config(
    wat: &str,
    config: &WasmTranslationConfig,
    expected_ir: midenc_expect_test::ExpectFile,
) {
    let context = Rc::new(midenc_hir::Context::default());

    let wasm = wat::parse_str(wat).unwrap();
    let output = translate(&wasm, config, context.clone())
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

/// A config whose linked packages are a single library exporting `procedures`.
fn config_with_library(procedures: Vec<(&str, midenc_hir::FunctionType)>) -> WasmTranslationConfig {
    use miden_mast_package::{PackageId, TargetType, Version};
    use midenc_package_interface::{
        PackageInterface, ProcedureClass, ProcedureItem, lower_signature,
    };
    use midenc_session::miden_assembly_syntax::ast::{AttributeSet, Path};

    let procedures = procedures
        .into_iter()
        .map(|(path, signature)| ProcedureItem {
            path: std::sync::Arc::from(Path::new(path).to_path_buf().into_boxed_path()),
            digest: Default::default(),
            class: ProcedureClass::Bindable(lower_signature(&signature).unwrap()),
            signature: Some(signature),
            attributes: AttributeSet::default(),
        })
        .collect();
    let package = PackageInterface {
        name: PackageId::from("lib"),
        version: Version::new(1, 2, 3),
        kind: TargetType::Library,
        digest: Default::default(),
        procedures,
        types: Vec::new(),
        constants: Vec::new(),
        modules: Vec::new(),
    };
    WasmTranslationConfig {
        linked_packages: Some(vec![package].into()),
        ..Default::default()
    }
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

/// Symbol names in a Wasm module are producer-controlled, so a user function may be named
/// exactly like the compiler's generated table symbol; the generated name must be bumped until
/// it is free instead of colliding with the user's function.
#[test]
fn call_indirect_table_name_collides_with_user_function() {
    check_module(
        r#"
        (module
            (type $binop (func (param i32 i32) (result i32)))
            (table 3 3 funcref)
            (elem (i32.const 1) func $__indirect_function_table_0 $mul)
            (memory (;0;) 16384)
            (func $__indirect_function_table_0 (type $binop) (param i32 i32) (result i32)
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
        expect_file!["./expected/call_indirect_table_name_collision.hir"],
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

/// The linker stub for an intrinsic the compiler lowers to an inlined operation (such as
/// `intrinsics::felt::add`, whose Wasm body is a single `unreachable`) is erased from the module,
/// so there is no procedure body whose MAST root a table slot could hold. Taking such a
/// function's address must be rejected at compile time rather than lowered to a null slot.
#[test]
fn call_indirect_rejects_inlined_intrinsic_table_entry() {
    check_module_err(
        r#"
        (module
            (type $binop (func (param f32 f32) (result f32)))
            (table 1 1 funcref)
            (elem (i32.const 0) func $intrinsics::felt::add)
            (memory (;0;) 16384)
            (func $intrinsics::felt::add (type $binop)
                unreachable)
            (func $dispatch (param i32 f32 f32) (result f32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $binop))
            (export "dispatch" (func $dispatch))
        )"#,
        "is an inlined intrinsic without a procedure body",
    )
}

/// A stub for an intrinsic lowered to a MASM procedure keeps its own body (an `exec` of the
/// intrinsic) and therefore its Wasm signature, which is exactly the one the entry's type tag
/// denotes. Such an entry is well-formed and must keep lowering.
#[test]
fn call_indirect_accepts_masm_procedure_intrinsic_table_entry() {
    check_module(
        r#"
        (module
            (type $hmerge (func (param i32 i32)))
            (table 1 1 funcref)
            (elem (i32.const 0) func $intrinsics::crypto::hmerge)
            (memory (;0;) 16384)
            (func $intrinsics::crypto::hmerge (type $hmerge)
                unreachable)
            (func $dispatch (param i32 i32 i32)
                local.get 1
                local.get 2
                local.get 0
                call_indirect (type $hmerge))
            (export "dispatch" (func $dispatch))
        )"#,
        expect_file!["./expected/call_indirect_intrinsic_stub.hir"],
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

/// The `script_root` note intrinsic (the SDK's `get_entrypoint_root()`) requires the frontend
/// metadata emitted by `#[note_script]`; in its absence the linker stub must be rejected with an
/// actionable diagnostic rather than compiled to a runtime trap or a wrong digest.
#[test]
fn note_script_root_intrinsic_requires_note_script_metadata() {
    check_module_err(
        r#"
        (module
            (memory (;0;) 1)
            (func $"intrinsics::note::script_root" (param i32)
                unreachable)
            (func $probe (param i32)
                local.get 0
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
        )"#,
        "requires a `#[note_script]` entrypoint",
    )
}

/// Unknown functions under `intrinsics::note` must be rejected with a diagnostic: the module as
/// a whole is classified as module-context stubs, so this is where an unknown name surfaces.
#[test]
fn note_intrinsic_stubs_reject_unknown_functions() {
    check_module_err(
        r#"
        (module
            (memory (;0;) 1)
            (func $"intrinsics::note::bogus" (param i32)
                unreachable)
            (func $probe (param i32)
                local.get 0
                call $"intrinsics::note::bogus")
            (export "probe" (func $probe))
        )"#,
        "unknown note intrinsic",
    )
}

/// A `script_root` stub whose signature does not take exactly the result pointer must be
/// rejected with a diagnostic instead of an assertion failure.
#[test]
fn note_script_root_stub_rejects_malformed_signatures() {
    check_module_err(
        r#"
        (module
            (memory (;0;) 1)
            (func $"intrinsics::note::script_root"
                unreachable)
            (func $probe
                call $"intrinsics::note::script_root")
            (export "probe" (func $probe))
        )"#,
        "expected exactly one parameter",
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

/// A data segment may occupy the last bytes of linear memory: `wasm-tools validate` accepts
/// this module, so the frontend must translate it to a segment at `0xfffffff0` and leave the
/// address-space arithmetic to the linker, which reports `LayoutOverflow` for it.
#[test]
fn translates_a_data_segment_at_the_end_of_memory() {
    let wat = r#"
        (module
            (memory 65536)
            (data (i32.const -16) "0123456789abcdef")
        )"#;
    let wasm = wat::parse_str(wat).unwrap();
    let context = Rc::new(midenc_hir::Context::default());
    let output = translate(&wasm, &WasmTranslationConfig::default(), context.clone())
        .expect("a segment at the end of memory is valid Wasm");

    let mut segments = Vec::new();
    let component = output.component.borrow();
    component.as_operation().prewalk_all(|op: &Operation| {
        if let Some(segment) = op.downcast_ref::<builtin::Segment>() {
            segments.push((*segment.get_offset(), segment.size_in_bytes()));
        }
    });

    assert_eq!(segments, vec![(0xffff_fff0u32, 16usize)]);
}

/// A linker stub whose name is exported by a linked package is lowered to an `exec` of that
/// package's procedure, with the signature the Miden ABI rule set derives from the manifest —
/// no hand-written table involved.
#[test]
fn manifest_resolved_stub() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::lib::add",
        FunctionType::new(CallConv::Fast, [Type::Felt, Type::Felt], [Type::Felt]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::add" (param f32 f32) (result f32)
                unreachable)
            (func $probe (param f32 f32) (result f32)
                local.get 0
                local.get 1
                call $"lib::add")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub.hir"),
    )
}

/// A pointer parameter the manifest declares in the element address space is passed through: the
/// stub's `i32` is taken to be an element address already and is only given the callee's pointer
/// type. No address arithmetic happens here; turning a Rust byte address into an element address
/// is the binding wrapper's job.
#[test]
fn manifest_resolved_stub_passes_an_element_space_pointer_through() {
    use alloc::sync::Arc;

    use midenc_hir::{AddressSpace, CallConv, FunctionType, PointerType, Type};

    let element_ptr =
        Type::Ptr(Arc::new(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)));
    let config = config_with_library(vec![(
        "::lib::write",
        FunctionType::new(CallConv::Fast, [element_ptr, Type::Felt], []),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::write" (param i32 f32)
                unreachable)
            (func $probe (param i32 f32)
                local.get 0
                local.get 1
                call $"lib::write")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub_element_pointer.hir"),
    )
}

/// A pointer result is cast to its `i32` carrier and nothing else: an element-space pointer comes
/// back as an element address. Scaling it to a byte address, with the range check that needs, is
/// the binding wrapper's job.
#[test]
fn manifest_resolved_stub_returns_an_element_space_pointer_as_its_address() {
    use alloc::sync::Arc;

    use midenc_hir::{AddressSpace, CallConv, FunctionType, PointerType, Type};

    let element_ptr =
        Type::Ptr(Arc::new(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)));
    let config = config_with_library(vec![(
        "::lib::cursor",
        FunctionType::new(CallConv::Fast, [], [element_ptr]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::cursor" (result i32)
                unreachable)
            (func $probe (result i32)
                call $"lib::cursor")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub_element_pointer_result.hir"),
    )
}

/// A stub rooted in a linked package's own namespace, but naming something that package does not
/// export, is a diagnostic, not a panic — a stale binding has to be reported rather than quietly
/// left as a diverging function.
#[test]
fn an_unresolvable_stub_is_reported() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::lib::present",
        FunctionType::new(CallConv::Fast, [Type::Felt], [Type::Felt]),
    )]);
    let wasm = wat::parse_str(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::missing" (param f32) (result f32)
                unreachable)
            (func $probe (param f32) (result f32)
                local.get 0
                call $"lib::missing")
            (export "probe" (func $probe))
        )"#,
    )
    .unwrap();
    let context = Rc::new(midenc_hir::Context::default());
    let msg = match translate(&wasm, &config, context) {
        Ok(_) => panic!("expected translation to fail"),
        Err(err) => format!("{err}"),
    };
    assert!(
        msg.contains("does not name a procedure exported by any linked package"),
        "got: {msg}"
    );
    assert!(msg.contains("lib 1.2.3"), "got: {msg}");
}

/// A diverging Rust function is not a linker stub. LLVM reduces `unreachable_unchecked`
/// wrappers, `drop_in_place` for uninhabited types and matches on empty enums to a lone
/// `unreachable`, and the name section spells them as Rust paths — which parse as MASM paths
/// just as well. Such a function is rooted in no linked package's namespace, so it is left
/// exactly as it is rather than becoming a hard "does not name a procedure" error.
#[test]
fn a_diverging_function_outside_every_linked_namespace_is_left_alone() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::lib::present",
        FunctionType::new(CallConv::Fast, [Type::Felt], [Type::Felt]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"core::ptr::drop_in_place" (param i32)
                unreachable)
            (func $probe (param i32)
                local.get 0
                call $"core::ptr::drop_in_place")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/diverging_function_outside_linked_namespaces.hir"),
    )
}

/// The stub deals in Wasm carrier types (`i32` for every integer of 32 bits or narrower) while
/// the manifest declares the callee's own widths and signedness, and codegen validates `exec`
/// arguments against the callee's declaration by exact type. So the stub converts in both
/// directions: `u32` by `bitcast`, `u16` by `trunc`, and the `u8` result back to its `i32`
/// carrier by `zext` (to `u32`, since `zext` produces an unsigned type) plus a `bitcast`.
#[test]
fn manifest_resolved_stub_converts_narrow_and_unsigned_carriers() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::lib::narrow",
        FunctionType::new(CallConv::Fast, [Type::U32, Type::U16, Type::Felt], [Type::U8]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::narrow" (param i32 i32 f32) (result i32)
                unreachable)
            (func $probe (param i32 i32 f32) (result i32)
                local.get 0
                local.get 1
                local.get 2
                call $"lib::narrow")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub_carrier_conversions.hir"),
    )
}

/// Two results are returned through the out pointer, and the conversion back to the carrier types
/// has to happen before the stores: the return area is laid out from the stored values' types, so
/// each `u16` must occupy the 4-byte `i32` slot the SDK wrapper reads back, not a 2-byte one.
#[test]
fn manifest_resolved_stub_widens_out_pointer_results_before_storing_them() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::lib::pair",
        FunctionType::new(CallConv::Fast, [Type::Felt], [Type::U16, Type::U16]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"lib::pair" (param f32 i32)
                unreachable)
            (func $probe (param f32 i32)
                local.get 0
                local.get 1
                call $"lib::pair")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub_out_pointer_carriers.hir"),
    )
}

/// Every callee resolved from a linked package is declared with the effects the compiler knows it
/// to have (`miden_abi::effects`): a core `mem::pipe_*` procedure reads the advice provider and
/// writes memory, which the advice-taint lint reads off the declaration, and any other export
/// carries none.
#[test]
fn a_resolved_callee_is_declared_with_the_effects_the_compiler_knows() {
    use midenc_hir::{
        BuilderExt, CallConv, FunctionType, Symbol, Type,
        dialects::builtin::attributes::AdviceResourceKind,
        effects::{AdviceEffect, MemoryEffect},
    };

    // The imports are declared in the world the component is translated into, so the test
    // supplies the world in order to walk it afterwards. `context` owns the IR and has to outlive
    // the walk.
    let context = Rc::new(midenc_hir::Context::default());
    let world = context.clone().builder().create::<builtin::World, ()>(Default::default())();
    let world = world.unwrap();
    let library = config_with_library(vec![
        (
            "::miden::core::mem::pipe_preimage_to_memory",
            FunctionType::new(
                CallConv::Fast,
                [Type::U32, Type::I32, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
                [Type::I32],
            ),
        ),
        (
            "::miden::core::mem::memcopy_words",
            FunctionType::new(CallConv::Fast, [Type::U32, Type::I32, Type::I32], []),
        ),
    ]);
    let config = WasmTranslationConfig {
        world: Some(world),
        ..library
    };
    let wasm = wat::parse_str(
        r#"
        (module
            (memory (;0;) 1)
            (func $"miden::core::mem::pipe_preimage_to_memory"
                (param i32 i32 f32 f32 f32 f32) (result i32)
                unreachable)
            (func $"miden::core::mem::memcopy_words" (param i32 i32 i32)
                unreachable)
            (func $probe (param i32 i32 f32 f32 f32 f32) (result i32)
                local.get 0
                local.get 1
                local.get 1
                call $"miden::core::mem::memcopy_words"
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                local.get 4
                local.get 5
                call $"miden::core::mem::pipe_preimage_to_memory")
            (export "probe" (func $probe))
        )"#,
    )
    .unwrap();
    translate(&wasm, &config, context.clone()).unwrap();

    let mut declared = Vec::new();
    world.borrow().as_operation().prewalk_all(|op: &Operation| {
        if let Some(function) = op.downcast_ref::<builtin::Function>()
            && function.is_declaration()
        {
            let advice: Vec<_> = function
                .advice_effects()
                .as_value()
                .iter()
                .map(|effect| (effect.effect, effect.resource))
                .collect();
            let memory: Vec<_> = function
                .memory_effects()
                .as_value()
                .iter()
                .map(|effect| effect.effect)
                .collect();
            declared.push((function.get_name().as_str(), advice, memory));
        }
    });
    declared.sort_by_key(|(name, ..)| *name);

    assert_eq!(
        declared,
        vec![
            ("memcopy_words", vec![], vec![]),
            (
                "pipe_preimage_to_memory",
                vec![(AdviceEffect::Read, AdviceResourceKind::Map)],
                vec![MemoryEffect::Write]
            ),
        ]
    );
}

/// A stub rooted in a namespace that MASM spells quoted — the default root module of a package
/// named `my-lib` is `::"my-lib"` — resolves like any other. The frontend compares the identifier
/// rather than its spelling (`names_a_linked_namespace`), and gives the local stub a linkage name
/// without the quotes, since the assembler cannot read a quoted procedure name that nests them;
/// the import it `exec`s keeps the MASM spelling.
#[test]
fn manifest_resolved_stub_in_a_quoted_namespace() {
    use midenc_hir::{CallConv, FunctionType, Type};

    let config = config_with_library(vec![(
        "::\"my-lib\"::add",
        FunctionType::new(CallConv::Fast, [Type::Felt, Type::Felt], [Type::Felt]),
    )]);
    check_module_with_config(
        r#"
        (module
            (memory (;0;) 1)
            (func $"\"my-lib\"::add" (param f32 f32) (result f32)
                unreachable)
            (func $probe (param f32 f32) (result f32)
                local.get 0
                local.get 1
                call $"\"my-lib\"::add")
            (export "probe" (func $probe))
        )"#,
        &config,
        expect_file!("./expected/manifest_resolved_stub_in_a_quoted_namespace.hir"),
    )
}
