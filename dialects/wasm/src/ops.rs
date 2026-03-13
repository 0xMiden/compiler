use alloc::format;

use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::TypeAttr,
    effects::MemoryEffectOpInterface,
    matchers::Matcher,
    traits::*,
    *,
};

use crate::WasmDialect;

/// Interprets the operand as a value of the source type and sign-extends it to the destination
/// type.
///
/// # Allowed types
///
/// The source type must be narrower than the destination type. The allowed source types per
/// destination type are:
///
/// | Destination | Source Types |
/// |-------------|--------------|
/// | `Type::I32` | `Type::I8`, `Type::I16` |
/// | `Type::I64` | `Type::I8`, `Type::I16`, `Type::I32` |
///
/// This is verified by `InferTypeOpInterface`.
///
/// # Mapping to wasm instructions
///
/// The corresponding Wasm instruction is `<dst_ty>.extend<src_ty>_s`, for example `i32.extend8_s`.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = WasmDialect,
    traits(UnaryOp),
    implements(UnaryOp, InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct SignExtend {
    #[operand]
    operand: Or<Int32, Int64>,
    /// Source type to sign-extend from.
    #[attr]
    src_ty: TypeAttr,
    /// Destination type to sign-extend to.
    #[attr]
    dst_ty: TypeAttr,
    #[result]
    result: Or<Int32, Int64>,
}

impl SignExtend {
    /// Interprets `x` as a value of the source type and sign-extends it to the destination type.
    pub fn sext_from_src(&self, x: Immediate) -> Immediate {
        match &*self.get_dst_ty() {
            Type::I32 => {
                // Handles `i32.extend<src_ty>_s`
                let value = x.as_i32().expect("operand should be i32");
                match &*self.get_src_ty() {
                    Type::I8 => Immediate::I32((value as i8) as i32),
                    Type::I16 => Immediate::I32((value as i16) as i32),
                    ty => panic!("invalid operation i32.extend<src_ty>_s: source cannot be {ty}"),
                }
            }
            Type::I64 => {
                // Handles `i64.extend<src_ty>_s`
                let value = x.as_i64().expect("operand should be i64");
                match &*self.get_src_ty() {
                    Type::I8 => Immediate::I64((value as i8) as i64),
                    Type::I16 => Immediate::I64((value as i16) as i64),
                    Type::I32 => Immediate::I64((value as i32) as i64),
                    ty => panic!("invalid operation i64.extend<src_ty>_s: source cannot be {ty}"),
                }
            }
            ty => panic!("invalid operation <dst_ty>.extend<src_ty>_s: destination cannot be {ty}"),
        }
    }

    /// Checks whether `x` is a valid operand.
    ///
    /// Note that the destination type determines the operand type. For example in `i32.extend8_s`,
    /// `i32` is the destination and operand type.
    pub fn is_valid_immediate(&self, x: Immediate) -> bool {
        matches!(
            (&*self.get_dst_ty(), x),
            (&Type::I32, Immediate::I32(_)) | (&Type::I64, Immediate::I64(_))
        )
    }
}

impl InferTypeOpInterface for SignExtend {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let operand_ty = self.operand().ty();
        let dst_ty = self.get_dst_ty().clone();
        let src_ty = self.get_src_ty().clone();

        if operand_ty != dst_ty {
            return Err(Report::msg(format!(
                "invalid operation <dst_ty>.extend*_<src_ty>: operand type {operand_ty} does not \
                 match destination type {dst_ty}",
            )));
        }

        match (&dst_ty, &src_ty) {
            (&Type::I32, &Type::I8 | &Type::I16) => {}
            (&Type::I64, &Type::I8 | &Type::I16 | &Type::I32) => {}
            (dst_ty, src_ty) => {
                return Err(Report::msg(format!(
                    "invalid operation <dst_ty>.extend*_<src_ty>: invalid (dst_ty, src_ty) \
                     combination: ({dst_ty}, {src_ty})"
                )));
            }
        };
        self.result_mut().set_type(dst_ty);
        Ok(())
    }
}

impl Foldable for SignExtend {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut attr_value) = matchers::foldable_operand_of_trait::<dyn IntegerLikeAttr>()
            .matches(&self.operand().as_operand_ref())
        {
            let mut attr_value_mut = attr_value.borrow_mut();
            let value = attr_value_mut.as_immediate();
            if self.is_valid_immediate(value) {
                let extended = self.sext_from_src(value);
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
            let value = attr.as_immediate();
            if self.is_valid_immediate(value) {
                let extended = self.sext_from_src(value);
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
