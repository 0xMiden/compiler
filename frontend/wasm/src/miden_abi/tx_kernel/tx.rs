use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Tx),
];

pub const GET_BLOCK_NUMBER: &str = "get_block_number";
pub const GET_BLOCK_COMMITMENT: &str = "get_block_commitment";
pub const GET_BLOCK_TIMESTAMP: &str = "get_block_timestamp";
pub const GET_INPUT_NOTES_COMMITMENT: &str = "get_input_notes_commitment";
pub const GET_OUTPUT_NOTES_COMMITMENT: &str = "get_output_notes_commitment";
pub const GET_NUM_INPUT_NOTES: &str = "get_num_input_notes";
pub const GET_NUM_OUTPUT_NOTES: &str = "get_num_output_notes";
pub const GET_EXPIRATION_BLOCK_DELTA: &str = "get_expiration_block_delta";
pub const UPDATE_EXPIRATION_BLOCK_DELTA: &str = "update_expiration_block_delta";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut tx: FunctionTypeMap = Default::default();
    tx.insert(Symbol::from(GET_BLOCK_NUMBER), FunctionType::new(CallConv::Wasm, [], [Felt]));
    tx.insert(
        Symbol::from(GET_BLOCK_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    tx.insert(Symbol::from(GET_BLOCK_TIMESTAMP), FunctionType::new(CallConv::Wasm, [], [Felt]));
    tx.insert(
        Symbol::from(GET_INPUT_NOTES_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    tx.insert(
        Symbol::from(GET_OUTPUT_NOTES_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    tx.insert(Symbol::from(GET_NUM_INPUT_NOTES), FunctionType::new(CallConv::Wasm, [], [Felt]));
    tx.insert(
        Symbol::from(GET_NUM_OUTPUT_NOTES),
        FunctionType::new(CallConv::Wasm, [], [Felt]),
    );
    tx.insert(
        Symbol::from(GET_EXPIRATION_BLOCK_DELTA),
        FunctionType::new(CallConv::Wasm, [], [Felt]),
    );
    tx.insert(
        Symbol::from(UPDATE_EXPIRATION_BLOCK_DELTA),
        FunctionType::new(CallConv::Wasm, [Felt], []),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), tx);
    m
}
