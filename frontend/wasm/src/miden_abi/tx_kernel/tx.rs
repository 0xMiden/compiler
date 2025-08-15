use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
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
pub const ADD_ASSET_TO_NOTE: &str = "add_asset_to_note";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(
        Symbol::from(CREATE_NOTE),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // tag
                Felt, // aux
                Felt, // note_type
                Felt, // execution-hint
                // recipient (4 felts)
                Felt, Felt, Felt, Felt,
            ],
            [Felt],
        ),
    );
    note.insert(
        Symbol::from(ADD_ASSET_TO_NOTE),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // asset (4 felts)
                Felt, // note_idx
            ],
            [
                Felt, Felt, Felt, Felt, // asset (4 felts)
                Felt, // note_idx
            ],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), note);
    m
}
