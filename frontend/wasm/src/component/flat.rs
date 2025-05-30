//! Convertion between the Wasm CM types and the Miden cross-context ABI types.
//!
//! See https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
//! and https://github.com/WebAssembly/tool-conventions/blob/main/BasicCABI.md
//! for the Wasm CM <-> core Wasm types conversion rules.

use midenc_hir::{
    diagnostics::{miette, Diagnostic},
    AbiParam, ArgumentExtension, ArgumentPurpose, CallConv, FunctionType, PointerType, Signature,
    StructType, Type, Visibility,
};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum CanonicalTypeError {
    #[error("unexpected use of reserved canonical abi type: {0}")]
    #[diagnostic()]
    Reserved(Type),
    #[error("type '{0}' is not supported by the canonical abi")]
    #[diagnostic()]
    Unsupported(Type),
}

/// Flattens the given CanonABI type into a list of ABI parameters.
pub fn flatten_type(ty: &Type) -> Result<Vec<AbiParam>, CanonicalTypeError> {
    // see https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
    Ok(match ty {
        Type::I1 => vec![AbiParam {
            ty: Type::I32,
            purpose: ArgumentPurpose::Default,
            extension: ArgumentExtension::Zext,
        }],
        Type::I8 => vec![AbiParam {
            ty: Type::I32,
            purpose: ArgumentPurpose::Default,
            extension: ArgumentExtension::Sext,
        }],
        Type::U8 => vec![AbiParam {
            ty: Type::I32,
            purpose: ArgumentPurpose::Default,
            extension: ArgumentExtension::Zext,
        }],
        Type::I16 => vec![AbiParam {
            ty: Type::I32,
            purpose: ArgumentPurpose::Default,
            extension: ArgumentExtension::Sext,
        }],
        Type::U16 => vec![AbiParam {
            ty: Type::I32,
            purpose: ArgumentPurpose::Default,
            extension: ArgumentExtension::Zext,
        }],
        Type::I32 => vec![AbiParam::new(Type::I32)],
        Type::U32 => vec![AbiParam::new(Type::I32)],
        Type::I64 => vec![AbiParam::new(Type::I64)],
        Type::U64 => vec![AbiParam::new(Type::I64)],
        Type::I128 | Type::U128 | Type::U256 => {
            unimplemented!("flattening of {ty} in canonical abi")
        }
        Type::F64 => return Err(CanonicalTypeError::Reserved(ty.clone())),
        Type::Felt => vec![AbiParam::new(Type::Felt)],
        Type::Struct(struct_ty) => struct_ty
            .fields()
            .iter()
            .map(|field| flatten_type(&field.ty))
            .try_collect::<Vec<Vec<AbiParam>>>()?
            .into_iter()
            .flatten()
            .collect(),
        Type::Array(array_ty) => {
            vec![AbiParam::new(array_ty.element_type().clone()); array_ty.len()]
        }
        Type::List(elem_ty) => vec![
            // pointer to the list element type
            AbiParam::sret(Type::from(PointerType::new(elem_ty.as_ref().clone()))),
            // length of the list
            AbiParam::new(Type::I32),
        ],
        Type::Unknown | Type::Never | Type::Ptr(_) | Type::Function(_) => {
            return Err(CanonicalTypeError::Unsupported(ty.clone()));
        }
    })
}

/// Flattens the given list of CanonABI types into a list of ABI parameters.
pub fn flatten_types(tys: &[Type]) -> Result<Vec<AbiParam>, CanonicalTypeError> {
    Ok(tys
        .iter()
        .map(flatten_type)
        .try_collect::<Vec<Vec<AbiParam>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Flattens the given CanonABI function type
pub fn flatten_function_type(
    func_ty: &FunctionType,
    cc: CallConv,
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
    const MAX_FLAT_PARAMS: usize = 16;
    const MAX_FLAT_RESULTS: usize = 1;
    let mut flat_params = flatten_types(&func_ty.params)?;
    let mut flat_results = flatten_types(&func_ty.results)?;
    if flat_params.len() > MAX_FLAT_PARAMS {
        // from https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#flattening
        //
        // When there are too many flat values, in general, a single `i32` pointer can be passed instead
        // (pointing to a tuple in linear memory). When lowering into linear memory, this requires the
        // Canonical ABI to call `realloc` to allocate space to put the tuple.
        let tuple = Type::from(StructType::new(func_ty.params.clone()));
        flat_params = vec![AbiParam::sret(Type::from(PointerType::new(tuple)))];
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
        match cc {
            CallConv::CanonLift => {
                flat_results = vec![AbiParam::sret(Type::from(PointerType::new(result)))];
            }
            CallConv::CanonLower => {
                flat_params.push(AbiParam::sret(Type::from(PointerType::new(result))));
                flat_results = vec![];
            }
            _ => panic!("unexpected call convention, only CanonLift and CanonLower are supported"),
        }
    }
    Ok(Signature {
        params: flat_params,
        results: flat_results,
        cc,
        visibility: Visibility::Public,
    })
}

/// Checks if the given function signature needs to be transformed, i.e., if it contains a pointer
pub fn needs_transformation(sig: &Signature) -> bool {
    let pointer_in_params =
        sig.params().iter().any(|param| param.purpose == ArgumentPurpose::StructReturn);
    let pointer_in_results = sig
        .results()
        .iter()
        .any(|result| result.purpose == ArgumentPurpose::StructReturn);

    // Check if the total size of parameters exceeds 16 felts (the maximum stack elements
    // accessible to the callee using the `call` instruction)
    let params_size_in_felts: usize =
        sig.params().iter().map(|param| param.ty.size_in_felts()).sum();
    let exceeds_felt_limit = params_size_in_felts > 16;

    pointer_in_params || pointer_in_results || exceeds_felt_limit
}

/// Asserts that the given core Wasm signature is equivalent to the given flattened signature
/// This checks that we flattened the Wasm CM function type correctly.
pub fn assert_core_wasm_signature_equivalence(
    wasm_core_sig: &Signature,
    flattened_sig: &Signature,
) {
    assert_eq!(
        wasm_core_sig.params().len(),
        flattened_sig.params().len(),
        "expected the same number of params"
    );
    assert_eq!(
        wasm_core_sig.results().len(),
        flattened_sig.results().len(),
        "expected the same number of results"
    );
    for (wasm_core_param, flattened_param) in
        wasm_core_sig.params().iter().zip(flattened_sig.params())
    {
        assert_eq!(wasm_core_param.ty, flattened_param.ty, "expected the same param type");
    }
}
