use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use miden_assembly::Assembler;
use miden_assembly_syntax::{Parse, ast::Instruction};
use miden_core::serde::Serializable;
use miden_package_registry::{
    NoPackageStore, PackageId, PackageRecord, PackageRegistry, PackageVersions, Version,
};
use miden_project::ProjectDependencyGraphBuilder;
use midenc_dialect_arith::{
    Add as ArithAdd, And as ArithAnd, Constant as ArithConstant, Eq as ArithEq,
    Ext2Add as ArithExt2Add, Ext2Div as ArithExt2Div, Ext2Inv as ArithExt2Inv,
    Ext2Mul as ArithExt2Mul, Ext2Neg as ArithExt2Neg, Ext2Sub as ArithExt2Sub, Incr as ArithIncr,
    Mul as ArithMul, Split as ArithSplit, Trunc as ArithTrunc, Zext as ArithZext,
};
use midenc_dialect_cf::Select as CfSelect;
use midenc_dialect_hir::{
    AdviceLoadWord as HirAdviceLoadWord, AdvicePipe as HirAdvicePipe, AdvicePop as HirAdvicePop,
    Assert as HirAssert, AssertEq as HirAssertEq, AssertU32 as HirAssertU32, Assertz as HirAssertz,
    Caller as HirCaller, Clk as HirClk, CryptoStream as HirCryptoStream, EmitEvent as HirEmitEvent,
    EmitEventImm as HirEmitEventImm, EvalCircuit as HirEvalCircuit,
    FriExt2Fold4 as HirFriExt2Fold4, HMerge as HirHMerge, HPerm as HirHPerm, Hash as HirHash,
    HornerBase as HirHornerBase, HornerExt as HirHornerExt, IntToPtr as HirIntToPtr,
    Load as HirLoad, LoadLocal as HirLoadLocal, LocalAddress as HirLocalAddress,
    LogPrecompile as HirLogPrecompile, MTreeGet as HirMTreeGet, MTreeMerge as HirMTreeMerge,
    MTreeSet as HirMTreeSet, MTreeVerify as HirMTreeVerify, MemStream as HirMemStream,
    Store as HirStore, StoreLocal as HirStoreLocal, SystemEvent as HirSystemEvent,
    analyses::{
        AdviceTaintAnalysis, AdviceTaintContextKind, AdviceTaintDiagnostic, AdviceTaintExitFinding,
        AdviceTaintExternalCallFinding, AdviceTaintFinding, AdviceTaintOriginKind,
    },
};
use midenc_dialect_scf::{If, While};
use midenc_hir::{
    AddressSpace, ArrayType, CallConv, FunctionType, Immediate, PointerType, SymbolName,
    SymbolTable, Type,
    diagnostics::{Report, Severity},
    dialects::builtin::{self, Function, UnrealizedConversionCast},
    pass::AnalysisManager,
};

use super::*;
use crate::semantics::{
    INFER_ONLY_INSTRUCTION_VARIANT_COUNT, InstructionSemantics,
    LIFT_AND_INFER_INSTRUCTION_VARIANT_COUNT, UNSUPPORTED_INSTRUCTION_VARIANT_COUNT,
    instruction_semantics,
};

#[test]
fn lifts_known_signature_u32_add() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc add(a: u32, b: u32) -> u32
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "add");
    let signature = function.borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 2);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    let entry = function.borrow().entry_block();
    assert_eq!(entry.borrow().body().iter().count(), 2);

    Ok(())
}

#[test]
fn rejects_missing_signature_in_phase_one() {
    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
pub proc no_sig
push.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    );
    let err = match result {
        Ok(_) => panic!("expected disassembly to reject a missing signature"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("missing a signature"));
}

#[test]
fn rejects_declared_signature_body_drift() {
    let cases = [
        (
            "extra_value",
            r#"
pub proc extra_value(value: felt) -> felt
dup.0
end
"#,
            "leaves 1 extra value(s) on the stack",
        ),
        (
            "missing_value",
            r#"
pub proc missing_value(value: felt) -> (felt, felt)
nop
end
"#,
            "stack underflow",
        ),
        (
            "undeclared_input",
            r#"
pub proc undeclared_input(value: felt) -> felt
add
end
"#,
            "stack underflow",
        ),
    ];

    for (name, source, expected) in cases {
        let context = Rc::new(Context::default());
        let result = disassemble_source(source, "test", &DisassemblerConfig::default(), context);
        let err = match result {
            Ok(_) => panic!("expected disassembly to reject signature drift in {name}"),
            Err(err) => err.to_string(),
        };

        assert!(
            err.contains(expected),
            "expected diagnostic for {name} to contain {expected:?}, got:\n{err}"
        );
    }
}

#[test]
fn rejects_unsupported_instruction_during_known_signature_lifting() {
    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
pub proc unsupported(value: u32) -> u32
dynexec
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    );
    let err = match result {
        Ok(_) => panic!("expected disassembly to reject unsupported instruction"),
        Err(err) => err,
    };

    let err = err.to_string();
    assert!(err.contains("not supported during disassembly"));
    assert!(err.contains("DynExec"));
}

#[test]
fn rejects_unsupported_instruction_during_signature_inference() {
    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
pub proc unsupported
dynexec
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    );
    let err = match result {
        Ok(_) => panic!("expected inference to reject unsupported instruction"),
        Err(err) => err,
    };

    let err = err.to_string();
    assert!(err.contains("signature inference is not implemented"));
    assert!(err.contains("DynExec"));
}

