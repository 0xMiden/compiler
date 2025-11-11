use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::tx";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Tx),
];

pub const GET_BLOCK_NUMBER: &str = "get_block_number";
pub const GET_INPUT_NOTES_COMMITMENT: &str = "get_input_notes_commitment";
pub const GET_OUTPUT_NOTES_COMMITMENT: &str = "get_output_notes_commitment";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut tx: FunctionTypeMap = Default::default();
    tx.insert(Symbol::from(GET_BLOCK_NUMBER), FunctionType::new(CallConv::Wasm, [], [Felt]));
    tx.insert(
        Symbol::from(GET_INPUT_NOTES_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    tx.insert(
        Symbol::from(GET_OUTPUT_NOTES_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), tx);
    m
}
