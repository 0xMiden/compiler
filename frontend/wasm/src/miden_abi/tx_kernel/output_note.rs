use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::output_note";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::OutputNote),
];

pub const CREATE: &str = "create";
pub const ADD_ASSET: &str = "add_asset";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut output_note: FunctionTypeMap = Default::default();
    output_note.insert(
        Symbol::from(CREATE),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // tag
                Felt, // aux
                Felt, // note_type
                Felt, // execution hint
                Felt, Felt, Felt, Felt, // recipient components
            ],
            [Felt],
        ),
    );
    output_note.insert(
        Symbol::from(ADD_ASSET),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // asset components
                Felt, // note_idx
            ],
            [],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), output_note);
    m
}
