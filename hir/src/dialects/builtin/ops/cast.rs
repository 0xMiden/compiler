use crate::{
    derive::operation,
    dialects::builtin::BuiltinDialect,
    effects::{EffectIterator, EffectOpInterface, MemoryEffect, MemoryEffectOpInterface},
    traits::{AnyType, InferTypeOpInterface, UnaryOp},
    Context, Report, Spanned, Type, Value,
};

#[operation(
    dialect = BuiltinDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct UnrealizedConversionCast {
    #[operand]
    operand: AnyType,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for UnrealizedConversionCast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl EffectOpInterface<MemoryEffect> for UnrealizedConversionCast {
    fn has_no_effect(&self) -> bool {
        true
    }

    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec::smallvec![])
    }
}
