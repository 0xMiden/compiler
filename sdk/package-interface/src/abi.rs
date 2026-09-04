//! The Miden ABI rule set: how a typed MASM signature is seen from Wasm.
//!
//! One deterministic function from a Miden-side `FunctionType` to a Wasm-side `extern "C"`
//! shape plus an adaptation strategy. The Wasm frontend and the binding generators both call it,
//! so the stub a generator emits and the call the frontend builds agree by construction.

use alloc::{sync::Arc, vec::Vec};

use miden_assembly_syntax::ast::types::{CallConv, FunctionType, PointerType, Type};
use smallvec::SmallVec;

/// The most operand stack elements a `Fast` procedure's arguments, or its results, may occupy.
///
/// The `Fast` convention requires that a procedure "must not require more than 16 elements of
/// the operand stack for arguments or results". `C` and `Wasm` are documented to spill excess
/// *arguments* to the caller's frame instead of failing, so this budget is enforced only for
/// `Fast`: `C` is lowered here too (see [`lower_signature`]) but is never subject to it, and a
/// `Wasm` signature's results are always scalars.
pub const MAX_STACK_ELEMENTS: usize = 16;

/// One Wasm-side scalar of a lowered signature.
///
/// Pointers are `i32` on the wire, and the value is an address in the pointer's own address
/// space: the frontend gives it the pointer type and never rescales it. The variant keeps the
/// pointee and address space for the generator, which names the pointee in the Rust declaration
/// and emits the wrapper that converts between a Rust byte address and an element address (the
/// alignment check going in, the scaling and 32-bit range check coming out).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WasmScalar {
    /// A boolean, passed as `i32` 0 or 1.
    I1,
    /// `i8`, sign-extended in an `i32`.
    I8,
    /// `u8`, zero-extended in an `i32`.
    U8,
    /// `i16`, sign-extended in an `i32`.
    I16,
    /// `u16`, zero-extended in an `i32`.
    U16,
    /// `i32`.
    I32,
    /// `u32`.
    U32,
    /// `i64`, lowered by the frontend to two elements.
    I64,
    /// `u64`, lowered by the frontend to two elements.
    U64,
    /// A field element, carried in an `f32`.
    Felt,
    /// A pointer, carried in an `i32`.
    ///
    /// Caveat: `miden-assembly-syntax` 0.29.1 drops a pointer's parsed `addrspace(..)` when
    /// resolving `TypeExpr::Ptr` (`src/ast/type.rs`, the `Ptr` arm builds
    /// `PointerType::new(pointee)`, which defaults to byte space), so every pointer read from an
    /// assembled package currently reports `AddressSpace::Byte`. See the ignored test
    /// `model::tests::an_element_space_pointer_parameter_keeps_its_address_space`.
    Ptr(Arc<PointerType>),
}

impl WasmScalar {
    /// The type the Wasm frontend assigns to this scalar when it reads a stub's signature.
    ///
    /// The frontend has no sub-word integers and no pointer types at the boundary: everything
    /// 32 bits or narrower, pointers included, is `i32`; 64-bit integers are `i64`; the `f32`
    /// carrier of a felt is `felt`.
    pub fn frontend_type(&self) -> Type {
        match self {
            Self::I64 | Self::U64 => Type::I64,
            Self::Felt => Type::Felt,
            Self::I1
            | Self::I8
            | Self::U8
            | Self::I16
            | Self::U16
            | Self::I32
            | Self::U32
            | Self::Ptr(_) => Type::I32,
        }
    }

    /// The Miden-side type this scalar was flattened from: the inverse of the scalar half of
    /// [`flatten_type`].
    ///
    /// Unlike [`Self::frontend_type`] this is lossless: narrow integers keep their width and
    /// signedness, and a pointer keeps its pointee and address space.
    pub fn miden_type(&self) -> Type {
        match self {
            Self::I1 => Type::I1,
            Self::I8 => Type::I8,
            Self::U8 => Type::U8,
            Self::I16 => Type::I16,
            Self::U16 => Type::U16,
            Self::I32 => Type::I32,
            Self::U32 => Type::U32,
            Self::I64 => Type::I64,
            Self::U64 => Type::U64,
            Self::Felt => Type::Felt,
            Self::Ptr(ptr) => Type::Ptr(ptr.clone()),
        }
    }

    /// How many operand stack elements this scalar occupies on the Miden side.
    ///
    /// A 64-bit integer is carried in two elements; everything else, pointers and narrow
    /// integers included, is one.
    fn stack_elements(&self) -> usize {
        match self {
            Self::I64 | Self::U64 => 2,
            Self::I1
            | Self::I8
            | Self::U8
            | Self::I16
            | Self::U16
            | Self::I32
            | Self::U32
            | Self::Felt
            | Self::Ptr(_) => 1,
        }
    }
}

/// One step into an aggregate: the field or element index, and the field name when the struct
/// declares one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldStep {
    /// Field index within a struct, or element index within an array.
    pub index: usize,
    /// The declared field name, if any. Arrays have none.
    pub name: Option<Arc<str>>,
}

/// One scalar produced by flattening a Miden-side type, with the path that reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Flattened {
    /// Steps from the original value to this scalar; empty when the value was already a scalar.
    pub path: SmallVec<[FieldStep; 2]>,
    /// The scalar.
    pub scalar: WasmScalar,
}

/// Flatten a Miden-side type into the Wasm-side scalars it occupies, in stack order.
///
/// Structs contribute their fields in declared order, arrays their elements in index order,
/// recursively. A zero-sized type contributes no scalars: an empty struct, or an array of length
/// zero, flattens to the empty sequence. A type with no Wasm C ABI lowering (`Unknown`, `Never`,
/// 128-bit and wider integers, floats, lists, function references, non-C-like enums) is returned
/// as the error so a diagnostic can name it.
pub fn flatten_type(ty: &Type) -> Result<Vec<Flattened>, Type> {
    let mut out = Vec::new();
    let mut path = SmallVec::new();
    flatten_into(ty, &mut path, &mut out)?;
    Ok(out)
}