#[test]
fn supported_instruction_matrix_lifts() {
    let mut cases = vec![
        instruction_case("nop", &[], &[], "nop"),
        instruction_case("drop", &["felt"], &[], "drop"),
        instruction_case("dropw", &["felt", "felt", "felt", "felt"], &[], "dropw"),
        felt_instruction_case("padw", 0, 4, "padw"),
        felt_instruction_case("push", 0, 0, "push.1 drop"),
        felt_instruction_case("push_word", 0, 0, "push.[1,2,3,4] dropw"),
        felt_instruction_case("push_slice", 0, 0, "push.[1,2,3,4][1..3] drop drop"),
        felt_instruction_case("push_felt_list", 0, 0, "push.1.2.3 drop drop drop"),
        instruction_case("sdepth", &["felt", "felt"], &felt_types(3), "sdepth"),
        instruction_case("caller", &[], &["[felt; 4]"], "caller"),
        instruction_case("clk", &[], &["felt"], "clk"),
        instruction_case("adv_push", &[], &felt_types(3), "adv_push.3"),
        instruction_case("adv_loadw", &felt_types(4), &felt_types(4), "adv_loadw"),
        instruction_case("adv_pipe", &felt_types(13), &felt_types(13), "adv_pipe"),
        instruction_case("emit", &["felt"], &["felt"], "emit"),
        instruction_case("emit_imm", &[], &[], r#"emit.event("phase28")"#),
        instruction_case("adv_push_mapval", &felt_types(4), &felt_types(4), "adv.push_mapval"),
        instruction_case(
            "adv_push_mapval_count",
            &felt_types(4),
            &felt_types(4),
            "adv.push_mapval_count",
        ),
        instruction_case(
            "adv_push_mapvaln_0",
            &felt_types(4),
            &felt_types(4),
            "adv.push_mapvaln.0",
        ),
        instruction_case(
            "adv_push_mapvaln_4",
            &felt_types(4),
            &felt_types(4),
            "adv.push_mapvaln.4",
        ),
        instruction_case(
            "adv_push_mapvaln_8",
            &felt_types(4),
            &felt_types(4),
            "adv.push_mapvaln.8",
        ),
        instruction_case("adv_has_mapkey", &felt_types(4), &felt_types(4), "adv.has_mapkey"),
        instruction_case("adv_push_mtnode", &felt_types(6), &felt_types(6), "adv.push_mtnode"),
        instruction_case("adv_insert_mem", &felt_types(6), &felt_types(6), "adv.insert_mem"),
        instruction_case("adv_insert_hdword", &felt_types(8), &felt_types(8), "adv.insert_hdword"),
        instruction_case(
            "adv_insert_hdword_d",
            &felt_types(9),
            &felt_types(9),
            "adv.insert_hdword_d",
        ),
        instruction_case("adv_insert_hperm", &felt_types(12), &felt_types(12), "adv.insert_hperm"),
        instruction_case(
            "adv_insert_hqword",
            &felt_types(16),
            &felt_types(16),
            "adv.insert_hqword",
        ),
        instruction_case("hash", &felt_types(4), &felt_types(4), "hash"),
        instruction_case("hmerge", &felt_types(8), &felt_types(4), "hmerge"),
        instruction_case("hperm", &felt_types(12), &felt_types(12), "hperm"),
        instruction_case("mtree_get", &felt_types(6), &felt_types(8), "mtree_get"),
        instruction_case("mtree_set", &felt_types(10), &felt_types(8), "mtree_set"),
        instruction_case("mtree_merge", &felt_types(8), &felt_types(4), "mtree_merge"),
        instruction_case("mtree_verify", &felt_types(10), &felt_types(10), "mtree_verify"),
        instruction_case(
            "mtree_verify_err",
            &felt_types(10),
            &felt_types(10),
            "mtree_verify.err=\"bad path\"",
        ),
        instruction_case("crypto_stream", &felt_types(14), &felt_types(14), "crypto_stream"),
        instruction_case("fri_ext2fold4", &felt_types(17), &felt_types(16), "fri_ext2fold4"),
        instruction_case("horner_eval_base", &felt_types(16), &felt_types(16), "horner_eval_base"),
        instruction_case("horner_eval_ext", &felt_types(16), &felt_types(16), "horner_eval_ext"),
        instruction_case("eval_circuit", &felt_types(3), &felt_types(3), "eval_circuit"),
        instruction_case("log_precompile", &felt_types(12), &felt_types(12), "log_precompile"),
        instruction_case("debug", &["felt"], &["felt"], "debug.stack"),
        instruction_case("trace", &["felt"], &["felt"], "trace.1"),
        instruction_case_with_locals("loc_load", 1, &[], &["felt"], "loc_load.0"),
        instruction_case_with_locals(
            "locaddr",
            1,
            &[],
            &["ptr<felt, addrspace(felt)>"],
            "locaddr.0",
        ),
        instruction_case_with_locals("loc_store", 1, &["felt"], &[], "loc_store.0"),
        instruction_case_with_locals("loc_loadw_be", 4, &[], &felt_types(4), "loc_loadw_be.0"),
        instruction_case_with_locals("loc_loadw_le", 4, &[], &felt_types(4), "loc_loadw_le.0"),
        instruction_case_with_locals(
            "loc_storew_be",
            4,
            &felt_types(4),
            &felt_types(4),
            "loc_storew_be.0",
        ),
        instruction_case_with_locals(
            "loc_storew_le",
            4,
            &felt_types(4),
            &felt_types(4),
            "loc_storew_le.0",
        ),
        instruction_case("mem_load", &["u32"], &["felt"], "mem_load"),
        instruction_case("mem_load_imm", &[], &["felt"], "mem_load.0"),
        instruction_case("mem_stream", &felt_types(13), &felt_types(13), "mem_stream"),
        instruction_case(
            "mem_loadw_be",
            &["u32", "felt", "felt", "felt", "felt"],
            &felt_types(4),
            "mem_loadw_be",
        ),
        instruction_case("mem_loadw_be_imm", &felt_types(4), &felt_types(4), "mem_loadw_be.0"),
        instruction_case(
            "mem_loadw_le",
            &["u32", "felt", "felt", "felt", "felt"],
            &felt_types(4),
            "mem_loadw_le",
        ),
        instruction_case("mem_loadw_le_imm", &felt_types(4), &felt_types(4), "mem_loadw_le.0"),
        instruction_case("mem_store", &["u32", "felt"], &[], "mem_store"),
        instruction_case("mem_store_imm", &["felt"], &[], "mem_store.0"),
        instruction_case(
            "mem_storew_be",
            &["u32", "felt", "felt", "felt", "felt"],
            &felt_types(4),
            "mem_storew_be",
        ),
        instruction_case("mem_storew_be_imm", &felt_types(4), &felt_types(4), "mem_storew_be.0"),
        instruction_case(
            "mem_storew_le",
            &["u32", "felt", "felt", "felt", "felt"],
            &felt_types(4),
            "mem_storew_le",
        ),
        instruction_case("mem_storew_le_imm", &felt_types(4), &felt_types(4), "mem_storew_le.0"),
        felt_instruction_case("add", 2, 1, "add"),
        felt_instruction_case("add_imm", 1, 1, "add.2"),
        felt_instruction_case("sub", 2, 1, "sub"),
        felt_instruction_case("sub_imm", 1, 1, "sub.2"),
        felt_instruction_case("mul", 2, 1, "mul"),
        felt_instruction_case("mul_imm", 1, 1, "mul.2"),
        felt_instruction_case("div", 2, 1, "div"),
        felt_instruction_case("div_imm", 1, 1, "div.2"),
        felt_instruction_case("ext2add", 4, 2, "ext2add"),
        felt_instruction_case("ext2sub", 4, 2, "ext2sub"),
        felt_instruction_case("ext2mul", 4, 2, "ext2mul"),
        felt_instruction_case("ext2div", 4, 2, "ext2div"),
        felt_instruction_case("ext2neg", 2, 2, "ext2neg"),
        felt_instruction_case("ext2inv", 2, 2, "ext2inv"),
        felt_instruction_case("neg", 1, 1, "neg"),
        felt_instruction_case("ilog2", 1, 1, "ilog2"),
        felt_instruction_case("inv", 1, 1, "inv"),
        felt_instruction_case("incr", 1, 1, "add.1"),
        felt_instruction_case("pow2", 1, 1, "pow2"),
        felt_instruction_case("exp", 2, 1, "exp"),
        felt_instruction_case("exp_imm", 1, 1, "exp.2"),
        instruction_case("not", &["i1"], &["i1"], "not"),
        instruction_case("and", &["i1", "i1"], &["i1"], "and"),
        instruction_case("or", &["i1", "i1"], &["i1"], "or"),
        instruction_case("xor", &["i1", "i1"], &["i1"], "xor"),
        instruction_case("eq", &["felt", "felt"], &["i1"], "eq"),
        instruction_case("eq_imm", &["felt"], &["i1"], "eq.2"),
        instruction_case("neq", &["felt", "felt"], &["i1"], "neq"),
        instruction_case("neq_imm", &["felt"], &["i1"], "neq.2"),
        instruction_case("eqw", &felt_types(8), &["i1"], "eqw"),
        instruction_case("lt", &["felt", "felt"], &["i1"], "lt"),
        instruction_case("lte", &["felt", "felt"], &["i1"], "lte"),
        instruction_case("gt", &["felt", "felt"], &["i1"], "gt"),
        instruction_case("gte", &["felt", "felt"], &["i1"], "gte"),
        instruction_case("is_odd", &["felt"], &["i1"], "is_odd"),
        instruction_case("assert", &["i1"], &[], "assert"),
        instruction_case("assert_err", &["i1"], &[], "assert.err=\"boom\""),
        instruction_case("assertz", &["i1"], &[], "assertz"),
        instruction_case("assertz_err", &["i1"], &[], "assertz.err=\"boom\""),
        instruction_case("assert_eq", &["felt", "felt"], &[], "assert_eq"),
        instruction_case("assert_eq_err", &["felt", "felt"], &[], "assert_eq.err=\"boom\""),
        instruction_case("assert_eqw", &felt_types(8), &[], "assert_eqw"),
        instruction_case("assert_eqw_err", &felt_types(8), &[], "assert_eqw.err=\"boom\""),
        instruction_case("u32cast", &["felt"], &["u32"], "u32cast"),
        instruction_case("u32assert", &["felt"], &["u32"], "u32assert"),
        instruction_case("u32assert_err", &["felt"], &["u32"], "u32assert.err=\"boom\""),
        instruction_case("u32assert2", &["felt", "felt"], &["u32", "u32"], "u32assert2"),
        instruction_case(
            "u32assert2_err",
            &["felt", "felt"],
            &["u32", "u32"],
            "u32assert2.err=\"boom\"",
        ),
        instruction_case("u32assertw", &felt_types(4), &u32_types(4), "u32assertw"),
        instruction_case(
            "u32assertw_err",
            &felt_types(4),
            &u32_types(4),
            "u32assertw.err=\"boom\"",
        ),
        instruction_case("u32test", &["felt"], &["i1", "felt"], "u32test"),
        instruction_case(
            "u32testw",
            &felt_types(4),
            &["i1", "felt", "felt", "felt", "felt"],
            "u32testw",
        ),
        instruction_case("u32split", &["felt"], &["u32", "u32"], "u32split"),
        instruction_case("cdrop", &["i1", "felt", "felt"], &["felt"], "cdrop"),
        instruction_case("cswap", &["i1", "felt", "felt"], &["felt", "felt"], "cswap"),
        instruction_case("cdropw", &felt_word_select_params(), &felt_types(4), "cdropw"),
        instruction_case("cswapw", &felt_word_select_params(), &felt_types(8), "cswapw"),
        instruction_case("u32wrapping_add", &["u32", "u32"], &["u32"], "u32wrapping_add"),
        instruction_case("u32wrapping_add_imm", &["u32"], &["u32"], "u32wrapping_add.2"),
        instruction_case("u32wrapping_add3", &u32_types(3), &["u32"], "u32wrapping_add3"),
        instruction_case(
            "u32overflowing_add",
            &["u32", "u32"],
            &["felt", "felt"],
            "u32overflowing_add",
        ),
        instruction_case(
            "u32overflowing_add_imm",
            &["u32"],
            &["felt", "felt"],
            "u32overflowing_add.2",
        ),
        instruction_case("u32widening_add", &u32_types(2), &u32_types(2), "u32widening_add"),
        instruction_case("u32widening_add_imm", &["u32"], &u32_types(2), "u32widening_add.2"),
        instruction_case("u32widening_add3", &u32_types(3), &u32_types(2), "u32widening_add3"),
        instruction_case(
            "u32overflowing_add3",
            &u32_types(3),
            &u32_types(2),
            "u32overflowing_add3",
        ),
        instruction_case("u32widening_madd", &u32_types(3), &u32_types(2), "u32widening_madd"),
        instruction_case("u32wrapping_madd", &u32_types(3), &["u32"], "u32wrapping_madd"),
        instruction_case("u32wrapping_sub", &["u32", "u32"], &["u32"], "u32wrapping_sub"),
        instruction_case("u32wrapping_sub_imm", &["u32"], &["u32"], "u32wrapping_sub.2"),
        instruction_case(
            "u32overflowing_sub",
            &["u32", "u32"],
            &["felt", "felt"],
            "u32overflowing_sub",
        ),
        instruction_case(
            "u32overflowing_sub_imm",
            &["u32"],
            &["felt", "felt"],
            "u32overflowing_sub.2",
        ),
        instruction_case("u32wrapping_mul", &["u32", "u32"], &["u32"], "u32wrapping_mul"),
        instruction_case("u32wrapping_mul_imm", &["u32"], &["u32"], "u32wrapping_mul.2"),
        instruction_case("u32widening_mul", &u32_types(2), &u32_types(2), "u32widening_mul"),
        instruction_case("u32widening_mul_imm", &["u32"], &u32_types(2), "u32widening_mul.2"),
        instruction_case("u32div", &["u32", "u32"], &["u32"], "u32div"),
        instruction_case("u32div_imm", &["u32"], &["u32"], "u32div.2"),
        instruction_case("u32mod", &["u32", "u32"], &["u32"], "u32mod"),
        instruction_case("u32mod_imm", &["u32"], &["u32"], "u32mod.2"),
        instruction_case("u32divmod", &["u32", "u32"], &["u32", "u32"], "u32divmod"),
        instruction_case("u32divmod_imm", &["u32"], &["u32", "u32"], "u32divmod.2"),
        instruction_case("u32and", &["u32", "u32"], &["u32"], "u32and"),
        instruction_case("u32or", &["u32", "u32"], &["u32"], "u32or"),
        instruction_case("u32xor", &["u32", "u32"], &["u32"], "u32xor"),
        instruction_case("u32not", &["u32"], &["u32"], "u32not"),
        instruction_case("u32shr", &["u32", "u32"], &["u32"], "u32shr"),
        instruction_case("u32shr_imm", &["u32"], &["u32"], "u32shr.2"),
        instruction_case("u32shl", &["u32", "u32"], &["u32"], "u32shl"),
        instruction_case("u32shl_imm", &["u32"], &["u32"], "u32shl.2"),
        instruction_case("u32rotr", &["u32", "u32"], &["u32"], "u32rotr"),
        instruction_case("u32rotr_imm", &["u32"], &["u32"], "u32rotr.2"),
        instruction_case("u32rotl", &["u32", "u32"], &["u32"], "u32rotl"),
        instruction_case("u32rotl_imm", &["u32"], &["u32"], "u32rotl.2"),
        instruction_case("u32popcnt", &["u32"], &["u32"], "u32popcnt"),
        instruction_case("u32ctz", &["u32"], &["u32"], "u32ctz"),
        instruction_case("u32clz", &["u32"], &["u32"], "u32clz"),
        instruction_case("u32clo", &["u32"], &["u32"], "u32clo"),
        instruction_case("u32cto", &["u32"], &["u32"], "u32cto"),
        instruction_case("u32lt", &["u32", "u32"], &["i1"], "u32lt"),
        instruction_case("u32lte", &["u32", "u32"], &["i1"], "u32lte"),
        instruction_case("u32gt", &["u32", "u32"], &["i1"], "u32gt"),
        instruction_case("u32gte", &["u32", "u32"], &["i1"], "u32gte"),
        instruction_case("u32min", &["u32", "u32"], &["u32"], "u32min"),
        instruction_case("u32max", &["u32", "u32"], &["u32"], "u32max"),
        felt_instruction_case("reversew", 4, 4, "reversew"),
        felt_instruction_case("reversedw", 8, 8, "reversedw"),
        felt_instruction_case("swapdw", 16, 16, "swapdw"),
    ];

    for depth in 0..=15 {
        cases.push(felt_instruction_case(
            format!("dup_{depth}"),
            depth + 1,
            depth + 2,
            format!("dup.{depth}"),
        ));
    }
    for depth in 1..=15 {
        cases.push(felt_instruction_case(
            format!("swap_{depth}"),
            depth + 1,
            depth + 1,
            format!("swap.{depth}"),
        ));
    }
    for depth in 2..=15 {
        cases.push(felt_instruction_case(
            format!("movup_{depth}"),
            depth + 1,
            depth + 1,
            format!("movup.{depth}"),
        ));
        cases.push(felt_instruction_case(
            format!("movdn_{depth}"),
            depth + 1,
            depth + 1,
            format!("movdn.{depth}"),
        ));
    }
    for depth in 0..=3 {
        cases.push(felt_instruction_case(
            format!("dupw_{depth}"),
            4 * (depth + 1),
            4 * (depth + 2),
            format!("dupw.{depth}"),
        ));
    }
    for depth in 1..=3 {
        cases.push(felt_instruction_case(
            format!("swapw_{depth}"),
            4 * (depth + 1),
            4 * (depth + 1),
            format!("swapw.{depth}"),
        ));
    }
    for depth in 2..=3 {
        cases.push(felt_instruction_case(
            format!("movupw_{depth}"),
            4 * (depth + 1),
            4 * (depth + 1),
            format!("movupw.{depth}"),
        ));
        cases.push(felt_instruction_case(
            format!("movdnw_{depth}"),
            4 * (depth + 1),
            4 * (depth + 1),
            format!("movdnw.{depth}"),
        ));
    }

    for case in &cases {
        assert_instruction_case_lifts(case);
    }
}

#[test]
fn supported_invocation_instruction_matrix_lifts() {
    for (name, instruction) in [("exec", "exec.callee"), ("call", "call.callee")] {
        let source = format!(
            r#"
proc callee(value: felt) -> felt
add.1
end

pub proc matrix_{name}(value: felt) -> felt
{instruction}
end
"#
        );

        let context = Rc::new(Context::default());
        if let Err(err) =
            disassemble_source(source.clone(), "test", &DisassemblerConfig::default(), context)
        {
            panic!("expected invocation matrix case '{name}' to lift\n{source}\nerror: {err}");
        }
    }

    let source = r#"
pub proc matrix_syscall(value: felt) -> felt
syscall.callee
end
"#;
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures
        .insert("::$kernel::callee".to_owned(), masm_signature([Type::Felt], [Type::Felt]));
    if let Err(err) = disassemble_source_with_external_signatures(
        source,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    ) {
        panic!("expected invocation matrix case 'syscall' to lift\n{source}\nerror: {err}");
    }
}

#[test]
fn lifts_ext2_instructions_to_first_class_arith_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc ext2_add(rhs0: felt, rhs1: felt, lhs0: felt, lhs1: felt) -> (felt, felt)
ext2add
end

pub proc ext2_sub(rhs0: felt, rhs1: felt, lhs0: felt, lhs1: felt) -> (felt, felt)
ext2sub
end

pub proc ext2_mul(rhs0: felt, rhs1: felt, lhs0: felt, lhs1: felt) -> (felt, felt)
ext2mul
end

pub proc ext2_div(rhs0: felt, rhs1: felt, lhs0: felt, lhs1: felt) -> (felt, felt)
ext2div
end

pub proc ext2_neg(operand0: felt, operand1: felt) -> (felt, felt)
ext2neg
end

pub proc ext2_inv(operand0: felt, operand1: felt) -> (felt, felt)
ext2inv
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(top_level_op_count::<ArithExt2Add>(find_function(output.module, "ext2_add")), 1);
    assert_eq!(top_level_op_count::<ArithExt2Sub>(find_function(output.module, "ext2_sub")), 1);
    assert_eq!(top_level_op_count::<ArithExt2Mul>(find_function(output.module, "ext2_mul")), 1);
    assert_eq!(top_level_op_count::<ArithExt2Div>(find_function(output.module, "ext2_div")), 1);
    assert_eq!(top_level_op_count::<ArithExt2Neg>(find_function(output.module, "ext2_neg")), 1);
    assert_eq!(top_level_op_count::<ArithExt2Inv>(find_function(output.module, "ext2_inv")), 1);
    Ok(())
}

