use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Return the caller procedure hash as a word.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read))]
pub struct Caller {
    #[result]
    result: AnyArray,
}

impl InferTypeOpInterface for Caller {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::from(ArrayType::new(Type::Felt, 4)));
        Ok(())
    }
}

/// Return the current VM clock cycle.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Clk {
    #[result]
    result: IntFelt,
}

impl InferTypeOpInterface for Clk {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::Felt);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MemGrow {
    #[operand]
    pages: UInt32,
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemGrow {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read))]
pub struct MemSize {
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemSize {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemSet {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    addr: AnyPointer,
    #[operand]
    count: UInt32,
    #[operand]
    value: AnyType,
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemCpy {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    source: AnyPointer,
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}

/// Prints a string to the debug output.
///
/// The string bytes are read from memory at the given pointer address and length.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(OpPrinter, MemoryEffectOpInterface)
)]
pub struct PrintLn {
    // TODO(pauls): The Write effect here is added to prevent DCE from removing this op, but
    // we should model this with a specific I/O effect
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
    ptr: PointerOf<UInt8>,
    #[operand]
    len: UInt32,
}
