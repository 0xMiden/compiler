use alloc::{rc::Rc, sync::Arc};
use std::string::ToString;

use midenc_dialect_arith::ArithOpBuilder as Arith;
use midenc_dialect_cf::ControlFlowOpBuilder as Cf;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder as Scf;
use midenc_expect_test::expect_file;
use midenc_hir::{
    AbiParam, AddressSpace, BlockRef, Builder, Context, Ident, Op, OpBuilder, OperationRef,
    PointerType, ProgramPoint, Report, Signature, SourceSpan, SymbolTable, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
    pass::AnalysisManager,
};

use crate::analyses::{
    SpillAnalysis,
    spills::{Predecessor, Split},
};

type AnalysisResult<T> = Result<T, Report>;

/// In this test, we force several values to be live simultaneously inside the same block,
/// of sufficient size on the operand stack so as to require some of them to be spilled
/// at least once, and later reloaded.
///
/// The purpose here is to validate the MIN algorithm that determines whether or not we need
/// to spill operands at each program point, in the following ways:
///
/// * Ensure that we spill values we expect to be spilled
/// * Ensure that spills are inserted at the appropriate locations
/// * Ensure that we reload values that were previously spilled
/// * Ensure that reloads are inserted at the appropriate locations
///
/// The following HIR is constructed for this test:
///
/// * `in` indicates the set of values in W at an instruction, with reloads included
/// * `out` indicates the set of values in W after an instruction, with spills excluded
/// * `spills` indicates the candidates from W which were selected to be spilled at the
///   instruction
/// * `reloads` indicates the set of values in S which must be reloaded at the instruction
///
/// ```text,ignore
/// (func (export #spill) (param (ptr u8)) (result u32)
///   (block 0 (param v0 (ptr u8))
///     (let (v1 u32) (ptrtoint v0))              ; in=[v0] out=[v1]
///     (let (v2 u32) (add v1 32))                ; in=[v1] out=[v1 v2]
///     (let (v3 (ptr u128)) (inttoptr v2))       ; in=[v1 v2] out=[v1 v2 v3]
///     (let (v4 u128) (load v3))                 ; in=[v1 v2 v3] out=[v1 v2 v3 v4]
///     (let (v5 u32) (add v1 64))                ; in=[v1 v2 v3 v4] out=[v1 v2 v3 v4 v5]
///     (let (v6 (ptr u128)) (inttoptr v5))       ; in=[v1 v2 v3 v4 v5] out=[v1 v2 v3 v4 v6]
///     (let (v7 u128) (load v6))                 ; in=[v1 v2 v3 v4 v6] out=[v1 v2 v3 v4 v6 v7]
///     (let (v8 u64) (const.u64 1))              ; in=[v1 v2 v3 v4 v6 v7] out=[v1 v2 v3 v4 v6 v7 v8]
///     (let (v9 u32) (call (#example) v6 v4 v7 v7 v8)) <-- operand stack pressure hits 18 here
///                                               ; in=[v1 v2 v3 v4 v6 v7 v7 v8] out=[v1 v7 v9]
///                                               ; spills=[v2 v3] (v2 is furthest next-use, followed by v3)
///     (let (v10 u32) (add v1 72))               ; in=[v1 v7] out=[v7 v10]
///     (let (v11 (ptr u64)) (inttoptr v10))      ; in=[v7 v10] out=[v7 v11]
///     (store v3 v7)                             ; reload=[v3] in=[v3 v7 v11] out=[v11]
///     (let (v12 u64) (load v11))                ; in=[v11] out=[v12]
///     (ret v2)                                  ; reload=[v2] in=[v2] out=[v2]
/// )
/// ```
#[test]
fn spills_intra_block() -> AnalysisResult<()> {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();
    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    // Module and test function
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
    // Callee with signature: (ptr u128, u128, u128, u128, u64) -> u32
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
    // Body
    let call_op;
    let store_op;
    let ret_op;
    let (v2, v3);
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
        let v8 = b.u64(1, span);
        let call = b.exec(callee, callee_sig.clone(), [v6, v4, v7, v7, v8], span)?;
        call_op = call.as_operation_ref();
        let _v9 = call.borrow().results()[0] as ValueRef;
        let k72 = b.u32(72, span);
        let v10 = b.add_unchecked(v1, k72, span)?;
        let store = b.store(v3c, v7, span)?;
        store_op = store.as_operation_ref();
        let _v11 = b.inttoptr(
            v10,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U64,
                AddressSpace::Element,
            ))),
            span,
        )?;
        // Load from the computed u64 pointer before returning, as described in the test doc
        let _v12 = b.load(_v11, span)?;
        let ret = b.ret(Some(v2c), span)?;
        ret_op = ret.as_operation_ref();
        v2 = v2c;
        v3 = v3c;
    }

    expect_file!["expected/spills_intra_block.hir"]
        .assert_eq(&func.as_operation_ref().borrow().to_string());

    let am = AnalysisManager::new(func.as_operation_ref(), None);
    let spills = am.get_analysis_for::<SpillAnalysis, Function>()?;

    assert!(spills.has_spills());
    assert_eq!(spills.spills().len(), 2);
    assert!(spills.is_spilled(&v2));
    assert!(spills.is_spilled_at(v2, ProgramPoint::before(call_op)));
    assert!(spills.is_spilled(&v3));
    assert!(spills.is_spilled_at(v3, ProgramPoint::before(call_op)));
    assert_eq!(spills.reloads().len(), 2);
    assert!(spills.is_reloaded_at(v3, ProgramPoint::before(store_op)));
    assert!(spills.is_reloaded_at(v2, ProgramPoint::before(ret_op)));
    Ok(())
}

