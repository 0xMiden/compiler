use alloc::{format, string::ToString, sync::Arc};

use litcheck_filecheck::{filecheck, litcheck};
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder as Cf;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_expect_test::expect_file;
use midenc_hir::{
    AddressSpace, Builder, Op, OperationRef, PointerType, ProgramPoint, Report, SourceSpan, Type,
    ValueRef, dialects::builtin::BuiltinOpBuilder, testing::Test,
};
use midenc_hir_transform::spill_reaches_reload;

use crate::{HirOpBuilder, transforms::TransformSpills};

type TestResult<T> = Result<T, Report>;

/// Returns the defining operation of `value`.
fn defining_op(value: ValueRef) -> OperationRef {
    value.borrow().get_defining_op().expect("expected value to have a defining op")
}

/// Build a simple single-block function which triggers spills and reloads,
/// then run the `TransformSpills` pass and check that spills/reloads are
/// materialized as `hir.store_local`/`hir.load_local`.
#[test]
fn materializes_spills_intra_block() -> TestResult<()> {
    let mut test = Test::named("materializes_spills_intra_block").in_module("test");
    let span = SourceSpan::UNKNOWN;

    test.with_function(
        test.name(),
        &[Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U8,
            AddressSpace::Element,
        )))],
        &[Type::U32],
    );
    let func = test.function();

    let callee = test.define_function(
        "example",
        &[
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            Type::U128,
            Type::U128,
            Type::U128,
            Type::U64,
        ],
        &[Type::U32],
    );

    {
        let mut b = test.function_builder();
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
        let callee_sig = callee.borrow().get_signature().clone();
        let _ret_from_call = b.exec(callee, callee_sig, [v6, v4, v7, v7, v8], span)?;
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

    let before_file = format!("expected/{}_before.hir", test.name());
    expect_file![&before_file].assert_eq(&before);

    test.apply_pass::<TransformSpills>(false)?;

    let after = func.as_operation_ref().borrow().to_string();
    // Check output IR: spills become store_local; reloads become load_local
    let after_file = format!("expected/{}_after.hir", test.name());
    expect_file![&after_file].assert_eq(&after);

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
    let mut test = Test::named("materializes_spills_branching_cfg").in_module("test");
    let span = SourceSpan::UNKNOWN;

    test.with_function(
        "materializes_spills_branching_cfg",
        &[Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U8,
            AddressSpace::Element,
        )))],
        &[Type::U32],
    );
    let func = test.function();

    let callee = test.define_function(
        "example",
        &[
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            Type::U128,
            Type::U128,
            Type::U128,
            Type::U64,
        ],
        &[Type::U32],
    );

    {
        let mut b = test.function_builder();
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
        let callee_sig = callee.borrow().get_signature().clone();
        let call = b.exec(callee, callee_sig, [v6, v4, v7, v7, v9], span)?;
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

    let before_file = format!("expected/{}_before.hir", test.name());
    expect_file![&before_file].assert_eq(&before);

    test.apply_pass::<TransformSpills>(false)?;

    let after = func.as_operation_ref().borrow().to_string();
    let after_file = format!("expected/{}_after.hir", test.name());
    expect_file![&after_file].assert_eq(&after);

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

/// Build a branching CFG in which *both* arms spill the same value (each contains a copy of the
/// high-pressure call), while the only use of that value after the spills lies *after* the join.
///
/// The spills analysis places one spill per arm, and the single reload lands at the join, so the
/// reload is covered by the *set* of spills, none of which individually dominates it. This is a
/// regression test for spill pruning: pruning spills that do not dominate a live reload erased
/// both arm spills while keeping the reload, and reload materialization then panicked because no
/// procedure local was ever allocated for the spilled value.
///
/// Operand stack pressure at each call (the analysis models pressure in felts with `K = 16`):
/// the call arguments require 15 felts (`v6: ptr` = 1, `v4: u128` = 4, `v7: u128` passed twice
/// = 8, `u64` = 2), and `v1: u32` and `v2: u32` are live across both calls (used only in the
/// join block), for a total of `15 + 2 = 17 > K`, so exactly one of `{v1, v2}` is spilled in
/// each arm.
#[test]
fn materializes_spills_join_covered_reload() -> TestResult<()> {
    let mut test = Test::named("materializes_spills_join_covered_reload").in_module("test");
    let span = SourceSpan::UNKNOWN;

    test.with_function(
        test.name(),
        &[Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U8,
            AddressSpace::Element,
        )))],
        &[Type::U32],
    );
    let func = test.function();

    let callee = test.define_function(
        "example",
        &[
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            Type::U128,
            Type::U128,
            Type::U128,
            Type::U64,
        ],
        &[Type::U32],
    );

    {
        let mut b = test.function_builder();
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
        let zero = b.u32(0, span);
        let v8 = b.eq(v1, zero, span)?;
        let t = b.create_block();
        let f = b.create_block();
        let join = b.create_block();
        Cf::cond_br(&mut b, v8, t, [], f, [], span)?;

        let callee_sig = callee.borrow().get_signature().clone();

        // then: the high-pressure call forces a spill on this path
        b.switch_to_block(t);
        let v9 = b.u64(1, span);
        let call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, v9], span)?;
        let v10 = call.borrow().results()[0] as ValueRef;
        b.br(join, [v10], span)?;

        // else: the same call, so the same value is spilled on this path as well
        b.switch_to_block(f);
        let v11 = b.u64(1, span);
        let call2 = b.exec(callee, callee_sig, [v6, v4, v7, v7, v11], span)?;
        let v12 = call2.borrow().results()[0] as ValueRef;
        b.br(join, [v12], span)?;

        // join: the only uses of the values live across the calls
        let v13 = b.append_block_param(join, Type::U32, span);
        b.switch_to_block(join);
        let k5 = b.u32(5, span);
        let v14 = b.add_unchecked(v1, k5, span)?;
        let v15 = b.add_unchecked(v14, v13, span)?;
        let v16 = b.add_unchecked(v15, v2, span)?;
        b.ret(Some(v16), span)?;
    }

    let before = func.as_operation_ref().borrow().to_string();
    let before_file = format!("expected/{}_before.hir", test.name());
    expect_file![&before_file].assert_eq(&before);

    test.apply_pass::<TransformSpills>(false)?;

    let after = func.as_operation_ref().borrow().to_string();
    let after_file = format!("expected/{}_after.hir", test.name());
    expect_file![&after_file].assert_eq(&after);

    // The spilled value must be stored once per arm, and reloaded once at the join
    let stores = after.lines().filter(|l| l.trim_start().starts_with("hir.store_local ")).count();
    let loads = after
        .lines()
        .filter(|l| {
            l.trim_start().contains("= hir.load_local ")
                || l.trim_start().starts_with("hir.load_local ")
        })
        .count();
    assert!(stores == 2, "expected one store_local op in each arm\n{after}");
    assert!(loads == 1, "expected one load_local op at the join\n{after}");
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
    let mut test = Test::named("materializes_spills_nested_scf_if").in_module("test");

    let span = SourceSpan::UNKNOWN;

    test.with_function(
        "materializes_spills_nested_scf_if",
        &[Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U8,
            AddressSpace::Element,
        )))],
        &[Type::U32],
    );
    let func = test.function();

    let callee = test.define_function(
        "example",
        &[
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            Type::U128,
            Type::U128,
            Type::U128,
            Type::U64,
        ],
        &[Type::U32],
    );

    {
        let mut b = test.function_builder();
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
        let callee_sig = callee.borrow().get_signature().clone();
        let call = b.exec(callee, callee_sig, [v6, v4, v7, v7, one], span)?;
        let v8 = call.borrow().results()[0] as ValueRef;

        // Make the branch condition depend only on the call result, so that values defined before
        // the call are only used inside the nested `scf.if` regions.
        let zero = b.u32(0, span);
        let cond = b.eq(v8, zero, span)?;

        let mut if_op = b.r#if(cond, &[Type::U32], span)?;
        let context = b.builder().context_rc();
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

    test.apply_pass::<TransformSpills>(false)?;

    let after = func.as_operation_ref().borrow().to_string();
    std::println!("{after}");

    filecheck!(
        &after,
        r#"
; COM: Spill before call
; CHECK: hir.store_local %{{\d+}} <{ local = #builtin.local_variable<[[L0:\d+]], ptr<u128, element>> }>
; CHECK-NEXT: hir.exec ::@test::@example

; COM: First reload in `then`
; CHECK-LABEL: scf.if %{{\d+}} then {
; CHECK-NEXT: hir.load_local <{ local = #builtin.local_variable<[[L0]], ptr<u128, element>> }>
; CHECK-NEXT: hir.store

; COM: Second reload in `else`
; CHECK-LABEL: } else {
; CHECK-NEXT: hir.load_local <{ local = #builtin.local_variable<[[L0]], ptr<u128, element>> }>
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
    let mut test =
        Test::named("materializes_spills_nested_scf_while_after_region").in_module("test");

    let span = SourceSpan::UNKNOWN;

    test.with_function(
        "materializes_spills_nested_scf_while_after_region",
        &[Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U8,
            AddressSpace::Element,
        )))],
        &[Type::U32],
    );
    let func = test.function();

    let callee = test.define_function(
        "example",
        &[
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U128,
                AddressSpace::Element,
            ))),
            Type::U128,
            Type::U128,
            Type::U128,
            Type::U64,
        ],
        &[Type::U32],
    );

    // We construct a synthetic spill/reload pair to focus this test on the rewrite logic.
    //
    // In particular, we spill a value before a call, reload it before the `scf.while`, and then
    // ensure the only post-reload uses occur in the `after` region of the `scf.while`.
    //
    // The CFG rewrite must treat those nested uses as "real" uses, otherwise the reload appears
    // unused and is removed, which then causes the spill to be removed as well.
    let (call_op, while_op, spilled_value) = {
        let mut b = test.function_builder();
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
        let callee_sig = callee.borrow().get_signature().clone();
        let call = b.exec(callee, callee_sig, [v6, v4, v7, v7, one], span)?;
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

    std::println!("{after}");
    litcheck_filecheck::filecheck!(
        &after,
        r#"
; CHECK: hir.store_local
; CHECK: hir.exec ::@test::@example
; CHECK: hir.load_local
; CHECK: scf.while
; CHECK: hir.store
"#
    );

    Ok(())
}

