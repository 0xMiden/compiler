//! Shared foreign procedure invocation lowering helpers.
//!
//! Both typed component import lowering and the raw FPI linker stub transform emit the same
//! `hir.exec_fpi` form; the helpers here cover the parts of that form they have in common.

use midenc_dialect_hir::{ExecFpi, HirOpBuilder};
use midenc_hir::{
    Builder, SourceSpan, Type, ValueRef, dialects::builtin::attributes::LocalVariable,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

/// Stores the wrapper-order FPI prefix felts into freshly allocated felt locals.
///
/// `prefix_felts` is the wrapper-order prefix (account id prefix, account id suffix, procedure
/// root felts), while the returned locals are in executor operand order (account id suffix,
/// account id prefix, procedure root felts), ready to be referenced by `hir.exec_fpi`.
pub(crate) fn store_fpi_prefix_locals<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<'_, B>,
    prefix_felts: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<[LocalVariable; ExecFpi::PREFIX_FELTS]> {
    let &[account_id_prefix, account_id_suffix, root0, root1, root2, root3] = prefix_felts else {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI lowering expected {} prefix felts, but received {}",
            ExecFpi::PREFIX_FELTS,
            prefix_felts.len()
        )));
    };

    let executor_order = [account_id_suffix, account_id_prefix, root0, root1, root2, root3];
    let mut prefix_locals = [LocalVariable::default(); ExecFpi::PREFIX_FELTS];
    for (local, value) in prefix_locals.iter_mut().zip(executor_order) {
        *local = fb.alloc_local(Type::Felt);
        fb.store_local(*local, value, span)?;
    }
    Ok(prefix_locals)
}