/// In this test, we are verifying the behavior of the spill analysis when applied to a
/// control flow graph with branching control flow, where spills are required along one
/// branch and not the other. This verifies the following:
///
/// * Control flow edges are split as necessary to insert required spills/reloads
/// * Propagation of the W and S sets from predecessors to successors is correct
/// * The W and S sets are properly computed at join points in the CFG
///
/// The following HIR is constructed for this test (see the first test in this file for
/// a description of the notation used, if unclear):
///
/// ```text,ignore
/// (func (export #spill) (param (ptr u8)) (result u32)
///   (block 0 (param v0 (ptr u8))
///     (let (v1 u32) (ptrtoint v0))              ; in=[v0] out=[v1]
///     (let (v2 u32) (add v1 32))                ; in=[v1] out=[v1 v2]
///     (let (v3 (ptr u128)) (inttoptr v2))       ; in=[v1 v2] out=[v1 v2 v3]
///     (let (v4 u128) (load v3))                 ; in=[v1 v2 v3] out=[v1 v2 v3 v4]
///     (let (v5 u32) (add v1 64))                ; in=[v1 v2 v3 v4] out=[v1 v2 v3 v4 v5]
///     (let (v6 (ptr u128)) (inttoptr v5))       ; in=[v1 v2 v3 v4 v5] out=[v1 v2 v3 v4 v6]
///     (let (v7 u128) (load v6))                 ; in=[v1 v2 v3 v4 v6] out=[v1 v2 v3 v4 v6 v7]
///     (let (v8 i1) (eq v1 0))                   ; in=[v1 v2 v3 v4 v6, v7] out=[v1 v2 v3 v4 v6 v7, v8]
///     (cond_br v8 (block 1) (block 2)))
///
///   (block 1
///     (let (v9 u64) (const.u64 1))              ; in=[v1 v2 v3 v4 v6 v7] out=[v1 v2 v3 v4 v6 v7 v9]
///     (let (v10 u32) (call (#example) v6 v4 v7 v7 v9)) <-- operand stack pressure hits 18 here
///                                               ; in=[v1 v2 v3 v4 v6 v7 v7 v9] out=[v1 v7 v10]
///                                               ; spills=[v2 v3] (v2 is furthest next-use, followed by v3)
///     (br (block 3 v10))) ; this edge will be split to reload v2/v3 as expected by block3
///
///   (block 2
///     (let (v11 u32) (add v1 8))                ; in=[v1 v2 v3 v7] out=[v1 v2 v3 v7 v11]
///     (br (block 3 v11))) ; this edge will be split to spill v2/v3 to match the edge from block1
///
///   (block 3 (param v12 u32)) ; we expect that the edge between block 2 and 3 will be split, and spills of v2/v3 inserted
///     (let (v13 u32) (add v1 72))               ; in=[v1 v7 v12] out=[v7 v12 v13]
///     (let (v14 u32) (add v13 v12))             ; in=[v7 v12 v13] out=[v7 v14]
///     (let (v15 (ptr u64)) (inttoptr v14))      ; in=[v7 v14] out=[v7 v15]
///     (store v3 v7)                             ; reload=[v3] in=[v3 v7 v15] out=[v15]
///     (let (v16 u64) (load v15))                ; in=[v15] out=[v16]
///     (ret v2))                                 ; reload=[v2] in=[v2] out=[v2]
/// )
/// ```
#[test]
fn spills_branching_control_flow() -> AnalysisResult<()> {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();
    let span = SourceSpan::UNKNOWN;
    let context = Rc::new(Context::default());
    let mut ob = OpBuilder::new(context.clone());

    // Module and test function
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
    // Callee with signature: (ptr u128, u128, u128, u128, u64) -> u32
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

    let (block1, block2, block3);
    let (v2, v3);
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

        block1 = t;
        block2 = f;
        block3 = join;
        v2 = v2c;
        v3 = v3c;
    }

    expect_file!["expected/spills_branch_cfg.hir"]
        .assert_eq(&func.as_operation_ref().borrow().to_string());

    let am = AnalysisManager::new(func.as_operation_ref(), None);
    let spills = am.get_analysis_for::<SpillAnalysis, Function>()?;

    assert!(spills.has_spills());
    assert_eq!(spills.spills().len(), 4);
    assert_eq!(spills.splits().len(), 2);

    let find_split = |to_block: BlockRef, from_block: BlockRef| -> Option<Split> {
        spills.splits().iter().find_map(|s| match (s.point, s.predecessor) {
            (ProgramPoint::Block { block, .. }, Predecessor::Block { op, .. })
                if block == to_block && op.parent() == Some(from_block) =>
            {
                Some(s.id)
            }
            _ => None,
        })
    };
    let split_blk1_blk3 = find_split(block3, block1).expect("missing split for block1->block3");
    let split_blk2_blk3 = find_split(block3, block2).expect("missing split for block2->block3");

    // v2 should have a spill inserted from block2 to block3, as it is spilled in block1
    assert!(spills.is_spilled_in_split(v2, split_blk2_blk3));
    // v3 should have a spill inserted from block2 to block3, as it is spilled in block1
    assert!(spills.is_spilled_in_split(v3, split_blk2_blk3));
    // v2 and v3 should be reloaded on the edge from block1 to block3, since they were
    // spilled previously, but must be in W on entry to block3
    assert_eq!(spills.reloads().len(), 2);
    assert!(spills.is_reloaded_in_split(v2, split_blk1_blk3));
    assert!(spills.is_reloaded_in_split(v3, split_blk1_blk3));

    Ok(())
}

