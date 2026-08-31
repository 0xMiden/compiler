//! Translates one MIR body into a Miden IR function.
//!
//! # Local slots
//!
//! Every MIR local that carries data gets one Miden IR local slot. The frontend stores the
//! entry block arguments into the slots of the argument locals, then writes every definition
//! with `hir.store_local` and reads every use with `hir.load_local`. Only the entry block has
//! block arguments, thus the frontend builds no block arguments of its own. The `local2reg`
//! pass rebuilds SSA form afterwards.

use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, Ident, OpBuilder, Report, SourceSpan, Type, ValueRef, Visibility,
    dialects::builtin::{
        FunctionBuilder, ModuleBuilder,
        attributes::{LocalVariable, Signature},
    },
};
use rustc_public::mir::{Body, Local, Place};

use super::{statement, terminator, types};

/// Translates one MIR body into a function of `module_builder`.
pub(crate) fn translate_function(
    name: &str,
    body: &Body,
    module_builder: &mut ModuleBuilder,
) -> Result<(), Report> {
    // Control flow is a later milestone. A body with more than one block needs block mapping
    // and branch terminators, which the frontend does not build yet.
    if body.blocks.len() != 1 {
        return Err(Report::msg(format!(
            "rust mir frontend: unsupported control flow in `{name}`: the body has {} basic \
             blocks, only one basic block is supported",
            body.blocks.len()
        )));
    }

    let signature = signature(body, module_builder)?;
    let function =
        module_builder.define_function(Ident::from(name), Visibility::Public, signature)?;
    let mut translator =
        BodyTranslator::new(FunctionBuilder::new(function, module_builder.builder()), body)?;

    let block = &body.blocks[0];
    for stmt in &block.statements {
        statement::translate_statement(&mut translator, stmt)?;
    }
    terminator::translate_terminator(&mut translator, &block.terminator)
}

/// Builds the Miden IR signature of a MIR body.
fn signature(body: &Body, module_builder: &mut ModuleBuilder) -> Result<Signature, Report> {
    let params = body
        .arg_locals()
        .iter()
        .map(|decl| types::translate_ty(decl.ty))
        .collect::<Result<Vec<Type>, Report>>()?;

    let ret_ty = body.ret_local().ty;
    let results = if types::is_unit(ret_ty) {
        Vec::new()
    } else {
        vec![types::translate_ty(ret_ty)?]
    };

    let context = module_builder.builder().context_rc();
    Ok(Signature::new(&context, params, results))
}

/// Holds the state that the translation of one MIR body needs.
pub(crate) struct BodyTranslator<'f> {
    /// The builder that appends operations to the function body.
    pub(super) builder: FunctionBuilder<'f, OpBuilder>,
    /// The local slot of every MIR local, indexed by the local. A local of the unit type has no
    /// slot.
    slots: Vec<Option<LocalVariable>>,
}

impl<'f> BodyTranslator<'f> {
    /// Creates the local slots of a MIR body and spills the function arguments into them.
    fn new(mut builder: FunctionBuilder<'f, OpBuilder>, body: &Body) -> Result<Self, Report> {
        let mut slots = Vec::with_capacity(body.locals().len());
        for decl in body.locals() {
            if types::is_unit(decl.ty) {
                slots.push(None);
                continue;
            }
            let ty = types::translate_ty(decl.ty)?;
            slots.push(Some(builder.alloc_local(ty)));
        }

        let arg_values: Vec<ValueRef> = builder
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|arg| *arg as ValueRef)
            .collect();

        // MIR numbers the argument locals `_1..=arg_count`, right after the return local.
        for (index, value) in arg_values.into_iter().enumerate() {
            let local = index + 1;
            let slot = slots[local].ok_or_else(|| {
                Report::msg(format!("rust mir frontend: argument local _{local} has no local slot"))
            })?;
            builder.store_local(slot, value, SourceSpan::UNKNOWN)?;
        }

        Ok(Self { builder, slots })
    }

    /// Returns the local slot of a MIR local.
    pub(super) fn slot(&self, local: Local) -> Result<LocalVariable, Report> {
        self.slots
            .get(local)
            .copied()
            .flatten()
            .ok_or_else(|| Report::msg(format!("rust mir frontend: local _{local} has no slot")))
    }

    /// Returns the local slot that a MIR place names.
    ///
    /// A place with a projection reads or writes a part of a local. That is a later milestone.
    pub(super) fn place_slot(&self, place: &Place) -> Result<LocalVariable, Report> {
        if !place.projection.is_empty() {
            return Err(Report::msg(format!(
                "rust mir frontend: unsupported place projection on local _{}",
                place.local
            )));
        }
        self.slot(place.local)
    }

    /// Tells whether a MIR local has a local slot.
    pub(super) fn has_slot(&self, local: Local) -> bool {
        self.slots.get(local).copied().flatten().is_some()
    }
}
