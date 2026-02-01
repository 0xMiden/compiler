use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Asset),
    ];
    SymbolPath::from_iter(parts)
}

pub const BUILD_FUNGIBLE_ASSET: &str = "build_fungible_asset";
pub const BUILD_NON_FUNGIBLE_ASSET: &str = "build_non_fungible_asset";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(BUILD_FUNGIBLE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(BUILD_NON_FUNGIBLE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    m.insert(module_path(), funcs);
    m
}