/// In this test, we are verifying the behavior of the spill analysis when applied to a
/// control flow graph with cyclical control flow, i.e. loops. We're interested specifically in
/// the following properties:
///
/// * W and S at entry to a loop are computed correctly
/// * Values live-through - but not live-in - a loop, which cannot survive the loop due to
///   operand stack pressure within the loop, are spilled outside of the loop, with reloads
///   placed on exit edges from the loop where needed
/// * W and S upon exit from a loop are computed correctly
///
/// The following HIR is constructed for this test (see the first test in this file for
/// a description of the notation used, if unclear):
///
/// ```text,ignore
/// (func (export #spill) (param (ptr u64)) (param u32) (param u32) (result u64)
///   (block 0 (param v0 (ptr u64)) (param v1 u32) (param v2 u32)
///     (let (v3 u32) (const.u32 0))         ; in=[v0, v1, v2] out=[v0, v1, v2, v3]
///     (let (v4 u32) (const.u32 0))         ; in=[v0, v1, v2, v3] out=[v0, v1, v2, v3, v4]
///     (let (v5 u64) (const.u64 0))         ; in=[v0, v1, v2, v3, v4] out=[v0, v1, v2, v3, v4, v5]
///     (br (block 1 v3 v4 v5)))
///
///   (block 1 (param v6 u32) (param v7 u32) (param v8 u64)) ; outer loop
///     (let (v9 i1) (eq v6 v1))             ; in=[v0, v2, v6, v7, v8] out=[v0, v1, v2, v6, v7, v8, v9]
///     (cond_br v9 (block 2) (block 3)))    ; in=[v0, v1, v2, v6, v7, v8, v9] out=[v0, v1, v2, v6, v7, v8]
///
///   (block 2 ; exit outer loop, return from function
///     (ret v8))                            ; in=[v0, v1, v2, v6, v7, v8] out=[v8]
///
///   (block 3 ; split edge
///     (br (block 4 v7 v8)))                ; in=[v0, v1, v2, v6, v7, v8] out=[v0, v1, v2, v6]
///
///   (block 4 (param v10 u32) (param v11 u64) ; inner loop
///     (let (v12 i1) (eq v10 v2))           ; in=[v0, v1, v2, v6, v10, v11] out=[v0, v1, v2, v6, v10, v11, v12]
///     (cond_br v12 (block 5) (block 6)))   ; in=[v0, v1, v2, v6, v10, v11, v12] out=[v0, v1, v2, v6, v10, v11]
///
///   (block 5 ; increment row count, continue outer loop
///     (let (v13 u32) (add v6 1))           ; in=[v0, v1, v2, v6, v10, v11] out=[v0, v1, v2, v10, v11, v13]
///     (br (block 1 v13 v10 v11)))
///
///   (block 6 ; load value at v0[row][col], add to sum, increment col, continue inner loop
///     (let (v14 u32) (sub.saturating v6 1)) ; row_offset := ROW_SIZE * row.saturating_sub(1)
///                                           ; in=[v0, v1, v2, v6, v10, v11] out=[v0, v1, v2, v6, v10, v11, 14]
///     (let (v15 u32) (mul v14 v2))          ; in=[v0, v1, v2, v6, v10, v11, 14] out=[v0, v1, v2, v6, v10, v11, 15]
///     (let (v16 u32) (add v10 v15))         ; offset := col + row_offset
///                                           ; in=[v0, v1, v2, v6, v10, v11, 15] out=[v0, v1, v2, v6, v10, v11, v16]
///     (let (v17 u32) (ptrtoint v0))         ; ptr := (v0 as u32 + offset) as *u64
///                                           ; in=[v0, v1, v2, v6, v10, v11, v16] out=[v0, v1, v2, v6, v10, v11, v16, 17]
///     (let (v18 u32) (add v17 v16))         ; in=[v0, v1, v2, v6, v10, v11, v16, v17] out=[v0, v1, v2, v6, v10, v11, v18]
///     (let (v19 (ptr u64)) (ptrtoint v18))  ; in=[v0, v1, v2, v6, v10, v11, v18] out=[v0, v1, v2, v6, v10, v11, v19]
///     (let (v20 u64) (load v19))            ; sum += *ptr
///                                           ; in=[v0, v1, v2, v6, v10, v11, v19] out=[v0, v1, v2, v6, v10, v11, v20]
///     (let (v21 u64) (add v11 v20))         ; in=[v0, v1, v2, v6, v10, v11, v20] out=[v0, v1, v2, v6, v10, v21]
///     (let (v22 u32) (add v10 1))           ; col++
///                                           ; in=[v0, v1, v2, v6, v10, v21] out=[v0, v1, v2, v6, v21, v22]
///     (br (block 4 v22 v21)))
/// )
/// ```
#[test]
fn spills_loop_nest() -> AnalysisResult<()> {
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
        Ident::with_empty_span("test::spill_loop".into()),
        Signature::new(
            [
                AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                    Type::U64,
                    AddressSpace::Element,
                )))),
                AbiParam::new(Type::U32),
                AbiParam::new(Type::U32),
            ],
            [AbiParam::new(Type::U64)],
        ),
    )?;
    module.borrow_mut().symbol_manager_mut().insert_new(func, ProgramPoint::Invalid);
    let callee_sig = Signature::new(
        [
            AbiParam::new(Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U64,
                AddressSpace::Element,
            )))),
            AbiParam::new(Type::U64),
            AbiParam::new(Type::U64),
            AbiParam::new(Type::U64),
            AbiParam::new(Type::U64),
            AbiParam::new(Type::U64),
            AbiParam::new(Type::U64),
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

    let call_op6 = {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();
        let (v0, v1, v2) = {
            let entry_b = entry.borrow();
            let args = entry_b.arguments();
            (args[0] as ValueRef, args[1] as ValueRef, args[2] as ValueRef)
        };

        let blk1 = b.create_block();
        let blk2 = b.create_block();
        let blk3 = b.create_block();
        let blk4 = b.create_block();
        let blk5 = b.create_block();
        let blk6 = b.create_block();

        // entry -> block1(r, c, sum)
        let r0 = b.u32(0, span);
        let c0 = b.u32(0, span);
        let s0 = b.u64(0, span);
        b.br(blk1, [r0, c0, s0], span)?;

        // block1: outer loop header
        let r = b.append_block_param(blk1, Type::U32, span);
        let c = b.append_block_param(blk1, Type::U32, span);
        let sum = b.append_block_param(blk1, Type::U64, span);
        b.switch_to_block(blk1);
        let cond_outer = b.eq(r, v1, span)?;
        Cf::cond_br(&mut b, cond_outer, blk2, [], blk3, [], span)?;

        // block2: return sum
        b.switch_to_block(blk2);
        b.ret(Some(sum), span)?;

        // block3: split edge to inner loop
        b.switch_to_block(blk3);
        b.br(blk4, [c, sum], span)?;

        // block4: inner loop header
        let col = b.append_block_param(blk4, Type::U32, span);
        let acc = b.append_block_param(blk4, Type::U64, span);
        b.switch_to_block(blk4);
        let cond_inner = b.eq(col, v2, span)?;
        // If inner loop finished (col == v2), forward state to blk5; otherwise run body
        Cf::cond_br(&mut b, cond_inner, blk5, [col, acc], blk6, [], span)?;

        // block5: latch to outer loop; receive forwarded inner state
        b.switch_to_block(blk5);
        let pcol = b.append_block_param(blk5, Type::U32, span);
        let pacc = b.append_block_param(blk5, Type::U64, span);
        let one = b.u32(1, span);
        let r1 = b.add_unchecked(r, one, span)?;
        b.br(blk1, [r1, pcol, pacc], span)?;

        // block6: inner loop body
        b.switch_to_block(blk6);
        let one = b.u32(1, span);
        let rowm1 = b.sub_unchecked(r, one, span)?;
        let row_off = b.mul_unchecked(rowm1, v2, span)?;
        let offset = b.add_unchecked(col, row_off, span)?;
        let base = b.ptrtoint(v0, Type::U32, span)?;
        let addr = b.add_unchecked(base, offset, span)?;
        let v3c = b.inttoptr(
            addr,
            Type::Ptr(Arc::new(PointerType::new_with_address_space(
                Type::U64,
                AddressSpace::Element,
            ))),
            span,
        )?;
        let load = b.load(v3c, span)?;
        // Create extra pressure by making multiple values live and passing them to a call
        let k1 = b.u64(1, span);
        let k2 = b.u64(2, span);
        let k3 = b.u64(3, span);
        let k4 = b.u64(4, span);
        let k5 = b.u64(5, span);
        let k6 = b.u64(6, span);
        let _k7 = b.u64(7, span);
        let call = b.exec(callee, callee_sig.clone(), [v3c, load, k1, k2, k3, k4, k5, k6], span)?;
        let accn = b.add_unchecked(acc, load, span)?;
        let one2 = b.u32(1, span);
        let coln = b.add_unchecked(col, one2, span)?;
        // Backedge: continue inner loop by jumping to its header
        b.br(blk4, [coln, accn], span)?;

        call.as_operation_ref()
    };

    expect_file!["expected/spills_loop_nest.hir"]
        .assert_eq(&func.as_operation_ref().borrow().to_string());

    let am = AnalysisManager::new(func.as_operation_ref(), None);
    let spills = am.get_analysis_for::<SpillAnalysis, Function>()?;

    // With the added call in the inner loop body, stack pressure exceeds 16,
    // so spills and reloads are expected.
    assert!(spills.has_spills());
    assert!(!spills.spills().is_empty());
    assert!(!spills.reloads().is_empty());

    // We expect that at least one of the key live values is spilled before the call in block6
    let spilled_at_call = spills
        .spills()
        .iter()
        .filter(|s| matches!(s.place, crate::analyses::spills::Placement::At(pp) if pp == ProgramPoint::before(call_op6)))
        .map(|s| s.value)
        .collect::<alloc::vec::Vec<_>>();
    assert!(!spilled_at_call.is_empty());
    // There should be at least one split created due to differing W/S sets across edges
    assert!(!spills.splits().is_empty());
    // And at least one reload must be placed on a split
    let reloads_on_splits = spills
        .reloads()
        .iter()
        .any(|r| matches!(r.place, crate::analyses::spills::Placement::Split(_)));
    assert!(reloads_on_splits);

    Ok(())
}