fn flatten_into(
    ty: &Type,
    path: &mut SmallVec<[FieldStep; 2]>,
    out: &mut Vec<Flattened>,
) -> Result<(), Type> {
    let scalar = match ty {
        Type::I1 => WasmScalar::I1,
        Type::I8 => WasmScalar::I8,
        Type::U8 => WasmScalar::U8,
        Type::I16 => WasmScalar::I16,
        Type::U16 => WasmScalar::U16,
        Type::I32 => WasmScalar::I32,
        Type::U32 => WasmScalar::U32,
        Type::I64 => WasmScalar::I64,
        Type::U64 => WasmScalar::U64,
        Type::Felt => WasmScalar::Felt,
        Type::Ptr(ptr) => WasmScalar::Ptr(ptr.clone()),
        // A struct's `TypeRepr` (`Align`, `Packed`, `Transparent`, `BigEndian`) only affects the
        // in-memory layout of the fields, never which scalars the struct flattens to or their
        // order, so the flattening ignores it.
        //
        // `StructRef::get` yields the one-level unfolding of a recursive struct, whose field
        // types are closed; the recursion itself can only pass through a pointer or a list,
        // where flattening stops, so this terminates.
        Type::Struct(st) => {
            for field in st.get().fields() {
                path.push(FieldStep {
                    index: field.index as usize,
                    name: field.name.clone(),
                });
                flatten_into(&field.ty, path, out)?;
                path.pop();
            }
            return Ok(());
        }
        Type::Array(array) => {
            for index in 0..array.len {
                path.push(FieldStep { index, name: None });
                flatten_into(&array.ty, path, out)?;
                path.pop();
            }
            return Ok(());
        }
        Type::Enum(en) if en.get().is_c_like() => {
            return flatten_into(en.get().discriminant(), path, out);
        }
        Type::Unknown
        | Type::Never
        | Type::Variadic
        | Type::I128
        | Type::U128
        | Type::U256
        | Type::F64
        | Type::List(_)
        | Type::Function(_)
        | Type::Enum(_) => return Err(ty.clone()),
    };
    out.push(Flattened {
        path: path.clone(),
        scalar,
    });
    Ok(())
}

/// Whether `flat` is the identity flattening of its source type: the type was already a scalar
/// or a pointer, so it contributes exactly one scalar reached by an empty field path.
fn is_flattening_identity(flat: &[Flattened]) -> bool {
    matches!(flat, [only] if only.path.is_empty())
}

/// One Wasm-side parameter of a lowered signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WasmParam {
    /// Index of the Miden-side parameter this scalar was flattened from.
    pub origin: usize,
    /// Steps from that parameter to this scalar; empty when the parameter was a scalar.
    pub path: SmallVec<[FieldStep; 2]>,
    /// The scalar.
    pub scalar: WasmScalar,
}

/// How a procedure's results reach the Wasm caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReturnStrategy {
    /// No results.
    Void,
    /// Exactly one scalar, returned as the function's return value.
    Direct(WasmScalar),
    /// Two or more scalars: the Wasm signature gains a trailing out-pointer parameter, the
    /// frontend stores the results through it in stack order, and the wrapper reads them back
    /// from a word-aligned return area laid out as this flattened sequence.
    ///
    /// Each result keeps the field path that reaches it, so a generator can name the fields of
    /// the struct it reconstructs from the return area.
    OutPointer(Vec<Flattened>),
}

impl ReturnStrategy {
    /// The result scalars in stack order, without their field paths: none for
    /// [`Self::Void`], the one scalar for [`Self::Direct`], and every flattened scalar for
    /// [`Self::OutPointer`].
    pub fn scalars(&self) -> Vec<WasmScalar> {
        match self {
            Self::Void => Vec::new(),
            Self::Direct(scalar) => alloc::vec![scalar.clone()],
            Self::OutPointer(results) => results.iter().map(|f| f.scalar.clone()).collect(),
        }
    }
}

/// A typed Miden signature and its Wasm C ABI shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoweredSignature {
    /// The Miden-side signature this was derived from.
    pub miden: FunctionType,
    /// The Wasm-side parameters, in order, *excluding* the out pointer [`ReturnStrategy::OutPointer`]
    /// appends.
    pub params: Vec<WasmParam>,
    /// How results are returned.
    pub ret: ReturnStrategy,
}

impl LoweredSignature {
    /// Whether the Wasm signature ends with an out-pointer parameter.
    pub fn has_out_pointer(&self) -> bool {
        matches!(self.ret, ReturnStrategy::OutPointer(_))
    }

    /// The function type a linker stub for this procedure must have, as the Wasm frontend reads
    /// it: every parameter as its [`WasmScalar::frontend_type`], the out pointer as `i32` when
    /// present, and at most one result.
    ///
    /// The frontend compares a stub's actual signature against this to detect bindings generated
    /// from a different package version.
    ///
    /// The returned type carries `CallConv::Wasm`, since that is the convention a Wasm module's
    /// own functions have. `FunctionType`'s equality includes the convention, so a consumer
    /// checking a stub's real signature against this one should compare `params()` and
    /// `results()` — or normalize the convention first — rather than comparing the two
    /// `FunctionType`s directly.
    pub fn stub_signature(&self) -> FunctionType {
        let mut params: Vec<Type> = self.params.iter().map(|p| p.scalar.frontend_type()).collect();
        let results: Vec<Type> = match &self.ret {
            ReturnStrategy::Void => Vec::new(),
            ReturnStrategy::Direct(scalar) => alloc::vec![scalar.frontend_type()],
            ReturnStrategy::OutPointer(_) => {
                params.push(Type::I32);
                Vec::new()
            }
        };
        FunctionType::new(CallConv::Wasm, params, results)
    }

    /// The flattened Miden-side callee signature the Wasm frontend declares for its `exec`
    /// import — what `frontend/wasm/src/miden_abi/*/signatures()` hand-writes today, including
    /// their `Wasm` convention; the callee's own convention is `miden`.
    ///
    /// Its parameters are the flattened parameters, with no out pointer: the out pointer is a
    /// Wasm-side artifact of [`ReturnStrategy::OutPointer`], and the callee returns its results
    /// by value on the operand stack. Every type is the scalar's [`WasmScalar::miden_type`], so
    /// widths and address spaces survive here where [`Self::stub_signature`] collapses them.
    pub fn import_signature(&self) -> FunctionType {
        let params: Vec<Type> = self.params.iter().map(|p| p.scalar.miden_type()).collect();
        let results: Vec<Type> = self.ret.scalars().iter().map(WasmScalar::miden_type).collect();
        FunctionType::new(CallConv::Wasm, params, results)
    }
}