#[test]
fn lifts_vm_context_instructions_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(1)
pub proc local_addr() -> ptr<felt, addrspace(felt)>
locaddr.0
end

pub proc caller_word() -> [felt; 4]
caller
end

pub proc current_clk() -> felt
clk
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(
        top_level_op_count::<HirLocalAddress>(find_function(output.module, "local_addr")),
        1
    );
    assert_eq!(top_level_op_count::<HirCaller>(find_function(output.module, "caller_word")), 1);
    assert_eq!(top_level_op_count::<HirClk>(find_function(output.module, "current_clk")), 1);
    Ok(())
}

#[test]
fn lifts_advice_and_event_ops_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc advice_values() -> (felt, felt, felt)
adv_push.3
end

pub proc advice_word(a: felt, b: felt, c: felt, d: felt) -> (felt, felt, felt, felt)
adv_loadw
end

pub proc emitted(event_id: felt) -> felt
emit
end

pub proc emitted_imm()
emit.event("phase28")
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(
        top_level_op_count::<HirAdvicePop>(find_function(output.module, "advice_values")),
        3
    );
    assert_eq!(
        top_level_op_count::<HirAdviceLoadWord>(find_function(output.module, "advice_word")),
        1
    );
    assert_eq!(top_level_op_count::<HirEmitEvent>(find_function(output.module, "emitted")), 1);
    assert_eq!(
        top_level_op_count::<HirEmitEventImm>(find_function(output.module, "emitted_imm")),
        1
    );
    Ok(())
}

#[test]
fn lifts_system_events_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let hqword_params = (0..16).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let hqword_results = vec!["felt"; 16].join(", ");
    let source = format!(
        r#"
pub proc map_event(k0: felt, k1: felt, k2: felt, k3: felt) -> (felt, felt, felt, felt)
adv.push_mapval
end

pub proc hqword_event({hqword_params}) -> ({hqword_results})
adv.insert_hqword
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(
        top_level_op_count::<HirSystemEvent>(find_function(output.module, "map_event")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirSystemEvent>(find_function(output.module, "hqword_event")),
        1
    );
    Ok(())
}

#[test]
fn lifts_core_hash_ops_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let state_params = (0..12).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let state_results = vec!["felt"; 12].join(", ");
    let source = format!(
        r#"
pub proc hash_word(a0: felt, a1: felt, a2: felt, a3: felt) -> (felt, felt, felt, felt)
hash
end

pub proc merge_words(a0: felt, a1: felt, a2: felt, a3: felt, b0: felt, b1: felt, b2: felt, b3: felt) -> (felt, felt, felt, felt)
hmerge
end

pub proc permute_state({state_params}) -> ({state_results})
hperm
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(top_level_op_count::<HirHash>(find_function(output.module, "hash_word")), 1);
    assert_eq!(top_level_op_count::<HirHMerge>(find_function(output.module, "merge_words")), 1);
    assert_eq!(top_level_op_count::<HirHPerm>(find_function(output.module, "permute_state")), 1);
    Ok(())
}

#[test]
fn lifts_merkle_ops_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let params6 = (0..6).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let params8 = (0..8).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let params10 = (0..10).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let results4 = vec!["felt"; 4].join(", ");
    let results8 = vec!["felt"; 8].join(", ");
    let results10 = vec!["felt"; 10].join(", ");
    let source = format!(
        r#"
pub proc get_node({params6}) -> ({results8})
mtree_get
end

pub proc set_node({params10}) -> ({results8})
mtree_set
end

pub proc merge_roots({params8}) -> ({results4})
mtree_merge
end

pub proc verify_node({params10}) -> ({results10})
mtree_verify.err="bad path"
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(top_level_op_count::<HirMTreeGet>(find_function(output.module, "get_node")), 1);
    assert_eq!(top_level_op_count::<HirMTreeSet>(find_function(output.module, "set_node")), 1);
    assert_eq!(
        top_level_op_count::<HirMTreeMerge>(find_function(output.module, "merge_roots")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirMTreeVerify>(find_function(output.module, "verify_node")),
        1
    );
    Ok(())
}

#[test]
fn lifts_crypto_stream_to_first_class_hir_op() -> Result<()> {
    let context = Rc::new(Context::default());
    let params = (0..14).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let results = vec!["felt"; 14].join(", ");
    let source = format!(
        r#"
pub proc stream_block({params}) -> ({results})
crypto_stream
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(
        top_level_op_count::<HirCryptoStream>(find_function(output.module, "stream_block")),
        1
    );
    Ok(())
}

#[test]
fn lifts_streaming_io_ops_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let params = (0..13).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let results = vec!["felt"; 13].join(", ");
    let source = format!(
        r#"
pub proc stream_memory({params}) -> ({results})
mem_stream
end

pub proc pipe_advice({params}) -> ({results})
adv_pipe
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(
        top_level_op_count::<HirMemStream>(find_function(output.module, "stream_memory")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirAdvicePipe>(find_function(output.module, "pipe_advice")),
        1
    );
    Ok(())
}

#[test]
fn lifts_proof_primitives_to_first_class_hir_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let params3 = (0..3).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let params12 = (0..12).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let params16 = (0..16).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let params17 = (0..17).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let results3 = vec!["felt"; 3].join(", ");
    let results12 = vec!["felt"; 12].join(", ");
    let results16 = vec!["felt"; 16].join(", ");
    let source = format!(
        r#"
pub proc fold_fri({params17}) -> ({results16})
fri_ext2fold4
end

pub proc horner_base({params16}) -> ({results16})
horner_eval_base
end

pub proc horner_ext({params16}) -> ({results16})
horner_eval_ext
end

pub proc eval_circuit_case({params3}) -> ({results3})
eval_circuit
end

pub proc log_precompile_case({params12}) -> ({results12})
log_precompile
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    assert_eq!(
        top_level_op_count::<HirFriExt2Fold4>(find_function(output.module, "fold_fri")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirHornerBase>(find_function(output.module, "horner_base")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirHornerExt>(find_function(output.module, "horner_ext")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirEvalCircuit>(find_function(output.module, "eval_circuit_case")),
        1
    );
    assert_eq!(
        top_level_op_count::<HirLogPrecompile>(find_function(output.module, "log_precompile_case")),
        1
    );
    Ok(())
}

#[test]
fn unsupported_instruction_matrix_reports_diagnostics() {
    let cases = [
        unsupported_instruction_case("dynexec", 0, "dynexec"),
        unsupported_instruction_case("dyncall", 0, "dyncall"),
        unsupported_instruction_case("exp_bit_length", 2, "exp.u32"),
    ];

    for case in &cases {
        assert_instruction_case_is_unsupported(case);
    }
}

#[test]
fn instruction_inventory_classifies_all_masm_instruction_variants() {
    assert_eq!(LIFT_AND_INFER_INSTRUCTION_VARIANT_COUNT, 234);
    assert_eq!(INFER_ONLY_INSTRUCTION_VARIANT_COUNT, 1);
    assert_eq!(UNSUPPORTED_INSTRUCTION_VARIANT_COUNT, 3);
    assert_eq!(
        LIFT_AND_INFER_INSTRUCTION_VARIANT_COUNT
            + INFER_ONLY_INSTRUCTION_VARIANT_COUNT
            + UNSUPPORTED_INSTRUCTION_VARIANT_COUNT,
        238
    );
    assert_eq!(instruction_semantics(&Instruction::Nop), InstructionSemantics::LiftAndInfer);
    assert_eq!(
        instruction_semantics(&Instruction::ProcRef(
            miden_assembly_syntax::ast::InvocationTarget::Symbol("foo".parse().unwrap())
        )),
        InstructionSemantics::InferOnly
    );
}

#[test]
fn infers_straight_line_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc inc
add.1
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "inc").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn infers_field_assertion_inputs_as_felt() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc assert_one
assert
end

pub proc assert_zero
assertz
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    for name in ["assert_one", "assert_zero"] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), 1);
        assert_eq!(signature.params()[0].ty, Type::Felt);
        assert!(signature.results().is_empty());
    }

    Ok(())
}

#[test]
fn infers_ext2_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc ext2_product
ext2mul
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "ext2_product").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 4);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 2);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    Ok(())
}

#[test]
fn infers_vm_context_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(1)
pub proc local_addr
locaddr.0
end

pub proc caller_word
caller
end

pub proc current_clk
clk
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "local_addr").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, felt_memory_pointer_type());

    let signature = find_function(output.module, "caller_word").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::from(ArrayType::new(Type::Felt, 4)));

    let signature = find_function(output.module, "current_clk").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);
    Ok(())
}

#[test]
fn infers_advice_and_event_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc advice_values
adv_push.2
end

pub proc advice_word
adv_loadw
end

pub proc emitted
emit
end

pub proc emitted_imm
emit.event("phase28")
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "advice_values").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 2);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));

    let signature = find_function(output.module, "advice_word").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 4);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 4);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));

    let signature = find_function(output.module, "emitted").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    let signature = find_function(output.module, "emitted_imm").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 0);
    Ok(())
}

#[test]
fn infers_system_event_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc map_event
adv.push_mapval
end

pub proc hqword_event
adv.insert_hqword
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "map_event").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 4);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 4);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));

    let signature = find_function(output.module, "hqword_event").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 16);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 16);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    Ok(())
}

#[test]
fn infers_core_hash_op_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc hash_word
hash
end

pub proc merge_words
hmerge
end

pub proc permute_state
hperm
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "hash_word").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 4);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 4);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));

    let signature = find_function(output.module, "merge_words").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 8);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 4);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));

    let signature = find_function(output.module, "permute_state").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 12);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 12);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    Ok(())
}

#[test]
fn infers_merkle_op_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc get_node
mtree_get
end

pub proc set_node
mtree_set
end

pub proc merge_roots
mtree_merge
end

pub proc verify_node
mtree_verify.err="bad path"
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    for (name, params, results) in [
        ("get_node", 6, 8),
        ("set_node", 10, 8),
        ("merge_roots", 8, 4),
        ("verify_node", 10, 10),
    ] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), params);
        assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
        assert_eq!(signature.results().len(), results);
        assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    }
    Ok(())
}