/// Positional reachability over a diamond CFG.
///
/// This pins the join-covered shape that dominance-based spill pruning got wrong: a reload after
/// the join is covered by the set of per-arm spills, none of which dominates it, so each arm must
/// count as reaching the join. Sibling arms must stay mutually unreachable so the dead-edge-spill
/// elimination keeps working.
#[test]
fn spill_reachability_in_branching_cfg() -> TestResult<()> {
    let mut test = Test::named("spill_reachability_in_branching_cfg").in_module("test");
    let span = SourceSpan::UNKNOWN;
    test.with_function(test.name(), &[Type::U32], &[Type::U32]);

    let mut b = test.function_builder();
    let entry = b.current_block();
    let v0 = entry.borrow().arguments()[0] as ValueRef;
    let k1 = b.u32(1, span);
    let entry_first = b.add_unchecked(v0, k1, span)?;
    let cond = b.eq(entry_first, k1, span)?;
    let left = b.create_block();
    let right = b.create_block();
    let join = b.create_block();
    Cf::cond_br(&mut b, cond, left, [], right, [], span)?;

    b.switch_to_block(left);
    let left_value = b.add_unchecked(entry_first, k1, span)?;
    b.br(join, [left_value], span)?;

    b.switch_to_block(right);
    let right_value = b.add_unchecked(entry_first, entry_first, span)?;
    b.br(join, [right_value], span)?;

    let join_arg = b.append_block_param(join, Type::U32, span);
    b.switch_to_block(join);
    let join_value = b.add_unchecked(join_arg, k1, span)?;
    b.ret(Some(join_value), span)?;

    let entry_first = defining_op(entry_first);
    let entry_second = defining_op(cond);
    let left_op = defining_op(left_value);
    let right_op = defining_op(right_value);
    let join_op = defining_op(join_value);

    assert!(spill_reaches_reload(left_op, join_op), "arm must reach the join");
    assert!(spill_reaches_reload(right_op, join_op), "arm must reach the join");
    assert!(
        !spill_reaches_reload(left_op, right_op),
        "sibling arms must not reach each other"
    );
    assert!(
        spill_reaches_reload(entry_first, entry_second),
        "earlier op must reach a later op in the same block"
    );
    assert!(
        !spill_reaches_reload(entry_second, entry_first),
        "later op must not reach an earlier op without a cycle"
    );
    assert!(!spill_reaches_reload(join_op, left_op), "join must not reach an arm");
    Ok(())
}

