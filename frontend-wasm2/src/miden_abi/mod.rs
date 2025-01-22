pub(crate) mod stdlib;
pub(crate) mod transform;
pub(crate) mod tx_kernel;

use midenc_hir::{FunctionIdent, FunctionType, FxHashMap, Ident, Symbol};
use tx_kernel::note;

use crate::intrinsics;

pub(crate) type FunctionTypeMap = FxHashMap<&'static str, FunctionType>;
pub(crate) type ModuleFunctionTypeMap = FxHashMap<&'static str, FunctionTypeMap>;

pub fn is_miden_abi_module(module_id: Symbol) -> bool {
    is_miden_stdlib_module(module_id) || is_miden_sdk_module(module_id)
}

pub fn miden_abi_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    if is_miden_stdlib_module(module_id) {
        miden_stdlib_function_type(module_id, function_id)
    } else {
        miden_sdk_function_type(module_id, function_id)
    }
}

fn is_miden_sdk_module(module_id: Symbol) -> bool {
    tx_kernel::signatures().contains_key(module_id.as_str())
}

/// Get the target Miden ABI tx kernel function type for the given module and function id
pub fn miden_sdk_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    let funcs = tx_kernel::signatures()
        .get(module_id.as_str())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {}", module_id));
    funcs.get(function_id.as_str()).cloned().unwrap_or_else(|| {
        panic!(
            "No Miden ABI function type found for function {} in module {}",
            function_id, module_id
        )
    })
}

fn is_miden_stdlib_module(module_id: Symbol) -> bool {
    stdlib::signatures().contains_key(module_id.as_str())
}

/// Get the target Miden ABI stdlib function type for the given module and function id
#[inline(always)]
fn miden_stdlib_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    let funcs = stdlib::signatures()
        .get(module_id.as_str())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {}", module_id));
    funcs.get(function_id.as_str()).cloned().unwrap_or_else(|| {
        panic!(
            "No Miden ABI function type found for function {} in module {}",
            function_id, module_id
        )
    })
}

/// Restore module and function names of the intrinsics and Miden SDK functions
/// that were renamed to satisfy the Wasm Component Model requirements.
///
/// Returns the pre-renamed (expected at the linking stage) module and function
/// names or given `wasm_module_id` and `wasm_function_id` ids if the function
/// is not an intrinsic or Miden SDK function
pub fn recover_imported_masm_function_id(
    wasm_module_id: &str,
    wasm_function_id: &str,
) -> FunctionIdent {
    // Hard-coding is error-prone.
    // See better option suggested in https://github.com/0xPolygonMiden/compiler/issues/342
    let module_id = if wasm_module_id.starts_with("miden:core-import/intrinsics-mem") {
        intrinsics::mem::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/intrinsics-felt") {
        intrinsics::felt::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/account") {
        tx_kernel::account::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/note") {
        note::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/tx") {
        tx_kernel::tx::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-mem") {
        stdlib::mem::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-dsa-rpo-falcon") {
        stdlib::crypto::dsa::rpo_falcon::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-hashes-blake3") {
        stdlib::crypto::hashes::blake3::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import") {
        panic!(
            "unrecovered intrinsics or Miden SDK import module ID: {wasm_module_id}, function: \
             {wasm_function_id}"
        )
    } else {
        // Unrecognized module ID, return as is
        return FunctionIdent {
            module: Ident::from(wasm_module_id),
            function: Ident::from(wasm_function_id),
        };
    };
    // Since `hash-1to1` is an invalid name in Wasm CM (dashed part cannot start with a digit),
    // we need to translate the CM name to the one that is expected at the linking stage
    let function_id = if wasm_function_id == "hash-one-to-one" {
        "hash_1to1".to_string()
    } else if wasm_function_id == "hash-two-to-one" {
        "hash_2to1".to_string()
    } else {
        wasm_function_id.replace("-", "_")
    };
    FunctionIdent {
        module: Ident::from(module_id),
        function: Ident::from(function_id.as_str()),
    }
}
