use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Std),
    SymbolNameComponent::Component(symbols::Crypto),
    SymbolNameComponent::Component(symbols::Hashes),
    SymbolNameComponent::Component(symbols::Blake3),
];

pub(crate) const HASH_1TO1: &str = "hash_1to1";
pub(crate) const HASH_2TO1: &str = "hash_2to1";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut blake3: FunctionTypeMap = Default::default();
    blake3.insert(
        Symbol::from(HASH_1TO1),
        FunctionType::new(
            CallConv::Wasm,
            [I32, I32, I32, I32, I32, I32, I32, I32],
            [I32, I32, I32, I32, I32, I32, I32, I32],
        ),
    );
    blake3.insert(
        Symbol::from(HASH_2TO1),
        FunctionType::new(
            CallConv::Wasm,
            [I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32],
            [I32, I32, I32, I32, I32, I32, I32, I32],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), blake3);
    m
}
