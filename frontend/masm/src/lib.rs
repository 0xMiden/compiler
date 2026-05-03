//! MASM-to-HIR disassembler.

mod error;
mod lift;
mod project;
mod signatures;

use std::{path::Path, rc::Rc};

use miden_assembly_syntax::{
    Parse, ParseOptions,
    ast::{Module, ModuleKind},
    debuginfo::{SourceLanguage, SourceManager, SourceManagerExt, Uri},
};
use midenc_hir::{Context, dialects::builtin};

pub use self::error::Result;

/// Configuration for MASM disassembly.
#[derive(Debug, Clone, Copy)]
pub struct DisassemblerConfig {
    /// Infer signatures for procedures whose MASM AST/package metadata does not provide one.
    ///
    /// Phase 1 only supports known signatures, so enabling this currently has no effect.
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
    let path = path.as_ref();
    let source_manager = context.session().source_manager.clone();
    let source_file = source_manager.load_file(path).map_err(|err| {
        error::error(format!("failed to load MASM source '{}': {err}", path.display()))
    })?;
    let module_path = masm_module_path_from_file(path)?;
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    disassemble_module(&module, config, context)
}

/// Disassemble a MASM source string into an HIR module.
pub fn disassemble_source(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from(module_path.as_ref().as_str().to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.into());
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    disassemble_module(&module, config, context)
}

/// Disassemble a target from a `miden-project.toml` package manifest.
pub fn disassemble_project_target(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let source_path =
        project::resolve_project_target_path(manifest_path.as_ref(), target, &context)?;
    disassemble_file(source_path, config, context)
}

/// Disassemble a parsed MASM AST module into HIR.
pub fn disassemble_module(
    module: &Module,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    lift::lift_module(module, config, context)
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
    use std::rc::Rc;

    use midenc_hir::{
        SymbolName, SymbolTable, Type,
        dialects::builtin::{self, Function},
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
}
