//! Convertion between the Wasm CM types and the Miden cross-context ABI types.
//!
//! See [the Canonical ABI docs](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening)
//! and [Wasm C ABI docs](https://github.com/WebAssembly/tool-conventions/blob/main/BasicCABI.md)
//! for the Wasm CM <-> core Wasm types conversion rules.

use alloc::rc::Rc;

use midenc_hir::{
    CallConv, Context, EnumType, FunctionType, PointerType, StructType, Type,
    diagnostics::{Diagnostic, miette},
    dialects::builtin::attributes::{AbiParam, Signature},
};

const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

/// Identifies which kind of component wrapper is being flattened for the canonical ABI.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CanonicalAbiMode {
    /// Flatten the signature for a component export wrapper, i.e. the wrapper synthesized for
    /// WAT `(canon lift)`.
    Export,
    /// Flatten the signature for a component import wrapper, i.e. the wrapper synthesized for
    /// WAT `(canon lower)`.
    Import,
}

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum CanonicalTypeError {
    #[error("unexpected use of reserved canonical abi type: {0}")]
    #[diagnostic()]
    Reserved(Type),
    #[error("type '{0}' is not supported by the canonical abi")]
    #[diagnostic()]
    Unsupported(Type),
    #[error("non-C-like enum '{0}' is not supported by the canonical abi")]
    #[diagnostic()]
    NonCLikeEnum(Type),
    #[error("canonical abi layout for type '{ty}' overflowed u32 offsets")]
    #[diagnostic()]
    LayoutOverflow { ty: Type },
}

/// Byte layout for one value produced by canonical ABI flattening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalFlatLayoutEntry {
    /// Byte offset of this flattened value relative to the containing tuple.
    pub offset: u32,
    /// Source type to load from memory for this flattened value.
    pub ty: Type,
}

/// Identifies the wrapper transformation required by canonical ABI flattening.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CanonicalAbiTransformation {
    /// The component function can be lowered or lifted directly.
    None,
    /// The component function parameters are passed through a tuple pointer.
    ParamTuple,
    /// The component function result is passed through an out pointer.
    ResultOutPtr,
    /// The component function requires both tupled parameters and an out-pointer result.
    Both,
}

impl CanonicalAbiTransformation {
    /// Returns true when the transformation includes tupled parameter passing.
    pub fn has_param_tuple(self) -> bool {
        matches!(self, Self::ParamTuple | Self::Both)
    }