#[test]
fn infers_crypto_stream_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc stream_block
crypto_stream
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "stream_block").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 14);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 14);
    assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    Ok(())
}

#[test]
fn infers_streaming_io_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc stream_memory
mem_stream
end

pub proc pipe_advice
adv_pipe
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    for name in ["stream_memory", "pipe_advice"] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), 13);
        assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
        assert_eq!(signature.results().len(), 13);
        assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    }
    Ok(())
}

#[test]
fn infers_proof_primitive_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc fold_fri
fri_ext2fold4
end

pub proc horner_base
horner_eval_base
end

pub proc horner_ext
horner_eval_ext
end

pub proc eval_circuit_case
eval_circuit
end

pub proc log_precompile_case
log_precompile
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    for (name, params, results) in [
        ("fold_fri", 17, 16),
        ("horner_base", 16, 16),
        ("horner_ext", 16, 16),
        ("eval_circuit_case", 3, 3),
        ("log_precompile_case", 12, 12),
    ] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), params);
        assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
        assert_eq!(signature.results().len(), results);
        assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    }
    Ok(())
}

#[test]
fn infers_procref_as_word_but_lifting_remains_unsupported() -> Result<()> {
    let source = r#"
proc target()
nop
end

pub proc capture
procref.target
end
"#;
    let context = Rc::new(Context::default());
    let module = parse_test_module(source, &context)?;
    let target = module
        .procedures()
        .find(|procedure| procedure.name().as_str() == "target")
        .expect("target procedure");
    let mut signatures = rustc_hash::FxHashMap::default();
    signatures.insert(
        target.name().as_str().to_owned(),
        signatures::convert_signature(&context, &module, target.signature().unwrap())?,
    );
    let capture = module
        .procedures()
        .find(|procedure| procedure.name().as_str() == "capture")
        .expect("capture procedure");
    let signature =
        infer::infer_signature(&context, capture, &signatures, &rustc_hash::FxHashMap::default())?;

    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::from(ArrayType::new(Type::Felt, 4)));

    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
proc target()
nop
end

pub proc capture() -> [felt; 4]
procref.target
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )
    .map(|_| ());
    let err = match result {
        Ok(()) => panic!("known-signature procref lifting should remain unsupported"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("not supported during disassembly"));
    assert!(err.contains("ProcRef"));
    Ok(())
}

#[test]
fn infers_error_annotated_assertion_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc assert_msg
assert.err="plain"
end

pub proc assert_eqw_msg
assert_eqw.err="word"
end

pub proc u32assert_msg
u32assert.err="u32"
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "assert_msg").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 0);

    let signature = find_function(output.module, "assert_eqw_msg").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 8);
    assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(signature.results().len(), 0);

    let signature = find_function(output.module, "u32assert_msg").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::U32);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    Ok(())
}

#[test]
fn infers_sdepth_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc depth
sdepth
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "depth").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn infers_debug_decorator_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc debugged
debug.stack
trace.1
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "debugged").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 0);
    assert_eq!(signature.results().len(), 0);

    Ok(())
}

#[test]
fn infers_local_callee_before_caller() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc inc
add.1
end

pub proc entry
exec.inc
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "entry").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn infers_control_flow_join_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc choose
if.true
    add.1
else
    add.2
end
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "choose").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 2);
    assert_eq!(signature.params()[0].ty, Type::I1);
    assert_eq!(signature.params()[1].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn infers_u32_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc add
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "add").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 2);
    assert!(signature.params().iter().all(|param| param.ty == Type::U32));
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    Ok(())
}

#[test]
fn infers_cumulative_and_alternative_type_constraints() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc sequential_refine
dup.0
add.1
drop
u32assert
end

pub proc branch_alternative
if.true
    u32assert
else
    add.1
end
end

pub proc branch_common
if.true
    u32assert
else
    u32cast
end
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let signature = find_function(output.module, "sequential_refine")
        .borrow()
        .get_signature()
        .clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::U32);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    let signature = find_function(output.module, "branch_alternative")
        .borrow()
        .get_signature()
        .clone();
    assert_eq!(signature.params().len(), 2);
    assert_eq!(signature.params()[0].ty, Type::I1);
    assert_eq!(signature.params()[1].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    let signature = find_function(output.module, "branch_common").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 2);
    assert_eq!(signature.params()[0].ty, Type::I1);
    assert_eq!(signature.params()[1].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    Ok(())
}

#[test]
fn infers_u32split_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc split
u32split
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let function = find_function(output.module, "split");
    let signature = function.borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 2);
    assert_eq!(signature.results()[0].ty, Type::U32);
    assert_eq!(signature.results()[1].ty, Type::U32);

    Ok(())
}

#[test]
fn infers_u32test_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc test_one
u32test
end

pub proc test_word
u32testw
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let one_signature = find_function(output.module, "test_one").borrow().get_signature().clone();
    assert_eq!(one_signature.params().len(), 1);
    assert_eq!(one_signature.params()[0].ty, Type::Felt);
    assert_eq!(one_signature.results().len(), 2);
    assert_eq!(one_signature.results()[0].ty, Type::I1);
    assert_eq!(one_signature.results()[1].ty, Type::Felt);

    let word_signature = find_function(output.module, "test_word").borrow().get_signature().clone();
    assert_eq!(word_signature.params().len(), 4);
    assert!(word_signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(word_signature.results().len(), 5);
    assert_eq!(word_signature.results()[0].ty, Type::I1);
    assert!(word_signature.results()[1..].iter().all(|result| result.ty == Type::Felt));

    Ok(())
}

#[test]
fn infers_u32_widening_arithmetic_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc add_wide
u32widening_add
end

pub proc add3_wide
u32widening_add3
end

pub proc add3_overflow
u32overflowing_add3
end

pub proc add3_wrapping
u32wrapping_add3
end

pub proc mul_wide
u32widening_mul
end

pub proc madd_wide
u32widening_madd
end

pub proc madd_wrapping
u32wrapping_madd
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    for name in ["add_wide", "mul_wide"] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), 2);
        assert!(signature.params().iter().all(|param| param.ty == Type::U32));
        assert_eq!(signature.results().len(), 2);
        assert!(signature.results().iter().all(|result| result.ty == Type::U32));
    }

    for name in ["add3_wide", "add3_overflow", "madd_wide"] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        assert_eq!(signature.params().len(), 3);
        assert!(signature.params().iter().all(|param| param.ty == Type::U32));
        assert_eq!(signature.results().len(), 2);
        assert!(signature.results().iter().all(|result| result.ty == Type::U32));
    }

    let signature = find_function(output.module, "add3_wrapping").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 3);
    assert!(signature.params().iter().all(|param| param.ty == Type::U32));
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    let signature = find_function(output.module, "madd_wrapping").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 3);
    assert!(signature.params().iter().all(|param| param.ty == Type::U32));
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::U32);

    Ok(())
}

#[test]
fn infers_conditional_stack_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc choose
cdrop
end

pub proc swap
cswap
end

pub proc choose_word
cdropw
end

pub proc swap_word
cswapw
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let choose_signature = find_function(output.module, "choose").borrow().get_signature().clone();
    assert_eq!(choose_signature.params().len(), 3);
    assert_eq!(choose_signature.params()[0].ty, Type::I1);
    assert!(choose_signature.params()[1..].iter().all(|param| param.ty == Type::Felt));
    assert_eq!(choose_signature.results().len(), 1);
    assert_eq!(choose_signature.results()[0].ty, Type::Felt);

    let swap_signature = find_function(output.module, "swap").borrow().get_signature().clone();
    assert_eq!(swap_signature.params().len(), 3);
    assert_eq!(swap_signature.params()[0].ty, Type::I1);
    assert!(swap_signature.params()[1..].iter().all(|param| param.ty == Type::Felt));
    assert_eq!(swap_signature.results().len(), 2);
    assert!(swap_signature.results().iter().all(|result| result.ty == Type::Felt));

    let choose_word_signature =
        find_function(output.module, "choose_word").borrow().get_signature().clone();
    assert_eq!(choose_word_signature.params().len(), 9);
    assert_eq!(choose_word_signature.params()[0].ty, Type::I1);
    assert!(choose_word_signature.params()[1..].iter().all(|param| param.ty == Type::Felt));
    assert_eq!(choose_word_signature.results().len(), 4);
    assert!(choose_word_signature.results().iter().all(|result| result.ty == Type::Felt));

    let swap_word_signature =
        find_function(output.module, "swap_word").borrow().get_signature().clone();
    assert_eq!(swap_word_signature.params().len(), 9);
    assert_eq!(swap_word_signature.params()[0].ty, Type::I1);
    assert!(swap_word_signature.params()[1..].iter().all(|param| param.ty == Type::Felt));
    assert_eq!(swap_word_signature.results().len(), 8);
    assert!(swap_word_signature.results().iter().all(|result| result.ty == Type::Felt));

    Ok(())
}

#[test]
fn infers_local_word_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(4)
pub proc load_word
loc_loadw_le.0
end

@locals(4)
pub proc store_word
loc_storew_be.0
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let load_signature = find_function(output.module, "load_word").borrow().get_signature().clone();
    assert_eq!(load_signature.params().len(), 0);
    assert_eq!(load_signature.results().len(), 4);
    assert!(load_signature.results().iter().all(|result| result.ty == Type::Felt));

    let store_signature =
        find_function(output.module, "store_word").borrow().get_signature().clone();
    assert_eq!(store_signature.params().len(), 4);
    assert!(store_signature.params().iter().all(|param| param.ty == Type::Felt));
    assert_eq!(store_signature.results().len(), 4);
    assert!(store_signature.results().iter().all(|result| result.ty == Type::Felt));

    Ok(())
}

#[test]
fn infers_memory_signatures() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc load
mem_load
end

pub proc load_imm
mem_load.0
end

pub proc load_word
mem_loadw_le
end

pub proc store
mem_store
end

pub proc store_imm
mem_store.0
end

pub proc store_word
mem_storew_be
end

pub proc store_word_imm
mem_storew_le.0
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    )?;

    let load = find_function(output.module, "load").borrow().get_signature().clone();
    assert_eq!(load.params().len(), 1);
    assert_eq!(load.params()[0].ty, Type::U32);
    assert_eq!(load.results().len(), 1);
    assert_eq!(load.results()[0].ty, Type::Felt);

    let load_imm = find_function(output.module, "load_imm").borrow().get_signature().clone();
    assert_eq!(load_imm.params().len(), 0);
    assert_eq!(load_imm.results().len(), 1);
    assert_eq!(load_imm.results()[0].ty, Type::Felt);

    let load_word = find_function(output.module, "load_word").borrow().get_signature().clone();
    assert_eq!(load_word.params().len(), 5);
    assert_eq!(load_word.params()[0].ty, Type::U32);
    assert!(load_word.params()[1..].iter().all(|param| param.ty == Type::Felt));
    assert_eq!(load_word.results().len(), 4);
    assert!(load_word.results().iter().all(|result| result.ty == Type::Felt));

    let store = find_function(output.module, "store").borrow().get_signature().clone();
    assert_eq!(store.params().len(), 2);
    assert_eq!(store.params()[0].ty, Type::U32);
    assert_eq!(store.params()[1].ty, Type::Felt);
    assert_eq!(store.results().len(), 0);

    let store_imm = find_function(output.module, "store_imm").borrow().get_signature().clone();
    assert_eq!(store_imm.params().len(), 1);
    assert_eq!(store_imm.params()[0].ty, Type::Felt);
    assert_eq!(store_imm.results().len(), 0);

    for name in ["store_word", "store_word_imm"] {
        let signature = find_function(output.module, name).borrow().get_signature().clone();
        let expected_params = if name == "store_word" { 5 } else { 4 };
        assert_eq!(signature.params().len(), expected_params);
        if name == "store_word" {
            assert_eq!(signature.params()[0].ty, Type::U32);
            assert!(signature.params()[1..].iter().all(|param| param.ty == Type::Felt));
        } else {
            assert!(signature.params().iter().all(|param| param.ty == Type::Felt));
        }
        assert_eq!(signature.results().len(), 4);
        assert!(signature.results().iter().all(|result| result.ty == Type::Felt));
    }

    Ok(())
}

