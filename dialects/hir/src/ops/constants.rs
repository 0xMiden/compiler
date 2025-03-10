use alloc::{boxed::Box, sync::Arc};

use midenc_hir2::{
    constants::{ConstantData, ConstantId},
    derive::operation,
    effects::MemoryEffectOpInterface,
    traits::*,
    *,
};

use crate::{HirDialect, PointerAttr};

/// An operation for expressing constant pointer values.
///
/// This is used to materialize folded constants for the HIR dialect.
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

has_no_effects!(ConstantPointer);

impl InferTypeOpInterface for ConstantPointer {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = Type::Ptr(Box::new(self.value().pointee_type().clone()));
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for ConstantPointer {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.get_attribute("value").unwrap().clone_value()));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

/// A constant operation used to define an array of arbitrary bytes.
///
/// This is intended for use in [super::GlobalVariable] initializer regions only. For non-global
/// uses, the maximum size of immediate values is limited to a single word. This restriction does
/// not apply to global variable initializers, which are used to express the data that should be
/// placed in memory at the address allocated for the variable, without explicit load/store ops.
#[operation(
    dialect = HirDialect,
    name = "bytes",
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct ConstantBytes {
    #[attr(hidden)]
    id: ConstantId,
    #[result]
    result: AnyArrayOf<UInt8>,
}

has_no_effects!(ConstantBytes);

impl InferTypeOpInterface for ConstantBytes {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let len = self.size_in_bytes();
        self.result_mut().set_type(Type::Array(Box::new(Type::U8), len));

        Ok(())
    }
}

impl Foldable for ConstantBytes {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.get_attribute("id").unwrap().clone_value()));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl ConstantBytes {
    pub fn size_in_bytes(&self) -> usize {
        self.as_operation().context().get_constant_size_in_bytes(*self.id())
    }

    pub fn value(&self) -> Arc<ConstantData> {
        self.as_operation().context().get_constant(*self.id())
    }
}