#[test]
fn spills_entry_block_args_over_k() -> AnalysisResult<()> {
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

    // Construct a function whose entry block arguments alone exceed K=16 stack slots.
    //
    // Each `u64` occupies 2 felts on the Miden stack, so 9×`u64` => 18 felts, which forces at least
    // one spill at the start of the entry block.
    let params = (0..9).map(|_| AbiParam::new(Type::U64)).collect::<alloc::vec::Vec<_>>();
    let func = ob.create_function(
        Ident::with_empty_span("test::spill_entry_args_over_k".into()),
        Signature::new(params, [AbiParam::new(Type::U32)]),
    )?;
    module.borrow_mut().symbol_manager_mut().insert_new(func, ProgramPoint::Invalid);

    let entry_block: BlockRef;
    let block_args: alloc::vec::Vec<ValueRef>;
    {
        let mut b = FunctionBuilder::new(func, &mut ob);
        entry_block = b.current_block();
        // Capture the entry block arguments so we can assert which one is spilled, and where.
        block_args = entry_block
            .borrow()
            .arguments()
            .iter()
            .copied()
            .map(|v| v as ValueRef)
            .collect();

        // Keep the function body minimal so the only stack pressure comes from the entry arguments.
        let zero = b.u32(0, span);
        b.ret(Some(zero), span)?;
    }

    let am = AnalysisManager::new(func.as_operation_ref(), None);
    let spills = am.get_analysis_for::<SpillAnalysis, Function>()?;

    // If entry args exceed K, SpillAnalysis must proactively spill enough args at block start so
    // W^entry fits in the working stack.
    assert!(spills.has_spills(), "expected spills when entry args exceed K");

    let entry = ProgramPoint::at_start_of(entry_block);
    let w_entry_usage = spills.w_entry(&entry).iter().map(|o| o.stack_size()).sum::<usize>();
    // Regression check: after inserting entry spills, the live working set on entry must be <= K.
    assert!(w_entry_usage <= 16, "expected W^entry to fit within K=16, got {w_entry_usage}");

    // We expect the highest-index argument(s) to be the first spill candidates.
    let spilled_arg =
        *block_args.last().expect("expected at least one block argument in over-K test");
    assert!(spills.is_spilled(&spilled_arg));
    // Regression check: the spill should be recorded at the start-of-block program point (not
    // remapped to end-of-block).
    assert!(spills.is_spilled_at(spilled_arg, entry));

    Ok(())
}

