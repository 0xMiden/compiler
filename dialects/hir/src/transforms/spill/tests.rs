use alloc::{boxed::Box, rc::Rc, sync::Arc};
use std::string::ToString;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder as Cf;
use midenc_dialect_scf::{ScfDialect, StructuredControlFlowOpBuilder};
use midenc_expect_test::expect_file;
use midenc_hir::{
    AbiParam, AddressSpace, Builder, Context, Ident, Op, OpBuilder, PointerType, ProgramPoint,
    Report, Signature, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
    pass::{Nesting, PassManager},
};

use crate::{HirOpBuilder, transforms::TransformSpills};

type TestResult<T> = Result<T, Report>;

/// Build a simple single-block function which triggers spills and reloads,
/// then run the `TransformSpills` pass and check that spills/reloads are
/// materialized as `hir.store_local`/`hir.load_local`.
#[test]
fn materializes_spills_intra_block() -> TestResult<()> {
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    let module = ob.create_module(Ident::with_empty_span("test".into()))?;
    let module_body = module.borrow().body().as_region_ref();
    ob.create_block(module_body, None, &[]);
    let func = ob.create_function(
        Ident::with_empty_span("test::spill".into()),
        Signature::new(
            [AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U8,
                AddressSpace::Element,
            ))))],
            [AbiParam::new(Type::U32)],
        ),
    )?;
    let callee_sig = Signature::new(
        [
            AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            )))),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U64),
        ],
        [AbiParam::new(Type::U32)],
    );
    let callee =
        ob.create_function(Ident::with_empty_span("example".into()), callee_sig.clone())?;

    {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();
        let v0 = entry.borrow().arguments()[0] as ValueRef;
        let v1 = b.ptrtoint(v0, Type::U32, span)?;
        let k32 = b.u32(32, span);
        let v2 = b.add_unchecked(v1, k32, span)?;
        let v3 = b.inttoptr(
            v2,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v4 = b.load(v3, span)?;
        let k64 = b.u32(64, span);
        let v5 = b.add_unchecked(v1, k64, span)?;
        let v6 = b.inttoptr(
            v5,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v7 = b.load(v6, span)?;
        let v8 = b.u64(1, span);
        let _ret_from_call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, v8], span)?;
        let k72 = b.u32(72, span);
        let v9 = b.add_unchecked(v1, k72, span)?;
        b.store(v3, v9, span)?;
        let v10 = b.inttoptr(
            v9,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U64,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let _v11 = b.load(v10, span)?;
        b.ret(Some(v2), span)?;
    }

    // We expect spills and reloads to be required around the call and later uses:
    // - At the `exec` call, operand stack pressure forces spilling of two live values.
    //   The analysis selects the farthest next-use candidates, which here correspond to
    //   the results used later.
    // - Before the `store` and `ret` we must reload them back.
    // After running TransformSpills, those spill/reload pseudos will be materialized as:
    //   store_local <spilled>
    //   load_local  : <type> #[local = lvN]
    let before = func.as_operation_ref().borrow().to_string();

    expect_file!["expected/materialize_spills_intra_block_before.hir"].assert_eq(&before);

    let mut pm = PassManager::on::<Function>(context, Nesting::Implicit);
    pm.add_pass(Box::new(TransformSpills));
    pm.run(func.as_operation_ref())?;

    let after = func.as_operation_ref().borrow().to_string();
    // Check output IR: spills become store_local; reloads become load_local
    expect_file!["expected/materialize_spills_intra_block_after.hir"].assert_eq(&after);

    // Also assert counts for materialized spills/reloads (similar to branching test style)
    let stores = after.lines().filter(|l| l.trim_start().starts_with("hir.store_local ")).count();
    let loads = after
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.contains("= hir.load_local ") || t.starts_with("hir.load_local ")
        })
        .count();
    assert!(stores == 2, "expected two store_local ops\n{after}");
    assert!(loads == 2, "expected two load_local ops\n{after}");

    Ok(())
}

