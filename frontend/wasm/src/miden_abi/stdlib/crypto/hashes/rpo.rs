use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::{Felt, I32},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "std::crypto::hashes::rpo";

pub const HASH_MEMORY: &str = "hash_memory";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut rpo: FunctionTypeMap = Default::default();
    // hash_memory takes (ptr: u32, num_elements: u32) and returns 4 Felt values on the stack
    rpo.insert(
        Symbol::from(HASH_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, I32], [Felt, Felt, Felt, Felt]),
    );

    let module_path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Std),
        SymbolNameComponent::Component(symbols::Crypto),
        SymbolNameComponent::Component(symbols::Hashes),
        SymbolNameComponent::Component(Symbol::intern("rpo")),
    ]);
    m.insert(module_path, rpo);
    m
}
