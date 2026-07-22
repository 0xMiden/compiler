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
    SymbolNameComponent::Component(symbols::ActiveAccount),
];

pub const GET_ID: &str = "get_id";
pub const GET_NONCE: &str = "get_nonce";
pub const GET_CODE_COMMITMENT: &str = "get_code_commitment";
pub const COMPUTE_COMMITMENT: &str = "compute_commitment";
pub const COMPUTE_STORAGE_COMMITMENT: &str = "compute_storage_commitment";
pub const GET_STORAGE_ITEM: &str = "get_item";
pub const GET_STORAGE_MAP_ITEM: &str = "get_map_item";
pub const GET_ASSET: &str = "get_asset";
pub const HAS_ASSET: &str = "has_asset";
pub const GET_VAULT_ROOT: &str = "get_vault_root";
pub const GET_NUM_PROCEDURES: &str = "get_num_procedures";
pub const GET_PROCEDURE_ROOT: &str = "get_procedure_root";
pub const HAS_PROCEDURE: &str = "has_procedure";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();

    let mut active_account: FunctionTypeMap = Default::default();
    active_account
        .insert(Symbol::from(GET_ID), FunctionType::new(CallConv::Wasm, [], [Felt, Felt]));
    active_account.insert(Symbol::from(GET_NONCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    active_account.insert(
        Symbol::from(GET_CODE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(COMPUTE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(COMPUTE_STORAGE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_STORAGE_ITEM),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(GET_STORAGE_MAP_ITEM),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt],
        ),
    );
    active_account.insert(
        Symbol::from(GET_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    active_account.insert(
        Symbol::from(HAS_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
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
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), active_account);

    m
}