/// Build a branching CFG (then/else/merge) and validate that spills on one path and reloads on the
/// other are materialized as `store_local`/`load_local`, with edges split as needed.
#[test]
fn materializes_spills_branching_cfg() -> TestResult<()> {
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    let module = ob.create_module(Ident::with_empty_span("test".into()))?;
    let module_body = module.borrow().body().as_region_ref();
    ob.create_block(module_body, None, &[]);
    let func = ob.create_function(
        Ident::with_empty_span("test::spill_branch".into()),
        Signature::new(
            [AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U8,
                AddressSpace::Element,
            ))))],
            [AbiParam::new(Type::U32)],
        ),
    )?;

    let callee_sig = Signature::new(
        [
            AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            )))),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U64),
        ],
        [AbiParam::new(Type::U32)],
    );
    let callee =
        ob.create_function(Ident::with_empty_span("example".into()), callee_sig.clone())?;

    {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();
        let v0 = entry.borrow().arguments()[0] as ValueRef;
        let v1 = b.ptrtoint(v0, Type::U32, span)?;
        let k32 = b.u32(32, span);
        let v2c = b.add_unchecked(v1, k32, span)?;
        let v3c = b.inttoptr(
            v2c,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v4 = b.load(v3c, span)?;
        let k64 = b.u32(64, span);
        let v5 = b.add_unchecked(v1, k64, span)?;
        let v6 = b.inttoptr(
            v5,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v7 = b.load(v6, span)?;
        let zero = b.u32(0, span);
        let v8 = b.eq(v1, zero, span)?;
        let t = b.create_block();
        let f = b.create_block();
        Cf::cond_br(&mut b, v8, t, [], f, [], span)?;

        // then
        b.switch_to_block(t);
        let v9 = b.u64(1, span);
        let call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, v9], span)?;
        let v10 = call.borrow().results()[0] as ValueRef;
        // Force a use of a spilled value (v1) after spills in the then-path to require a reload
        b.store(v3c, v7, span)?; // use ptr after spills
        let k5 = b.u32(5, span);
        let _use_v1 = b.add_unchecked(v1, k5, span)?; // use v1 after spills
        let join = b.create_block();
        b.br(join, [v10], span)?;

        // else
        b.switch_to_block(f);
        let k8 = b.u32(8, span);
        let v11 = b.add_unchecked(v1, k8, span)?;
        b.br(join, [v11], span)?;

        // join
        let v12 = b.append_block_param(join, Type::U32, span);
        b.switch_to_block(join);
        let k72 = b.u32(72, span);
        let v13 = b.add_unchecked(v1, k72, span)?;
        let v14 = b.add_unchecked(v13, v12, span)?;
        let v15 = b.inttoptr(
            v14,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U64,
                AddressSpace::Element,
            ))),
            span,
        )?;
        b.store(v3c, v7, span)?;
        let _v16 = b.load(v15, span)?;
        b.ret(Some(v2c), span)?;
    }

    let before = func.as_operation_ref().borrow().to_string();
    assert!(before.contains("cf.cond_br") && before.contains("hir.exec"));

    expect_file!["expected/materialize_spills_branch_cfg_before.hir"].assert_eq(&before);

    let mut pm = PassManager::on::<Function>(context, Nesting::Implicit);
    pm.add_pass(Box::new(TransformSpills));
    pm.run(func.as_operation_ref())?;

    let after = func.as_operation_ref().borrow().to_string();

    expect_file!["expected/materialize_spills_branch_cfg_after.hir"].assert_eq(&after);

    let stores = after.lines().filter(|l| l.trim_start().starts_with("hir.store_local ")).count();
    let loads = after
        .lines()
        .filter(|l| {
            l.trim_start().contains("= hir.load_local ")
                || l.trim_start().starts_with("hir.load_local ")
        })
        .count();
    assert!(stores == 1, "expected only one store_local ops\n{after}");
    assert!(loads == 1, "expected only one load_local op\n{after}");
    Ok(())
}

