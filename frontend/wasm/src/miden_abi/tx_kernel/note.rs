use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::note";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Note),
];

pub const GET_INPUTS: &str = "get_inputs";
pub const GET_ASSETS: &str = "get_assets";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(Symbol::from(GET_INPUTS), FunctionType::new(CallConv::Wasm, [I32], [I32, I32]));
    note.insert(Symbol::from(GET_ASSETS), FunctionType::new(CallConv::Wasm, [I32], [I32, I32]));
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), note);
    m
}
