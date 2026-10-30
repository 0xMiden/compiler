//! Closed inline assembly construction. Parsing grants no target or frame authority.
use alloc::{format, string::ToString, vec, vec::Vec};

use midenc_hir::{Op, Operation, Report, Spanned, SymbolPath, SymbolResolutionError, SymbolTable};

use crate::{
    lower::expand::{AnnotatedMasmBody, AnnotatedOp, annotate_raw},
    masm,
    plan::*,
    select::SelectEnv,
    verify::{CallArgument, CallPath, CallSite, UnitEvidence},
};

pub(crate) struct InlineAsm {
    body: AnnotatedMasmBody,
}

impl InlineAsm {
    pub fn from_source(
        op: &crate::hir::InlineAsm,
        env: &SelectEnv<'_>,
    ) -> Result<(Self, UnitEvidence), Report> {
        op.validate_signature()?;
        let text = op.get_body();
        if text.trim().is_empty() && op.num_results() != 0 {
            return Err(invalid("empty inline assembly requires equal nonzero lane counts"));
        }
        let raw = parse(text.as_str(), op.as_operation(), env)?;
        validate_instructions(&raw)?;
        let mut body = annotate_raw(&raw, SelectedStepId(0));
        let mut proof = UnitEvidence::default();
        collect_calls(&mut body, &mut vec![], op.as_operation(), env, &mut proof)?;
        Ok((Self { body }, proof))
    }

    pub fn expand(&self, id: SelectedStepId) -> AnnotatedMasmBody {
        fn assign(body: &mut AnnotatedMasmBody, id: SelectedStepId) {
            for op in body.ops_mut() {
                match op {
                    AnnotatedOp::Instruction { unit, .. } => *unit = id,
                    AnnotatedOp::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        assign(then_body, id);
                        assign(else_body, id);
                    }
                    _ => unreachable!("validated inline body contains only instructions and if"),
                }
            }
        }
        let mut body = self.body.clone();
        assign(&mut body, id);
        body
    }
}

fn parse(text: &str, op: &Operation, env: &SelectEnv<'_>) -> Result<Vec<masm::Op>, Report> {
    if text.trim().is_empty() {
        return Ok(vec![]);
    }
    // Imports only admit existing nearest-scope HIR spellings to the MASM parser. Calls are
    // resolved again against HIR below; neither these imports nor body text grant authority.
    let mut imports = alloc::collections::BTreeMap::new();
    if let Some(table) = op.nearest_symbol_table() {
        let table = table.borrow();
        let table = table.as_trait::<dyn SymbolTable>().unwrap();
        for symbol in table.symbol_manager().symbols().symbols() {
            if let Ok(resolved) = symbol.resolve_callable() {
                let name = symbol.borrow().name().as_str().to_string();
                let target = crate::lower::lowering::invocation_target_from_symbol(
                    resolved.named_symbol(),
                    None,
                    op.span(),
                );
                let target = if let Some(owners) = env.owners {
                    owners.target(target)?
                } else {
                    target
                };
                imports.insert(name, target.unwrap_path().to_path_buf());
            }
        }
    }
    let mut wrapper = "__inline_asm".to_string();
    while imports.contains_key(&wrapper) {
        wrapper.push('_');
    }
    let mut prefix = alloc::string::String::new();
    for (name, target) in &imports {
        let local = masm::Ident::new(name)
            .map_err(|error| invalid(format!("invalid inline callable name: {error}")))?;
        let source = masm::Ident::new(target.last().unwrap()).unwrap();
        prefix.push_str(&format!(
            "pub use {{{source} as {local}}} from {}\n",
            target.parent().unwrap()
        ));
    }
    let manager = op.context().session().source_manager.clone();
    let source = manager.load(
        midenc_hir::diagnostics::SourceLanguage::Masm,
        "inline-assembly.masm".into(),
        format!("{prefix}proc {wrapper}\n{text}\nend"),
    );
    let module = miden_assembly::ModuleParser::new(Some(masm::ModuleKind::Library)).parse(
        Some(masm::Path::new("::__inline_asm")),
        source,
        manager,
    )?;
    let mut procedures = module.procedures();
    let procedure =
        procedures.next().ok_or_else(|| invalid("malformed inline assembly wrapper"))?;
    if module.path() != masm::Path::new("::__inline_asm")
        || module.items().len() != 1
        || procedures.next().is_some()
        || module.imports().count() != imports.len()
        || module.imports().any(|import| match import {
            masm::Import::Item(import) => {
                imports
                    .get(import.local_name().as_str())
                    .is_none_or(|target| target.as_path() != import.target_path().inner().as_ref())
                    || !import.visibility().is_exported()
            }
            _ => true,
        })
        || !module.required_packages().is_empty()
        || !module.submodules().is_empty()
        || !module.advice_map().is_empty()
        || module.docs().is_some()
        || procedure.docs().is_some()
        || procedure.name().as_str() != wrapper
        || procedure.num_locals() != 0
        || procedure.signature().is_some()
        || !procedure.attributes().is_empty()
        || procedure.is_syscall()
    {
        return Err(invalid(
            "malformed inline assembly wrapper: declarations, imports and frame directives are \
             forbidden",
        ));
    }
    Ok(procedure.iter().cloned().collect())
}

