use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Pop one field element from the VM advice stack.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct AdvicePop {
    #[result]
    result: IntFelt,
}

impl InferTypeOpInterface for AdvicePop {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::Felt);
        Ok(())
    }
}

/// Pop one word from the VM advice stack, overwriting the top four operand stack slots.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct AdviceLoadWord {
    #[operand]
    old0: AnyType,
    #[operand]
    old1: AnyType,
    #[operand]
    old2: AnyType,
    #[operand]
    old3: AnyType,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
}

impl InferTypeOpInterface for AdviceLoadWord {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result0_mut().set_type(Type::Felt);
        self.result1_mut().set_type(Type::Felt);
        self.result2_mut().set_type(Type::Felt);
        self.result3_mut().set_type(Type::Felt);
        Ok(())
    }
}
