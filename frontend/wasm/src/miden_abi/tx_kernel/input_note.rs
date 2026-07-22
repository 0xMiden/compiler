use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Protocol),
        SymbolNameComponent::Component(symbols::InputNote),
    ];
    SymbolPath::from_iter(parts)
}

pub const GET_INITIAL_ASSETS_INFO: &str = "get_initial_assets_info";
pub const GET_INITIAL_ASSETS: &str = "get_initial_assets";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_METADATA: &str = "get_metadata";
pub const GET_SENDER: &str = "get_sender";
pub const GET_STORAGE_INFO: &str = "get_storage_info";
pub const GET_SCRIPT_ROOT: &str = "get_script_root";
pub const GET_SERIAL_NUMBER: &str = "get_serial_number";
pub const GET_ATTACHMENTS_COMMITMENT: &str = "get_attachments_commitment";
pub const GET_ATTACHMENTS_COMMITMENT_RAW: &str = "get_attachments_commitment_raw";
pub const WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY: &str = "write_attachment_commitments_to_memory";
pub const WRITE_ATTACHMENT_TO_MEMORY: &str = "write_attachment_to_memory";
pub const FIND_ATTACHMENT: &str = "find_attachment";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(GET_INITIAL_ASSETS_INFO),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_INITIAL_ASSETS),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32]),
    );
    funcs.insert(
        Symbol::from(GET_RECIPIENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_METADATA),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_SENDER),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_STORAGE_INFO),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_SCRIPT_ROOT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_SERIAL_NUMBER),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_ATTACHMENTS_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_ATTACHMENTS_COMMITMENT_RAW),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32]),
    );
    funcs.insert(
        Symbol::from(WRITE_ATTACHMENT_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [I32, Felt, Felt], [I32]),
    );
    funcs.insert(
        Symbol::from(FIND_ATTACHMENT),
        FunctionType::new(CallConv::Wasm, [Felt, Felt], [Felt, Felt]),
    );
    m.insert(module_path(), funcs);
    m
}
