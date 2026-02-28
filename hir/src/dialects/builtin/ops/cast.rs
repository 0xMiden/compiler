use crate::{
    Context, OpPrinter, Report, Spanned, Value,
    derive::{EffectOpInterface, OpPrinter, operation},
    dialects::builtin::{BuiltinDialect, attributes::TypeAttr},
    effects::MemoryEffectOpInterface,
    traits::{AnyType, InferTypeOpInterface, UnaryOp},
};

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = BuiltinDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
