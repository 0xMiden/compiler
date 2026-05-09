//! MASM-to-HIR disassembler.

mod error;
mod infer;
mod lift;
mod project;
mod signatures;

use std::{collections::BTreeMap, path::Path, rc::Rc};

use miden_assembly_syntax::{
    Parse, ParseOptions,
    ast::{Module, ModuleKind},
    debuginfo::{SourceLanguage, SourceManager, SourceManagerExt, Uri},
};
use midenc_hir::{Context, FunctionType, dialects::builtin};

pub use self::error::Result;

/// External procedure signatures keyed by absolute MASM procedure path.
///
/// These entries are used for calls to procedures outside the module being disassembled. Project
/// disassembly can populate this from package metadata; tests or embedding tools can provide it
/// directly when they already know the callee contracts.
pub type ExternalSignatureMap = BTreeMap<String, FunctionType>;

/// Configuration for MASM disassembly.
#[derive(Debug, Clone, Copy)]
pub struct DisassemblerConfig {
    /// Infer signatures for procedures whose MASM AST/package metadata does not provide one.
    ///
    /// When enabled, missing signatures are inferred from stack underflow and final stack shape.
    pub infer_missing_signatures: bool,
}

impl Default for DisassemblerConfig {
    fn default() -> Self {
        Self {
            infer_missing_signatures: false,
        }
    }
}

/// Result of disassembling a MASM module.
pub struct DisassembledModule {
    /// The HIR context which owns the lifted module and all nested IR entities.
    pub context: Rc<Context>,
    /// The lifted HIR module.
    pub module: builtin::ModuleRef,
}

/// Disassemble a MASM file into an HIR module.
pub fn disassemble_file(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    disassemble_file_with_external_signatures(path, config, &ExternalSignatureMap::new(), context)
}

/// Disassemble a MASM file into an HIR module, using externally-provided procedure signatures for
/// path-based invoke targets.
pub fn disassemble_file_with_external_signatures(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let path = path.as_ref();
    let module_path = masm_module_path_from_file(path)?;
    disassemble_file_with_module_path_and_external_signatures(
        path,
        module_path,
        config,
        external_signatures,
        context,
    )
}

fn disassemble_file_with_module_path_and_external_signatures(
    path: impl AsRef<Path>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let path = path.as_ref();
    let source_manager = context.session().source_manager.clone();
    let source_file = source_manager.load_file(path).map_err(|err| {
        error::error(format!("failed to load MASM source '{}': {err}", path.display()))
    })?;
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    lift::lift_module(&module, config, external_signatures, context)
}

/// Disassemble a MASM source string into an HIR module.
pub fn disassemble_source(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    disassemble_source_with_external_signatures(
        source,
        module_path,
        config,
        &ExternalSignatureMap::new(),
        context,
    )
}

/// Disassemble a MASM source string into an HIR module, using externally-provided procedure
/// signatures for path-based invoke targets.
pub fn disassemble_source_with_external_signatures(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from(module_path.as_ref().as_str().to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.into());
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    lift::lift_module(&module, config, external_signatures, context)
}

/// Disassemble a target from a `miden-project.toml` package manifest.
pub fn disassemble_project_target(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let target = project::resolve_project_target(manifest_path.as_ref(), target, &context)?;
    disassemble_file_with_module_path_and_external_signatures(
        target.source_path,
        target.module_path,
        config,
        &target.external_signatures,
        context,
    )
}

/// Disassemble a target from a `miden-project.toml` package manifest, using a precomputed
/// dependency graph to discover external procedure signatures.
pub fn disassemble_project_target_with_dependency_graph(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let target = project::resolve_project_target_with_dependency_graph(
        manifest_path.as_ref(),
        target,
        dependency_graph,
        &context,
    )?;
    disassemble_file_with_module_path_and_external_signatures(
        target.source_path,
        target.module_path,
        config,
        &target.external_signatures,
        context,
    )
}

