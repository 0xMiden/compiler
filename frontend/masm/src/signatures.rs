use std::{rc::Rc, sync::Arc};

use miden_assembly_syntax::ast::{FunctionType, TypeExpr};
use midenc_hir::{
    ArrayType, CallConv, Context, FunctionType as HirFunctionType, PointerType, Type,
    dialects::builtin::attributes::Signature,
};

use crate::{Result, error};

pub(crate) fn convert_signature(
    context: &Rc<Context>,
    signature: &FunctionType,
) -> Result<Signature> {
    let params = signature.args.iter().map(convert_type_expr).collect::<Result<Vec<_>>>()?;
    let results = signature.results.iter().map(convert_type_expr).collect::<Result<Vec<_>>>()?;

    Ok(Signature::with_convention(
        context,
        convert_callconv(signature.cc),
        params,
        results,
    ))
}

pub(crate) fn convert_hir_function_type(
    context: &Rc<Context>,
    signature: &HirFunctionType,
) -> Signature {
    Signature::with_convention(
        context,
        signature.calling_convention(),
        signature.params().iter().cloned(),
        signature.results().iter().cloned(),
    )
}

fn convert_callconv(cc: CallConv) -> CallConv {
    cc
}

fn convert_type_expr(ty: &TypeExpr) -> Result<Type> {
    match ty {
        TypeExpr::Primitive(ty) => Ok(ty.inner().clone()),
        TypeExpr::Ptr(ptr) => Ok(Type::Ptr(Arc::new(PointerType::new_with_address_space(
            convert_type_expr(&ptr.pointee)?,
            ptr.address_space(),
        )))),
        TypeExpr::Array(array) => Ok(Type::Array(Arc::new(ArrayType::new(
            convert_type_expr(&array.elem)?,
            array.arity,
        )))),
        TypeExpr::Struct(_) | TypeExpr::Ref(_) => Err(error::error(format!(
            "MASM type expression '{ty:?}' requires symbol resolution, which is not implemented \
             yet"
        ))),
    }
}
