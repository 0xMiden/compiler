use midenc_hir::{
    interner::{symbols, Symbol},
    FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_ID: &str = "std::crypto::dsa::rpo_falcon";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Std),
    SymbolNameComponent::Component(symbols::Crypto),
    SymbolNameComponent::Component(symbols::Dsa),
    SymbolNameComponent::Component(symbols::RpoFalcon),
];

pub(crate) const RPO_FALCON512_VERIFY: &str = "rpo_falcon512_verify";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(RPO_FALCON512_VERIFY),
        FunctionType::new([Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt], []),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), funcs);
    m
}
