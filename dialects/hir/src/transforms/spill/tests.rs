use alloc::{format, rc::Rc, string::ToString, sync::Arc};

use litcheck_filecheck::{filecheck, litcheck};
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder as Cf;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_expect_test::expect_file;
use midenc_hir::{
    AddressSpace, BlockRef, Builder, Context, Op, Operation, OperationRef, PointerType,
    ProgramPoint, Reachability, Report, SourceSpan, Type, ValueRef,
    diagnostics::Uri,
    dialects::builtin::{BuiltinOpBuilder, FunctionRef},
    parse::{self, ParserConfig},
    testing::{Test, parse_function_fixpoint},
};

use crate::{HirOpBuilder, transforms::TransformSpills};

type TestResult<T> = Result<T, Report>;

/// Shorthand for [Operation::reachability] in assertions.
fn reach(from: OperationRef, to: OperationRef) -> Reachability {
    Operation::reachability(from, to)
}

/// Returns the `index`-th block in the body of `function`.
fn block_at(function: FunctionRef, index: usize) -> BlockRef {
    let function = function.borrow();
    function
        .body()
        .body()
        .iter()
        .nth(index)
        .map(|block| block.as_block_ref())
        .expect("block index out of bounds")
}

/// Returns the `index`-th operation in `block`.
fn op_at(block: BlockRef, index: usize) -> OperationRef {
    let block = block.borrow();
    block
        .body()
        .iter()
        .nth(index)
        .map(|op| op.as_operation_ref())
        .expect("operation index out of bounds")
}

