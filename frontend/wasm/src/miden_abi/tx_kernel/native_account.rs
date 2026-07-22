use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Protocol),
    SymbolNameComponent::Component(symbols::NativeAccount),
];

pub const ADD_ASSET: &str = "add_asset";
pub const REMOVE_ASSET: &str = "remove_asset";
pub const GET_ID: &str = "get_id";
pub const COMPUTE_DELTA_COMMITMENT: &str = "compute_delta_commitment";
pub const SET_STORAGE_ITEM: &str = "set_item";
pub const SET_STORAGE_MAP_ITEM: &str = "set_map_item";
pub const INCR_NONCE: &str = "incr_nonce";
pub const WAS_PROCEDURE_CALLED: &str = "was_procedure_called";
pub const GET_INITIAL_COMMITMENT: &str = "get_initial_commitment";
pub const GET_INITIAL_STORAGE_COMMITMENT: &str = "get_initial_storage_commitment";
pub const GET_INITIAL_VAULT_ROOT: &str = "get_initial_vault_root";
pub const GET_INITIAL_ASSET: &str = "get_initial_asset";
pub const GET_INITIAL_STORAGE_ITEM: &str = "get_initial_item";
pub const GET_INITIAL_STORAGE_MAP_ITEM: &str = "get_initial_map_item";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();

    let mut native_account: FunctionTypeMap = Default::default();
    native_account.insert(
        Symbol::from(ADD_ASSET),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // asset key
                Felt, Felt, Felt, Felt, // asset value
            ],
            [Felt, Felt, Felt, Felt], // asset value
        ),
    );
    native_account.insert(
        Symbol::from(REMOVE_ASSET),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // ASSET_KEY
                Felt, Felt, Felt, Felt, // ASSET_VALUE
            ],
            [Felt, Felt, Felt, Felt],
        ),
    );
    native_account
        .insert(Symbol::from(GET_ID), FunctionType::new(CallConv::Wasm, [], [Felt, Felt]));
    native_account.insert(
        Symbol::from(COMPUTE_DELTA_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(SET_STORAGE_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, // slot_id_suffix, slot_id_prefix
                Felt, Felt, Felt, Felt, // value components
            ],
            [Felt, Felt, Felt, Felt], // old value
        ),
    );
    native_account.insert(
        Symbol::from(SET_STORAGE_MAP_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, // slot_id_suffix, slot_id_prefix
                Felt, Felt, Felt, Felt, // key components
                Felt, Felt, Felt, Felt, // value components
            ],
            [Felt, Felt, Felt, Felt], // old value
        ),
    );
    native_account.insert(Symbol::from(INCR_NONCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    native_account.insert(
        Symbol::from(WAS_PROCEDURE_CALLED),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_VAULT_ROOT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_MAP_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), native_account);

    m
}
