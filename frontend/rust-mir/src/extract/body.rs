//! Extracts one MIR body.

use rustc_public::mir::{Body, Place};

use super::{ExtractResult, statement, terminator, types};
use crate::extracted::{ExtractedFunction, Local};

/// Extracts one MIR body as the function `symbol`.
pub(super) fn extract_function(symbol: String, body: &Body) -> ExtractResult<ExtractedFunction> {
    // Control flow is a later milestone. A body with more than one block needs block mapping
    // and branch terminators, which the frontend does not build yet.
    if body.blocks.len() != 1 {
        return Err(format!(
            "rust mir frontend: unsupported control flow in `{symbol}`: the body has {} basic \
             blocks, only one basic block is supported",
            body.blocks.len()
        ));
    }

    let params = body
        .arg_locals()
        .iter()
        .map(|decl| types::extract_ty(decl.ty))
        .collect::<ExtractResult<Vec<_>>>()?;
    let result = types::extract_value_ty(body.ret_local().ty)?;
    let locals = body
        .locals()
        .iter()
        .map(|decl| types::extract_value_ty(decl.ty))
        .collect::<ExtractResult<Vec<_>>>()?;

    let block = &body.blocks[0];
    let mut statements = Vec::with_capacity(block.statements.len());
    for stmt in &block.statements {
        if let Some(assign) = statement::extract_statement(stmt)? {
            statements.push(assign);
        }
    }
    terminator::check_terminator(&block.terminator)?;

    Ok(ExtractedFunction {
        symbol,
        params,
        result,
        locals,
        statements,
    })
}

/// Returns the local that a MIR place names.
///
/// A place with a projection reads or writes a part of a local. That is a later milestone.
pub(super) fn place_local(place: &Place) -> ExtractResult<Local> {
    if !place.projection.is_empty() {
        return Err(format!(
            "rust mir frontend: unsupported place projection on local _{}",
            place.local
        ));
    }
    Ok(place.local)
}
