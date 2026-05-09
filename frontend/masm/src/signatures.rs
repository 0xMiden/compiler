use std::{collections::BTreeSet, rc::Rc, sync::Arc};

use miden_assembly_syntax::ast::{Export, FunctionType, Module, SymbolResolution, TypeExpr};
use midenc_hir::{
    ArrayType, CallConv, Context, FunctionType as HirFunctionType, PointerType,
    StructType as HirStructType, Type, dialects::builtin::attributes::Signature,
};

use crate::{ExternalTypeMap, Result, error};

const MAX_TYPE_EXPR_DEPTH: usize = 256;

pub(crate) fn convert_signature(
    context: &Rc<Context>,
    module: &Module,
    signature: &FunctionType,
) -> Result<Signature> {
    let external_types = ExternalTypeMap::new();
    convert_signature_with_external_types(context, module, signature, &external_types)
}

pub(crate) fn convert_signature_with_external_types(
    context: &Rc<Context>,
    module: &Module,
    signature: &FunctionType,
    external_types: &ExternalTypeMap,
) -> Result<Signature> {
    let params = signature
        .args
        .iter()
        .map(|ty| convert_type_expr_with_external_types(context, module, ty, external_types))
        .collect::<Result<Vec<_>>>()?;
    let results = signature
        .results
        .iter()
        .map(|ty| convert_type_expr_with_external_types(context, module, ty, external_types))
        .collect::<Result<Vec<_>>>()?;

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

pub(crate) fn convert_ast_function_type(
    context: &Context,
    module: &Module,
    signature: &FunctionType,
) -> Result<HirFunctionType> {
    let external_types = ExternalTypeMap::new();
    convert_ast_function_type_with_external_types(context, module, signature, &external_types)
}

pub(crate) fn convert_ast_function_type_with_external_types(
    context: &Context,
    module: &Module,
    signature: &FunctionType,
    external_types: &ExternalTypeMap,
) -> Result<HirFunctionType> {
    let params = signature
        .args
        .iter()
        .map(|ty| convert_type_expr_with_external_types(context, module, ty, external_types))
        .collect::<Result<Vec<_>>>()?;
    let results = signature
        .results
        .iter()
        .map(|ty| convert_type_expr_with_external_types(context, module, ty, external_types))
        .collect::<Result<Vec<_>>>()?;

    Ok(HirFunctionType::new(convert_callconv(signature.cc), params, results))
}

fn convert_callconv(cc: CallConv) -> CallConv {
    cc
}

pub(crate) fn convert_type_expr_with_external_types(
    context: &Context,
    module: &Module,
    ty: &TypeExpr,
    external_types: &ExternalTypeMap,
) -> Result<Type> {
    convert_type_expr_with_depth(context, module, ty, external_types, 0)
}

fn convert_type_expr_with_depth(
    context: &Context,
    module: &Module,
    ty: &TypeExpr,
    external_types: &ExternalTypeMap,
    depth: usize,
) -> Result<Type> {
    if depth > MAX_TYPE_EXPR_DEPTH {
        return Err(error::error(format!(
            "MASM type expression nesting exceeds maximum depth of {MAX_TYPE_EXPR_DEPTH}"
        )));
    }

    match ty {
        TypeExpr::Primitive(ty) => Ok(ty.inner().clone()),
        TypeExpr::Ptr(ptr) => Ok(Type::Ptr(Arc::new(PointerType::new_with_address_space(
            convert_type_expr_with_depth(context, module, &ptr.pointee, external_types, depth + 1)?,
            ptr.address_space(),
        )))),
        TypeExpr::Array(array) => Ok(Type::Array(Arc::new(ArrayType::new(
            convert_type_expr_with_depth(context, module, &array.elem, external_types, depth + 1)?,
            array.arity,
        )))),
        TypeExpr::Struct(struct_ty) => {
            let fields = struct_ty
                .fields
                .iter()
                .map(|field| {
                    let ty = convert_type_expr_with_depth(
                        context,
                        module,
                        &field.ty,
                        external_types,
                        depth + 1,
                    )?;
                    Ok((Arc::<str>::from(field.name.as_str()), ty))
                })
                .collect::<Result<Vec<_>>>()?;
            let name = struct_ty.name.as_ref().map(|name| Arc::<str>::from(name.as_str()));
            Ok(Type::Struct(Arc::new(HirStructType::from_parts(
                name,
                *struct_ty.repr.inner(),
                fields,
            ))))
        }
        TypeExpr::Ref(path) => {
            resolve_type_ref(context, module, path.clone(), external_types, depth + 1)
        }
    }
}

fn resolve_type_ref(
    context: &Context,
    module: &Module,
    mut path: miden_assembly_syntax::debuginfo::Span<Arc<miden_assembly_syntax::Path>>,
    external_types: &ExternalTypeMap,
    depth: usize,
) -> Result<Type> {
    let source_manager = context.session().source_manager.clone();
    let original = path.inner().to_string();
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(path.inner().to_string()) {
            return Err(error::error(format!(
                "cyclic MASM type alias resolution involving '{}'",
                path.inner()
            )));
        }

        match module.resolve_path(path.as_deref(), source_manager.clone()) {
            Ok(SymbolResolution::Local(item)) => {
                let item = &module[item.into_inner()];
                let Export::Type(decl) = item else {
                    return Err(error::error(format!(
                        "MASM symbol '{}' does not resolve to a type",
                        path.inner()
                    )));
                };
                return convert_type_expr_with_depth(
                    context,
                    module,
                    &decl.ty(),
                    external_types,
                    depth + 1,
                );
            }
            Ok(SymbolResolution::External(resolved)) => {
                let resolved_key = external_type_key(resolved.inner());
                if let Some(ty) = external_types.get(&resolved_key) {
                    return Ok(ty.clone());
                }
                if resolved != path {
                    path = resolved;
                    continue;
                }
                return Err(error::error(format!(
                    "MASM type reference '{}' resolves to external type '{}', but no external \
                     type metadata was provided{}",
                    original,
                    resolved_key,
                    external_type_metadata_hint(external_types)
                )));
            }
            Ok(SymbolResolution::Exact { .. }) => {
                return Err(error::error(format!(
                    "MASM type reference '{}' could not be resolved from external type metadata{}",
                    original,
                    external_type_metadata_hint(external_types)
                )));
            }
            Ok(SymbolResolution::Module { .. }) | Ok(SymbolResolution::MastRoot(_)) => {
                return Err(error::error(format!(
                    "MASM symbol '{}' does not resolve to a type",
                    path.inner()
                )));
            }
            Err(err) => {
                return Err(error::error(format!(
                    "failed to resolve MASM type reference '{}': {err}",
                    path.inner()
                )));
            }
        }
    }
}

fn external_type_key(path: &miden_assembly_syntax::Path) -> String {
    path.to_absolute().to_string()
}

fn external_type_metadata_hint(external_types: &ExternalTypeMap) -> String {
    if external_types.is_empty() {
        return "; no external type metadata is available".to_string();
    }

    let paths = external_types.keys().take(8).cloned().collect::<Vec<_>>();
    let omitted = external_types.len().saturating_sub(paths.len());
    let mut hint = format!("; available external types: {}", paths.join(", "));
    if omitted > 0 {
        hint.push_str(&format!(" (+{omitted} more)"));
    }
    hint
}
