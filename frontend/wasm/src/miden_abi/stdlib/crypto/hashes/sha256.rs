use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const HASH: &str = "hash";
pub const MERGE: &str = "merge";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut sha256: FunctionTypeMap = Default::default();
    sha256.insert(
        Symbol::from(HASH),
        FunctionType::new(
            CallConv::Wasm,
            [I32, I32, I32, I32, I32, I32, I32, I32],
            [I32, I32, I32, I32, I32, I32, I32, I32],
        ),
    );
    sha256.insert(
        Symbol::from(MERGE),
        FunctionType::new(
            CallConv::Wasm,
            [I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32],
            [I32, I32, I32, I32, I32, I32, I32, I32],
        ),
    );

    let module_path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Core),
        SymbolNameComponent::Component(symbols::Crypto),
        SymbolNameComponent::Component(symbols::Hashes),
        SymbolNameComponent::Component(symbols::Sha256),
    ]);
    m.insert(module_path, sha256);
    m
}