/// Build a small multi-block CFG containing a `scf.if`, where spilled values are only used inside
/// the nested regions of the `scf.if`.
///
/// This is a regression test for issue #831, where spill/reload rewriting failed to account for
/// uses inside nested regions during CFG reconstruction. Without that rewrite, reloads appear
/// unused and are removed, causing their corresponding spills to be removed as well.
///
/// Operand stack pressure before the `hir.exec @test/example` call:
///
/// - The spills analysis models stack pressure in felts and enforces `K = 16`.
/// - The call arguments require 15 felts total:
///   - `v9: ptr` = 1 felt
///   - `v6: u128` = 4 felts
///   - `v10: u128` is passed twice = 8 felts
///   - `v11: u64` = 2 felts
/// - We keep `v4: u32` and `v5: ptr` live across the call (both used only after the call, inside
///   the `scf.if`), adding 2 more felts.
///
/// Total pressure at the call is `15 + 2 = 17`, which exceeds `K`, so at least one of `{v4, v5}`
/// must be spilled.
///
/// In this specific IR, `v4` and `v5` have equal spill priority:
///
/// - Both are 1 felt
/// - Both have the same next use (`hir.store v5, v4` inside the `scf.if`)
///
/// The MIN heuristic orders candidates stably and `W` preserves insertion order, so `{v4, v5}` stay
/// in that order and we pick from the end, which means we currently spill `v5`.
#[test]
fn materializes_spills_nested_scf_if() -> TestResult<()> {
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    context.get_or_register_dialect::<ScfDialect>();
    let mut ob = OpBuilder::new(context.clone());

    let module = ob.create_module(Ident::with_empty_span("test".into()))?;
    let module_body = module.borrow().body().as_region_ref();
    ob.create_block(module_body, None, &[]);
    let func = ob.create_function(
        Ident::with_empty_span("test::spill_nested_scf_if".into()),
        Signature::new(
            [AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U8,
                AddressSpace::Element,
            ))))],
            [AbiParam::new(Type::U32)],
        ),
    )?;

    let callee_sig = Signature::new(
        [
            AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            )))),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U64),
        ],
        [AbiParam::new(Type::U32)],
    );
    let callee =
        ob.create_function(Ident::with_empty_span("example".into()), callee_sig.clone())?;

    {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();
        let exit = b.create_block();
        let exit_arg = b.append_block_param(exit, Type::U32, span);
        let v0 = entry.borrow().arguments()[0] as ValueRef;
        let v1 = b.ptrtoint(v0, Type::U32, span)?;
        let k32 = b.u32(32, span);
        let v2 = b.add_unchecked(v1, k32, span)?;
        let v3 = b.inttoptr(
            v2,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v4 = b.load(v3, span)?;
        let k64 = b.u32(64, span);
        let v5 = b.add_unchecked(v1, k64, span)?;
        let v6 = b.inttoptr(
            v5,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v7 = b.load(v6, span)?;
        let one = b.u64(1, span);
        let call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, one], span)?;
        let v8 = call.borrow().results()[0] as ValueRef;

        // Make the branch condition depend only on the call result, so that values defined before
        // the call are only used inside the nested `scf.if` regions.
        let zero = b.u32(0, span);
        let cond = b.eq(v8, zero, span)?;

        let mut if_op = b.r#if(cond, &[Type::U32], span)?;
        let (then_block, else_block) = (context.create_block(), context.create_block());
        {
            let mut if_op = if_op.borrow_mut();
            if_op.then_body_mut().push_back(then_block);
            if_op.else_body_mut().push_back(else_block);
        }

        // then
        b.switch_to_block(then_block);
        b.store(v3, v2, span)?;
        b.r#yield([v8], span)?;

        // else
        b.switch_to_block(else_block);
        b.store(v3, v2, span)?;
        b.r#yield([v8], span)?;

        // back to entry
        b.switch_to_block(entry);
        let v9 = if_op.as_operation_ref().borrow().results()[0] as ValueRef;
        b.br(exit, [v9], span)?;

        // exit
        b.switch_to_block(exit);
        b.ret(Some(exit_arg), span)?;
    }

    let mut pm = PassManager::on::<Function>(context, Nesting::Implicit);
    pm.add_pass(Box::new(TransformSpills));
    pm.run(func.as_operation_ref())?;

    let after = func.as_operation_ref().borrow().to_string();

    litcheck_filecheck::filecheck!(
        &after,
        r#"
; COM: Spill before call
; CHECK: hir.store_local
; CHECK: hir.exec @test/example
; CHECK: scf.if

; COM: First reload in `then`
; CHECK-LABEL: ^block3:
; CHECK: hir.load_local
; CHECK-NEXT: hir.store

; COM: Second reload in `else`
; CHECK-LABEL: ^block4:
; CHECK: hir.load_local
; CHECK-NEXT: hir.store
"#
    );

    Ok(())
}