#[test]
fn lifts_external_path_call_with_known_signature() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures
        .insert("::dep::callee".to_owned(), masm_signature([Type::Felt], [Type::Felt]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    )?;

    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    Ok(())
}

#[test]
fn infers_signature_through_external_path_call() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures
        .insert("::dep::callee".to_owned(), masm_signature([Type::U32], [Type::Felt]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry
exec.::dep::callee
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        &external_signatures,
        context,
    )?;

    let signature = find_function(output.module, "entry").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::U32);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn missing_external_callee_diagnostic_lists_available_metadata() {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures
        .insert("::dep::callee".to_owned(), masm_signature([Type::Felt], [Type::Felt]));

    let err = match disassemble_source_with_external_signatures(
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::missing
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    ) {
        Ok(_) => panic!("missing external metadata should be rejected"),
        Err(err) => err,
    };
    let message = err.to_string();
    assert!(message.contains("unresolved external callee '::dep::missing'"));
    assert!(message.contains("available external signatures"));
    assert!(message.contains("::dep::callee"));
}

#[test]
fn missing_external_callee_inference_diagnostic_lists_available_metadata() {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures
        .insert("::dep::callee".to_owned(), masm_signature([Type::Felt], [Type::Felt]));

    let err = match disassemble_source_with_external_signatures(
        r#"
pub proc entry
exec.::dep::missing
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        &external_signatures,
        context,
    ) {
        Ok(_) => panic!("missing external metadata should be rejected during inference"),
        Err(err) => err,
    };
    let message = err.to_string();
    assert!(
        message.contains("signature inference could not resolve external callee '::dep::missing'")
    );
    assert!(message.contains("available external signatures"));
    assert!(message.contains("::dep::callee"));
}

#[test]
fn lifts_known_signature_with_local_type_alias() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
type Scalar = felt

pub proc inc(a: Scalar) -> Scalar
add.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let signature = find_function(output.module, "inc").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    Ok(())
}

#[test]
fn project_disassembly_uses_source_dependency_signatures() -> Result<()> {
    let (root, app_dir) = write_source_dependency_project("midenc_frontend_masm_source_dep");

    let context = Rc::new(Context::default());
    let output = disassemble_project_target(
        app_dir.join("miden-project.toml"),
        None,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_dependency_graph_resolves_imported_external_type_metadata() -> Result<()> {
    let (root, app_dir) =
        write_imported_type_dependency_project("midenc_frontend_masm_graph_type_metadata", true);
    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let output = disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_dependency_graph_reports_unresolved_external_type_metadata() -> Result<()> {
    let (root, app_dir) = write_imported_type_dependency_project(
        "midenc_frontend_masm_graph_missing_type_metadata",
        false,
    );
    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let err = match disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    ) {
        Ok(_) => panic!("missing external type metadata should be rejected"),
        Err(err) => err,
    };
    let message = err.to_string();
    assert!(message.contains("external type '::types::Scalar'"));
    assert!(message.contains("no external type metadata"));

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_target_resolves_imported_external_type_in_declared_signature() -> Result<()> {
    let (root, app_dir) =
        write_root_imported_type_project("midenc_frontend_masm_root_type_metadata");

    let context = Rc::new(Context::default());
    let output = disassemble_project_target(
        app_dir.join("miden-project.toml"),
        None,
        &DisassemblerConfig::default(),
        context,
    )?;
    let signature = find_function(output.module, "entry").borrow().get_signature().clone();
    assert_eq!(signature.params().len(), 1);
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results().len(), 1);
    assert_eq!(signature.results()[0].ty, Type::Felt);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_disassembly_uses_workspace_dependency_signatures() -> Result<()> {
    let (root, app_dir) = write_workspace_dependency_project("midenc_frontend_masm_workspace_dep");

    let context = Rc::new(Context::default());
    let output = disassemble_project_target(
        app_dir.join("miden-project.toml"),
        None,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_disassembly_consumes_precomputed_dependency_graph() -> Result<()> {
    let (root, app_dir) = write_source_dependency_project("midenc_frontend_masm_graph_dep");
    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let output = disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_disassembly_uses_workspace_dependency_graph_signatures() -> Result<()> {
    let (root, app_dir) =
        write_workspace_dependency_project("midenc_frontend_masm_workspace_graph_dep");

    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let output = disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_disassembly_uses_preassembled_dependency_graph_signatures() -> Result<()> {
    let (root, app_dir) =
        write_preassembled_dependency_project("midenc_frontend_masm_preassembled_graph_dep");

    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let output = disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_disassembly_uses_git_dependency_graph_signatures() -> Result<()> {
    let (root, app_dir) = write_git_dependency_project("midenc_frontend_masm_git_graph_dep");

    let context = Rc::new(Context::default());
    let registry = NoPackageStore::default();
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .with_git_cache_root(root.join("git-cache"))
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let output = disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    )?;
    let function = find_function(output.module, "entry");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::Exec>(function), 1);

    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn project_graph_registry_nodes_require_artifacts() -> Result<()> {
    let root = temp_project_dir("midenc_frontend_masm_registry_graph");
    let app_dir = root.join("app");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep = "1.0.0"
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
    )
    .unwrap();

    let context = Rc::new(Context::default());
    let mut registry = TestRegistry::default();
    registry.insert("dep", "1.0.0");
    let dependency_graph = ProjectDependencyGraphBuilder::new(&registry)
        .with_source_manager(context.session().source_manager.clone())
        .build_from_path(app_dir.join("miden-project.toml"))?;

    let err = match disassemble_project_target_with_dependency_graph(
        app_dir.join("miden-project.toml"),
        None,
        &dependency_graph,
        &DisassemblerConfig::default(),
        context,
    ) {
        Ok(_) => panic!("registry-only graph nodes should not provide external signatures"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("registry package artifacts"));
    let _ = fs::remove_dir_all(root);

    Ok(())
}

#[test]
fn lifts_felt_add_imm() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc inc(a: felt) -> felt
add.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "inc");
    let signature = function.borrow().get_signature().clone();
    assert_eq!(signature.params()[0].ty, Type::Felt);
    assert_eq!(signature.results()[0].ty, Type::Felt);
    assert_eq!(function.borrow().entry_block().borrow().body().iter().count(), 2);

    Ok(())
}

#[test]
fn lifts_if_to_scf_if() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc choose(cond: u8) -> felt
if.true
    push.1
else
    push.2
end
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "choose");
    assert_eq!(top_level_op_count::<If>(function), 1);

    Ok(())
}

#[test]
fn lifts_multi_result_if_with_distinct_result_indices() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc choose(left: felt, right: felt, cond: i1) -> (felt, felt)
if.true
    swap.1
else
    nop
end
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "choose");
    let entry = function.borrow().entry_block();
    let entry = entry.borrow();
    let if_op = entry.body().iter().find(|op| op.is::<If>()).expect("expected lifted scf.if");
    let result_indices = if_op
        .results()
        .all()
        .iter()
        .map(|result| result.borrow().index())
        .collect::<Vec<_>>();
    assert_eq!(result_indices, [0, 1]);

    Ok(())
}

#[test]
fn lifts_repeat_by_unrolling() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc inc3(a: felt) -> felt
repeat.3
    add.1
end
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "inc3");
    assert_eq!(top_level_op_count::<ArithIncr>(function), 3);

    Ok(())
}

#[test]
fn lifts_while_to_scf_while() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc loop_once(cond: u8) -> felt
while.true
    push.0
end
push.7
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "loop_once");
    assert_eq!(top_level_op_count::<While>(function), 1);

    Ok(())
}

#[test]
fn lifts_word_stack_manipulation() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc shuffle(
a: felt, b: felt, c: felt, d: felt,
e: felt, f: felt, g: felt, h: felt,
i: felt, j: felt, k: felt, l: felt,
m: felt, n: felt, o: felt, p: felt
) -> (felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt)
swapw.2
swapw.3
swapdw
movupw.2
movdnw.2
movupw.3
movdnw.3
reversedw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "shuffle");
    assert_eq!(function.borrow().get_signature().results().len(), 16);

    Ok(())
}

#[test]
fn lifts_push_word_immediate() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc word() -> (felt, felt, felt, felt)
push.[1,2,3,4]
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "word");
    assert_eq!(top_level_op_count::<ArithConstant>(function), 4);
    assert_eq!(felt_constant_values(top_level_arith_constant_values(function)), [4, 3, 2, 1]);

    Ok(())
}

#[test]
fn lifts_push_word_slice_in_vm_push_order() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc slice() -> (felt, felt)
push.[1,2,3,4][1..3]
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "slice");
    assert_eq!(felt_constant_values(top_level_arith_constant_values(function)), [3, 2]);

    Ok(())
}

#[test]
fn rejects_empty_push_word_slice() {
    let context = Rc::new(Context::default());
    let err = match disassemble_source(
        r#"
pub proc empty_slice
push.[1,2,3,4][1..1]
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    ) {
        Ok(_) => panic!("empty push word slices should be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("empty push word slice"), "{err}");
}

#[test]
fn lifts_u32cast_as_truncating_cast() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc cast(value: felt) -> u32
u32cast
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(top_level_op_count::<ArithTrunc>(find_function(output.module, "cast")), 1);

    Ok(())
}

#[test]
fn lifts_sdepth_to_current_stack_depth_constant() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc depth(a: felt, b: felt) -> (felt, felt, felt)
sdepth
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let constants = top_level_arith_constant_values(find_function(output.module, "depth"));
    assert_eq!(constants.len(), 1);
    match constants[0] {
        Immediate::Felt(value) => assert_eq!(value.as_canonical_u64(), 2),
        value => panic!("expected sdepth to materialize a felt constant, got {value:?}"),
    }

    Ok(())
}

#[test]
fn lifts_eqw_to_arith_comparisons() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc words_equal(
a: felt, b: felt, c: felt, d: felt,
e: felt, f: felt, g: felt, h: felt
) -> i1
eqw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "words_equal");
    assert_eq!(top_level_op_count::<ArithEq>(function), 4);
    assert_eq!(top_level_op_count::<ArithAnd>(function), 3);

    Ok(())
}

#[test]
fn lifts_assert_eqw_to_hir_assert_eqs() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc assert_words(
a: felt, b: felt, c: felt, d: felt,
e: felt, f: felt, g: felt, h: felt
)
assert_eqw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "assert_words");
    assert_eq!(top_level_op_count::<midenc_dialect_hir::AssertEq>(function), 4);

    Ok(())
}

#[test]
fn preserves_error_messages_on_hir_assertions() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc assert_msg(value: i1)
assert.err="plain"
end

pub proc assertz_msg(value: i1)
assertz.err="zero"
end

pub proc assert_eq_msg(a: felt, b: felt)
assert_eq.err="equal"
end

pub proc assert_eqw_msg(
a: felt, b: felt, c: felt, d: felt,
e: felt, f: felt, g: felt, h: felt
)
assert_eqw.err="word"
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(top_level_assert_messages(find_function(output.module, "assert_msg")), ["plain"]);
    assert_eq!(
        top_level_assertz_messages(find_function(output.module, "assertz_msg")),
        ["zero"]
    );
    assert_eq!(
        top_level_assert_eq_messages(find_function(output.module, "assert_eq_msg")),
        ["equal"]
    );
    assert_eq!(
        top_level_assert_eq_messages(find_function(output.module, "assert_eqw_msg")),
        ["word", "word", "word", "word"]
    );

    Ok(())
}

#[test]
fn lifts_u32assertw_as_u32_range_contract() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc assert_word(a: felt, b: felt, c: felt, d: felt) -> (u32, u32, u32, u32)
u32assertw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "assert_word");
    assert_eq!(top_level_op_count::<HirAssertU32>(function), 4);

    Ok(())
}

