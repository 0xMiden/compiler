use midenc_hir2::{
    derive::operation,
    effects::*,
    traits::*,
    transforms::{ReloadLike, SpillLike},
    *,
};
use smallvec::smallvec;

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    traits(SameOperandsAndResultType),
    implements(MemoryEffectOpInterface, SpillLike)
)]
pub struct Spill {
    #[operand]
    value: AnyType,
}

impl SpillLike for Spill {
    fn spilled(&self) -> OpOperand {
        self.value().as_operand_ref()
    }

    fn spilled_value(&self) -> ValueRef {
        self.value().as_value_ref()
    }
}

impl EffectOpInterface<MemoryEffect> for Spill {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new_for_value(
            MemoryEffect::Write,
            self.spilled_value()
        ),])
    }
}

#[operation(
    dialect = HirDialect,
    traits(SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, ReloadLike)
)]
pub struct Reload {
    #[operand]
    spill: AnyType,
    #[result]
    result: AnyType,
}

impl ReloadLike for Reload {
    fn spilled(&self) -> OpOperand {
        self.spill().as_operand_ref()
    }

    fn spilled_value(&self) -> ValueRef {
        self.spill().as_value_ref()
    }

    fn reloaded(&self) -> ValueRef {
        self.result().as_value_ref()
    }
}

impl EffectOpInterface<MemoryEffect> for Reload {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new_for_value(
            MemoryEffect::Read,
            self.spilled_value()
        )])
    }
}

impl InferTypeOpInterface for Reload {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.spill().ty();
        self.result_mut().set_type(ty);
        Ok(())
    }
}
