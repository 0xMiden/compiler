use alloc::format;

use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpPrinter, operation},
    dialects::builtin::attributes::TypeAttr,
    effects::MemoryEffectOpInterface,
    matchers::Matcher,
    traits::*,
    *,
};

use crate::WasmDialect;

/// Interprets the operand as value its logical type and sign-extends it to `I32`.
///
/// Handles the following Wasm instructions:
///
/// - `i32.extend8_s`
/// - `i32.extend16_s`
#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = WasmDialect,
    traits(UnaryOp),
    implements(UnaryOp, InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct I32ExtendS {
    #[operand]
    operand: Int32,
    /// Valide source types are `Type::I8` and `Type::I16`.
    #[attr]
    src_ty: TypeAttr,
    #[result]
    result: Int32,
}

impl I32ExtendS {
    /// Interprets `x` as value of the source type and sign-extends it to `i32`. Returns `None` if
    /// the operations source type is invalid.
    pub fn sext_from_src(&self, x: i32) -> Option<i32> {
        match &*self.get_src_ty() {
            Type::I8 => Some((x as i8) as i32),
            Type::I16 => Some((x as i16) as i32),
            _ => None,
        }
    }
}

impl InferTypeOpInterface for I32ExtendS {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let is_valid = matches!(*self.get_src_ty(), Type::I8 | Type::I16);
        if !is_valid {
            return Err(Report::msg(format!(
                "invalid operation i32.extend*_s: source cannot be {}",
                *self.get_src_ty()
            )));
        }
        self.result_mut().set_type(Type::I32);
        Ok(())
    }
}

impl Foldable for I32ExtendS {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut attr_value) = matchers::foldable_operand_of_trait::<dyn IntegerLikeAttr>()
            .matches(&self.operand().as_operand_ref())
        {
            let mut attr_value_mut = attr_value.borrow_mut();
            let value = attr_value_mut.as_immediate().as_i32();
            let extended = value.and_then(|v| self.sext_from_src(v).map(Immediate::I32));

            if let Some(extended) = extended {
                attr_value_mut.set_from_immediate_lossy(extended);
                results.push(OpFoldResult::Attribute(attr_value as AttributeRef));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(attr) = operands[0].as_ref().and_then(|o| {
            let attr = EntityRef::map(o.borrow(), |o| o.as_attr());
            if attr.implements::<dyn IntegerLikeAttr>() {
                Some(EntityRef::map(attr, |attr| attr.as_trait::<dyn IntegerLikeAttr>().unwrap()))
            } else {
                None
            }
        }) {
            let value = attr.as_immediate().as_i32();
            let extended = value.and_then(|v| self.sext_from_src(v).map(Immediate::I32));
            if let Some(extended) = extended {
                let mut new_attr = attr.name().dyn_clone(&*attr);
                let mut new_attr_mut = new_attr.borrow_mut();
                new_attr_mut
                    .as_attr_mut()
                    .as_trait_mut::<dyn IntegerLikeAttr>()
                    .unwrap()
                    .set_from_immediate_lossy(extended);
                results.push(OpFoldResult::Attribute(new_attr));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }
}