/// Why a signature has no Wasm C ABI lowering.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnsupportedSignature {
    /// A parameter's declared type, or a type nested in it, cannot be lowered.
    #[error(
        "parameter {index} of type `{declared}` cannot be lowered: `{offender}` has no Wasm C ABI \
         lowering"
    )]
    Param {
        /// Zero-based parameter index.
        index: usize,
        /// The parameter's declared type, as the signature spells it.
        declared: Type,
        /// The offending type: the declared type itself, or a type nested inside it.
        offender: Type,
    },
    /// A result's declared type, or a type nested in it, cannot be lowered.
    #[error(
        "result {index} of type `{declared}` cannot be lowered: `{offender}` has no Wasm C ABI \
         lowering"
    )]
    Result {
        /// Zero-based result index.
        index: usize,
        /// The result's declared type, as the signature spells it.
        declared: Type,
        /// The offending type: the declared type itself, or a type nested inside it.
        offender: Type,
    },
    /// The signature's calling convention has no Wasm C ABI lowering under this rule set.
    ///
    /// `ComponentModel` is a `call` target rather than an `exec` one, so it lands here
    /// unconditionally. `Wasm` has no aggregate types at all, so a `Wasm` signature carrying one
    /// lands here too. `C` is not rejected outright; its own rules are verified separately (see
    /// [`Self::CAggregateResult`] and [`Self::CAggregateParam`]).
    #[error("procedures with the `{0}` calling convention have no Wasm C ABI lowering")]
    CallingConvention(CallConv),
    /// A `C` procedure's result is an aggregate returned by value, which the convention forbids:
    /// aggregates must be returned through a leading `sret` pointer parameter, which a frontend
    /// producing a `@callconv("C")` procedure has already injected where needed.
    #[error(
        "result {index} of type `{ty}` is an aggregate returned by value, which the `C` \
         convention forbids: the producer must return it through a leading `sret` pointer \
         parameter"
    )]
    CAggregateResult {
        /// Zero-based result index.
        index: usize,
        /// The result's declared type.
        ty: Type,
    },
    /// A `C` procedure's struct or array parameter is larger than 64 bits, so it cannot be
    /// passed by value; the convention requires it be passed by reference instead.
    #[error(
        "parameter {index} of type `{ty}` ({size_in_bytes} bytes) is an aggregate larger than 64 \
         bits passed by value, which the `C` convention forbids: pass it by reference"
    )]
    CAggregateParam {
        /// Zero-based parameter index.
        index: usize,
        /// The parameter's declared type.
        ty: Type,
        /// The type's size in bytes ([`Type::size_in_bytes`]).
        size_in_bytes: usize,
    },
    /// The flattened arguments or results need more operand stack elements than the convention
    /// allows.
    #[error(
        "signature needs {params} operand stack elements for its arguments and {results} for its \
         results, but no more than {limit} are available for either"
    )]
    StackBudget {
        /// Elements the flattened arguments occupy.
        params: usize,
        /// Elements the flattened results occupy.
        results: usize,
        /// The limit that was exceeded, i.e. [`MAX_STACK_ELEMENTS`].
        limit: usize,
    },
}

