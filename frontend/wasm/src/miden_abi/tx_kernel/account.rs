use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const NATIVE_MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::NativeAccount),
];

pub(crate) const ACTIVE_MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::ActiveAccount),
];

pub const ADD_ASSET: &str = "add_asset";
pub const REMOVE_ASSET: &str = "remove_asset";
pub const GET_ID: &str = "get_id";
pub const GET_NONCE: &str = "get_nonce";
pub const GET_INITIAL_COMMITMENT: &str = "get_initial_commitment";
pub const GET_CODE_COMMITMENT: &str = "get_code_commitment";
pub const COMPUTE_COMMITMENT: &str = "compute_commitment";
pub const COMPUTE_DELTA_COMMITMENT: &str = "compute_delta_commitment";
pub const GET_INITIAL_STORAGE_COMMITMENT: &str = "get_initial_storage_commitment";
pub const COMPUTE_STORAGE_COMMITMENT: &str = "compute_storage_commitment";
pub const GET_STORAGE_ITEM: &str = "get_item";
pub const GET_INITIAL_STORAGE_ITEM: &str = "get_initial_item";
pub const SET_STORAGE_ITEM: &str = "set_item";
pub const GET_STORAGE_MAP_ITEM: &str = "get_map_item";
pub const GET_INITIAL_STORAGE_MAP_ITEM: &str = "get_initial_map_item";
pub const SET_STORAGE_MAP_ITEM: &str = "set_map_item";
pub const INCR_NONCE: &str = "incr_nonce";
pub const GET_BALANCE: &str = "get_balance";
pub const GET_INITIAL_BALANCE: &str = "get_initial_balance";
pub const HAS_NON_FUNGIBLE_ASSET: &str = "has_non_fungible_asset";
pub const GET_INITIAL_VAULT_ROOT: &str = "get_initial_vault_root";
pub const GET_VAULT_ROOT: &str = "get_vault_root";
pub const GET_NUM_PROCEDURES: &str = "get_num_procedures";
pub const GET_PROCEDURE_ROOT: &str = "get_procedure_root";
pub const HAS_PROCEDURE: &str = "has_procedure";
pub const WAS_PROCEDURE_CALLED: &str = "was_procedure_called";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();

    let mut native_account: FunctionTypeMap = Default::default();
    native_account.insert(
        Symbol::from(ADD_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(REMOVE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(COMPUTE_DELTA_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    native_account.insert(
        Symbol::from(SET_STORAGE_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    native_account.insert(
        Symbol::from(SET_STORAGE_MAP_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    native_account.insert(Symbol::from(INCR_NONCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    native_account.insert(
        Symbol::from(WAS_PROCEDURE_CALLED),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    m.insert(SymbolPath::from_iter(NATIVE_MODULE_PREFIX.iter().copied()), native_account);

    let mut active_account: FunctionTypeMap = Default::default();
    active_account
        .insert(Symbol::from(GET_ID), FunctionType::new(CallConv::Wasm, [], [Felt, Felt]));
    active_account.insert(Symbol::from(GET_NONCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    active_account.insert(
        Symbol::from(GET_INITIAL_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_CODE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(COMPUTE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(COMPUTE_STORAGE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_STORAGE_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_STORAGE_MAP_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_INITIAL_STORAGE_MAP_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_BALANCE),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt]),
    );
    active_account.insert(
        Symbol::from(GET_INITIAL_BALANCE),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt]),
    );
    active_account.insert(
        Symbol::from(HAS_NON_FUNGIBLE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    active_account.insert(
        Symbol::from(GET_INITIAL_VAULT_ROOT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_VAULT_ROOT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account
        .insert(Symbol::from(GET_NUM_PROCEDURES), FunctionType::new(CallConv::Wasm, [], [Felt]));
    active_account.insert(
        Symbol::from(GET_PROCEDURE_ROOT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(HAS_PROCEDURE),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    m.insert(SymbolPath::from_iter(ACTIVE_MODULE_PREFIX.iter().copied()), active_account);

    m
}
