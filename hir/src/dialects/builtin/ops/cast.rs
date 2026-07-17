use crate::{
    Context, OpPrinter, Report, Spanned, Value,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::{BuiltinDialect, attributes::TypeAttr},
    effects::MemoryEffectOpInterface,
    traits::{
        AnyType, InferTypeOpInterface, OperandRangeRequirement, OperandRangeRequirementOpInterface,
        UnaryOp,
    },
};

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = BuiltinDialect,
    traits(UnaryOp),
    implements(
        InferTypeOpInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct UnrealizedConversionCast {
    #[operand]
    operand: AnyType,
    #[attr]
    ty: TypeAttr,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for UnrealizedConversionCast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl OperandRangeRequirementOpInterface for UnrealizedConversionCast {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // Unrealized casts bridge representations while conversion/legalization is in progress.
        // The operation that semantically consumes a constrained value should carry the range
        // requirement.
        OperandRangeRequirement::None
    }
}