/// Positional reachability through a loop back-edge.
#[test]
fn spill_reachability_through_loop_back_edge() -> TestResult<()> {
    let mut test = Test::named("spill_reachability_through_loop_back_edge").in_module("test");
    let span = SourceSpan::UNKNOWN;
    test.with_function(test.name(), &[Type::U32], &[Type::U32]);

    let mut b = test.function_builder();
    let entry = b.current_block();
    let v0 = entry.borrow().arguments()[0] as ValueRef;
    let header = b.create_block();
    let body = b.create_block();
    let exit = b.create_block();
    b.br(header, [v0], span)?;

    let header_arg = b.append_block_param(header, Type::U32, span);
    b.switch_to_block(header);
    let k1 = b.u32(1, span);
    let header_value = b.add_unchecked(header_arg, k1, span)?;
    let cond = b.eq(header_value, k1, span)?;
    Cf::cond_br(&mut b, cond, body, [], exit, [], span)?;

    b.switch_to_block(body);
    let body_value = b.add_unchecked(header_value, k1, span)?;
    b.br(header, [body_value], span)?;

    b.switch_to_block(exit);
    let exit_value = b.add_unchecked(header_value, header_value, span)?;
    b.ret(Some(exit_value), span)?;

    let header_op = defining_op(header_value);
    let header_second = defining_op(cond);
    let body_op = defining_op(body_value);
    let exit_op = defining_op(exit_value);

    assert!(spill_reaches_reload(body_op, header_op), "back edge must reach the header");
    assert!(
        spill_reaches_reload(header_second, header_op),
        "later op must reach an earlier op in the same block through the loop"
    );
    assert!(!spill_reaches_reload(exit_op, body_op), "exit must not reach the loop body");
    Ok(())
}

