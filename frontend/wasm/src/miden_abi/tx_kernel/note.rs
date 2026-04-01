use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const BUILD_RECIPIENT: &str = "build_recipient";

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
        Symbol::from(BUILD_RECIPIENT),
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
    m.insert(module_path(), note);
    m
}
