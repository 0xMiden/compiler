//! Translates one extracted MIR body into a Miden IR function.
//!
//! # Local slots
//!
//! Every MIR local that carries data gets one Miden IR local slot. The frontend stores the
//! entry block arguments into the slots of the argument locals, then writes every definition
//! with `hir.store_local` and reads every use with `hir.load_local`. Only the entry block has
//! block arguments, thus the frontend builds no block arguments of its own. The `local2reg`
//! pass rebuilds SSA form afterwards.

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, Ident, OpBuilder, Report, SourceSpan, ValueRef, Visibility,
    dialects::builtin::{
        BuiltinOpBuilder, FunctionBuilder, ModuleBuilder,
        attributes::{LocalVariable, Signature},
    },
};

use crate::extracted::{ExtractedFunction, Local, Operand, RETURN_LOCAL, Rvalue};

/// Translates one extracted function into a function of `module_builder`.
pub(super) fn translate_function(
    function: &ExtractedFunction,
    module_builder: &mut ModuleBuilder,
) -> Result<(), Report> {
    let context = module_builder.builder().context_rc();
    let signature = Signature::new(&context, function.params.clone(), function.result.clone());
    let function_ref = module_builder.define_function(
        Ident::from(function.symbol.as_str()),
        Visibility::Public,
        signature,
    )?;
    // The function body gets its own builder. A function builder moves the insertion point of
    // its builder into the body, thus the module builder would insert the next function there.
    let mut op_builder = OpBuilder::new(context);
    let mut translator =
        BodyTranslator::new(FunctionBuilder::new(function_ref, &mut op_builder), function)?;

    for assign in &function.statements {
        let slot = translator.slot(assign.dest)?;
        let value = translator.translate_rvalue(&assign.value)?;
        translator.builder.store_local(slot, value, SourceSpan::UNKNOWN)?;
    }
    translator.translate_return()
}

/// Holds the state that the translation of one body needs.
struct BodyTranslator<'f> {
    /// The builder that appends operations to the function body.
    builder: FunctionBuilder<'f, OpBuilder>,
    /// The local slot of every MIR local, indexed by the local. A local of the unit type has no
    /// slot.
    slots: Vec<Option<LocalVariable>>,
}

impl<'f> BodyTranslator<'f> {
    /// Creates the local slots of a body and spills the function arguments into them.
    fn new(
        mut builder: FunctionBuilder<'f, OpBuilder>,
        function: &ExtractedFunction,
    ) -> Result<Self, Report> {
        let slots: Vec<Option<LocalVariable>> = function
            .locals
            .iter()
            .map(|ty| ty.clone().map(|ty| builder.alloc_local(ty)))
            .collect();

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
            let slot = slots.get(local).copied().flatten().ok_or_else(|| {
                Report::msg(format!("rust mir frontend: argument local _{local} has no local slot"))
            })?;
            builder.store_local(slot, value, SourceSpan::UNKNOWN)?;
        }

        Ok(Self { builder, slots })
    }

    /// Returns the local slot of a MIR local.
    fn slot(&self, local: Local) -> Result<LocalVariable, Report> {
        self.slots
            .get(local)
            .copied()
            .flatten()
            .ok_or_else(|| Report::msg(format!("rust mir frontend: local _{local} has no slot")))
    }

    /// Translates an rvalue and returns the value it produces.
    fn translate_rvalue(&mut self, rvalue: &Rvalue) -> Result<ValueRef, Report> {
        match rvalue {
            Rvalue::Use(operand) => self.translate_operand(operand),
            Rvalue::AddWrapping(lhs, rhs) => {
                let lhs = self.translate_operand(lhs)?;
                let rhs = self.translate_operand(rhs)?;
                self.builder.add_wrapping(lhs, rhs, SourceSpan::UNKNOWN)
            }
        }
    }

    /// Translates an operand and returns the value it produces.
    fn translate_operand(&mut self, operand: &Operand) -> Result<ValueRef, Report> {
        match operand {
            Operand::Local(local) => {
                let slot = self.slot(*local)?;
                self.builder.load_local(slot, SourceSpan::UNKNOWN)
            }
            Operand::Constant(value) => Ok(self.builder.imm(*value, SourceSpan::UNKNOWN)),
        }
    }

    /// Translates the MIR `return` terminator.
    ///
    /// MIR holds the result of a function in the return local `_0`. A function that returns the
    /// unit type has no result value.
    fn translate_return(&mut self) -> Result<(), Report> {
        if self.slots.get(RETURN_LOCAL).copied().flatten().is_some() {
            let slot = self.slot(RETURN_LOCAL)?;
            let value = self.builder.load_local(slot, SourceSpan::UNKNOWN)?;
            self.builder.ret([value], SourceSpan::UNKNOWN)?;
        } else {
            self.builder.ret([], SourceSpan::UNKNOWN)?;
        }
        Ok(())
    }
}
