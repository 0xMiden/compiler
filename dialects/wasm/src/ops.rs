use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpPrinter, operation},
    effects::MemoryEffectOpInterface,
    matchers::Matcher,
    traits::*,
    *,
};

use crate::WasmDialect;

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = WasmDialect,
    traits(UnaryOp),
    implements(UnaryOp, InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct I32Extend8S {
    #[operand]
    operand: Int32,
    #[result]
    result: Int32,
}

impl InferTypeOpInterface for I32Extend8S {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::I32);
        Ok(())
    }
}

impl Foldable for I32Extend8S {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut attr_value) = matchers::foldable_operand_of_trait::<dyn IntegerLikeAttr>()
            .matches(&self.operand().as_operand_ref())
        {
            let mut attr_value_mut = attr_value.borrow_mut();
            let extended = attr_value_mut.as_immediate().as_i32();

            if let Some(extended) = extended {
                attr_value_mut.set_from_immediate_lossy(Immediate::I32(extended));
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
            let extended = attr.as_immediate().as_i32().map(|v| Immediate::I32((v as i8) as i32));
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
