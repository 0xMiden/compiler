use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::{Felt, I32},
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const HASH_ELEMENTS: &str = "hash_elements";
pub const HASH_WORDS: &str = "hash_words";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut rpo: FunctionTypeMap = Default::default();
    // hash_elements takes (ptr: u32, num_elements: u32) and returns 4 Felt values on the stack
    rpo.insert(
        Symbol::from(HASH_ELEMENTS),
        FunctionType::new(CallConv::Wasm, [I32, I32], [Felt, Felt, Felt, Felt]),
    );
    // hash_words takes (start_addr: u32, end_addr: u32) and returns 4 Felt values on the stack
    rpo.insert(
        Symbol::from(HASH_WORDS),
        FunctionType::new(CallConv::Wasm, [I32, I32], [Felt, Felt, Felt, Felt]),
    );

    let module_path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Core),
        SymbolNameComponent::Component(symbols::Crypto),
        SymbolNameComponent::Component(symbols::Hashes),
        SymbolNameComponent::Component(symbols::Rpo256),
    ]);
    m.insert(module_path, rpo);
    m
}
