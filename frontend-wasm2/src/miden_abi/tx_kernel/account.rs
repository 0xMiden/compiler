use midenc_hir::FunctionType;
use midenc_hir_type::Type::*;

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::account";

pub const ADD_ASSET: &str = "add_asset";
pub const REMOVE_ASSET: &str = "remove_asset";
pub const GET_ID: &str = "get_id";

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
    m.insert(MODULE_ID, account);
    m
}
