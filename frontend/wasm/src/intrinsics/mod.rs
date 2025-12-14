mod intrinsic;

pub use self::intrinsic::*;

pub mod advice;
pub mod crypto;
pub mod debug;
pub mod felt;
pub mod mem;

use midenc_hir::{Builder, SmallVec, SourceSpan, ValueRef, dialects::builtin::FunctionRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

/// Convert a call to a Miden intrinsic function into instruction(s)
pub fn convert_intrinsics_call<B: ?Sized + Builder>(
    intrinsic: Intrinsic,
    function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match intrinsic {
        Intrinsic::Debug(function) => {
            debug::convert_debug_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Mem(function) => {
            mem::convert_mem_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Felt(function) => {
            felt::convert_felt_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Crypto(function) => {
            crypto::convert_crypto_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Advice(function) => {
            advice::convert_advice_intrinsics(function, function_ref, args, builder, span)
        }
    }
}
