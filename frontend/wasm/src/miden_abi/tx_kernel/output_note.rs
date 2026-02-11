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
    SymbolNameComponent::Component(symbols::OutputNote),
];

pub const CREATE: &str = "create";
pub const ADD_ASSET: &str = "add_asset";
pub const SET_ATTACHMENT: &str = "set_attachment";
pub const SET_WORD_ATTACHMENT: &str = "set_word_attachment";
pub const SET_ARRAY_ATTACHMENT: &str = "set_array_attachment";
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
                Felt, // note_type
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
        Symbol::from(SET_ATTACHMENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // note_idx
                Felt, // attachment_scheme
                Felt, // attachment_kind
                Felt, Felt, Felt, Felt, // attachment word
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(SET_WORD_ATTACHMENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // note_idx
                Felt, // attachment_scheme
                Felt, Felt, Felt, Felt, // attachment word
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(SET_ARRAY_ATTACHMENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // note_idx
                Felt, // attachment_scheme
                Felt, Felt, Felt, Felt, // attachment commitment
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
        FunctionType::new(
            CallConv::Wasm,
            [Felt],
            [
                Felt, Felt, Felt, Felt, // NOTE_ATTACHMENT
                Felt, Felt, Felt, Felt, // METADATA_HEADER
            ],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), output_note);
    m
}
