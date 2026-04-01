use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Protocol),
        SymbolNameComponent::Component(symbols::Faucet),
    ];
    SymbolPath::from_iter(parts)
}

pub const CREATE_FUNGIBLE_ASSET: &str = "create_fungible_asset";
pub const CREATE_NON_FUNGIBLE_ASSET: &str = "create_non_fungible_asset";
pub const MINT: &str = "mint";
pub const BURN: &str = "burn";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(CREATE_FUNGIBLE_ASSET),
        FunctionType::new(
            CallConv::Wasm,
            [Felt],
            [
                Felt, Felt, Felt, Felt, // ASSET_KEY
                Felt, Felt, Felt, Felt, // ASSET_VALUE
            ],
        ),
    );
    funcs.insert(
        Symbol::from(CREATE_NON_FUNGIBLE_ASSET),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt],
            [
                Felt, Felt, Felt, Felt, // ASSET_KEY
                Felt, Felt, Felt, Felt, // ASSET_VALUE
            ],
        ),
    );
    funcs.insert(
        Symbol::from(MINT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // ASSET_KEY
                Felt, Felt, Felt, Felt, // ASSET_VALUE
            ],
            [Felt, Felt, Felt, Felt],
        ),
    );
    funcs.insert(
        Symbol::from(BURN),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // ASSET_KEY
                Felt, Felt, Felt, Felt, // ASSET_VALUE
            ],
            [Felt, Felt, Felt, Felt],
        ),
    );
    m.insert(module_path(), funcs);
    m
}