/// Returns the `op_index`-th operation in the entry block of `op`'s `region_index`-th region.
fn op_in_region(op: OperationRef, region_index: usize, op_index: usize) -> OperationRef {
    let op = op.borrow();
    let region = op.regions().iter().nth(region_index).expect("region index out of bounds");
    let entry = region.entry_block_ref().expect("expected region to have an entry block");
    op_at(entry, op_index)
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

/// All spills of one value must share one procedure local, even when SSA reconstruction has
/// rewritten a spill's operand.
///
/// We synthesize a `spill; reload; spill; reload` chain over a single value: the rewrite phase
/// redirects the second spill's operand to the first reload's result (spill operands are not
/// exempt from use rewriting), so pairing the local slot by the op operand would allocate a
/// second local that no reload ever reads. Materialization must key locals by the value tracked
/// in the spills analysis, storing the (possibly rewritten) operand into the shared slot.
#[test]
fn materializes_spills_shared_local_for_rewritten_spill() -> TestResult<()> {
    let mut test =
        Test::named("materializes_spills_shared_local_for_rewritten_spill").in_module("test");
    let span = SourceSpan::UNKNOWN;

    test.with_function(test.name(), &[Type::U32], &[Type::U32]);
    let func = test.function();

    let (spilled_value, spill_points, reload_points) = {
        let mut b = test.function_builder();
        let entry = b.current_block();
        let v0 = entry.borrow().arguments()[0] as ValueRef;
        let k1 = b.u32(1, span);
        let v = b.add_unchecked(v0, k1, span)?;
        // Anchor operations to place the spill/reload pseudo-ops in front of
        let a1 = b.add_unchecked(v0, v0, span)?;
        let a2 = b.add_unchecked(a1, k1, span)?;
        let a3 = b.add_unchecked(a2, k1, span)?;
        let a4 = b.add_unchecked(a3, k1, span)?;
        // The post-reload use of the spilled value
        let out = b.add_unchecked(v, a4, span)?;
        b.ret(Some(out), span)?;

        let anchor = |value: ValueRef| {
            ProgramPoint::before(
                value.borrow().get_defining_op().expect("expected anchor to have a defining op"),
            )
        };
        (v, [anchor(a1), anchor(a3)], [anchor(a2), anchor(a4)])
    };

    let mut analysis = midenc_hir_analysis::analyses::SpillAnalysis::default();
    analysis.spilled.insert(spilled_value);
    for (index, place) in spill_points.into_iter().enumerate() {
        analysis.spills.push(midenc_hir_analysis::analyses::spills::SpillInfo {
            id: midenc_hir_analysis::analyses::spills::Spill::new(index),
            place: midenc_hir_analysis::analyses::spills::Placement::At(place),
            value: spilled_value,
            span,
            inst: None,
        });
    }
    for (index, place) in reload_points.into_iter().enumerate() {
        analysis.reloads.push(midenc_hir_analysis::analyses::spills::ReloadInfo {
            id: midenc_hir_analysis::analyses::spills::Reload::new(index),
            place: midenc_hir_analysis::analyses::spills::Placement::At(place),
            value: spilled_value,
            span,
            inst: None,
        });
    }

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

    assert_eq!(
        func.borrow().num_locals(),
        1,
        "all spills of one value must share one procedure local\n{after}"
    );
    // The second store must write the first reload's result into the same local, proving the
    // rewritten operand landed in the shared slot
    filecheck!(
        &after,
        r#"
; CHECK: hir.store_local %{{\d+}} <{ local = #builtin.local_variable<[[L:\d+]], u32> }>
; CHECK: %[[R0:\d+]] = hir.load_local <{ local = #builtin.local_variable<[[L]], u32> }>
; CHECK: hir.store_local %[[R0]] <{ local = #builtin.local_variable<[[L]], u32> }>
; CHECK: hir.load_local <{ local = #builtin.local_variable<[[L]], u32> }>
"#
    );

    Ok(())
}

/// Positional reachability over a diamond CFG.
///
/// This pins the join-covered shape that dominance-based spill pruning got wrong: a reload after
/// the join is covered by the set of per-arm spills, none of which dominates it, so each arm must
/// count as (maybe) reaching the join. Sibling arms must stay provably unreachable from each
/// other so the dead-edge-spill elimination keeps working.
#[test]
fn reachability_in_branching_cfg() -> TestResult<()> {
    let source = r#"builtin.function public extern("C") @reachability_in_branching_cfg(%a: u32) -> u32 {
    %one = arith.constant 1 : u32;
    %first = arith.add %a, %one <{ overflow = #builtin.overflow<unchecked> }>;
    %cond = arith.eq %first, %one;
    cf.cond_br %cond ^left, ^right : (i1);
^left:
    %lv = arith.add %first, %one <{ overflow = #builtin.overflow<unchecked> }>;
    cf.br ^join(%lv : u32);
^right:
    %rv = arith.add %first, %first <{ overflow = #builtin.overflow<unchecked> }>;
    cf.br ^join(%rv : u32);
^join(%j: u32):
    %jv = arith.add %j, %one <{ overflow = #builtin.overflow<unchecked> }>;
    builtin.ret %jv : (u32);
};"#;

    let context = Rc::new(Context::default());
    let (function, _) =
        parse_function_fixpoint(&context, "reachability_in_branching_cfg.hir", source)?;

    let entry = block_at(function, 0);
    let entry_first = op_at(entry, 1);
    let entry_second = op_at(entry, 2);
    let left_op = op_at(block_at(function, 1), 0);
    let right_op = op_at(block_at(function, 2), 0);
    let join_op = op_at(block_at(function, 3), 0);

    assert_eq!(reach(left_op, join_op), Reachability::Maybe, "arm must reach the join");
    assert_eq!(reach(right_op, join_op), Reachability::Maybe, "arm must reach the join");
    assert_eq!(
        reach(left_op, right_op),
        Reachability::Impossible,
        "sibling arms must not reach each other"
    );
    assert_eq!(
        reach(entry_first, entry_second),
        Reachability::Guaranteed,
        "an earlier op always flows into a later op in the same block"
    );
    assert_eq!(
        reach(entry_second, entry_first),
        Reachability::Impossible,
        "later op must not reach an earlier op without a cycle"
    );
    assert_eq!(reach(join_op, left_op), Reachability::Impossible, "join must not reach an arm");
    Ok(())
}

/// Positional reachability through a loop back-edge.
#[test]
fn reachability_through_loop_back_edge() -> TestResult<()> {
    let source = r#"builtin.function public extern("C") @reachability_through_loop_back_edge(%n: u32) -> u32 {
    cf.br ^header(%n : u32);
^header(%i: u32):
    %one = arith.constant 1 : u32;
    %next = arith.add %i, %one <{ overflow = #builtin.overflow<unchecked> }>;
    %cond = arith.eq %next, %one;
    cf.cond_br %cond ^body, ^exit : (i1);
^body:
    %step = arith.add %next, %one <{ overflow = #builtin.overflow<unchecked> }>;
    cf.br ^header(%step : u32);
^exit:
    %out = arith.add %next, %next <{ overflow = #builtin.overflow<unchecked> }>;
    builtin.ret %out : (u32);
};"#;

    let context = Rc::new(Context::default());
    let (function, _) =
        parse_function_fixpoint(&context, "reachability_through_loop_back_edge.hir", source)?;

    let header = block_at(function, 1);
    let header_op = op_at(header, 1);
    let header_second = op_at(header, 2);
    let body_op = op_at(block_at(function, 2), 0);
    let exit_op = op_at(block_at(function, 3), 0);

    assert_eq!(
        reach(body_op, header_op),
        Reachability::Maybe,
        "back edge must reach the header"
    );
    assert_eq!(
        reach(header_second, header_op),
        Reachability::Maybe,
        "later op must reach an earlier op in the same block through the loop"
    );
    assert_eq!(
        reach(exit_op, body_op),
        Reachability::Impossible,
        "exit must not reach the loop body"
    );
    Ok(())
}

/// Positional reachability with operations nested in `scf.if` regions.
///
/// Nested operations are normalized to their ancestor in the common region; positions involving
/// a normalized side are at most [Reachability::Maybe], since entering a sub-region depends on
/// the region op's semantics, and operations nested under the same region-branch op
/// conservatively reach each other (e.g. across loop iterations).
#[test]
fn reachability_across_nested_regions() -> TestResult<()> {
    let source = r#"builtin.function public extern("C") @reachability_across_nested_regions(%a: u32) -> u32 {
    %one = arith.constant 1 : u32;
    %pre = arith.add %a, %one <{ overflow = #builtin.overflow<unchecked> }>;
    %cond = arith.eq %pre, %one;
    %r = scf.if %cond then {
        %t = arith.add %pre, %one <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %t : (u32);
    } else {
        %e = arith.add %pre, %pre <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %e : (u32);
    } : (i1) -> (u32);
    %post = arith.add %r, %one <{ overflow = #builtin.overflow<unchecked> }>;
    builtin.ret %post : (u32);
};"#;

    let context = Rc::new(Context::default());
    let (function, _) =
        parse_function_fixpoint(&context, "reachability_across_nested_regions.hir", source)?;

    let entry = block_at(function, 0);
    let pre_op = op_at(entry, 1);
    let if_op = op_at(entry, 3);
    let post_op = op_at(entry, 4);
    let then_op = op_in_region(if_op, 0, 0);
    let else_op = op_in_region(if_op, 1, 0);

    assert_eq!(
        reach(pre_op, then_op),
        Reachability::Maybe,
        "op before the region op must reach into it"
    );
    assert_eq!(
        reach(then_op, post_op),
        Reachability::Maybe,
        "nested op must reach past the region op"
    );
    assert_eq!(
        reach(post_op, then_op),
        Reachability::Impossible,
        "op after the region op must not reach back into it"
    );
    assert_eq!(
        reach(then_op, else_op),
        Reachability::Maybe,
        "sibling regions of one op conservatively reach each other"
    );
    assert_eq!(
        reach(if_op, then_op),
        Reachability::Maybe,
        "a region op may reach the operations nested within it"
    );
    assert_eq!(
        reach(then_op, if_op),
        Reachability::Maybe,
        "a nested operation flows back through its region op"
    );
    Ok(())
}

/// Positional reachability through re-entry of a repetitive region.
///
/// A repetitive region (e.g. the `before` region of an `scf.while`) has no CFG back edge: its
/// block ends in a region terminator with no block successors, and the loop is expressed in the
/// region graph of the parent op. An op positioned after another op in such a region still
/// reaches it, through the next iteration.
#[test]
fn reachability_through_region_re_entry() -> TestResult<()> {
    let source = r#"builtin.function public extern("C") @reachability_through_region_re_entry(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %one = arith.constant 1 : u32;
        %early = arith.add %i, %one <{ overflow = #builtin.overflow<unchecked> }>;
        %late = arith.add %early, %early <{ overflow = #builtin.overflow<unchecked> }>;
        %continue = arith.lt %late, %n;
        scf.condition %continue, %late : (i1, u32);
    } after {
    ^body(%j: u32):
        %next = arith.incr %j;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};"#;

    let context = Rc::new(Context::default());
    let (function, _) =
        parse_function_fixpoint(&context, "reachability_through_region_re_entry.hir", source)?;

    let entry = block_at(function, 0);
    let while_op = op_at(entry, 1);
    let ret_op = op_at(entry, 2);
    let early_op = op_in_region(while_op, 0, 1);
    let late_op = op_in_region(while_op, 0, 2);

    assert_eq!(
        reach(late_op, early_op),
        Reachability::Maybe,
        "a later op must reach an earlier op in the same repetitive region through re-entry"
    );
    assert_eq!(
        reach(early_op, late_op),
        Reachability::Guaranteed,
        "an earlier op always flows into a later op in the same block"
    );
    assert_eq!(
        reach(ret_op, late_op),
        Reachability::Impossible,
        "an op after the loop must not reach into it"
    );
    Ok(())
}

/// Positional reachability through re-execution of a region whose owner sits on a CFG cycle.
///
/// The `then` region of the `scf.if` is not repetitive in the op's own region graph, but the
/// block hosting the `scf.if` lies on a CFG cycle, so every iteration re-enters the region: an
/// op positioned after another op inside it still reaches it, through the next iteration.
#[test]
fn reachability_through_cfg_cycle_re_entry() -> TestResult<()> {
    let source = r#"builtin.function public extern("C") @reachability_through_cfg_cycle_re_entry(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    cf.br ^head(%zero : u32);
^head(%i: u32):
    %one = arith.constant 1 : u32;
    %c = arith.eq %i, %one;
    %r = scf.if %c then {
        %early = arith.add %i, %one <{ overflow = #builtin.overflow<unchecked> }>;
        %late = arith.add %early, %early <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %late : (u32);
    } else {
        scf.yield %i : (u32);
    } : (i1) -> (u32);
    %done = arith.eq %r, %n;
    cf.cond_br %done ^exit, ^head(%r : u32) : (i1);
^exit:
    builtin.ret %r : (u32);
};"#;

    let context = Rc::new(Context::default());
    let (function, _) =
        parse_function_fixpoint(&context, "reachability_through_cfg_cycle_re_entry.hir", source)?;

    let head = block_at(function, 1);
    let if_op = op_at(head, 2);
    let early_op = op_in_region(if_op, 0, 0);
    let late_op = op_in_region(if_op, 0, 1);
    let ret_op = op_at(block_at(function, 2), 0);

    assert_eq!(
        reach(late_op, early_op),
        Reachability::Maybe,
        "re-entry of a region through an outer CFG cycle must count as reachable"
    );
    assert_eq!(
        reach(ret_op, early_op),
        Reachability::Impossible,
        "an op after the loop must not reach into it"
    );
    Ok(())
}

/// Queries that leave a single function's control flow have no positional answer.
///
/// Operations in two different functions can only be related interprocedurally, and for the
/// function operations themselves the module body is a graph-like region where operation order
/// does not define control flow. Both classifications are direction-independent.
#[test]
fn reachability_across_functions_and_graph_regions() -> TestResult<()> {
    let source = r#"builtin.module public @test {
    builtin.function public extern("C") @first(%a: u32) -> u32 {
        %r = arith.add %a, %a <{ overflow = #builtin.overflow<unchecked> }>;
        builtin.ret %r : (u32);
    };
    builtin.function public extern("C") @second(%b: u32) -> u32 {
        %r = arith.add %b, %b <{ overflow = #builtin.overflow<unchecked> }>;
        builtin.ret %r : (u32);
    };
};"#;

    let context = Rc::new(Context::default());
    let module_op = parse::parse_any(
        ParserConfig::new(context.clone()),
        Uri::new("reachability_across_functions_and_graph_regions.hir"),
        source,
    )?;

    let first_fn = op_in_region(module_op, 0, 0);
    let second_fn = op_in_region(module_op, 0, 1);
    let in_first = op_in_region(first_fn, 0, 0);
    let in_second = op_in_region(second_fn, 0, 0);

    assert_eq!(
        reach(in_second, in_first),
        Reachability::MaybeInterprocedurally,
        "ops in different functions relate only interprocedurally, regardless of module order"
    );
    assert_eq!(
        reach(in_first, in_second),
        Reachability::MaybeInterprocedurally,
        "ops in different functions relate only interprocedurally"
    );
    assert_eq!(
        reach(second_fn, first_fn),
        Reachability::Indeterminate,
        "graph-region op order does not define control flow, regardless of module order"
    );
    assert_eq!(
        reach(first_fn, second_fn),
        Reachability::Indeterminate,
        "graph-region op order does not define control flow"
    );
    assert_eq!(
        reach(first_fn, in_first),
        Reachability::Maybe,
        "enclosure is intra-procedural even when the encloser resides in a graph region"
    );
    assert_eq!(
        reach(in_first, first_fn),
        Reachability::Maybe,
        "enclosure is intra-procedural in both directions"
    );
    Ok(())
}