    /// Returns true when any transformation is required.
    pub fn is_needed(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Flattens the given CanonABI type into a list of ABI parameters.
pub fn flatten_type(context: &Rc<Context>, ty: &Type) -> Result<Vec<AbiParam>, CanonicalTypeError> {
    // see https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
    Ok(match ty {
        Type::I1 => vec![AbiParam::zext(Type::I32, context)],
        Type::I8 => vec![AbiParam::sext(Type::I32, context)],
        Type::U8 => vec![AbiParam::zext(Type::I32, context)],
        Type::I16 => vec![AbiParam::sext(Type::I32, context)],
        Type::U16 => vec![AbiParam::zext(Type::I32, context)],
        Type::I32 => vec![AbiParam::new(Type::I32)],
        Type::U32 => vec![AbiParam::new(Type::I32)],
        Type::I64 => vec![AbiParam::new(Type::I64)],
        Type::U64 => vec![AbiParam::new(Type::I64)],
        Type::I128 | Type::U128 | Type::U256 => {
            unimplemented!("flattening of {ty} in canonical abi")
        }
        Type::F64 => return Err(CanonicalTypeError::Reserved(ty.clone())),
        Type::Felt => vec![AbiParam::new(Type::Felt)],
        Type::Enum(enum_ty) => flatten_enum_type(context, enum_ty)?,
        Type::Struct(struct_ty) => struct_ty
            .fields()
            .iter()
            .map(|field| flatten_type(context, &field.ty))
            .try_collect::<Vec<Vec<AbiParam>>>()?
            .into_iter()
            .flatten()
            .collect(),
        Type::Array(array_ty) => {
            vec![AbiParam::new(array_ty.element_type().clone()); array_ty.len()]
        }
        Type::List(elem_ty) => vec![
            // pointer to the list element type
            AbiParam::sret(Type::from(PointerType::new(elem_ty.as_ref().clone())), context),
            // length of the list
            AbiParam::new(Type::I32),
        ],
        Type::Unknown | Type::Never | Type::Ptr(_) | Type::Function(_) => {
            return Err(CanonicalTypeError::Unsupported(ty.clone()));
        }
    })
}

/// Returns byte offsets for values produced by canonical ABI flattening.
pub fn flattened_type_layout(
    ty: &Type,
    offset: u32,
) -> Result<Vec<CanonicalFlatLayoutEntry>, CanonicalTypeError> {
    let mut layout = Vec::new();
    push_flattened_type_layout(ty, offset, &mut layout)?;
    Ok(layout)
}

/// Returns byte offsets for a canonical ABI tuple of flattened parameter values.
pub fn flattened_types_layout(
    tys: &[Type],
) -> Result<Vec<CanonicalFlatLayoutEntry>, CanonicalTypeError> {
    let tuple = StructType::new(tys.iter().cloned());
    let mut layout = Vec::new();

    for field in tuple.fields() {
        push_flattened_type_layout(&field.ty, field.offset, &mut layout)?;
    }

    Ok(layout)
}

/// Appends byte offsets for values produced by canonical ABI flattening.
fn push_flattened_type_layout(
    ty: &Type,
    offset: u32,
    layout: &mut Vec<CanonicalFlatLayoutEntry>,
) -> Result<(), CanonicalTypeError> {
    match ty {
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::I64
        | Type::U64
        | Type::Felt => layout.push(CanonicalFlatLayoutEntry {
            offset,
            ty: ty.clone(),
        }),
        Type::F64 => return Err(CanonicalTypeError::Reserved(ty.clone())),
        Type::Enum(enum_ty) => {
            if !enum_ty.is_c_like() {
                return Err(CanonicalTypeError::NonCLikeEnum(ty.clone()));
            }
            push_flattened_type_layout(enum_ty.discriminant(), offset, layout)?;
        }
        Type::Struct(struct_ty) => {
            for field in struct_ty.fields() {
                let field_offset = canonical_layout_offset(offset, field.offset, &field.ty)?;
                push_flattened_type_layout(&field.ty, field_offset, layout)?;
            }
        }
        Type::Array(array_ty) => {
            let elem_ty = array_ty.element_type();
            let elem_stride = u32::try_from(elem_ty.aligned_size_in_bytes()).map_err(|_| {
                CanonicalTypeError::LayoutOverflow {
                    ty: elem_ty.clone(),
                }
            })?;
            for index in 0..array_ty.len() {
                let index =
                    u32::try_from(index).map_err(|_| CanonicalTypeError::LayoutOverflow {
                        ty: elem_ty.clone(),
                    })?;
                let elem_offset = index.checked_mul(elem_stride).ok_or_else(|| {
                    CanonicalTypeError::LayoutOverflow {
                        ty: elem_ty.clone(),
                    }
                })?;
                let elem_offset = canonical_layout_offset(offset, elem_offset, elem_ty)?;
                push_flattened_type_layout(elem_ty, elem_offset, layout)?;
            }
        }
        Type::List(_) => {
            layout.push(CanonicalFlatLayoutEntry {
                offset,
                ty: Type::I32,
            });
            layout.push(CanonicalFlatLayoutEntry {
                offset: canonical_layout_offset(offset, 4, ty)?,
                ty: Type::I32,
            });
        }
        Type::I128
        | Type::U128
        | Type::U256
        | Type::Unknown
        | Type::Never
        | Type::Ptr(_)
        | Type::Function(_) => {
            return Err(CanonicalTypeError::Unsupported(ty.clone()));
        }
    }

    Ok(())
}

/// Adds a relative byte offset within a canonical ABI tuple layout.
fn canonical_layout_offset(base: u32, relative: u32, ty: &Type) -> Result<u32, CanonicalTypeError> {
    base.checked_add(relative)
        .ok_or_else(|| CanonicalTypeError::LayoutOverflow { ty: ty.clone() })
}

/// Flattens a HIR enum according to the component-model variant flattening rules.
fn flatten_enum_type(
    context: &Rc<Context>,
    enum_ty: &EnumType,
) -> Result<Vec<AbiParam>, CanonicalTypeError> {
    let mut flat = flatten_type(context, enum_ty.discriminant())?;
    if enum_ty.is_c_like() {
        return Ok(flat);
    }

    let mut payload = Vec::<AbiParam>::new();
    for variant in enum_ty.variants() {
        let Some(value_ty) = variant.value.as_ref() else {
            continue;
        };
        let flattened = flatten_type(context, value_ty)?;
        for (index, param) in flattened.into_iter().enumerate() {
            if let Some(joined) = payload.get_mut(index) {
                *joined = join_abi_param(joined, &param)?;
            } else {
                payload.push(param);
            }
        }
    }

    flat.extend(payload);
    Ok(flat)
}

/// Joins two flattened ABI parameters at the same variant payload position.
fn join_abi_param(left: &AbiParam, right: &AbiParam) -> Result<AbiParam, CanonicalTypeError> {
    let ty = join_flat_types(&left.ty, &right.ty)?;
    if left.ty == ty && right.ty == ty && left.extension() == right.extension() {
        return Ok(left.clone());
    }
    Ok(AbiParam::new(ty))
}

/// Joins two component flat types at the same variant payload position.
pub(crate) fn join_flat_types(left: &Type, right: &Type) -> Result<Type, CanonicalTypeError> {
    if left == right {
        return Ok(left.clone());
    }

    match (left, right) {
        (Type::I32, Type::I64) | (Type::I64, Type::I32) => Ok(Type::I64),
        (Type::I32, Type::Felt) | (Type::Felt, Type::I32) => Ok(Type::I32),
        (Type::I64, Type::Felt) | (Type::Felt, Type::I64) => Ok(Type::I64),
        (Type::I64, Type::F64) | (Type::F64, Type::I64) => Ok(Type::I64),
        (Type::Ptr(_), Type::I32) | (Type::I32, Type::Ptr(_)) => Ok(Type::I32),
        (Type::Ptr(_), Type::Ptr(_)) => Ok(Type::I32),
        _ => Err(CanonicalTypeError::Unsupported(left.clone())),
    }
}

/// Flattens the given list of CanonABI types into a list of ABI parameters.
pub fn flatten_types(
    context: &Rc<Context>,
    tys: &[Type],
) -> Result<Vec<AbiParam>, CanonicalTypeError> {
    Ok(tys
        .iter()
        .map(|t| flatten_type(context, t))
        .try_collect::<Vec<Vec<AbiParam>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Returns true when flattened parameters exceed the direct cross-context call budget.
pub(crate) fn flat_params_need_tuple(flat_params: &[AbiParam]) -> bool {
    flat_params.len() > MAX_FLAT_PARAMS
        || flat_params.iter().map(|param| param.ty.size_in_felts()).sum::<usize>() > MAX_FLAT_PARAMS
}

/// Classifies the canonical ABI transformation required by a component function type.
pub fn classify_function_type(
    context: &Rc<Context>,
    func_ty: &FunctionType,
) -> Result<CanonicalAbiTransformation, CanonicalTypeError> {
    assert!(
        func_ty.abi.is_wasm_canonical_abi(),
        "unexpected function abi: {:?}",
        &func_ty.abi
    );

    let flat_params = flatten_types(context, &func_ty.params)?;
    let flat_results = flatten_types(context, &func_ty.results)?;
    let needs_param_tuple = flat_params_need_tuple(&flat_params);
    let needs_result_out_ptr = flat_results.len() > MAX_FLAT_RESULTS;

    Ok(match (needs_param_tuple, needs_result_out_ptr) {
        (false, false) => CanonicalAbiTransformation::None,
        (true, false) => CanonicalAbiTransformation::ParamTuple,
        (false, true) => CanonicalAbiTransformation::ResultOutPtr,
        (true, true) => CanonicalAbiTransformation::Both,
    })
}

/// Flattens the given CanonABI function type
pub fn flatten_function_type(
    context: &Rc<Context>,
    func_ty: &FunctionType,
    mode: CanonicalAbiMode,
) -> Result<Signature, CanonicalTypeError> {
    // from https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
    //
    // For a variety of practical reasons, we need to limit the total number of flattened
    // parameters and results, falling back to storing everything in linear memory. The number of
    // flattened results is currently limited to 1 due to various parts of the toolchain (notably
    // the C ABI) not yet being able to express multi-value returns. Hopefully this limitation is
    // temporary and can be lifted before the Component Model is fully standardized.
    assert!(
        func_ty.abi.is_wasm_canonical_abi(),
        "unexpected function abi: {:?}",
        &func_ty.abi
    );
    let mut flat_params = flatten_types(context, &func_ty.params)?;
    let mut flat_results = flatten_types(context, &func_ty.results)?;
    if flat_params_need_tuple(&flat_params) {
        // from https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
        //
        // When there are too many flat values, in general, a single `i32` pointer can be passed instead
        // (pointing to a tuple in linear memory). When lowering into linear memory, this requires the
        // Canonical ABI to call `realloc` to allocate space to put the tuple.
        let tuple = Type::from(StructType::new(func_ty.params.clone()));
        flat_params = vec![AbiParam::sret(Type::from(PointerType::new(tuple)), context)];
    }
    if flat_results.len() > MAX_FLAT_RESULTS {
        // from https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
        //
        // As an optimization, when lowering the return value of an imported function (via `canon
        // lower`), the caller can have already allocated space for the return value (e.g.,
        // efficiently on the stack), passing in an `i32` pointer as an parameter instead of
        // returning an `i32` as a return value.
        assert_eq!(func_ty.results.len(), 1, "expected a single result");
        let result = func_ty.results.first().expect("unexpected empty results").clone();
        match mode {
            CanonicalAbiMode::Export => {
                flat_results = vec![AbiParam::sret(Type::from(PointerType::new(result)), context)];
            }
            CanonicalAbiMode::Import => {
                flat_params.push(AbiParam::sret(Type::from(PointerType::new(result)), context));
                flat_results = vec![];
            }
        }
    }
    Ok(Signature {
        params: flat_params,
        results: flat_results,
        cc: CallConv::ComponentModel,
    })
}

/// Checks that the given core Wasm signature is equivalent to the flattened component signature.
///
/// This compares the canonical ABI parameter/result shape (arity and core types), but not the
/// wrapper calling convention. Extension attributes (zext/sext) are ignored: core Wasm signatures
/// are built from bare core types and never carry them, while flattening annotates small scalars
/// with the extension expected from the wrapper.
pub fn check_core_wasm_signature_equivalence(
    wasm_core_sig: &Signature,
    flattened_sig: &Signature,
) -> Result<(), String> {
    if wasm_core_sig.params().len() != flattened_sig.params().len() {
        return Err(format!(
            "expected {} params, got {}",
            flattened_sig.params().len(),
            wasm_core_sig.params().len()
        ));
    }
    if wasm_core_sig.results().len() != flattened_sig.results().len() {
        return Err(format!(
            "expected {} results, got {}",
            flattened_sig.results().len(),
            wasm_core_sig.results().len()
        ));
    }

    for (index, (wasm_core_param, flattened_param)) in
        wasm_core_sig.params().iter().zip(flattened_sig.params()).enumerate()
    {
        if wasm_core_param.ty != flattened_param.ty {
            return Err(format!(
                "expected param {index} to be {}, got {}",
                flattened_param.ty, wasm_core_param.ty
            ));
        }
    }
    for (index, (wasm_core_result, flattened_result)) in
        wasm_core_sig.results().iter().zip(flattened_sig.results()).enumerate()
    {
        if wasm_core_result.ty != flattened_result.ty {
            return Err(format!(
                "expected result {index} to be {}, got {}",
                flattened_result.ty, wasm_core_result.ty
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use midenc_hir::{
        ArrayType, EnumType, Variant, dialects::builtin::attributes::ArgumentExtension,
    };

    use super::*;

    #[test]
    fn test_flatten_type_integers() {
        let context = Rc::new(Context::default());

        // Test I1 (bool)
        let result = flatten_type(&context, &Type::I1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);

        // Test I8
        let result = flatten_type(&context, &Type::I8).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Sext);

        // Test U8
        let result = flatten_type(&context, &Type::U8).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);

        // Test I16
        let result = flatten_type(&context, &Type::I16).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Sext);

        // Test U16
        let result = flatten_type(&context, &Type::U16).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);

        // Test I32
        let result = flatten_type(&context, &Type::I32).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::None);

        // Test U32
        let result = flatten_type(&context, &Type::U32).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::None);

        // Test I64
        let result = flatten_type(&context, &Type::I64).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I64);
        assert_eq!(result[0].extension(), ArgumentExtension::None);

        // Test U64
        let result = flatten_type(&context, &Type::U64).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I64);
        assert_eq!(result[0].extension(), ArgumentExtension::None);
    }

