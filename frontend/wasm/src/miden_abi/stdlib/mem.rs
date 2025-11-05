use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_ID: &str = "std::mem";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Std),
    SymbolNameComponent::Component(symbols::Mem),
];

pub(crate) const PIPE_WORDS_TO_MEMORY: &str = "pipe_words_to_memory";
pub(crate) const PIPE_DOUBLE_WORDS_TO_MEMORY: &str = "pipe_double_words_to_memory";
pub(crate) const PIPE_PREIMAGE_TO_MEMORY: &str = "pipe_preimage_to_memory";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(PIPE_WORDS_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // num_words
                I32,  // write_ptr
            ],
            [
                Felt, Felt, Felt, Felt, // HASH
                I32,  // write_ptr'
            ],
        ),
    );
    funcs.insert(
        Symbol::from(PIPE_DOUBLE_WORDS_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // C
                Felt, Felt, Felt, Felt, // B
                Felt, Felt, Felt, Felt, // A
                I32,  // write_ptr
                I32,  // end_ptr
            ],
            [
                Felt, Felt, Felt, Felt, // C
                Felt, Felt, Felt, Felt, // B
                Felt, Felt, Felt, Felt, // A
                I32,  // write_ptr
            ],
        ),
    );
    funcs.insert(
        Symbol::from(PIPE_PREIMAGE_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // num_words
                I32,  // write_ptr
                Felt, Felt, Felt, Felt, // COM (commitment)
            ],
            [
                I32, // write_ptr'
            ],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), funcs);
    m
}
