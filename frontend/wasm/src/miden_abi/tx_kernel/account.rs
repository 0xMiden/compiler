use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::account";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Account),
];

pub const ADD_ASSET: &str = "add_asset";
pub const REMOVE_ASSET: &str = "remove_asset";
pub const GET_ID: &str = "get_id";
pub const GET_NONCE: &str = "get_nonce";
pub const GET_INITIAL_COMMITMENT: &str = "get_initial_commitment";
pub const COMPUTE_CURRENT_COMMITMENT: &str = "compute_current_commitment";
pub const COMPUTE_DELTA_COMMITMENT: &str = "compute_delta_commitment";
pub const GET_STORAGE_ITEM: &str = "get_item";
pub const SET_STORAGE_ITEM: &str = "set_item";
pub const GET_STORAGE_MAP_ITEM: &str = "get_map_item";
pub const SET_STORAGE_MAP_ITEM: &str = "set_map_item";
pub const INCR_NONCE: &str = "incr_nonce";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut account: FunctionTypeMap = Default::default();
    account.insert(
        Symbol::from(ADD_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(REMOVE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(Symbol::from(GET_ID), FunctionType::new(CallConv::Wasm, [], [Felt, Felt]));
    account.insert(Symbol::from(GET_NONCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    account.insert(
        Symbol::from(GET_INITIAL_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(COMPUTE_CURRENT_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(COMPUTE_DELTA_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(GET_STORAGE_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(SET_STORAGE_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    account.insert(
        Symbol::from(GET_STORAGE_MAP_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        Symbol::from(SET_STORAGE_MAP_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    account.insert(Symbol::from(INCR_NONCE), FunctionType::new(CallConv::Wasm, [Felt], []));
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), account);
    m
}
