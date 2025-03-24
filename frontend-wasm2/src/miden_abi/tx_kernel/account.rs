use midenc_hir_type::{FunctionType, Type::*};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::account";

pub const ADD_ASSET: &str = "add_asset";
pub const REMOVE_ASSET: &str = "remove_asset";
pub const GET_ID: &str = "get_id";
pub const GET_STORAGE_ITEM: &str = "get_storage_item";
pub const SET_STORAGE_ITEM: &str = "set_storage_item";
pub const GET_STORAGE_MAP_ITEM: &str = "get_storage_map_item";
pub const SET_STORAGE_MAP_ITEM: &str = "set_storage_map_item";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut account: FunctionTypeMap = Default::default();
    account
        .insert(ADD_ASSET, FunctionType::new([Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]));
    account.insert(
        REMOVE_ASSET,
        FunctionType::new([Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(GET_ID, FunctionType::new([], [Felt]));
    account.insert(GET_STORAGE_ITEM, FunctionType::new([Felt], [Felt, Felt, Felt, Felt]));
    account.insert(
        SET_STORAGE_ITEM,
        FunctionType::new(
            [Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    account.insert(
        GET_STORAGE_MAP_ITEM,
        FunctionType::new([Felt, Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    account.insert(
        SET_STORAGE_MAP_ITEM,
        FunctionType::new(
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    m.insert(MODULE_ID, account);
    m
}
