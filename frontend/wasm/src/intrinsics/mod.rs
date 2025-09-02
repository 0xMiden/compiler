mod intrinsic;

pub use self::intrinsic::*;

pub mod advice;
pub mod crypto;
pub mod debug;
pub mod felt;
pub mod mem;

use midenc_hir::{
    dialects::builtin::FunctionRef, Builder, FxHashSet, SmallVec, SourceSpan, SymbolPath, ValueRef,
};
use midenc_hir_symbol::sync::LazyLock;

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

fn modules() -> &'static FxHashSet<SymbolPath> {
    static MODULES: LazyLock<FxHashSet<SymbolPath>> = LazyLock::new(|| {
        let mut s = FxHashSet::default();
        s.insert(SymbolPath::from_iter(mem::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(felt::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(debug::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(crypto::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(advice::MODULE_PREFIX.iter().copied()));
        s
    });
    &MODULES
}

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