fn validate_instructions(body: &[masm::Op]) -> Result<(), Report> {
    use masm::{Instruction as I, Op as O};
    for item in body {
        match item {
            O::If {
                then_blk, else_blk, ..
            } => {
                validate_instructions(&then_blk.iter().cloned().collect::<Vec<_>>())?;
                validate_instructions(&else_blk.iter().cloned().collect::<Vec<_>>())?;
            }
            O::While { .. } | O::Repeat { .. } | O::DoWhile { .. } => {
                return Err(invalid(
                    "inline assembly loops (while/repeat/do_while) have no replay schema",
                ));
            }
            O::Inst(instruction) => match instruction.inner() {
                I::Exec(_) | I::Call(_) | I::SysCall(_) => {}
                I::Locaddr(_)
                | I::LocLoad(_)
                | I::LocLoadWBe(_)
                | I::LocLoadWLe(_)
                | I::LocStore(_)
                | I::LocStoreWBe(_)
                | I::LocStoreWLe(_) => {
                    return Err(invalid("inline assembly local-frame instructions are forbidden"));
                }
                I::Sdepth => return Err(invalid("inline assembly sdepth is forbidden")),
                I::DynExec | I::DynCall => {
                    return Err(invalid("inline assembly dynamic calls are forbidden"));
                }
                I::Emit | I::EmitImm(_) | I::Trace | I::TraceImm(_) => {
                    return Err(invalid("inline assembly emit/trace observations are forbidden"));
                }
                I::DebugVar(_) | I::DebugInlineCall(_) | I::DebugInlineCallClear => {
                    return Err(invalid("inline assembly debug/frame directives are forbidden"));
                }
                other => {
                    TargetEffects::instruction(other).map_err(|error| {
                        invalid(format!(
                            "inline assembly instruction '{other}' is unsupported: {error}"
                        ))
                    })?;
                }
            },
        }
    }
    Ok(())
}

fn collect_calls(
    body: &mut AnnotatedMasmBody,
    path: &mut Vec<CallPath>,
    op: &Operation,
    env: &SelectEnv<'_>,
    proof: &mut UnitEvidence,
) -> Result<(), Report> {
    use masm::Instruction as I;
    for item in body.ops_mut() {
        match item {
            AnnotatedOp::If {
                id,
                then_body,
                else_body,
            } => {
                path.push(CallPath::Then(*id));
                collect_calls(then_body, path, op, env, proof)?;
                path.pop();
                path.push(CallPath::Else(*id));
                collect_calls(else_body, path, op, env, proof)?;
                path.pop();
            }
            AnnotatedOp::Instruction { instruction, .. } => {
                let (target, family) = match instruction {
                    I::Exec(target) => (target, Invocation::Exec),
                    I::Call(target) => (target, Invocation::Call),
                    I::SysCall(target) => (target, Invocation::SysCall),
                    _ => continue,
                };
                let (abi, emission) = resolve_call(target, op, env)?;
                *target = emission;
                abi.invocation(family)?;
                proof.calls.push(CallSite {
                    instruction: instruction.clone(),
                    abi,
                    inputs: vec![CallArgument::Local; abi.arguments() as usize],
                    path: path.clone(),
                    truncate: false,
                    outputs: vec![],
                    publications: vec![],
                    selector: None,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_call(
    target: &masm::InvocationTarget,
    op: &Operation,
    env: &SelectEnv<'_>,
) -> Result<(ProcedureAbi, masm::InvocationTarget), Report> {
    if !matches!(target, masm::InvocationTarget::MastRoot(_)) {
        let target_path = target.unwrap_path();
        let mut path =
            SymbolPath::from_library_path(target_path.parent().unwrap_or(masm::Path::new("")));
        path.set_name(midenc_hir::SymbolName::intern(target_path.last().unwrap()));
        if let Some(table) = op.nearest_symbol_table() {
            let table = table.borrow();
            let table = table.as_trait::<dyn SymbolTable>().unwrap();
            match table.resolve_callable(&path) {
                Ok(resolved) => {
                    crate::legalization::validate_callee_visibility(op, resolved.named_symbol())?;
                    let symbol = resolved.target().as_symbol_ref();
                    let symbol = symbol.borrow();
                    if super::calls::is_truncate(&*symbol) {
                        return Err(invalid("inline assembly truncate_stack is forbidden"));
                    }
                    let signature = resolved.signature();
                    let abi = super::calls::signature_abi(
                        &signature.cc,
                        signature.params().iter().map(|p| p.ty.clone()),
                        signature.results().iter().map(|p| p.ty.clone()),
                        super::calls::is_kernel(&*symbol),
                    )?;
                    let target = super::calls::named_target(resolved.named_symbol(), op, env)?;
                    return Ok((abi, target));
                }
                Err(SymbolResolutionError::UnknownSymbol { .. }) => {}
                Err(error) => {
                    return Err(invalid(format!(
                        "inline assembly callee resolution failed: {error}"
                    )));
                }
            }
        }
    }
    if let Some(linked) = env.linked_callables {
        return linked.resolve(target);
    }
    Err(invalid(format!(
        "inline assembly calls a procedure with no linked signature: {target:?}"
    )))
}

fn invalid(message: impl Into<alloc::string::String>) -> Report {
    crate::select::invalid(message)
}
