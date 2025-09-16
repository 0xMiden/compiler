use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_ID: &str = "std::crypto::dsa::rpo_falcon512";

pub(crate) const RPO_FALCON512_VERIFY: &str = "verify";

fn module_path() -> SymbolPath {
    // Build 'std::crypto::dsa::rpo_falcon512' without relying on a predeclared symbol
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Std),
        SymbolNameComponent::Component(symbols::Crypto),
        SymbolNameComponent::Component(symbols::Dsa),
        SymbolNameComponent::Component(Symbol::from("rpo_falcon512")),
    ];
    SymbolPath::from_iter(parts)
}

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(RPO_FALCON512_VERIFY),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt], []),
    );
    m.insert(module_path(), funcs);
    m
}
