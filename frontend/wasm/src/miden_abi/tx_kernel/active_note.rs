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
    SymbolNameComponent::Component(symbols::ActiveNote),
];

/// Writes the active note's inputs ("storage" in protocol v0.14+) to memory and returns the count.
pub const GET_STORAGE: &str = "get_storage";
pub const GET_ASSETS: &str = "get_assets";
pub const GET_SENDER: &str = "get_sender";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_SCRIPT_ROOT: &str = "get_script_root";
pub const GET_SERIAL_NUMBER: &str = "get_serial_number";
pub const GET_METADATA: &str = "get_metadata";
pub const IS_PUBLIC: &str = "is_public";
pub const IS_PRIVATE: &str = "is_private";
pub const GET_ATTACHMENTS_COMMITMENT: &str = "get_attachments_commitment";
pub const WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY: &str = "write_attachment_commitments_to_memory";
pub const WRITE_ATTACHMENT_TO_MEMORY: &str = "write_attachment_to_memory";
pub const FIND_ATTACHMENT: &str = "find_attachment";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(Symbol::from(GET_STORAGE), FunctionType::new(CallConv::Wasm, [I32], [I32]));
    note.insert(Symbol::from(GET_ASSETS), FunctionType::new(CallConv::Wasm, [I32], [I32]));
    note.insert(Symbol::from(GET_SENDER), FunctionType::new(CallConv::Wasm, [], [Felt, Felt]));
    note.insert(
        Symbol::from(GET_RECIPIENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(GET_SCRIPT_ROOT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(GET_SERIAL_NUMBER),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(GET_METADATA),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    note.insert(Symbol::from(IS_PUBLIC), FunctionType::new(CallConv::Wasm, [], [Felt]));
    note.insert(Symbol::from(IS_PRIVATE), FunctionType::new(CallConv::Wasm, [], [Felt]));
    note.insert(
        Symbol::from(GET_ATTACHMENTS_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32], [I32]),
    );
    note.insert(
        Symbol::from(WRITE_ATTACHMENT_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32]),
    );
    note.insert(
        Symbol::from(FIND_ATTACHMENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt]),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), note);
    m
}