/// Positional reachability with operations nested in `scf.if` regions.
///
/// Nested operations are normalized to their ancestor in the common region; operations nested
/// under the same region-branch op conservatively reach each other (e.g. across loop iterations).
#[test]
fn spill_reachability_across_nested_regions() -> TestResult<()> {
    let mut test = Test::named("spill_reachability_across_nested_regions").in_module("test");
    let span = SourceSpan::UNKNOWN;
    test.with_function(test.name(), &[Type::U32], &[Type::U32]);

    let mut b = test.function_builder();
    let entry = b.current_block();
    let v0 = entry.borrow().arguments()[0] as ValueRef;
    let k1 = b.u32(1, span);
    let pre_value = b.add_unchecked(v0, k1, span)?;
    let cond = b.eq(pre_value, k1, span)?;

    let mut if_op = b.r#if(cond, &[Type::U32], span)?;
    let context = b.builder().context_rc();
    let (then_block, else_block) = (context.create_block(), context.create_block());
    {
        let mut if_op = if_op.borrow_mut();
        if_op.then_body_mut().push_back(then_block);
        if_op.else_body_mut().push_back(else_block);
    }

    b.switch_to_block(then_block);
    let then_value = b.add_unchecked(pre_value, k1, span)?;
    b.r#yield([then_value], span)?;

    b.switch_to_block(else_block);
    let else_value = b.add_unchecked(pre_value, pre_value, span)?;
    b.r#yield([else_value], span)?;

    b.switch_to_block(entry);
    let if_result = if_op.as_operation_ref().borrow().results()[0] as ValueRef;
    let post_value = b.add_unchecked(if_result, k1, span)?;
    b.ret(Some(post_value), span)?;

    let pre_op = defining_op(pre_value);
    let then_op = defining_op(then_value);
    let else_op = defining_op(else_value);
    let post_op = defining_op(post_value);

    assert!(
        spill_reaches_reload(pre_op, then_op),
        "op before the region op must reach into it"
    );
    assert!(
        spill_reaches_reload(then_op, post_op),
        "nested op must reach past the region op"
    );
    assert!(
        !spill_reaches_reload(post_op, then_op),
        "op after the region op must not reach back into it"
    );
    assert!(
        spill_reaches_reload(then_op, else_op),
        "sibling regions of one op conservatively reach each other"
    );
    Ok(())
}
