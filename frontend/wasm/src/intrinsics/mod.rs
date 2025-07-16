mod intrinsic;

pub use self::intrinsic::*;

pub mod crypto;
pub mod debug;
pub mod felt;
pub mod mem;

use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{FunctionRef, ModuleBuilder, WorldBuilder},
    Builder, FxHashSet, Signature, SmallVec, SourceSpan, SymbolPath, ValueRef,
};
use midenc_hir_symbol::sync::LazyLock;

use crate::{
    callable::CallableFunction, error::WasmResult, module::function_builder_ext::FunctionBuilderExt,
};

fn modules() -> &'static FxHashSet<SymbolPath> {
    static MODULES: LazyLock<FxHashSet<SymbolPath>> = LazyLock::new(|| {
        let mut s = FxHashSet::default();
        s.insert(SymbolPath::from_iter(mem::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(felt::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(debug::MODULE_PREFIX.iter().copied()));
        s.insert(SymbolPath::from_iter(crypto::MODULE_PREFIX.iter().copied()));
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
    }
}

/// Returns [`CallableFunction`] for a given intrinsics in core Wasm module imports
pub fn process_intrinsics_import(
    world_builder: &mut WorldBuilder,
    intrinsic: Intrinsic,
    signature: Signature,
) -> CallableFunction {
    let conversion_result = intrinsic.conversion_result().expect("unknown intrinsic");
    if conversion_result.is_operation() {
        return CallableFunction::Instruction {
            intrinsic,
            signature,
        };
    }

    // This intrinsic function will be defined further down the pipeline.
    //
    // We are declaring it now, creating the module hierarchy as needed.
    let path = intrinsic.into_symbol_path();
    let module_path = path.without_leaf();
    let intrinsic_module = world_builder
        .declare_module_tree(&module_path)
        .unwrap_or_else(|err| panic!("{err}"));

    let mut intrinsic_module_builder = ModuleBuilder::new(intrinsic_module);
    let function_ref = intrinsic_module_builder
        .define_function(intrinsic.function_name().into(), signature.clone())
        .wrap_err("failed to declare function")
        .unwrap_or_else(|err| panic!("{err}"));

    CallableFunction::Intrinsic {
        intrinsic,
        function_ref,
        signature,
    }
}