/// Lower a Miden-side signature to its Wasm C ABI shape.
///
/// Parameters flatten in order (see [`flatten_type`]). Results flatten likewise and are returned
/// directly when there is exactly one scalar, through a trailing out pointer when there are two
/// or more, and not at all when there are none.
///
/// Three calling conventions have this shape:
///
/// * `Fast` passes every argument and result by value on the operand stack, which is exactly
///   what the flattening describes. It is accepted, subject to the operand stack budget (see
///   [`MAX_STACK_ELEMENTS`]).
/// * `Wasm` supports no aggregate types, so it is accepted only when flattening is the identity
///   for every parameter and result — each is already a scalar or a pointer. A signature with an
///   aggregate anywhere is rejected with [`UnsupportedSignature::CallingConvention`].
/// * `C` is accepted on the assumption that a frontend producing a `@callconv("C")` procedure has
///   already injected the `sret` pointer where needed; this rule set only verifies the
///   convention's own rules, then lowers exactly like `Fast`. Every result must be a scalar or a
///   pointer, never a struct or array by value ([`UnsupportedSignature::CAggregateResult`]
///   otherwise), and every struct or array parameter must be at most 64 bits
///   (`ty.size_in_bytes() <= 8`), a larger one having to be passed by reference instead
///   ([`UnsupportedSignature::CAggregateParam`] otherwise). Unlike `Fast`, `C` has no operand
///   stack budget: it spills excess arguments to the caller's frame instead of failing.
///
/// `ComponentModel` is rejected because it may only be the target of `call`, never `exec`.
pub fn lower_signature(sig: &FunctionType) -> Result<LoweredSignature, UnsupportedSignature> {
    let cc = sig.calling_convention();
    match cc {
        CallConv::Fast | CallConv::Wasm | CallConv::C => {}
        // `ComponentModel` may only be the target of `call`; an `Extern` convention is defined by
        // some other frontend, and this rule set knows nothing about its shape.
        CallConv::ComponentModel | CallConv::Extern(_) => {
            return Err(UnsupportedSignature::CallingConvention(cc));
        }
    }

    if cc == CallConv::C {
        for (index, ty) in sig.results().iter().enumerate() {
            if matches!(ty, Type::Struct(_) | Type::Array(_)) {
                return Err(UnsupportedSignature::CAggregateResult {
                    index,
                    ty: ty.clone(),
                });
            }
        }
        for (index, ty) in sig.params().iter().enumerate() {
            if matches!(ty, Type::Struct(_) | Type::Array(_)) {
                let size_in_bytes = ty.size_in_bytes();
                if size_in_bytes > 8 {
                    return Err(UnsupportedSignature::CAggregateParam {
                        index,
                        ty: ty.clone(),
                        size_in_bytes,
                    });
                }
            }
        }
    }

    // `Wasm` has no aggregates: every parameter and result must flatten to itself.
    let mut has_aggregate = false;
    let mut params = Vec::new();
    for (index, ty) in sig.params().iter().enumerate() {
        let flat = flatten_type(ty).map_err(|offender| UnsupportedSignature::Param {
            index,
            declared: ty.clone(),
            offender,
        })?;
        has_aggregate |= !is_flattening_identity(&flat);
        params.extend(flat.into_iter().map(|Flattened { path, scalar }| WasmParam {
            origin: index,
            path,
            scalar,
        }));
    }

    let mut results = Vec::new();
    for (index, ty) in sig.results().iter().enumerate() {
        let flat = flatten_type(ty).map_err(|offender| UnsupportedSignature::Result {
            index,
            declared: ty.clone(),
            offender,
        })?;
        has_aggregate |= !is_flattening_identity(&flat);
        results.extend(flat);
    }

    if cc == CallConv::Wasm && has_aggregate {
        return Err(UnsupportedSignature::CallingConvention(CallConv::Wasm));
    }

    if cc == CallConv::Fast {
        let param_elements: usize = params.iter().map(|p| p.scalar.stack_elements()).sum();
        let result_elements: usize = results.iter().map(|f| f.scalar.stack_elements()).sum();
        if param_elements > MAX_STACK_ELEMENTS || result_elements > MAX_STACK_ELEMENTS {
            return Err(UnsupportedSignature::StackBudget {
                params: param_elements,
                results: result_elements,
                limit: MAX_STACK_ELEMENTS,
            });
        }
    }

    let ret = if results.is_empty() {
        ReturnStrategy::Void
    } else if results.len() == 1 {
        ReturnStrategy::Direct(results.remove(0).scalar)
    } else {
        ReturnStrategy::OutPointer(results)
    };

    Ok(LoweredSignature {
        miden: sig.clone(),
        params,
        ret,
    })
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, sync::Arc, vec, vec::Vec};

    use miden_assembly_syntax::ast::types::{
        AddressSpace, ArrayType, PointerType, StructType, Type,
    };

    use super::*;

    fn word() -> Type {
        Type::Array(Arc::new(ArrayType {
            ty: Type::Felt,
            len: 4,
        }))
    }

    fn account_id() -> Type {
        Type::from(StructType::named("AccountId".into(), [Type::Felt, Type::Felt]))
    }

    fn scalars(flat: &[Flattened]) -> Vec<WasmScalar> {
        flat.iter().map(|f| f.scalar.clone()).collect()
    }

    #[test]
    fn scalars_flatten_to_themselves() {
        for (ty, scalar) in [
            (Type::I1, WasmScalar::I1),
            (Type::U8, WasmScalar::U8),
            (Type::U16, WasmScalar::U16),
            (Type::I32, WasmScalar::I32),
            (Type::U32, WasmScalar::U32),
            (Type::U64, WasmScalar::U64),
            (Type::I64, WasmScalar::I64),
            (Type::Felt, WasmScalar::Felt),
        ] {
            let flat = flatten_type(&ty).unwrap();
            assert_eq!(scalars(&flat), vec![scalar]);
            assert!(flat[0].path.is_empty());
        }
    }

    #[test]
    fn a_word_is_four_felts_in_order() {
        let flat = flatten_type(&word()).unwrap();
        assert_eq!(scalars(&flat), vec![WasmScalar::Felt; 4]);
        let indices: Vec<usize> = flat.iter().map(|f| f.path[0].index).collect();
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_struct_flattens_in_declared_field_order_with_names() {
        let asset = Type::from(StructType::named("Asset".into(), [word(), word()]));
        let flat = flatten_type(&asset).unwrap();
        assert_eq!(flat.len(), 8);
        // field 0 (id) then field 1 (value); each a word of four felts
        assert_eq!(flat[0].path[0].index, 0);
        assert_eq!(flat[3].path[0].index, 0);
        assert_eq!(flat[4].path[0].index, 1);
        assert_eq!(flat[7].path[1].index, 3);
        let two = flatten_type(&account_id()).unwrap();
        assert_eq!(scalars(&two), vec![WasmScalar::Felt, WasmScalar::Felt]);
    }

    #[test]
    fn pointers_keep_their_pointee_and_address_space() {
        let ptr = PointerType::new_with_address_space(Type::Felt, AddressSpace::Element);
        let flat = flatten_type(&Type::Ptr(Arc::new(ptr.clone()))).unwrap();
        assert_eq!(scalars(&flat), vec![WasmScalar::Ptr(Arc::new(ptr))]);
        assert_eq!(flat[0].scalar.frontend_type(), Type::I32);
    }

    #[test]
    fn unsupported_types_name_the_offender() {
        let bad = Type::from(StructType::named("Bad".into(), [Type::Felt, Type::U128]));
        assert_eq!(flatten_type(&bad), Err(Type::U128));
        assert_eq!(flatten_type(&Type::Unknown), Err(Type::Unknown));
        assert_eq!(flatten_type(&Type::F64), Err(Type::F64));
    }

    #[test]
    fn frontend_types_collapse_to_wasm_value_types() {
        assert_eq!(WasmScalar::I1.frontend_type(), Type::I32);
        assert_eq!(WasmScalar::U16.frontend_type(), Type::I32);
        assert_eq!(WasmScalar::U64.frontend_type(), Type::I64);
        assert_eq!(WasmScalar::Felt.frontend_type(), Type::Felt);
    }

    fn sig(
        params: impl IntoIterator<Item = Type>,
        results: impl IntoIterator<Item = Type>,
    ) -> FunctionType {
        FunctionType::new(CallConv::Fast, params, results)
    }

    #[test]
    fn a_single_scalar_result_returns_directly() {
        // `active_account::get_nonce() -> felt`
        let lowered = lower_signature(&sig([], [Type::Felt])).unwrap();
        assert!(lowered.params.is_empty());
        assert_eq!(lowered.ret, ReturnStrategy::Direct(WasmScalar::Felt));
        assert!(!lowered.has_out_pointer());
        let stub = lowered.stub_signature();
        assert_eq!(stub.calling_convention(), CallConv::Wasm);
        assert_eq!(stub.params(), &[] as &[Type]);
        assert_eq!(stub.results(), &[Type::Felt]);
    }

    #[test]
    fn two_or_more_result_scalars_go_through_an_out_pointer() {
        // `active_account::get_id() -> AccountId` (two felts)
        let lowered = lower_signature(&sig([], [account_id()])).unwrap();
        assert_eq!(lowered.ret.scalars(), vec![WasmScalar::Felt, WasmScalar::Felt]);
        assert!(lowered.has_out_pointer());
        let stub = lowered.stub_signature();
        assert_eq!(stub.params(), &[Type::I32], "the trailing out pointer is the only Wasm param");
        assert!(stub.results().is_empty());

        // `blake3::hash(u32 x 8) -> u32 x 8`
        let eight = core::iter::repeat_n(Type::U32, 8);
        let lowered = lower_signature(&sig(eight.clone(), eight)).unwrap();
        assert_eq!(lowered.params.len(), 8);
        assert!(matches!(&lowered.ret, ReturnStrategy::OutPointer(r) if r.len() == 8));
        assert_eq!(lowered.stub_signature().params().len(), 9);
    }

    #[test]
    fn no_results_is_void() {
        // `mem::pipe_preimage_to_memory(...) -> u32` is direct; a procedure with no results is void
        let lowered = lower_signature(&sig([Type::Felt, Type::U32], [])).unwrap();
        assert_eq!(lowered.ret, ReturnStrategy::Void);
        assert_eq!(lowered.stub_signature().params(), &[Type::Felt, Type::I32]);
        let direct = lower_signature(&sig([Type::Felt, Type::U32], [Type::U32])).unwrap();
        assert_eq!(direct.ret, ReturnStrategy::Direct(WasmScalar::U32));
    }

    #[test]
    fn aggregate_params_flatten_with_their_origin() {
        // `native_account::set_item(slot_id: StorageSlotId, value: word) -> word`
        let lowered = lower_signature(&sig([account_id(), word()], [word()])).unwrap();
        assert_eq!(lowered.params.len(), 6);
        assert!(lowered.params[..2].iter().all(|p| p.origin == 0));
        assert!(lowered.params[2..].iter().all(|p| p.origin == 1));
        assert_eq!(lowered.params[5].path[0].index, 3);
        assert_eq!(lowered.ret.scalars(), vec![WasmScalar::Felt; 4]);
        assert_eq!(lowered.miden, sig([account_id(), word()], [word()]));
        let stub = lowered.stub_signature();
        assert_eq!(
            stub.params(),
            &[
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::I32,
            ],
            "the six flattened felts are followed by the trailing out pointer"
        );
        assert!(stub.results().is_empty());
    }

    #[test]
    fn wide_integers_and_pointers_lower_to_their_wasm_carriers() {
        let ptr = Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::Felt,
            AddressSpace::Element,
        )));
        let lowered = lower_signature(&sig([Type::U64, ptr], [Type::U64])).unwrap();
        assert_eq!(lowered.stub_signature().params(), &[Type::I64, Type::I32]);
        assert_eq!(lowered.stub_signature().results(), &[Type::I64]);
    }

    #[test]
    fn unsupported_signatures_say_what_and_where() {
        let err = lower_signature(&sig([Type::Felt, Type::U128], [])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::Param {
                index: 1,
                declared: Type::U128,
                offender: Type::U128
            }
        );
        let err = lower_signature(&sig([], [Type::Felt, Type::F64])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::Result {
                index: 1,
                declared: Type::F64,
                offender: Type::F64
            }
        );
    }

    #[test]
    fn only_fast_wasm_and_c_signatures_lower() {
        // `Fast` passes everything by value on the operand stack: aggregates included.
        lower_signature(&FunctionType::new(CallConv::Fast, [word()], [account_id()])).unwrap();
        // `Wasm` has no aggregate types, so it lowers only when flattening is the identity.
        lower_signature(&FunctionType::new(CallConv::Wasm, [Type::Felt, Type::U64], [Type::I32]))
            .unwrap();
        assert_eq!(
            lower_signature(&FunctionType::new(CallConv::Wasm, [word()], [])).unwrap_err(),
            UnsupportedSignature::CallingConvention(CallConv::Wasm),
            "a `word` parameter is an aggregate, which the Wasm convention cannot express"
        );
        assert_eq!(
            lower_signature(&FunctionType::new(CallConv::Wasm, [], [account_id()])).unwrap_err(),
            UnsupportedSignature::CallingConvention(CallConv::Wasm),
            "an aggregate result is rejected just the same"
        );
        // `C` lowers too (see the `c_*` tests below); only `ComponentModel` is rejected outright,
        // because it may only be the target of `call`, never `exec`.
        assert_eq!(
            lower_signature(&FunctionType::new(CallConv::ComponentModel, [Type::Felt], []))
                .unwrap_err(),
            UnsupportedSignature::CallingConvention(CallConv::ComponentModel)
        );
    }

    #[test]
    fn c_accepts_scalar_params_and_a_scalar_result() {
        // A frontend producing `@callconv("C")` has already injected any `sret` pointer needed,
        // so a scalar-only signature is accepted outright.
        let lowered =
            lower_signature(&FunctionType::new(CallConv::C, [Type::Felt, Type::U32], [Type::Felt]))
                .unwrap();
        assert_eq!(lowered.params.len(), 2);
        assert_eq!(lowered.ret, ReturnStrategy::Direct(WasmScalar::Felt));
    }

    #[test]
    fn c_rejects_an_aggregate_result() {
        let err = lower_signature(&FunctionType::new(CallConv::C, [], [word()])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::CAggregateResult {
                index: 0,
                ty: word()
            }
        );
        assert_eq!(
            err.to_string(),
            alloc::format!(
                "result 0 of type `{}` is an aggregate returned by value, which the `C` \
                 convention forbids: the producer must return it through a leading `sret` pointer \
                 parameter",
                word()
            )
        );
    }

    #[test]
    fn c_accepts_a_small_aggregate_param_flattened_to_its_fields() {
        // `AccountId` is two felts: exactly 64 bits, the boundary the `C` convention allows by
        // value.
        let lowered = lower_signature(&FunctionType::new(CallConv::C, [account_id()], [])).unwrap();
        let params: Vec<WasmScalar> = lowered.params.iter().map(|p| p.scalar.clone()).collect();
        assert_eq!(params, vec![WasmScalar::Felt, WasmScalar::Felt]);
        assert!(lowered.params.iter().all(|p| p.origin == 0));
    }

    #[test]
    fn c_rejects_an_aggregate_param_larger_than_64_bits() {
        let err = lower_signature(&FunctionType::new(CallConv::C, [word()], [])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::CAggregateParam {
                index: 0,
                ty: word(),
                size_in_bytes: 16
            }
        );
        assert_eq!(
            err.to_string(),
            alloc::format!(
                "parameter 0 of type `{}` (16 bytes) is an aggregate larger than 64 bits passed \
                 by value, which the `C` convention forbids: pass it by reference",
                word()
            )
        );
    }

    #[test]
    fn c_accepts_a_pointer_to_a_word_param() {
        // A pointer is never a struct or array, whatever it points to, so the size check never
        // applies to it.
        let ptr = Type::Ptr(Arc::new(PointerType::new(word())));
        lower_signature(&FunctionType::new(CallConv::C, [ptr], [])).unwrap();
    }

    #[test]
    fn c_has_no_operand_stack_budget() {
        // `C` spills excess arguments to the caller's frame instead of failing, so this must
        // lower even though `Fast` would reject it (see `the_operand_stack_budget_caps_a_fast_signature`).
        let felts = core::iter::repeat_n(Type::Felt, 17);
        lower_signature(&FunctionType::new(CallConv::C, felts, [])).unwrap();
    }

    #[test]
    fn unsupported_nested_types_name_the_declared_type_and_the_inner_offender() {
        let bad_param = Type::from(StructType::named("Bad".into(), [Type::Felt, Type::U128]));
        let err = lower_signature(&sig([bad_param.clone()], [])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::Param {
                index: 0,
                declared: bad_param.clone(),
                offender: Type::U128
            }
        );
        assert_eq!(
            err.to_string(),
            alloc::format!(
                "parameter 0 of type `{bad_param}` cannot be lowered: `u128` has no Wasm C ABI \
                 lowering"
            )
        );

        let bad_result = Type::from(StructType::named("Bad".into(), [Type::F64]));
        let err = lower_signature(&sig([], [bad_result.clone()])).unwrap_err();
        assert_eq!(
            err,
            UnsupportedSignature::Result {
                index: 0,
                declared: bad_result.clone(),
                offender: Type::F64
            }
        );
        assert_eq!(
            err.to_string(),
            alloc::format!(
                "result 0 of type `{bad_result}` cannot be lowered: `f64` has no Wasm C ABI \
                 lowering"
            )
        );
    }

    #[test]
    fn a_single_field_struct_result_returns_directly() {
        let wrapper = Type::from(StructType::named("Wrapper".into(), [Type::Felt]));
        let lowered = lower_signature(&sig([], [wrapper])).unwrap();
        assert_eq!(lowered.ret, ReturnStrategy::Direct(WasmScalar::Felt));
        assert_eq!(lowered.stub_signature().results(), &[Type::Felt]);
    }

    #[test]
    fn a_zero_sized_type_contributes_no_scalars() {
        let empty = Type::from(StructType::named("Empty".into(), core::iter::empty::<Type>()));
        assert!(flatten_type(&empty).unwrap().is_empty());

        // As a parameter it contributes nothing, and the parameters after it keep their origins.
        let lowered = lower_signature(&sig([empty.clone(), Type::Felt], [])).unwrap();
        assert_eq!(lowered.params.len(), 1);
        assert_eq!(
            lowered.params[0].origin, 1,
            "origins index the Miden signature, not the Wasm one"
        );
        assert_eq!(lowered.stub_signature().params(), &[Type::Felt]);

        // As the sole result there is nothing to return.
        let lowered = lower_signature(&sig([], [empty])).unwrap();
        assert_eq!(lowered.ret, ReturnStrategy::Void);
        assert!(lowered.stub_signature().results().is_empty());
        assert!(lowered.import_signature().results().is_empty());
    }

    #[test]
    fn miden_types_invert_the_flattening_of_a_scalar() {
        for (ty, scalar) in [
            (Type::I1, WasmScalar::I1),
            (Type::I8, WasmScalar::I8),
            (Type::U8, WasmScalar::U8),
            (Type::I16, WasmScalar::I16),
            (Type::U16, WasmScalar::U16),
            (Type::I32, WasmScalar::I32),
            (Type::U32, WasmScalar::U32),
            (Type::I64, WasmScalar::I64),
            (Type::U64, WasmScalar::U64),
            (Type::Felt, WasmScalar::Felt),
        ] {
            assert_eq!(scalar.miden_type(), ty);
            assert_eq!(flatten_type(&ty).unwrap()[0].scalar, scalar);
        }
        let ptr = Arc::new(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element));
        assert_eq!(WasmScalar::Ptr(ptr.clone()).miden_type(), Type::Ptr(ptr));
    }

    #[test]
    fn the_import_signature_is_the_flattened_miden_side_callee() {
        // `native_account::set_item(slot_id: StorageSlotId, value: word) -> word`
        let lowered = lower_signature(&sig([account_id(), word()], [word()])).unwrap();
        let import = lowered.import_signature();
        assert_eq!(import.calling_convention(), CallConv::Wasm);
        assert_eq!(
            import.params(),
            vec![Type::Felt; 6].as_slice(),
            "the flattened parameters, with no out pointer"
        );
        assert_eq!(
            import.results(),
            vec![Type::Felt; 4].as_slice(),
            "the flattened results, returned by value"
        );

        // Narrow integers keep their width on the Miden side but widen to `i32` in the stub.
        let lowered = lower_signature(&sig([], [Type::U16])).unwrap();
        assert_eq!(lowered.import_signature().results(), &[Type::U16]);
        assert_eq!(lowered.stub_signature().results(), &[Type::I32]);

        let lowered = lower_signature(&sig([Type::Felt, Type::U32], [])).unwrap();
        assert_eq!(lowered.import_signature().params(), &[Type::Felt, Type::U32]);
        assert!(lowered.import_signature().results().is_empty());
    }

    #[test]
    fn out_pointer_results_keep_the_field_path_that_reaches_each_scalar() {
        // `active_account::get_id() -> AccountId`: the wrapper reads field 0 then field 1 back
        // out of the return area, so the strategy has to name them.
        let lowered = lower_signature(&sig([], [account_id()])).unwrap();
        let ReturnStrategy::OutPointer(results) = &lowered.ret else {
            panic!("two result scalars go through an out pointer, got {:?}", lowered.ret);
        };
        let indices: Vec<usize> = results.iter().map(|f| f.path[0].index).collect();
        assert_eq!(indices, vec![0, 1]);

        // A nested aggregate keeps the whole path, not just the outermost step.
        let pair_of_words = Type::from(StructType::named("PairOfWords".into(), [word(), word()]));
        let lowered = lower_signature(&sig([], [pair_of_words])).unwrap();
        let ReturnStrategy::OutPointer(results) = &lowered.ret else {
            panic!("eight result scalars go through an out pointer, got {:?}", lowered.ret);
        };
        assert_eq!(results.len(), 8);
        let last: Vec<usize> = results[7].path.iter().map(|step| step.index).collect();
        assert_eq!(last, vec![1, 3], "field 1 of the struct, element 3 of that word");
    }

    #[test]
    fn scalars_flattens_a_return_strategy_for_consumers_that_only_want_the_scalars() {
        assert!(ReturnStrategy::Void.scalars().is_empty());
        assert_eq!(
            ReturnStrategy::Direct(WasmScalar::U16).scalars(),
            vec![WasmScalar::U16],
            "a direct result is its one scalar"
        );
        let lowered = lower_signature(&sig([], [account_id()])).unwrap();
        assert_eq!(lowered.ret.scalars(), vec![WasmScalar::Felt, WasmScalar::Felt]);
    }

    #[test]
    fn the_operand_stack_budget_caps_a_fast_signature() {
        let felts = |n: usize| core::iter::repeat_n(Type::Felt, n);
        assert_eq!(MAX_STACK_ELEMENTS, 16);
        // `blake3::merge` shape: sixteen elements in, eight out, exactly at the limit.
        lower_signature(&sig(felts(16), felts(8))).unwrap();
        assert_eq!(
            lower_signature(&sig(felts(17), [])).unwrap_err(),
            UnsupportedSignature::StackBudget {
                params: 17,
                results: 0,
                limit: MAX_STACK_ELEMENTS
            }
        );
        assert_eq!(
            lower_signature(&sig([], felts(17))).unwrap_err(),
            UnsupportedSignature::StackBudget {
                params: 0,
                results: 17,
                limit: MAX_STACK_ELEMENTS
            }
        );
        // A 64-bit integer occupies two elements, so nine of them overrun a budget that nine
        // felts would not.
        let u64s = |n: usize| core::iter::repeat_n(Type::U64, n);
        lower_signature(&sig(u64s(8), [])).unwrap();
        assert_eq!(
            lower_signature(&sig(u64s(9), [])).unwrap_err(),
            UnsupportedSignature::StackBudget {
                params: 18,
                results: 0,
                limit: MAX_STACK_ELEMENTS
            }
        );
        assert_eq!(
            lower_signature(&sig([], u64s(9))).unwrap_err(),
            UnsupportedSignature::StackBudget {
                params: 0,
                results: 18,
                limit: MAX_STACK_ELEMENTS
            }
        );
        // Aggregates count as the elements they flatten to: four words fit, a fifth felt does not.
        lower_signature(&sig([word(), word(), word(), word()], [])).unwrap();
        assert_eq!(
            lower_signature(&sig([word(), word(), word(), word(), Type::Felt], [])).unwrap_err(),
            UnsupportedSignature::StackBudget {
                params: 17,
                results: 0,
                limit: MAX_STACK_ELEMENTS
            }
        );
    }

    /// Rows transcribed from `frontend/wasm/src/miden_abi/{stdlib,tx_kernel}` (signatures) and
    /// `transform.rs` (strategies). `true` means `ReturnViaPointer`, `false` means `NoTransform`.
    /// Each group's source comment gives a path relative to `frontend/wasm/src/miden_abi/`.
    /// Every procedure `get_transform_strategy` recognizes is covered here except
    /// `tx::execute_foreign_procedure_indirect`, whose `FpiIndirectReturnViaPointer` strategy is a
    /// compiler intrinsic (raw FPI executor ABI), out of scope for this rule set.
    #[test]
    fn the_rule_set_reproduces_the_hand_written_strategy_table() {
        let felt = || Type::Felt;
        let i32_ = || Type::I32;
        let rows: &[(&str, Vec<Type>, Vec<Type>, bool)] = &[
            // mem (stdlib/mem.rs)
            (
                "mem::pipe_words_to_memory",
                vec![felt(), i32_()],
                vec![
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    i32_(),
                ],
                true,
            ),
            (
                "mem::pipe_double_words_to_memory",
                vec![
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    i32_(),
                    i32_(),
                ],
                vec![
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    i32_(),
                ],
                true,
            ),
            (
                "mem::pipe_preimage_to_memory",
                vec![felt(), i32_(), felt(), felt(), felt(), felt()],
                vec![i32_()],
                false,
            ),
            // crypto::hashes::blake3 (stdlib/crypto/hashes/blake3.rs)
            ("blake3::hash", vec![i32_(); 8], vec![i32_(); 8], true),
            ("blake3::merge", vec![i32_(); 16], vec![i32_(); 8], true),
            // crypto::hashes::sha256 (stdlib/crypto/hashes/sha256.rs)
            ("sha256::hash", vec![i32_(); 8], vec![i32_(); 8], true),
            ("sha256::merge", vec![i32_(); 16], vec![i32_(); 8], true),
            // crypto::hashes::poseidon2 (stdlib/crypto/hashes/poseidon2.rs)
            ("poseidon2::hash_elements", vec![i32_(); 2], vec![felt(); 4], true),
            ("poseidon2::hash_words", vec![i32_(); 2], vec![felt(); 4], true),
            ("poseidon2::merge", vec![felt(); 8], vec![felt(); 4], true),
            // crypto::dsa::rpo_falcon512 (stdlib/crypto/dsa/rpo_falcon512.rs)
            ("rpo_falcon512::verify", vec![felt(); 8], vec![], false),
            // collections::smt (stdlib/collections/smt.rs)
            ("smt::get", vec![felt(); 8], vec![felt(); 8], true),
            ("smt::set", vec![felt(); 12], vec![felt(); 8], true),
            // native_account (tx_kernel/native_account.rs)
            ("native_account::add_asset", vec![felt(); 8], vec![felt(); 4], true),
            ("native_account::remove_asset", vec![felt(); 8], vec![felt(); 4], true),
            ("native_account::get_id", vec![], vec![felt(); 2], true),
            ("native_account::compute_delta_commitment", vec![], vec![felt(); 4], true),
            ("native_account::set_item", vec![felt(); 6], vec![felt(); 4], true),
            ("native_account::set_map_item", vec![felt(); 10], vec![felt(); 4], true),
            ("native_account::incr_nonce", vec![], vec![felt()], false),
            ("native_account::was_procedure_called", vec![felt(); 4], vec![felt()], false),
            ("native_account::get_initial_commitment", vec![], vec![felt(); 4], true),
            ("native_account::get_initial_storage_commitment", vec![], vec![felt(); 4], true),
            ("native_account::get_initial_vault_root", vec![], vec![felt(); 4], true),
            ("native_account::get_initial_asset", vec![felt(); 4], vec![felt(); 4], true),
            ("native_account::get_initial_item", vec![felt(); 2], vec![felt(); 4], true),
            ("native_account::get_initial_map_item", vec![felt(); 6], vec![felt(); 4], true),
            // note (tx_kernel/note.rs)
            (
                "note::compute_and_store_recipient",
                vec![
                    i32_(),
                    i32_(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                    felt(),
                ],
                vec![felt(); 4],
                true,
            ),
            ("note::compute_storage_commitment", vec![i32_(); 2], vec![felt(); 4], true),
            (
                "note::write_attachment_commitments_to_memory",
                vec![felt(), felt(), felt(), felt(), i32_()],
                vec![i32_()],
                false,
            ),
            (
                "note::write_attachment_to_memory",
                vec![felt(), felt(), felt(), felt(), i32_()],
                vec![i32_()],
                false,
            ),
            (
                "note::write_indexed_attachment_to_memory",
                vec![felt(), i32_(), felt(), i32_()],
                vec![i32_()],
                false,
            ),
            ("note::compute_recipient", vec![felt(); 12], vec![felt(); 4], true),
            ("note::metadata_into_sender", vec![felt(); 4], vec![felt(); 2], true),
            ("note::metadata_into_attachment_schemes", vec![felt(); 4], vec![felt(); 4], true),
            ("note::metadata_into_note_type", vec![felt(); 4], vec![felt()], false),
            ("note::metadata_into_tag", vec![felt(); 4], vec![felt()], false),
            ("note::find_attachment_idx", vec![felt(); 5], vec![felt(); 2], true),
            // active_account (tx_kernel/active_account.rs)
            ("active_account::get_id", vec![], vec![felt(); 2], true),
            ("active_account::get_nonce", vec![], vec![felt()], false),
            ("active_account::get_code_commitment", vec![], vec![felt(); 4], true),
            ("active_account::compute_commitment", vec![], vec![felt(); 4], true),
            ("active_account::compute_storage_commitment", vec![], vec![felt(); 4], true),
            ("active_account::get_item", vec![felt(); 2], vec![felt(); 4], true),
            ("active_account::get_map_item", vec![felt(); 6], vec![felt(); 4], true),
            ("active_account::get_asset", vec![felt(); 4], vec![felt(); 4], true),
            ("active_account::has_asset", vec![felt(); 4], vec![felt()], false),
            ("active_account::get_vault_root", vec![], vec![felt(); 4], true),
            ("active_account::get_num_procedures", vec![], vec![felt()], false),
            ("active_account::get_procedure_root", vec![felt()], vec![felt(); 4], true),
            ("active_account::has_procedure", vec![felt(); 4], vec![felt()], false),
            // faucet (tx_kernel/faucet.rs)
            ("faucet::mint", vec![felt(); 8], vec![], false),
            ("faucet::burn", vec![felt(); 8], vec![], false),
            // active_note (tx_kernel/active_note.rs)
            ("active_note::get_storage", vec![i32_()], vec![i32_()], false),
            ("active_note::get_initial_assets", vec![i32_()], vec![i32_()], false),
            ("active_note::get_sender", vec![], vec![felt(); 2], true),
            ("active_note::get_recipient", vec![], vec![felt(); 4], true),
            ("active_note::get_script_root", vec![], vec![felt(); 4], true),
            ("active_note::get_serial_number", vec![], vec![felt(); 4], true),
            ("active_note::get_metadata", vec![], vec![felt(); 4], true),
            ("active_note::is_public", vec![], vec![felt()], false),
            ("active_note::is_private", vec![], vec![felt()], false),
            ("active_note::get_attachments_commitment", vec![], vec![felt(); 4], true),
            (
                "active_note::write_attachment_commitments_to_memory",
                vec![i32_()],
                vec![i32_()],
                false,
            ),
            (
                "active_note::write_attachment_to_memory",
                vec![i32_(), felt()],
                vec![i32_()],
                false,
            ),
            ("active_note::find_attachment", vec![felt()], vec![felt(); 2], true),
            // input_note (tx_kernel/input_note.rs)
            ("input_note::get_initial_assets_info", vec![felt()], vec![felt(); 5], true),
            ("input_note::get_initial_assets", vec![i32_(), felt()], vec![i32_()], false),
            ("input_note::get_recipient", vec![felt()], vec![felt(); 4], true),
            ("input_note::get_metadata", vec![felt()], vec![felt(); 4], true),
            ("input_note::get_sender", vec![felt()], vec![felt(); 2], true),
            ("input_note::get_storage_info", vec![felt()], vec![felt(); 5], true),
            ("input_note::get_script_root", vec![felt()], vec![felt(); 4], true),
            ("input_note::get_serial_number", vec![felt()], vec![felt(); 4], true),
            ("input_note::get_attachments_commitment", vec![felt()], vec![felt(); 4], true),
            (
                "input_note::get_attachments_commitment_raw",
                vec![felt(); 2],
                vec![felt(); 4],
                true,
            ),
            (
                "input_note::write_attachment_commitments_to_memory",
                vec![i32_(), felt()],
                vec![i32_()],
                false,
            ),
            (
                "input_note::write_attachment_to_memory",
                vec![i32_(), felt(), felt()],
                vec![i32_()],
                false,
            ),
            ("input_note::find_attachment", vec![felt(); 2], vec![felt(); 2], true),
            // output_note (tx_kernel/output_note.rs)
            ("output_note::create", vec![felt(); 6], vec![felt()], false),
            ("output_note::add_asset", vec![felt(); 9], vec![], false),
            ("output_note::add_attachment", vec![felt(); 6], vec![], false),
            ("output_note::add_word_attachment", vec![felt(); 6], vec![], false),
            (
                "output_note::add_attachment_from_memory",
                vec![felt(), i32_(), i32_(), felt()],
                vec![],
                false,
            ),
            ("output_note::get_assets_info", vec![felt()], vec![felt(); 5], true),
            ("output_note::get_assets", vec![i32_(), felt()], vec![i32_()], false),
            ("output_note::get_attachments_commitment", vec![felt()], vec![felt(); 4], true),
            ("output_note::get_recipient", vec![felt()], vec![felt(); 4], true),
            ("output_note::get_metadata", vec![felt()], vec![felt(); 4], true),
            ("output_note::find_attachment", vec![felt(); 2], vec![felt(); 2], true),
            (
                "output_note::write_attachment_commitments_to_memory",
                vec![i32_(), felt()],
                vec![i32_()],
                false,
            ),
            (
                "output_note::write_attachment_to_memory",
                vec![i32_(), felt(), felt()],
                vec![i32_()],
                false,
            ),
            // tx (tx_kernel/tx.rs)
            // execute_foreign_procedure_indirect is skipped: FpiIndirectReturnViaPointer is the
            // raw FPI compiler intrinsic, out of scope for this rule set.
            ("tx::get_block_number", vec![], vec![felt()], false),
            ("tx::get_block_commitment", vec![], vec![felt(); 4], true),
            ("tx::get_block_timestamp", vec![], vec![felt()], false),
            ("tx::get_input_notes_commitment", vec![], vec![felt(); 4], true),
            ("tx::get_output_notes_commitment", vec![], vec![felt(); 4], true),
            ("tx::get_num_input_notes", vec![], vec![felt()], false),
            ("tx::get_num_output_notes", vec![], vec![felt()], false),
            ("tx::get_expiration_block_delta", vec![], vec![felt()], false),
            ("tx::update_expiration_block_delta", vec![felt()], vec![], false),
            ("tx::get_tx_script_root", vec![], vec![felt(); 4], true),
        ];
        for (name, params, results, via_pointer) in rows {
            let lowered = lower_signature(&sig(params.clone(), results.clone()))
                .unwrap_or_else(|err| panic!("{name}: {err}"));
            assert_eq!(
                lowered.has_out_pointer(),
                *via_pointer,
                "{name}: rule set disagrees with the hand-written table"
            );
        }
    }
}