#[test]
fn advice_taint_reports_raw_advice_used_by_u32_arithmetic() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_diagnostics_include_actionable_context() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(diagnostics.len(), 1);

    let diagnostic = &diagnostics[0];
    assert!(
        diagnostic
            .message()
            .contains("unconstrained advice value reaches u32-presuming operation `arith.add`"),
        "{}",
        diagnostic.message()
    );
    assert!(diagnostic.message().contains("function 'entry'"), "{}", diagnostic.message());
    assert!(diagnostic.help_message().contains("u32assert"));
    assert!(diagnostic.help_message().contains("u32test"));
    assert_eq!(
        diagnostic.label_messages().collect::<Vec<_>>(),
        [
            "`arith.add` consumes unconstrained advice as a u32",
            "unconstrained advice originates here",
        ]
    );

    let reports = advice_taint_reports(output.module)?;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].severity(), Some(Severity::Warning));

    Ok(())
}

#[test]
fn advice_taint_marks_adv_loadw_results_as_raw() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(k0: felt, k1: felt, k2: felt, k3: felt) -> (felt, felt, u32)
adv_loadw
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);

    Ok(())
}

#[test]
fn advice_taint_marks_adv_pipe_results_as_raw() -> Result<()> {
    let context = Rc::new(Context::default());
    let params = (0..13).map(|i| format!("v{i}: felt")).collect::<Vec<_>>().join(", ");
    let results = (0..12)
        .map(|i| if i == 11 { "u32" } else { "felt" })
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
pub proc entry({params}) -> ({results})
adv_pipe
u32wrapping_add
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);

    Ok(())
}

#[test]
fn advice_taint_preserves_non_advice_adv_pipe_outputs() -> Result<()> {
    let context = Rc::new(Context::default());
    let params = [
        "v0: felt",
        "v1: felt",
        "v2: felt",
        "v3: felt",
        "v4: felt",
        "v5: felt",
        "v6: felt",
        "v7: felt",
        "lhs: u32",
        "rhs: u32",
        "v10: felt",
        "v11: felt",
        "addr: felt",
    ]
    .join(", ");
    let source = format!(
        r#"
pub proc entry({params}) -> u32
adv_pipe
repeat.8
    drop
end
movup.4
drop
movup.3
drop
movup.2
drop
u32wrapping_add
end
"#
    );
    let output = disassemble_source(source, "test", &DisassemblerConfig::default(), context)?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "expected preserved adv_pipe outputs to remain clean");

    Ok(())
}

#[test]
fn advice_taint_propagates_adv_pipe_memory_writes() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(rhs: u32) -> u32
push.0
repeat.12
    push.0
end
adv_pipe
repeat.13
    drop
end
mem_load.0
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_treats_u32assert_as_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
u32assert
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "expected u32assert to sanitize raw advice");

    Ok(())
}

#[test]
fn advice_taint_treats_u32assertw_as_multi_value_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> (u32, u32, u32)
adv_push.4
u32assertw
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "expected u32assertw to sanitize all word elements");

    Ok(())
}

#[test]
fn advice_taint_sanitizes_only_the_asserted_alias() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
dup.0
u32assert
swap.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);

    Ok(())
}

#[test]
fn advice_taint_suppresses_straight_line_duplicate_uses() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
push.1
u32wrapping_add
push.2
u32wrapping_mul
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);

    Ok(())
}

#[test]
fn advice_taint_reports_first_unconstrained_use_per_control_flow_path() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(lhs: u32, rhs: u32, cond: felt) -> u32
adv_push.1
swap.1
if.true
    u32assert
    u32wrapping_add
else
    u32wrapping_sub
end
u32wrapping_mul
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.sub"]);

    Ok(())
}

#[test]
fn advice_taint_reports_later_sink_for_raw_branch_path_not_reported_earlier() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(cond: felt) -> u32
adv_push.1
swap.1
if.true
    push.1
    u32wrapping_add
else
    nop
end
push.2
u32wrapping_mul
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add", "arith.mul"]);

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_returned_from_callee() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

pub proc entry(rhs: u32) -> u32
exec.source
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_through_multi_hop_exec_chain() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

proc forward() -> felt
exec.source
end

pub proc entry(rhs: u32) -> u32
exec.forward
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_returned_from_call() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

pub proc entry(rhs: u32) -> u32
call.source
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_into_callee_arguments() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc consume(value: felt, rhs: u32) -> u32
u32wrapping_add
end

pub proc entry(rhs: u32) -> u32
adv_push.1
exec.consume
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("consume"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_into_call_arguments() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc consume(value: felt, rhs: u32) -> u32
u32wrapping_add
end

pub proc entry(rhs: u32) -> u32
adv_push.1
call.consume
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("consume"));

    Ok(())
}

#[test]
fn advice_taint_handles_repeated_call_sites_with_mixed_clean_and_tainted_arguments() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc consume(value: felt, rhs: u32) -> u32
u32wrapping_add
end

pub proc entry(rhs: u32) -> u32
adv_push.1
u32assert
exec.consume
adv_push.1
exec.consume
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("consume"));

    Ok(())
}

#[test]
fn advice_taint_diagnostics_include_call_argument_context() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc consume(value: felt, rhs: u32) -> u32
u32wrapping_add
end

pub proc entry(rhs: u32) -> u32
adv_push.1
exec.consume
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].contexts.len(), 1);
    assert_eq!(findings[0].contexts[0].kind, AdviceTaintContextKind::CallArgument);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(
        diagnostics[0].label_messages().collect::<Vec<_>>(),
        [
            "`arith.add` consumes unconstrained advice as a u32",
            "unconstrained value is passed as a call argument here",
            "unconstrained advice originates here",
        ]
    );

    Ok(())
}

#[test]
fn advice_taint_diagnostics_include_call_result_context() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

pub proc entry(rhs: u32) -> u32
exec.source
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].contexts.len(), 1);
    assert_eq!(findings[0].contexts[0].kind, AdviceTaintContextKind::CallResult);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(
        diagnostics[0].label_messages().collect::<Vec<_>>(),
        [
            "`arith.add` consumes unconstrained advice as a u32",
            "unconstrained value returns from a call here",
            "unconstrained advice originates here",
        ]
    );

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_through_local_store_load() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(1)
pub proc entry(rhs: u32) -> u32
adv_push.1
loc_store.0
loc_load.0
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_through_memory_store_load() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(rhs: u32) -> u32
adv_push.1
mem_store.0
mem_load.0
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_joins_local_store_taint_across_branches() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(1)
pub proc entry(rhs: u32, cond: i1) -> u32
if.true
    adv_push.1
    loc_store.0
else
    push.0
    loc_store.0
end
loc_load.0
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_joins_memory_store_taint_across_branches() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(rhs: u32, cond: i1) -> u32
if.true
    adv_push.1
    mem_store.0
else
    push.0
    mem_store.0
end
mem_load.0
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_propagates_raw_advice_through_dynamic_memory_store_load() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry(rhs: u32, addr: u32) -> u32
dup.0
adv_push.1
swap.1
mem_store
mem_load
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));

    Ok(())
}

#[test]
fn advice_taint_treats_external_call_results_as_unconstrained() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::source".to_owned(), masm_signature([], [Type::Felt]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry(rhs: u32) -> u32
exec.::dep::source
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.add"]);
    assert_eq!(findings[0].function.map(|name| name.as_str()), Some("entry"));
    assert_eq!(findings[0].origin.kind, AdviceTaintOriginKind::ExternalCall);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert!(
        diagnostic
            .message()
            .contains("unconstrained external call result reaches u32-presuming operation"),
        "{}",
        diagnostic.message()
    );
    assert_eq!(
        diagnostic.label_messages().collect::<Vec<_>>(),
        [
            "`arith.add` consumes an unconstrained external call result as a u32",
            "external call result is modeled as unconstrained here",
        ]
    );

    Ok(())
}

#[test]
fn advice_taint_treats_external_u32_results_as_constrained() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::source".to_owned(), masm_signature([], [Type::U32]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry(rhs: u32) -> u32
exec.::dep::source
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "{findings:#?}");

    Ok(())
}

#[test]
fn advice_taint_reports_raw_advice_passed_to_external_u32_parameter() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::consume".to_owned(), masm_signature([Type::U32], []));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry
adv_push.1
exec.::dep::consume
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        &external_signatures,
        context,
    )?;

    let findings = advice_taint_external_call_findings(output.module)?;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].argument_index, 0);
    assert_eq!(findings[0].parameter_type, Type::U32);
    assert_eq!(findings[0].origin.kind, AdviceTaintOriginKind::Advice);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics[0]
            .message()
            .contains("unconstrained advice value is passed to external parameter #0"),
        "{}",
        diagnostics[0].message()
    );

    Ok(())
}

#[test]
fn advice_taint_allows_raw_advice_passed_to_external_felt_parameter() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::consume".to_owned(), masm_signature([Type::Felt], []));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry
adv_push.1
exec.::dep::consume
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        &external_signatures,
        context,
    )?;

    let findings = advice_taint_external_call_findings(output.module)?;
    assert!(findings.is_empty(), "{findings:#?}");

    Ok(())
}

#[test]
fn advice_taint_treats_u32assert_as_external_result_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::source".to_owned(), masm_signature([], [Type::Felt]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry(rhs: u32) -> u32
exec.::dep::source
u32assert
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "{findings:#?}");

    Ok(())
}

#[test]
fn advice_taint_reports_public_function_returning_raw_advice() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

pub proc entry() -> felt
exec.source
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "{findings:#?}");

    let exits = advice_taint_exit_findings(output.module)?;
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].function.as_str(), "entry");
    assert_eq!(exits[0].result_index, 0);
    assert_eq!(exits[0].origin.kind, AdviceTaintOriginKind::Advice);
    assert_eq!(exits[0].contexts.len(), 1);
    assert_eq!(exits[0].contexts[0].kind, AdviceTaintContextKind::CallResult);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert!(
        diagnostic
            .message()
            .contains("public function 'entry' returns unconstrained advice value as result #0"),
        "{}",
        diagnostic.message()
    );
    assert_eq!(
        diagnostic.label_messages().collect::<Vec<_>>(),
        [
            "public function returns unconstrained advice as result #0",
            "unconstrained value returns from a call here",
            "unconstrained advice originates here",
        ]
    );

    Ok(())
}

#[test]
fn advice_taint_does_not_report_private_function_returning_raw_advice() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
proc source() -> felt
adv_push.1
end

pub proc entry() -> felt
push.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let exits = advice_taint_exit_findings(output.module)?;
    assert!(exits.is_empty(), "{exits:#?}");

    Ok(())
}

#[test]
fn advice_taint_does_not_report_sanitized_public_return() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
u32assert
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let exits = advice_taint_exit_findings(output.module)?;
    assert!(exits.is_empty(), "{exits:#?}");

    Ok(())
}

#[test]
fn advice_taint_reports_public_function_returning_external_result() -> Result<()> {
    let context = Rc::new(Context::default());
    let mut external_signatures = ExternalSignatureMap::new();
    external_signatures.insert("::dep::source".to_owned(), masm_signature([], [Type::Felt]));

    let output = disassemble_source_with_external_signatures(
        r#"
pub proc entry() -> felt
exec.::dep::source
end
"#,
        "test",
        &DisassemblerConfig::default(),
        &external_signatures,
        context,
    )?;

    let exits = advice_taint_exit_findings(output.module)?;
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].function.as_str(), "entry");
    assert_eq!(exits[0].origin.kind, AdviceTaintOriginKind::ExternalCall);

    let diagnostics = advice_taint_diagnostics(output.module)?;
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics[0]
            .message()
            .contains("public function 'entry' returns unconstrained external call result"),
        "{}",
        diagnostics[0].message()
    );

    Ok(())
}

#[test]
fn advice_taint_reports_raw_advice_used_by_unary_u32_operation() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc pop_count() -> u32
adv_push.1
u32popcnt
end

pub proc bitwise_not() -> u32
adv_push.1
u32not
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.popcnt", "arith.bnot"]);

    Ok(())
}

#[test]
fn advice_taint_reports_raw_advice_used_by_widening_u32_operation() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> (u32, u32)
adv_push.2
u32widening_mul
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.zext"]);

    Ok(())
}

#[test]
fn advice_taint_treats_u32assert2_as_widening_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> (u32, u32)
adv_push.2
u32assert2
u32widening_mul
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(findings.is_empty(), "expected u32assert2 to sanitize widening operands");

    Ok(())
}

#[test]
fn advice_taint_reports_raw_advice_used_by_u32_add3_and_madd() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc add3() -> u32
adv_push.3
u32wrapping_add3
end