#[test]
fn spills_region_branch_results_over_k() -> AnalysisResult<()> {
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

    // Construct a function which returns an `scf.if` result set which alone exceeds K=16 stack slots.
    //
    // Each `u64` occupies 2 felts on the Miden stack, so 9×`u64` => 18 felts, which forces at least
    // one spill at `ProgramPoint::after(if_op)`.
    let func = ob.create_function(
        Ident::with_empty_span("test::spill_region_branch_results_over_k".into()),
        Signature::new([], [AbiParam::new(Type::U64)]),
    )?;
    module.borrow_mut().symbol_manager_mut().insert_new(func, ProgramPoint::Invalid);

    let if_op: OperationRef;
    let if_results: alloc::vec::Vec<ValueRef>;
    {
        let mut b = FunctionBuilder::new(func, &mut ob);
        let entry = b.current_block();

        let cond = b.i1(true, span);
        let result_tys = alloc::vec![Type::U64; 9];
        let scf_if = b.r#if(cond, &result_tys, span)?;
        if_op = scf_if.as_operation_ref();
        if_results = if_op
            .borrow()
            .results()
            .all()
            .iter()
            .map(|r| r.borrow().as_value_ref())
            .collect();

        // Populate the `then` and `else` regions with yields for all results.
        let (then_region, else_region) = {
            let if_op = scf_if.borrow();
            (if_op.then_body().as_region_ref(), if_op.else_body().as_region_ref())
        };

        let then_block = b.create_block_in_region(then_region);
        b.switch_to_block(then_block);
        let then_values = (0..result_tys.len())
            .map(|_| b.u64(0, span))
            .collect::<alloc::vec::Vec<_>>();
        b.r#yield(then_values, span)?;

        let else_block = b.create_block_in_region(else_region);
        b.switch_to_block(else_block);
        let else_values = (0..result_tys.len())
            .map(|_| b.u64(1, span))
            .collect::<alloc::vec::Vec<_>>();
        b.r#yield(else_values, span)?;

        // Return the first result so the `scf.if` is reachable and well-formed.
        b.switch_to_block(entry);
        let first = *if_results.first().expect("expected at least one `scf.if` result");
        b.ret(Some(first), span)?;
    }

    let am = AnalysisManager::new(func.as_operation_ref(), None);
    let spills = am.get_analysis_for::<SpillAnalysis, Function>()?;

    assert!(
        spills.has_spills(),
        "expected spills when region branch results exceed K"
    );

    let after_if = ProgramPoint::after(if_op);
    let spilled_result = *if_results
        .last()
        .expect("expected at least one `scf.if` result in over-K test");
    assert!(spills.is_spilled(&spilled_result));
    assert!(spills.is_spilled_at(spilled_result, after_if));

    Ok(())
}
