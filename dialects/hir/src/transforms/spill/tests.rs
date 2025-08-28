use alloc::{boxed::Box, rc::Rc, sync::Arc};
use std::string::ToString;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder as Cf;
use midenc_expect_test::expect_file;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
    pass::{Nesting, PassManager},
    AbiParam, AddressSpace, Builder, Context, Ident, Op, OpBuilder, PointerType, ProgramPoint,
    Report, Signature, SourceSpan, SymbolTable, Type, ValueRef,
};

use crate::{transforms::TransformSpills, HirOpBuilder};

type TestResult<T> = Result<T, Report>;

/// Build a simple single-block function which triggers spills and reloads,
/// then run the `TransformSpills` pass and check that spills/reloads are
/// materialized as `hir.store_local`/`hir.load_local`.
#[test]
fn materializes_spills_intra_block() -> TestResult<()> {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    let mut module = ob.create_module(Ident::with_empty_span("test".into()))?;
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
    module.borrow_mut().symbol_manager_mut().insert_new(func, ProgramPoint::Invalid);
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
    module
        .borrow_mut()
        .symbol_manager_mut()
        .insert_new(callee, ProgramPoint::Invalid);

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
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    let mut module = ob.create_module(Ident::with_empty_span("test".into()))?;
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
    module.borrow_mut().symbol_manager_mut().insert_new(func, ProgramPoint::Invalid);

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
    module
        .borrow_mut()
        .symbol_manager_mut()
        .insert_new(callee, ProgramPoint::Invalid);

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