pub proc madd() -> u32
adv_push.3
u32wrapping_madd
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert_eq!(sink_names(&findings), ["arith.zext", "arith.zext"]);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.function.map(|name| name.as_str()))
            .collect::<Vec<_>>(),
        [Some("add3"), Some("madd")]
    );

    Ok(())
}

#[test]
fn advice_taint_treats_u32test_assert_as_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
u32test
assert
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(
        findings.is_empty(),
        "expected u32test followed by assert to sanitize raw advice"
    );

    Ok(())
}

#[test]
fn advice_taint_treats_u32testw_assert_as_word_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> (u32, u32, u32)
adv_push.4
u32testw
assert
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(
        findings.is_empty(),
        "expected u32testw followed by assert to sanitize raw advice"
    );

    Ok(())
}

#[test]
fn advice_taint_treats_error_annotated_u32test_assert_as_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
u32test
assert.err="u32"
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(
        findings.is_empty(),
        "expected error-annotated u32test/assert to sanitize raw advice"
    );
    assert_eq!(top_level_assert_u32_messages(find_function(output.module, "entry")), ["u32"]);

    Ok(())
}

#[test]
fn advice_taint_treats_stack_preserving_u32test_assert_as_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> u32
adv_push.1
u32test
dup.0
assert.err="u32"
drop
push.1
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(
        findings.is_empty(),
        "expected stack-preserving u32test/assert sequence to sanitize raw advice"
    );
    assert_eq!(top_level_assert_u32_messages(find_function(output.module, "entry")), ["u32"]);

    Ok(())
}

#[test]
fn advice_taint_treats_stack_preserving_u32testw_assert_as_sanitizer() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc entry() -> (u32, u32, u32)
adv_push.4
u32testw
dup.0
assert
drop
u32wrapping_add
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let findings = advice_taint_findings(output.module)?;
    assert!(
        findings.is_empty(),
        "expected stack-preserving u32testw/assert sequence to sanitize raw advice"
    );

    Ok(())
}

#[test]
fn lifts_u32split_to_arith_split() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc split(value: felt) -> (u32, u32)
u32split
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "split");
    assert_eq!(top_level_op_count::<UnrealizedConversionCast>(function), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(function), 1);

    Ok(())
}

#[test]
fn lifts_u32test_to_range_check() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc test(value: felt) -> (i1, felt)
u32test
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "test");
    assert_eq!(top_level_op_count::<UnrealizedConversionCast>(function), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(function), 1);
    assert_eq!(top_level_op_count::<ArithEq>(function), 1);

    Ok(())
}

#[test]
fn lifts_u32testw_to_range_checks() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc testw(a: felt, b: felt, c: felt, d: felt) -> (i1, felt, felt, felt, felt)
u32testw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let function = find_function(output.module, "testw");
    assert_eq!(top_level_op_count::<UnrealizedConversionCast>(function), 4);
    assert_eq!(top_level_op_count::<ArithSplit>(function), 4);
    assert_eq!(top_level_op_count::<ArithEq>(function), 4);
    assert_eq!(top_level_op_count::<ArithAnd>(function), 3);

    Ok(())
}

#[test]
fn lifts_u32_widening_arithmetic() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc add_wide(a: u32, b: u32) -> (u32, u32)
u32widening_add
end

pub proc add3_overflow(a: u32, b: u32, c: u32) -> (u32, u32)
u32overflowing_add3
end

pub proc mul_wide(a: u32, b: u32) -> (u32, u32)
u32widening_mul
end

pub proc madd_wide(b: u32, a: u32, c: u32) -> (u32, u32)
u32widening_madd
end

pub proc madd_wrapping(b: u32, a: u32, c: u32) -> u32
u32wrapping_madd
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let add = find_function(output.module, "add_wide");
    assert_eq!(top_level_op_count::<ArithZext>(add), 2);
    assert_eq!(top_level_op_count::<ArithAdd>(add), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(add), 1);

    let add3 = find_function(output.module, "add3_overflow");
    assert_eq!(top_level_op_count::<ArithZext>(add3), 3);
    assert_eq!(top_level_op_count::<ArithAdd>(add3), 2);
    assert_eq!(top_level_op_count::<ArithSplit>(add3), 1);

    let mul = find_function(output.module, "mul_wide");
    assert_eq!(top_level_op_count::<ArithZext>(mul), 2);
    assert_eq!(top_level_op_count::<ArithMul>(mul), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(mul), 1);

    let madd = find_function(output.module, "madd_wide");
    assert_eq!(top_level_op_count::<ArithZext>(madd), 3);
    assert_eq!(top_level_op_count::<ArithMul>(madd), 1);
    assert_eq!(top_level_op_count::<ArithAdd>(madd), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(madd), 1);

    let wrapping_madd = find_function(output.module, "madd_wrapping");
    assert_eq!(top_level_op_count::<ArithZext>(wrapping_madd), 3);
    assert_eq!(top_level_op_count::<ArithMul>(wrapping_madd), 1);
    assert_eq!(top_level_op_count::<ArithAdd>(wrapping_madd), 1);
    assert_eq!(top_level_op_count::<ArithSplit>(wrapping_madd), 1);

    Ok(())
}

#[test]
fn lifts_conditional_stack_ops_to_cf_selects() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc choose(cond: i1, b: felt, a: felt) -> felt
cdrop
end

pub proc swap(cond: i1, b: felt, a: felt) -> (felt, felt)
cswap
end

pub proc choose_word(
cond: i1,
b0: felt, b1: felt, b2: felt, b3: felt,
a0: felt, a1: felt, a2: felt, a3: felt
) -> (felt, felt, felt, felt)
cdropw
end

pub proc swap_word(
cond: i1,
b0: felt, b1: felt, b2: felt, b3: felt,
a0: felt, a1: felt, a2: felt, a3: felt
) -> (felt, felt, felt, felt, felt, felt, felt, felt)
cswapw
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(top_level_op_count::<CfSelect>(find_function(output.module, "choose")), 1);
    assert_eq!(top_level_op_count::<CfSelect>(find_function(output.module, "swap")), 2);
    assert_eq!(top_level_op_count::<CfSelect>(find_function(output.module, "choose_word")), 4);
    assert_eq!(top_level_op_count::<CfSelect>(find_function(output.module, "swap_word")), 8);

    Ok(())
}

#[test]
fn lifts_local_word_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
@locals(4)
pub proc load_word() -> (felt, felt, felt, felt)
loc_loadw_be.0
end

@locals(4)
pub proc store_word(a: felt, b: felt, c: felt, d: felt) -> (felt, felt, felt, felt)
loc_storew_le.0
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    assert_eq!(top_level_op_count::<HirLoadLocal>(find_function(output.module, "load_word")), 4);
    assert_eq!(
        top_level_op_count::<HirStoreLocal>(find_function(output.module, "store_word")),
        4
    );

    Ok(())
}

#[test]
fn lifts_memory_ops() -> Result<()> {
    let context = Rc::new(Context::default());
    let output = disassemble_source(
        r#"
pub proc load(addr: u32) -> felt
mem_load
end

pub proc load_imm() -> felt
mem_load.0
end

pub proc load_word(addr: u32, a: felt, b: felt, c: felt, d: felt) -> (felt, felt, felt, felt)
mem_loadw_be
end

pub proc store(addr: u32, value: felt)
mem_store
end

pub proc store_word(addr: u32, a: felt, b: felt, c: felt, d: felt) -> (felt, felt, felt, felt)
mem_storew_le
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    )?;

    let load = find_function(output.module, "load");
    assert_eq!(top_level_op_count::<HirIntToPtr>(load), 1);
    assert_eq!(top_level_op_count::<HirLoad>(load), 1);

    let load_imm = find_function(output.module, "load_imm");
    assert_eq!(top_level_op_count::<HirIntToPtr>(load_imm), 1);
    assert_eq!(top_level_op_count::<HirLoad>(load_imm), 1);

    let load_word = find_function(output.module, "load_word");
    assert_eq!(top_level_op_count::<HirIntToPtr>(load_word), 4);
    assert_eq!(top_level_op_count::<HirLoad>(load_word), 4);

    let store = find_function(output.module, "store");
    assert_eq!(top_level_op_count::<HirIntToPtr>(store), 1);
    assert_eq!(top_level_op_count::<HirStore>(store), 1);

    let store_word = find_function(output.module, "store_word");
    assert_eq!(top_level_op_count::<HirIntToPtr>(store_word), 4);
    assert_eq!(top_level_op_count::<HirStore>(store_word), 4);

    Ok(())
}

