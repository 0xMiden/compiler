use alloc::{string::ToString, sync::Arc};

use midenc_hir::{
    constants::ConstantData,
    derive::{EffectOpInterface, operation},
    dialects::builtin::attributes::BytesAttr,
    effects::MemoryEffectOpInterface,
    traits::*,
    *,
};

use crate::{HirDialect, PointerAttr};

/// An operation for expressing constant pointer values.
///
/// This is used to materialize folded constants for the HIR dialect.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct ConstantPointer {
    #[attr(hidden)]
    value: PointerAttr,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for ConstantPointer {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = Type::from(PointerType::new(self.value().pointee_type().clone()));
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for ConstantPointer {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl OpPrinter for ConstantPointer {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        let ptr = self.get_value();
        printer.print_decimal_integer(ptr.addr());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}

/// A constant operation used to define an array of arbitrary bytes.
///
/// This is intended for use in [super::GlobalVariable] initializer regions only. For non-global
/// uses, the maximum size of immediate values is limited to a single word. This restriction does
/// not apply to global variable initializers, which are used to express the data that should be
/// placed in memory at the address allocated for the variable, without explicit load/store ops.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    name = "bytes",
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct ConstantBytes {
    #[attr(hidden)]
    bytes: BytesAttr,
    #[result]
    result: AnyArrayOf<UInt8>,
}

impl InferTypeOpInterface for ConstantBytes {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let len = self.size_in_bytes();
        self.result_mut().set_type(Type::from(ArrayType::new(Type::U8, len)));

        Ok(())
    }
}

impl Foldable for ConstantBytes {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.bytes));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl ConstantBytes {
    pub fn size_in_bytes(&self) -> usize {
        self.get_bytes().len()
    }

    pub fn value(&self) -> Arc<ConstantData> {
        self.get_bytes().clone()
    }
}

impl OpPrinter for ConstantBytes {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        let bytes = self.get_bytes();
        printer.print_string(bytes.to_string());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}
