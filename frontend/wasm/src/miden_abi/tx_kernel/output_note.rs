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
pub const ADD_ATTACHMENT: &str = "add_attachment";
pub const ADD_WORD_ATTACHMENT: &str = "add_word_attachment";
pub const ADD_ATTACHMENT_FROM_MEMORY: &str = "add_attachment_from_memory";
pub const GET_ASSETS_INFO: &str = "get_assets_info";
pub const GET_ASSETS: &str = "get_assets";
pub const GET_ATTACHMENTS_COMMITMENT: &str = "get_attachments_commitment";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_METADATA: &str = "get_metadata";
pub const FIND_ATTACHMENT: &str = "find_attachment";
pub const WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY: &str = "write_attachment_commitments_to_memory";
pub const WRITE_ATTACHMENT_TO_MEMORY: &str = "write_attachment_to_memory";

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
                Felt, Felt, Felt, Felt, // asset key
                Felt, Felt, Felt, Felt, // asset value
                Felt, // note_idx
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(ADD_ATTACHMENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // attachment_scheme
                Felt, Felt, Felt, Felt, // attachment commitment
                Felt, // note_idx
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(ADD_WORD_ATTACHMENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // attachment_scheme
                Felt, Felt, Felt, Felt, // attachment word
                Felt, // note_idx
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(ADD_ATTACHMENT_FROM_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // attachment_scheme
                I32,  // num_words
                I32,  // attachment_ptr
                Felt, // note_idx
            ],
            [],
        ),
    );
    output_note.insert(
        Symbol::from(GET_ASSETS_INFO),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt, Felt]),
    );
    output_note
        .insert(Symbol::from(GET_ASSETS), FunctionType::new(CallConv::Wasm, [I32, Felt], [I32]));
    output_note.insert(
        Symbol::from(GET_ATTACHMENTS_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(GET_RECIPIENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(GET_METADATA),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(FIND_ATTACHMENT),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt, Felt]),
    );
    output_note.insert(
        Symbol::from(WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32]),
    );
    output_note.insert(
        Symbol::from(WRITE_ATTACHMENT_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, Felt, Felt], [I32]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), output_note);
    m
}
