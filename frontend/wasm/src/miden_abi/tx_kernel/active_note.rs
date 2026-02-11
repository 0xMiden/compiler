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

pub const GET_INPUTS: &str = "get_inputs";
pub const GET_ASSETS: &str = "get_assets";
pub const GET_SENDER: &str = "get_sender";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_SCRIPT_ROOT: &str = "get_script_root";
pub const GET_SERIAL_NUMBER: &str = "get_serial_number";
pub const GET_METADATA: &str = "get_metadata";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(Symbol::from(GET_INPUTS), FunctionType::new(CallConv::Wasm, [I32], [I32, I32]));
    note.insert(Symbol::from(GET_ASSETS), FunctionType::new(CallConv::Wasm, [I32], [I32, I32]));
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
        FunctionType::new(
            CallConv::Wasm,
            [],
            [
                Felt, Felt, Felt, Felt, // NOTE_ATTACHMENT
                Felt, Felt, Felt, Felt, // METADATA_HEADER
            ],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), note);
    m
}