#[test]
fn rejects_invalid_local_word_indices() {
    let context = Rc::new(Context::default());
    let unaligned = disassemble_source(
        r#"
@locals(8)
pub proc bad() -> (felt, felt, felt, felt)
loc_loadw_le.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context.clone(),
    );
    let err = match unaligned {
        Ok(_) => panic!("expected disassembly to reject an unaligned local word index"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("not word-aligned"));

    let out_of_range = disassemble_source(
        r#"
@locals(4)
pub proc bad() -> (felt, felt, felt, felt)
loc_loadw_le.4
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    );
    let err = match out_of_range {
        Ok(_) => panic!("expected disassembly to reject an out-of-range local word index"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("invalid local index 4"));
}

#[test]
fn rejects_invalid_memory_word_addresses() {
    let context = Rc::new(Context::default());
    let known_signature = disassemble_source(
        r#"
pub proc bad(a: felt, b: felt, c: felt, d: felt) -> (felt, felt, felt, felt)
mem_loadw_le.1
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context.clone(),
    );
    let err = match known_signature {
        Ok(_) => panic!("expected disassembly to reject an unaligned memory word address"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("memory word address 1 is not word-aligned"));

    let inferred_signature = disassemble_source(
        r#"
pub proc bad
mem_storew_be.1
end
"#,
        "test",
        &DisassemblerConfig {
            infer_missing_signatures: true,
        },
        context,
    );
    let err = match inferred_signature {
        Ok(_) => panic!("expected inference to reject an unaligned memory word address"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("memory word address 1 is not word-aligned"));
}

#[test]
fn rejects_if_branch_stack_shape_mismatch() {
    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
pub proc bad(cond: u8) -> felt
if.true
    push.1
else
    push.1
    push.2
end
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    );
    let err = match result {
        Ok(_) => panic!("expected disassembly to reject mismatched branch stack depths"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("if branches leave different stack depths"));
}

#[test]
fn rejects_indirect_recursion() {
    let context = Rc::new(Context::default());
    let result = disassemble_source(
        r#"
pub proc a() -> felt
exec.b
end

pub proc b() -> felt
exec.a
end
"#,
        "test",
        &DisassemblerConfig::default(),
        context,
    );
    let err = match result {
        Ok(_) => panic!("expected disassembly to reject indirect recursion"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("recursive MASM procedure calls"));
}

struct InstructionCase {
    name: String,
    locals: usize,
    params: Vec<&'static str>,
    results: Vec<&'static str>,
    body: String,
}

fn felt_instruction_case(
    name: impl Into<String>,
    params: usize,
    results: usize,
    body: impl Into<String>,
) -> InstructionCase {
    instruction_case(name, &felt_types(params), &felt_types(results), body)
}

fn instruction_case(
    name: impl Into<String>,
    params: &[&'static str],
    results: &[&'static str],
    body: impl Into<String>,
) -> InstructionCase {
    instruction_case_with_locals(name, 0, params, results, body)
}

fn instruction_case_with_locals(
    name: impl Into<String>,
    locals: usize,
    params: &[&'static str],
    results: &[&'static str],
    body: impl Into<String>,
) -> InstructionCase {
    InstructionCase {
        name: name.into(),
        locals,
        params: params.to_vec(),
        results: results.to_vec(),
        body: body.into(),
    }
}

fn unsupported_instruction_case(
    name: impl Into<String>,
    locals: usize,
    body: impl Into<String>,
) -> InstructionCase {
    instruction_case_with_locals(name, locals, &[], &[], body)
}

fn parse_test_module(
    source: &str,
    context: &Rc<Context>,
) -> Result<Box<miden_assembly_syntax::ast::Module>> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from("test".to_owned().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.to_owned());
    Ok(source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, "test"))?)
}

fn felt_types(count: usize) -> Vec<&'static str> {
    vec!["felt"; count]
}

fn felt_word_select_params() -> Vec<&'static str> {
    let mut params = Vec::with_capacity(9);
    params.push("i1");
    params.extend(felt_types(8));
    params
}

fn u32_types(count: usize) -> Vec<&'static str> {
    vec!["u32"; count]
}

fn felt_memory_pointer_type() -> Type {
    Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element))
}

fn assert_instruction_case_lifts(case: &InstructionCase) {
    let source = instruction_case_source(case);
    let context = Rc::new(Context::default());
    if let Err(err) =
        disassemble_source(source.clone(), "test", &DisassemblerConfig::default(), context)
    {
        panic!(
            "expected instruction matrix case '{}' to lift\n{}\nerror: {}",
            case.name, source, err
        );
    }
}

fn assert_instruction_case_is_unsupported(case: &InstructionCase) {
    let source = instruction_case_source(case);
    let context = Rc::new(Context::default());
    let err =
        match disassemble_source(source.clone(), "test", &DisassemblerConfig::default(), context) {
            Ok(_) => panic!(
                "expected instruction matrix case '{}' to be unsupported\n{}",
                case.name, source
            ),
            Err(err) => err,
        };

    let err = err.to_string();
    assert!(
        err.contains("not supported during disassembly"),
        "expected unsupported-instruction diagnostic for '{}'\n{}\nerror: {}",
        case.name,
        source,
        err
    );
}

fn instruction_case_source(case: &InstructionCase) -> String {
    let attrs = if case.locals == 0 {
        String::new()
    } else {
        format!("@locals({})\n", case.locals)
    };
    let params = masm_params(&case.params);
    let results = masm_results(&case.results);
    let body = indent_masm_body(&case.body);
    format!(
        r#"
{attrs}pub proc matrix_{name}{params}{results}
{body}
end
"#,
        name = case.name
    )
}

fn masm_params(params: &[&str]) -> String {
    let params = params
        .iter()
        .enumerate()
        .map(|(index, ty)| format!("p{index}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("({params})")
}

fn masm_results(results: &[&str]) -> String {
    match results {
        [] => String::new(),
        [ty] => format!(" -> {ty}"),
        many => format!(" -> ({})", many.join(", ")),
    }
}

fn indent_masm_body(body: &str) -> String {
    body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n")
}

fn advice_taint_findings(module: builtin::ModuleRef) -> Result<Vec<AdviceTaintFinding>> {
    let analysis_manager = AnalysisManager::new(module.as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.findings().to_vec())
}

fn advice_taint_exit_findings(module: builtin::ModuleRef) -> Result<Vec<AdviceTaintExitFinding>> {
    let analysis_manager = AnalysisManager::new(module.as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.exit_findings().to_vec())
}

fn advice_taint_external_call_findings(
    module: builtin::ModuleRef,
) -> Result<Vec<AdviceTaintExternalCallFinding>> {
    let analysis_manager = AnalysisManager::new(module.as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.external_call_findings().to_vec())
}

fn advice_taint_diagnostics(module: builtin::ModuleRef) -> Result<Vec<AdviceTaintDiagnostic>> {
    let analysis_manager = AnalysisManager::new(module.as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.diagnostics())
}

fn advice_taint_reports(module: builtin::ModuleRef) -> Result<Vec<Report>> {
    let analysis_manager = AnalysisManager::new(module.as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.reports())
}

fn sink_names(findings: &[AdviceTaintFinding]) -> Vec<String> {
    findings.iter().map(|finding| finding.sink.to_string()).collect()
}

fn find_function(module: builtin::ModuleRef, name: &str) -> builtin::FunctionRef {
    if let Some(symbol) = module.borrow().get(SymbolName::intern(name)) {
        let op = symbol.borrow();
        return op
            .as_symbol_operation()
            .downcast_ref::<Function>()
            .unwrap_or_else(|| panic!("expected symbol '{name}' to be a function"))
            .as_function_ref();
    }

    for op in module.borrow().body().entry().body().iter() {
        if let Some(function) = op.downcast_ref::<Function>()
            && function.get_name().as_str() == name
        {
            return function.as_function_ref();
        }
    }

    panic!("expected function '{name}'");
}

fn top_level_op_count<T: midenc_hir::Op + 'static>(function: builtin::FunctionRef) -> usize {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter(|op| op.is::<T>())
        .count()
}

fn top_level_arith_constant_values(function: builtin::FunctionRef) -> Vec<Immediate> {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter_map(|op| op.downcast_ref::<ArithConstant>().map(|op| *op.get_value()))
        .collect()
}

fn felt_constant_values(values: Vec<Immediate>) -> Vec<u64> {
    values
        .into_iter()
        .map(|value| match value {
            Immediate::Felt(value) => value.as_canonical_u64(),
            value => panic!("expected felt constant, got {value:?}"),
        })
        .collect()
}

fn top_level_assert_messages(function: builtin::FunctionRef) -> Vec<String> {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<HirAssert>().map(|op| op.get_message().as_str().to_owned())
        })
        .collect()
}

fn top_level_assertz_messages(function: builtin::FunctionRef) -> Vec<String> {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<HirAssertz>().map(|op| op.get_message().as_str().to_owned())
        })
        .collect()
}

fn top_level_assert_eq_messages(function: builtin::FunctionRef) -> Vec<String> {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<HirAssertEq>().map(|op| op.get_message().as_str().to_owned())
        })
        .collect()
}

fn top_level_assert_u32_messages(function: builtin::FunctionRef) -> Vec<String> {
    function
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<HirAssertU32>().map(|op| op.get_message().as_str().to_owned())
        })
        .collect()
}

fn masm_signature(
    params: impl IntoIterator<Item = Type>,
    results: impl IntoIterator<Item = Type>,
) -> FunctionType {
    FunctionType::new(CallConv::Fast, params, results)
}

fn temp_project_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), nanos))
}

fn write_source_dependency_project(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let dep_dir = root.join("dep");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&dep_dir).unwrap();

    fs::write(
        dep_dir.join("miden-project.toml"),
        r#"[package]
name = "dep"
version = "0.0.1"

[lib]
path = "lib.masm"
"#,
    )
    .unwrap();
    fs::write(
        dep_dir.join("lib.masm"),
        r#"
type Scalar = felt

pub proc callee(a: Scalar) -> Scalar
add.1
end
"#,
    )
    .unwrap();

    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep = { path = "../dep" }
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn write_imported_type_dependency_project(
    prefix: &str,
    include_type_dependency: bool,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let dep_dir = root.join("dep");
    let types_dir = root.join("types");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&dep_dir).unwrap();
    fs::create_dir_all(&types_dir).unwrap();

    fs::write(
        types_dir.join("miden-project.toml"),
        r#"[package]
name = "types"
version = "0.0.1"

[lib]
path = "lib.masm"
"#,
    )
    .unwrap();
    fs::write(
        types_dir.join("lib.masm"),
        r#"
pub type Scalar = felt
"#,
    )
    .unwrap();

    let dep_manifest = if include_type_dependency {
        r#"[package]
name = "dep"
version = "0.0.1"

[lib]
path = "lib.masm"

[dependencies]
types = { path = "../types" }
"#
    } else {
        r#"[package]
name = "dep"
version = "0.0.1"

[lib]
path = "lib.masm"
"#
    };
    fs::write(dep_dir.join("miden-project.toml"), dep_manifest).unwrap();
    fs::write(
        dep_dir.join("lib.masm"),
        r#"
use ::types::Scalar

pub type Wrapped = Scalar

pub proc callee(a: Wrapped) -> Wrapped
add.1
end
"#,
    )
    .unwrap();

    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep = { path = "../dep" }
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn write_root_imported_type_project(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let types_dir = root.join("types");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&types_dir).unwrap();

    fs::write(
        types_dir.join("miden-project.toml"),
        r#"[package]
name = "types"
version = "0.0.1"

[lib]
path = "lib.masm"
"#,
    )
    .unwrap();
    fs::write(
        types_dir.join("lib.masm"),
        r#"
pub type Scalar = felt
"#,
    )
    .unwrap();

    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
types = { path = "../types" }
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
use ::types::Scalar

pub proc entry(a: Scalar) -> Scalar
add.1
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn write_workspace_dependency_project(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let dep_dir = root.join("dep");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&dep_dir).unwrap();

    fs::write(
        root.join("miden-project.toml"),
        r#"[workspace]
members = ["dep", "app"]

[workspace.dependencies]
dep = { path = "dep" }
"#,
    )
    .unwrap();
    fs::write(
        dep_dir.join("miden-project.toml"),
        r#"[package]
name = "dep"
version = "0.0.1"

[lib]
path = "lib.masm"
"#,
    )
    .unwrap();
    fs::write(
        dep_dir.join("lib.masm"),
        r#"
type Scalar = felt

pub proc callee(a: Scalar) -> Scalar
add.1
end
"#,
    )
    .unwrap();

    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep.workspace = true
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn write_git_dependency_project(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let dep_repo_dir = root.join("dep-repo");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&dep_repo_dir).unwrap();

    fs::write(
        dep_repo_dir.join("miden-project.toml"),
        r#"[package]
name = "dep"
version = "0.0.1"

[lib]
path = "lib.masm"
"#,
    )
    .unwrap();
    fs::write(
        dep_repo_dir.join("lib.masm"),
        r#"
type Scalar = felt

pub proc callee(a: Scalar) -> Scalar
add.1
end
"#,
    )
    .unwrap();
    run_git(&dep_repo_dir, &["init", "-b", "main"]);
    run_git(&dep_repo_dir, &["config", "user.email", "test@example.com"]);
    run_git(&dep_repo_dir, &["config", "user.name", "Test"]);
    run_git(&dep_repo_dir, &["config", "commit.gpgsign", "false"]);
    run_git(&dep_repo_dir, &["add", "."]);
    run_git(&dep_repo_dir, &["commit", "-m", "init"]);

    let dep_git_uri = format!("file://{}", dep_repo_dir.display());
    fs::write(
        app_dir.join("miden-project.toml"),
        format!(
            r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep = {{ git = "{dep_git_uri}", branch = "main" }}
"#
        ),
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::callee
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn write_preassembled_dependency_project(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_project_dir(prefix);
    let app_dir = root.join("app");
    let dep_src_dir = root.join("dep-src");
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&dep_src_dir).unwrap();

    fs::write(
        dep_src_dir.join("api.masm"),
        r#"
pub proc callee(a: felt) -> felt
add.1
end
"#,
    )
    .unwrap();
    let library = Assembler::default().assemble_library_from_dir(&dep_src_dir, "dep").unwrap();
    let package = miden_mast_package::Package::from_library(
        miden_mast_package::PackageId::from("dep"),
        "1.0.0".parse::<miden_mast_package::Version>().unwrap(),
        miden_mast_package::TargetType::Library,
        library,
        std::iter::empty(),
    );
    fs::write(root.join("dep.masp"), package.to_bytes()).unwrap();

    fs::write(
        app_dir.join("miden-project.toml"),
        r#"[package]
name = "app"
version = "0.0.1"

[lib]
path = "main.masm"

[dependencies]
dep = { path = "../dep.masp" }
"#,
    )
    .unwrap();
    fs::write(
        app_dir.join("main.masm"),
        r#"
pub proc entry(a: felt) -> felt
exec.::dep::api::callee
end
"#,
    )
    .unwrap();

    (root, app_dir)
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap_or_else(|err| panic!("failed to run git {}: {err}", args.join(" ")));
    assert!(status.success(), "git {} failed with {status}", args.join(" "));
}

#[derive(Default)]
struct TestRegistry {
    packages: BTreeMap<PackageId, PackageVersions>,
}

impl TestRegistry {
    fn insert(&mut self, name: &str, version: &str) {
        let version = version.parse::<Version>().unwrap();
        let record = PackageRecord::new(version, std::iter::empty());
        self.packages
            .entry(PackageId::from(name))
            .or_default()
            .insert(record.semantic_version().clone(), record);
    }
}

impl PackageRegistry for TestRegistry {
    fn available_versions(&self, package: &PackageId) -> Option<&PackageVersions> {
        self.packages.get(package)
    }
}
