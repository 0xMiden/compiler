pub(crate) mod stdlib;
pub(crate) mod transform;
pub(crate) mod tx_kernel;

use midenc_hir::{FunctionType, FxHashMap, SymbolNameComponent, SymbolPath, interner::Symbol};
use midenc_hir_symbol::symbols;

pub(crate) type FunctionTypeMap = FxHashMap<Symbol, FunctionType>;
pub(crate) type ModuleFunctionTypeMap = FxHashMap<SymbolPath, FunctionTypeMap>;

pub fn is_miden_abi_module(path: &SymbolPath) -> bool {
    let module_path = path.without_leaf();
    is_miden_stdlib_module(&module_path) || is_miden_sdk_module(&module_path)
}

fn is_miden_sdk_module(module_path: &SymbolPath) -> bool {
    tx_kernel::signatures().contains_key(module_path)
}

fn is_miden_stdlib_module(module_path: &SymbolPath) -> bool {
    stdlib::signatures().contains_key(module_path)
}

pub fn miden_abi_function_type(path: &SymbolPath) -> FunctionType {
    try_miden_abi_function_type(path)
        .unwrap_or_else(|| panic!("No Miden ABI function type found for function {path}"))
}

pub fn try_miden_abi_function_type(path: &SymbolPath) -> Option<FunctionType> {
    const STD: &[SymbolNameComponent] = &[
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Core),
    ];

    if path.is_prefixed_by(STD) {
        try_miden_stdlib_function_type(path)
    } else {
        try_miden_sdk_function_type(path)
    }
}

/// Get the target Miden ABI tx kernel function type for the given module and function id
pub fn miden_sdk_function_type(path: &SymbolPath) -> FunctionType {
    try_miden_sdk_function_type(path)
        .unwrap_or_else(|| panic!("No Miden ABI function type found for function {path}"))
}

fn try_miden_sdk_function_type(path: &SymbolPath) -> Option<FunctionType> {
    let module_path = path.without_leaf();
    let funcs = tx_kernel::signatures().get(module_path.as_ref())?;
    funcs.get(&path.name()).cloned()
}

fn try_miden_stdlib_function_type(path: &SymbolPath) -> Option<FunctionType> {
    let module_path = path.without_leaf();
    let funcs = stdlib::signatures().get(module_path.as_ref())?;
    funcs.get(&path.name()).cloned()
}