/// Build a small multi-block CFG containing a `scf.while`, where spilled values are only used
/// inside the `after` region of the `scf.while`.
///
/// This is a regression test for spill/reload rewriting in the CFG reconstruction pass: the
/// rewrite must account for uses inside *all* regions nested under a region-branch op. In
/// particular, `scf.while`'s `after` region is not directly reachable from the parent, but uses
/// within it still require reloads to be preserved and rewritten correctly.
#[test]
fn materializes_spills_nested_scf_while_after_region() -> TestResult<()> {
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    context.get_or_register_dialect::<ScfDialect>();
    let mut ob = OpBuilder::new(context.clone());

    let module = ob.create_module(Ident::with_empty_span("test".into()))?;
    let module_body = module.borrow().body().as_region_ref();
    ob.create_block(module_body, None, &[]);
    let func = ob.create_function(
        Ident::with_empty_span("test::spill_nested_scf_while".into()),
        Signature::new(
            [AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U8,
                AddressSpace::Element,
            ))))],
            [AbiParam::new(Type::U32)],
        ),
    )?;

    let callee_sig = Signature::new(
        [
            AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            )))),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U128),
            AbiParam::new(Type::U64),
        ],
        [AbiParam::new(Type::U32)],
    );
    let callee =
        ob.create_function(Ident::with_empty_span("example".into()), callee_sig.clone())?;

    // We construct a synthetic spill/reload pair to focus this test on the rewrite logic.
    //
    // In particular, we spill a value before a call, reload it before the `scf.while`, and then
    // ensure the only post-reload uses occur in the `after` region of the `scf.while`.
    //
    // The CFG rewrite must treat those nested uses as "real" uses, otherwise the reload appears
    // unused and is removed, which then causes the spill to be removed as well.
    let (call_op, while_op, spilled_value) = {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();
        let exit = b.create_block();
        let exit_arg = b.append_block_param(exit, Type::U32, span);

        let v0 = entry.borrow().arguments()[0] as ValueRef;
        let v1 = b.ptrtoint(v0, Type::U32, span)?;
        let k32 = b.u32(32, span);
        let v2 = b.add_unchecked(v1, k32, span)?;
        let v3 = b.inttoptr(
            v2,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v4 = b.load(v3, span)?;
        let k64 = b.u32(64, span);
        let v5 = b.add_unchecked(v1, k64, span)?;
        let v6 = b.inttoptr(
            v5,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let v7 = b.load(v6, span)?;
        let one = b.u64(1, span);
        let call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, one], span)?;
        let call_op = call.as_operation_ref();
        let v8 = call.borrow().results()[0] as ValueRef;

        // Ensure the loop condition is not known at compile time, and does not depend on the
        // values we want to force to be spilled across the call.
        let zero = b.u32(0, span);
        let cond = b.eq(v8, zero, span)?;

        let while_op = b.r#while(core::iter::empty::<ValueRef>(), &[], span)?;
        let while_op_ref = while_op.as_operation_ref();
        let (before_block, after_block) = {
            let while_op = while_op.borrow();
            (
                while_op.before().entry_block_ref().unwrap(),
                while_op.after().entry_block_ref().unwrap(),
            )
        };

        // before: if `cond` is true, enter the `after` region, otherwise exit the loop.
        b.switch_to_block(before_block);
        b.condition(cond, core::iter::empty::<ValueRef>(), span)?;

        // after: use values defined before the call so they are live across it, but only used in
        // the `after` region (and not forwarded via the `before` region terminator).
        b.switch_to_block(after_block);
        let k72 = b.u32(72, span);
        let v9 = b.add_unchecked(v1, k72, span)?;
        b.store(v3, v2, span)?;
        b.store(v3, v9, span)?;
        b.r#yield(core::iter::empty::<ValueRef>(), span)?;

        // back to entry: exit the function.
        b.switch_to_block(entry);
        b.br(exit, [v8], span)?;

        b.switch_to_block(exit);
        b.ret(Some(exit_arg), span)?;
        (call_op, while_op_ref, v2)
    };

    let mut analysis = midenc_hir_analysis::analyses::SpillAnalysis::default();
    analysis.spilled.insert(spilled_value);
    analysis.spills.push(midenc_hir_analysis::analyses::spills::SpillInfo {
        id: midenc_hir_analysis::analyses::spills::Spill::new(0),
        place: midenc_hir_analysis::analyses::spills::Placement::At(ProgramPoint::before(call_op)),
        value: spilled_value,
        span,
        inst: None,
    });
    analysis.reloads.push(midenc_hir_analysis::analyses::spills::ReloadInfo {
        id: midenc_hir_analysis::analyses::spills::Reload::new(0),
        place: midenc_hir_analysis::analyses::spills::Placement::At(ProgramPoint::before(while_op)),
        value: spilled_value,
        span,
        inst: None,
    });

    let mut interface = super::TransformSpillsImpl {
        function: func,
        locals: Default::default(),
    };
    let analysis_manager = midenc_hir::pass::AnalysisManager::new(func.as_operation_ref(), None);
    midenc_hir_transform::transform_spills(
        func.as_operation_ref(),
        &mut analysis,
        &mut interface,
        analysis_manager,
    )?;

    let after = func.as_operation_ref().borrow().to_string();

    litcheck_filecheck::filecheck!(
        &after,
        r#"
; CHECK: hir.store_local
; CHECK: hir.exec @test/example
; CHECK: hir.load_local
; CHECK: scf.while
; CHECK: hir.store
"#
    );

    Ok(())
}