    #[test]
    fn test_flatten_type_felt() {
        let context = Rc::new(Context::default());

        let result = flatten_type(&context, &Type::Felt).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::Felt);
        assert_eq!(result[0].extension(), ArgumentExtension::None);
    }

    #[test]
    fn test_signature_equivalence_checks_result_types() {
        let core_sig = Signature {
            params: vec![],
            results: vec![AbiParam::new(Type::I32)],
            cc: CallConv::ComponentModel,
        };
        let flattened_sig = Signature {
            params: vec![],
            results: vec![AbiParam::new(Type::I64)],
            cc: CallConv::ComponentModel,
        };

        let err = check_core_wasm_signature_equivalence(&core_sig, &flattened_sig)
            .expect_err("result type mismatch should be rejected");
        assert!(err.contains("result 0"), "unexpected diagnostic: {err}");
    }

    #[test]
    fn test_signature_equivalence_ignores_extension_attributes() {
        let context = Rc::new(Context::default());
        let core_sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };
        let flattened_sig = Signature {
            params: vec![AbiParam::zext(Type::I32, &context)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        // Core Wasm signatures are built from bare core types and never carry the
        // zext/sext annotations that flattening adds for small scalars.
        check_core_wasm_signature_equivalence(&core_sig, &flattened_sig)
            .expect("extension attribute difference should be allowed");
    }

    #[test]
    fn test_signature_equivalence_allows_calling_convention_difference() {
        let core_sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![AbiParam::new(Type::I64)],
            cc: CallConv::C,
        };
        let flattened_sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![AbiParam::new(Type::I64)],
            cc: CallConv::ComponentModel,
        };

        check_core_wasm_signature_equivalence(&core_sig, &flattened_sig)
            .expect("calling convention difference should be allowed");
    }

    #[test]
    fn test_flatten_type_c_like_enum() {
        let context = Rc::new(Context::default());
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "status".into(),
                Type::U8,
                [Variant::c_like("ok".into(), Some(0)), Variant::c_like("err".into(), Some(1))],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);
    }

    #[test]
    fn test_flatten_type_payload_enum() {
        let context = Rc::new(Context::default());
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "result".into(),
                Type::U8,
                [
                    Variant::new("ok".into(), Type::Felt, Some(0)),
                    Variant::new("err".into(), Type::I32, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);
        assert_eq!(result[1].ty, Type::I32);
        assert_eq!(result[1].extension(), ArgumentExtension::None);
    }

    #[test]
    fn test_flatten_type_payload_enum_joins_missing_trailing_payloads() {
        let context = Rc::new(Context::default());
        let word_ty = Type::from(StructType::new([Type::Felt, Type::Felt, Type::Felt, Type::Felt]));
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "request".into(),
                Type::U8,
                [
                    Variant::new("scalar".into(), Type::Felt, Some(0)),
                    Variant::new("word".into(), word_ty, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].ty, Type::I32);
        assert!(result[1..].iter().all(|param| param.ty == Type::Felt));
    }

    #[test]
    fn test_flatten_type_payload_enum_joins_word_and_u64() {
        let context = Rc::new(Context::default());
        let word_ty = Type::from(StructType::new([Type::Felt, Type::Felt, Type::Felt, Type::Felt]));
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "request".into(),
                Type::U8,
                [
                    Variant::new("word".into(), word_ty, Some(0)),
                    Variant::new("amount".into(), Type::U64, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::I64);
        assert!(result[2..].iter().all(|param| param.ty == Type::Felt));
    }

    #[test]
    fn test_flatten_type_payload_enum_joins_record_field_orders() {
        let context = Rc::new(Context::default());
        let payload_a = Type::from(StructType::new([Type::U64, Type::U32]));
        let payload_b = Type::from(StructType::new([Type::U32, Type::U64]));
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "request".into(),
                Type::U8,
                [
                    Variant::new("a".into(), payload_a, Some(0)),
                    Variant::new("b".into(), payload_b, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::I64);
        assert_eq!(result[2].ty, Type::I64);
    }

    #[test]
    fn test_flatten_type_payload_enum_joins_u8_and_u64() {
        let context = Rc::new(Context::default());
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "request".into(),
                Type::U8,
                [
                    Variant::new("tiny".into(), Type::U8, Some(0)),
                    Variant::new("wide".into(), Type::U64, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let result = flatten_type(&context, &enum_ty).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::I64);
    }

    #[test]
    fn test_flatten_type_c_like_enum_discriminant_boundary() {
        let context = Rc::new(Context::default());
        let cases_255 = (0..255)
            .map(|index| Variant::c_like(format!("case-{index}").into(), Some(index)))
            .collect::<Vec<_>>();
        let enum_255 =
            Type::Enum(Arc::new(EnumType::new("enum255".into(), Type::U8, cases_255).unwrap()));

        let result = flatten_type(&context, &enum_255).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);

        let cases_256 = (0..256)
            .map(|index| Variant::c_like(format!("case-{index}").into(), Some(index)))
            .collect::<Vec<_>>();
        let enum_256 =
            Type::Enum(Arc::new(EnumType::new("enum256".into(), Type::U16, cases_256).unwrap()));

        let result = flatten_type(&context, &enum_256).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[0].extension(), ArgumentExtension::Zext);
    }

    #[test]
    fn test_flatten_type_struct() {
        let context = Rc::new(Context::default());

        // Empty struct
        let empty_struct = Type::from(StructType::new(core::iter::empty::<Type>()));
        let result = flatten_type(&context, &empty_struct).unwrap();
        assert_eq!(result.len(), 0);

        // Simple struct with two fields
        let struct_ty = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let result = flatten_type(&context, &struct_ty).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::Felt);

        // Nested struct
        let inner_struct = Type::from(StructType::new(vec![Type::I8, Type::U16]));
        let outer_struct = Type::from(StructType::new(vec![Type::I32, inner_struct]));
        let result = flatten_type(&context, &outer_struct).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::I32); // I8 flattened to I32
        assert_eq!(result[1].extension(), ArgumentExtension::Sext);
        assert_eq!(result[2].ty, Type::I32); // U16 flattened to I32
        assert_eq!(result[2].extension(), ArgumentExtension::Zext);
    }

    #[test]
    fn test_flatten_type_array() {
        let context = Rc::new(Context::default());

        // Array of 3 I32s
        let array_ty = Type::from(ArrayType::new(Type::I32, 3));
        let result = flatten_type(&context, &array_ty).unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|param| param.ty == Type::I32));

        // Array of 5 Felts
        let array_ty = Type::from(ArrayType::new(Type::Felt, 5));
        let result = flatten_type(&context, &array_ty).unwrap();
        assert_eq!(result.len(), 5);
        assert!(result.iter().all(|param| param.ty == Type::Felt));

        // Empty array
        let array_ty = Type::from(ArrayType::new(Type::I32, 0));
        let result = flatten_type(&context, &array_ty).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_flatten_type_list() {
        let context = Rc::new(Context::default());

        // List of I32s
        let list_ty = Type::List(Arc::new(Type::I32));
        let result = flatten_type(&context, &list_ty).unwrap();
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0].ty, Type::Ptr(_)));
        assert!(result[0].is_sret_param());
        assert_eq!(result[1].ty, Type::I32); // length

        // List of structs
        let struct_ty = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let list_ty = Type::List(Arc::new(struct_ty));
        let result = flatten_type(&context, &list_ty).unwrap();
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0].ty, Type::Ptr(_)));
        assert!(result[0].is_sret_param());
        assert_eq!(result[1].ty, Type::I32); // length
    }

    #[test]
    fn test_flatten_types() {
        let context = Rc::new(Context::default());

        // Empty types
        let result = flatten_types(&context, &[]).unwrap();
        assert_eq!(result.len(), 0);

        // Single type
        let result = flatten_types(&context, &[Type::I32]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ty, Type::I32);

        // Multiple types
        let result = flatten_types(&context, &[Type::I32, Type::Felt, Type::I8]).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::Felt);
        assert_eq!(result[2].ty, Type::I32); // I8 flattened to I32

        // Types that expand (struct)
        let struct_ty = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let result = flatten_types(&context, &[Type::I32, struct_ty]).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].ty, Type::I32);
        assert_eq!(result[1].ty, Type::I32);
        assert_eq!(result[2].ty, Type::Felt);
    }

    #[test]
    fn test_flattened_types_layout_matches_flattened_shape() {
        let context = Rc::new(Context::default());
        let nested = Type::from(StructType::new(vec![Type::U8, Type::U64, Type::Felt]));
        let params = vec![Type::I32, nested, Type::from(ArrayType::new(Type::U16, 2))];

        let flattened = flatten_types(&context, &params).unwrap();
        let layout = flattened_types_layout(&params).unwrap();

        assert_eq!(layout.len(), flattened.len());
        assert_eq!(
            layout.iter().map(|entry| entry.ty.clone()).collect::<Vec<_>>(),
            vec![Type::I32, Type::U8, Type::U64, Type::Felt, Type::U16, Type::U16]
        );
    }

    #[test]
    fn test_flattened_types_layout_uses_canonical_tuple_offsets() {
        let nested = Type::from(StructType::new(vec![Type::U8, Type::U64]));
        let params = vec![Type::Felt, nested.clone()];
        let tuple = StructType::new(params.clone());
        let Type::Struct(nested_struct) = &nested else {
            panic!("expected nested struct");
        };
        let nested_base = tuple.fields()[1].offset;
        let expected_offsets = vec![
            tuple.fields()[0].offset,
            nested_base + nested_struct.fields()[0].offset,
            nested_base + nested_struct.fields()[1].offset,
        ];

        let layout = flattened_types_layout(&params).unwrap();

        assert_eq!(layout.iter().map(|entry| entry.offset).collect::<Vec<_>>(), expected_offsets);
    }

    #[test]
    fn test_flattened_type_layout_rejects_non_c_like_enum() {
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "result".into(),
                Type::U8,
                [
                    Variant::c_like("ok".into(), Some(0)),
                    Variant::new("err".into(), Type::I32, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let err = flattened_type_layout(&enum_ty, 0)
            .expect_err("non-C-like enum layouts must return a diagnostic");
        let message = err.to_string();

        assert!(message.contains("non-C-like enum"), "unexpected error: {message}");
    }

    #[test]
    fn test_flatten_function_type_simple() {
        let context = Rc::new(Context::default());

        let mut func_ty =
            FunctionType::new(CallConv::Fast, vec![Type::I32, Type::Felt], vec![Type::I32]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 2);
        assert_eq!(sig.params()[0].ty, Type::I32);
        assert_eq!(sig.params()[1].ty, Type::Felt);

        assert_eq!(sig.results().len(), 1);
        assert_eq!(sig.results()[0].ty, Type::I32);

        assert_eq!(sig.cc, CallConv::ComponentModel);
    }

    #[test]
    fn test_flatten_function_type_max_params() {
        let context = Rc::new(Context::default());

        // Exactly 16 params - should not be transformed
        let params = vec![Type::I32; 16];
        let mut func_ty = FunctionType::new(CallConv::Fast, params, vec![Type::I32]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 16);
        assert!(sig.params().iter().all(|p| p.ty == Type::I32));

        // 17 params - should be transformed to pointer
        let params = vec![Type::I32; 17];
        let mut func_ty = FunctionType::new(CallConv::Fast, params, vec![Type::I32]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 1);
        assert!(matches!(sig.params()[0].ty, Type::Ptr(_)));
        assert!(sig.params()[0].is_sret_param());

        // Nine i64 params fit the Canon ABI flat-value count but exceed the 16-felt call budget.
        let params = vec![Type::I64; 9];
        let mut func_ty = FunctionType::new(CallConv::Fast, params, vec![Type::I32]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 1);
        assert!(matches!(sig.params()[0].ty, Type::Ptr(_)));
        assert!(sig.params()[0].is_sret_param());
    }

    #[test]
    fn test_flatten_function_type_max_results_canon_lift() {
        let context = Rc::new(Context::default());

        // Single result - should not be transformed
        let mut func_ty = FunctionType::new(CallConv::Fast, vec![Type::I32], vec![Type::Felt]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.results().len(), 1);
        assert_eq!(sig.results()[0].ty, Type::Felt);

        // Multiple results with struct - should be transformed for lifted wrappers
        let struct_ty = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let mut func_ty = FunctionType::new(CallConv::Fast, vec![Type::I32], vec![struct_ty]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 1);
        assert_eq!(sig.params()[0].ty, Type::I32);

        assert_eq!(sig.results().len(), 1);
        assert!(matches!(sig.results()[0].ty, Type::Ptr(_)));
        assert!(sig.results()[0].is_sret_param());
    }

    #[test]
    fn test_flatten_function_type_max_results_canon_lower() {
        let context = Rc::new(Context::default());

        // Multiple results with struct - should be transformed differently for lowered imports
        let struct_ty = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let mut func_ty = FunctionType::new(CallConv::Fast, vec![Type::I32], vec![struct_ty]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Import).unwrap();

        assert_eq!(sig.params().len(), 2); // original param + return pointer
        assert_eq!(sig.params()[0].ty, Type::I32);
        assert!(matches!(sig.params()[1].ty, Type::Ptr(_)));
        assert!(sig.params()[1].is_sret_param());

        assert_eq!(sig.results().len(), 0); // no results for lowered imports
    }

    #[test]
    fn test_flatten_function_type_edge_cases() {
        let context = Rc::new(Context::default());

        // Empty function
        let mut func_ty = FunctionType::new(CallConv::Fast, vec![], vec![]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();
        assert_eq!(sig.params().len(), 0);
        assert_eq!(sig.results().len(), 0);

        // Many params that expand (structs)
        let struct_ty = Type::from(StructType::new(vec![Type::I32; 10]));
        let params = vec![struct_ty.clone(), struct_ty]; // 20 total params when flattened
        let mut func_ty = FunctionType::new(CallConv::Fast, params, vec![]);
        func_ty.abi = CallConv::ComponentModel;
        let sig = flatten_function_type(&context, &func_ty, CanonicalAbiMode::Export).unwrap();

        assert_eq!(sig.params().len(), 1); // transformed to pointer
        assert!(matches!(sig.params()[0].ty, Type::Ptr(_)));
    }

    #[test]
    fn test_classify_function_type() {
        let context = Rc::new(Context::default());

        let component_func = |params, results| {
            let mut func_ty = FunctionType::new(CallConv::Fast, params, results);
            func_ty.abi = CallConv::ComponentModel;
            func_ty
        };

        // No transformation needed - simple types.
        let func_ty = component_func(vec![Type::I32, Type::Felt], vec![Type::I32]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::None
        );

        // Parameter tuple needed - more than 16 flattened parameters.
        let func_ty = component_func(vec![Type::I32; 17], vec![]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::ParamTuple
        );

        // Result out-pointer needed - result flattens to more than one value.
        let result = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let func_ty = component_func(vec![Type::I32], vec![result]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::ResultOutPtr
        );

        // Both transformations needed.
        let result = Type::from(StructType::new(vec![Type::I32, Type::Felt]));
        let func_ty = component_func(vec![Type::I32; 17], vec![result]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::Both
        );

        // Edge case - exactly 16 flattened parameters.
        let func_ty = component_func(vec![Type::Felt; 16], vec![]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::None
        );

        // Parameter tuple needed - at most 16 flattened parameters can still exceed 16 felts.
        let func_ty = component_func(vec![Type::I64; 9], vec![]);
        assert_eq!(
            classify_function_type(&context, &func_ty).unwrap(),
            CanonicalAbiTransformation::ParamTuple
        );
    }
}
