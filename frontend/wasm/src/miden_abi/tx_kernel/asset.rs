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
    SymbolNameComponent::Component(symbols::Asset),
];

pub const ID_INTO_FAUCET_ID: &str = "id_into_faucet_id";
pub const ID_INTO_ASSET_CLASS: &str = "id_into_asset_class";
pub const ID_INTO_COMPOSITION: &str = "id_into_composition";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut asset: FunctionTypeMap = Default::default();
    asset.insert(
        Symbol::from(ID_INTO_FAUCET_ID),
        // ASSET_ID -> faucet_id_suffix, faucet_id_prefix
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt]),
    );
    asset.insert(
        Symbol::from(ID_INTO_ASSET_CLASS),
        // ASSET_ID -> asset_class_suffix, asset_class_prefix
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt]),
    );
    asset.insert(
        Symbol::from(ID_INTO_COMPOSITION),
        // ASSET_ID -> asset_composition
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), asset);
    m
}