/// Disassemble a parsed MASM AST module into HIR.
pub fn disassemble_module(
    module: &Module,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    lift::lift_module(module, config, &ExternalSignatureMap::new(), context)
}

fn masm_module_path_from_file(path: &Path) -> Result<miden_assembly_syntax::PathBuf> {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
        error::error(format!("failed to derive MASM module name from '{}'", path.display()))
    })?;
    stem.parse::<miden_assembly_syntax::PathBuf>()
        .map_err(|err| error::error(format!("invalid MASM module path '{}': {err}", stem)))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::Path,
        process::Command,
        rc::Rc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use miden_assembly::Assembler;
    use miden_core::serde::Serializable;
    use miden_package_registry::{
        NoPackageStore, PackageId, PackageRecord, PackageRegistry, PackageVersions, Version,
    };
    use miden_project::ProjectDependencyGraphBuilder;
    use midenc_dialect_arith::{
        Add as ArithAdd, And as ArithAnd, Constant as ArithConstant, Eq as ArithEq,
        Incr as ArithIncr, Mul as ArithMul, Split as ArithSplit, Zext as ArithZext,
    };
    use midenc_dialect_cf::Select as CfSelect;
    use midenc_dialect_hir::{
        IntToPtr as HirIntToPtr, Load as HirLoad, LoadLocal as HirLoadLocal, Store as HirStore,
        StoreLocal as HirStoreLocal,
    };
    use midenc_dialect_scf::{If, While};
    use midenc_hir::{
        CallConv, FunctionType, Immediate, SymbolName, SymbolTable, Type,
        dialects::builtin::{self, Function, UnrealizedConversionCast},
    };

    use super::*;

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
    fn rejects_unsupported_instruction_during_known_signature_lifting() {
        let context = Rc::new(Context::default());
        let result = disassemble_source(
            r#"
pub proc unsupported(value: u32) -> u32
    ext2add
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
        assert!(err.contains("Ext2Add"));
    }

    #[test]
    fn rejects_unsupported_instruction_during_signature_inference() {
        let context = Rc::new(Context::default());
        let result = disassemble_source(
            r#"
pub proc unsupported
    ext2add
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
        assert!(err.contains("Ext2Add"));
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
            instruction_case_with_locals("loc_load", 1, &[], &["felt"], "loc_load.0"),
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
            instruction_case(
                "mem_storew_be_imm",
                &felt_types(4),
                &felt_types(4),
                "mem_storew_be.0",
            ),
            instruction_case(
                "mem_storew_le",
                &["u32", "felt", "felt", "felt", "felt"],
                &felt_types(4),
                "mem_storew_le",
            ),
            instruction_case(
                "mem_storew_le_imm",
                &felt_types(4),
                &felt_types(4),
                "mem_storew_le.0",
            ),
            felt_instruction_case("add", 2, 1, "add"),
            felt_instruction_case("add_imm", 1, 1, "add.2"),
            felt_instruction_case("sub", 2, 1, "sub"),
            felt_instruction_case("sub_imm", 1, 1, "sub.2"),
            felt_instruction_case("mul", 2, 1, "mul"),
            felt_instruction_case("mul_imm", 1, 1, "mul.2"),
            felt_instruction_case("div", 2, 1, "div"),
            felt_instruction_case("div_imm", 1, 1, "div.2"),
            felt_instruction_case("neg", 1, 1, "neg"),
            felt_instruction_case("ilog2", 1, 1, "ilog2"),
            felt_instruction_case("inv", 1, 1, "inv"),
            felt_instruction_case("incr", 1, 1, "add.1"),
            felt_instruction_case("pow2", 1, 1, "pow2"),
            felt_instruction_case("exp", 2, 1, "exp"),
            felt_instruction_case("exp_imm", 1, 1, "exp.2"),
            felt_instruction_case("exp_bit_length", 2, 1, "exp.u8"),
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
            instruction_case("assertz", &["i1"], &[], "assertz"),
            instruction_case("assert_eq", &["felt", "felt"], &[], "assert_eq"),
            instruction_case("assert_eqw", &felt_types(8), &[], "assert_eqw"),
            instruction_case("u32cast", &["felt"], &["u32"], "u32cast"),
            instruction_case("u32assert", &["felt"], &["u32"], "u32assert"),
            instruction_case("u32assert2", &["felt", "felt"], &["u32", "u32"], "u32assert2"),
            instruction_case("u32assertw", &felt_types(4), &u32_types(4), "u32assertw"),
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
    fn unsupported_instruction_matrix_reports_diagnostics() {
        let cases = [
            unsupported_instruction_case("assert_err", 0, "assert.err=\"boom\""),
            unsupported_instruction_case("assert_eq_err", 0, "assert_eq.err=\"boom\""),
            unsupported_instruction_case("assert_eqw_err", 0, "assert_eqw.err=\"boom\""),
            unsupported_instruction_case("assertz_err", 0, "assertz.err=\"boom\""),
            unsupported_instruction_case("u32assert_err", 0, "u32assert.err=\"boom\""),
            unsupported_instruction_case("u32assert2_err", 0, "u32assert2.err=\"boom\""),
            unsupported_instruction_case("u32assertw_err", 0, "u32assertw.err=\"boom\""),
            unsupported_instruction_case("ext2add", 0, "ext2add"),
            unsupported_instruction_case("locaddr", 1, "locaddr.0"),
            unsupported_instruction_case("caller", 0, "caller"),
            unsupported_instruction_case("clk", 0, "clk"),
            unsupported_instruction_case("hash", 0, "hash"),
            unsupported_instruction_case("fri_ext2fold4", 0, "fri_ext2fold4"),
            unsupported_instruction_case("dynexec", 0, "dynexec"),
            unsupported_instruction_case("emit", 0, "emit"),
            unsupported_instruction_case("trace", 0, "trace.1"),
        ];

        for case in &cases {
            assert_instruction_case_is_unsupported(case);
        }
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

        let one_signature =
            find_function(output.module, "test_one").borrow().get_signature().clone();
        assert_eq!(one_signature.params().len(), 1);
        assert_eq!(one_signature.params()[0].ty, Type::Felt);
        assert_eq!(one_signature.results().len(), 2);
        assert_eq!(one_signature.results()[0].ty, Type::I1);
        assert_eq!(one_signature.results()[1].ty, Type::Felt);

        let word_signature =
            find_function(output.module, "test_word").borrow().get_signature().clone();
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

        for name in ["add3_wide", "add3_overflow"] {
            let signature = find_function(output.module, name).borrow().get_signature().clone();
            assert_eq!(signature.params().len(), 3);
            assert!(signature.params().iter().all(|param| param.ty == Type::U32));
            assert_eq!(signature.results().len(), 2);
            assert!(signature.results().iter().all(|result| result.ty == Type::U32));
        }

        let signature =
            find_function(output.module, "add3_wrapping").borrow().get_signature().clone();
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

        let choose_signature =
            find_function(output.module, "choose").borrow().get_signature().clone();
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

        let load_signature =
            find_function(output.module, "load_word").borrow().get_signature().clone();
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
    fn project_disassembly_uses_workspace_dependency_signatures() -> Result<()> {
        let (root, app_dir) =
            write_workspace_dependency_project("midenc_frontend_masm_workspace_dep");

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
    fn lifts_u32assertw_as_u32_cast_contract() -> Result<()> {
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
        assert_eq!(top_level_op_count::<UnrealizedConversionCast>(function), 4);

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

        assert_eq!(
            top_level_op_count::<HirLoadLocal>(find_function(output.module, "load_word")),
            4
        );
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
        let err = match disassemble_source(
            source.clone(),
            "test",
            &DisassemblerConfig::default(),
            context,
        ) {
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

    fn write_workspace_dependency_project(
        prefix: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
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

    fn write_preassembled_dependency_project(
        prefix: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
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
}
