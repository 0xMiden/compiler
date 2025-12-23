use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::faucet";

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(Symbol::intern("faucet")),
    ];
    SymbolPath::from_iter(parts)
}

pub const CREATE_FUNGIBLE_ASSET: &str = "create_fungible_asset";
pub const CREATE_NON_FUNGIBLE_ASSET: &str = "create_non_fungible_asset";
pub const MINT: &str = "mint";
pub const BURN: &str = "burn";
pub const GET_TOTAL_ISSUANCE: &str = "get_total_issuance";
pub const IS_NON_FUNGIBLE_ASSET_ISSUED: &str = "is_non_fungible_asset_issued";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(CREATE_FUNGIBLE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(CREATE_NON_FUNGIBLE_ASSET),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(MINT),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(BURN),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(Symbol::from(GET_TOTAL_ISSUANCE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    funcs.insert(
        Symbol::from(IS_NON_FUNGIBLE_ASSET_ISSUED),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    m.insert(module_path(), funcs);
    m
}
