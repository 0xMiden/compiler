use midenc_hir::{
    interner::{symbols, Symbol},
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::input_note";

fn module_path() -> SymbolPath {
    let parts = [
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(Symbol::intern("input_note")),
    ];
    SymbolPath::from_iter(parts)
}

pub const GET_ASSETS_INFO: &str = "get_assets_info";
pub const GET_ASSETS: &str = "get_assets";
pub const GET_RECIPIENT: &str = "get_recipient";
pub const GET_METADATA: &str = "get_metadata";
pub const GET_SENDER: &str = "get_sender";
pub const GET_INPUTS_INFO: &str = "get_inputs_info";
pub const GET_SCRIPT_ROOT: &str = "get_script_root";
pub const GET_SERIAL_NUMBER: &str = "get_serial_number";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(GET_ASSETS_INFO),
        FunctionType::new(CallConv::Wasm, [Felt], [Felt, Felt, Felt, Felt, Felt]),
    );
    funcs.insert(
        Symbol::from(GET_ASSETS),
        FunctionType::new(CallConv::Wasm, [I32, Felt], [I32, I32]),
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
        Symbol::from(GET_INPUTS_INFO),
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
    m.insert(module_path(), funcs);
    m
}
