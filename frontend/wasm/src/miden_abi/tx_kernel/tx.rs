use midenc_hir::{
    interner::{symbols, Symbol},
    FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::tx";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Tx),
];

pub const CREATE_NOTE: &str = "create_note";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(
        Symbol::from(CREATE_NOTE),
        FunctionType::new([Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt], [Felt]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), note);
    m
}
