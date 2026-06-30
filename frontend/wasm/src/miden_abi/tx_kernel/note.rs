use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const COMPUTE_AND_STORE_RECIPIENT: &str = "compute_and_store_recipient";
pub const COMPUTE_STORAGE_COMMITMENT: &str = "compute_storage_commitment";
pub const WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY: &str = "write_attachment_commitments_to_memory";
pub const WRITE_ATTACHMENT_TO_MEMORY: &str = "write_attachment_to_memory";
pub const WRITE_INDEXED_ATTACHMENT_TO_MEMORY: &str = "write_indexed_attachment_to_memory";
pub const COMPUTE_RECIPIENT: &str = "compute_recipient";
pub const METADATA_INTO_SENDER: &str = "metadata_into_sender";
pub const METADATA_INTO_ATTACHMENT_SCHEMES: &str = "metadata_into_attachment_schemes";
pub const METADATA_INTO_NOTE_TYPE: &str = "metadata_into_note_type";
pub const METADATA_INTO_TAG: &str = "metadata_into_tag";
pub const FIND_ATTACHMENT_IDX: &str = "find_attachment_idx";

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Protocol),
        SymbolNameComponent::Component(symbols::Note),
    ];
    SymbolPath::from_iter(parts)
}

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(
        Symbol::from(COMPUTE_AND_STORE_RECIPIENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                I32, // storage_ptr (Miden element address)
                I32, // num_storage_items
                Felt, Felt, Felt, Felt, // serial_num
                Felt, Felt, Felt, Felt, // script_root
            ],
            [Felt, Felt, Felt, Felt],
        ),
    );
    note.insert(
        Symbol::from(COMPUTE_STORAGE_COMMITMENT),
        FunctionType::new(CallConv::Wasm, [I32, I32], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, I32], [I32, I32]),
    );
    note.insert(
        Symbol::from(WRITE_ATTACHMENT_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, I32], [I32, I32]),
    );
    note.insert(
        Symbol::from(WRITE_INDEXED_ATTACHMENT_TO_MEMORY),
        FunctionType::new(CallConv::Wasm, [Felt, I32, Felt, I32], [I32, I32]),
    );
    note.insert(
        Symbol::from(COMPUTE_RECIPIENT),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // serial_num
                Felt, Felt, Felt, Felt, // script_root
                Felt, Felt, Felt, Felt, // storage_commitment
            ],
            [Felt, Felt, Felt, Felt],
        ),
    );
    note.insert(
        Symbol::from(METADATA_INTO_SENDER),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt]),
    );
    note.insert(
        Symbol::from(METADATA_INTO_ATTACHMENT_SCHEMES),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt, Felt, Felt, Felt]),
    );
    note.insert(
        Symbol::from(METADATA_INTO_NOTE_TYPE),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    note.insert(
        Symbol::from(METADATA_INTO_TAG),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt], [Felt]),
    );
    note.insert(
        Symbol::from(FIND_ATTACHMENT_IDX),
        FunctionType::new(CallConv::Wasm, [Felt, Felt, Felt, Felt, Felt], [Felt, Felt]),
    );
    m.insert(module_path(), note);
    m
}
