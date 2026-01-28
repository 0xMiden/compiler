use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::OutputNote),
];

pub const CREATE: &str = "create";
pub const ADD_ASSET: &str = "add_asset";
pub const GET_ASSETS_INFO: &str = "get_assets_info";
pub const GET_ASSETS: &str = "get_assets";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_METADATA: &str = "get_metadata";

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
    output_note.insert(
        Symbol::from(GET_ASSETS_INFO),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(GET_ASSETS),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32, I32]),
    );
    output_note.insert(
        Symbol::from(GET_RECIPIENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(GET_METADATA),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), output_note);
    m
}
