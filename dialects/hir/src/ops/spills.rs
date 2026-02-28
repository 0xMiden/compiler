use midenc_hir::{
    derive::{EffectOpInterface, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};
use midenc_hir_transform::{ReloadLike, SpillLike};

use crate::HirDialect;

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(MemoryEffectOpInterface, SpillLike, OpPrinter)
)]
pub struct Spill {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
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

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, ReloadLike, OpPrinter)
)]
pub struct Reload {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
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

impl InferTypeOpInterface for Reload {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.spill().ty();
        self.result_mut().set_type(ty);
        Ok(())
    }
}
