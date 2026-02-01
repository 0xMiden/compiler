use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const GET: &str = "get";
pub(crate) const SET: &str = "set";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(GET),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    funcs.insert(
        Symbol::from(SET),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // value
                Felt, Felt, Felt, Felt, // key
                Felt, Felt, Felt, Felt, // root
            ],
            [
                Felt, Felt, Felt, Felt, // old value
                Felt, Felt, Felt, Felt, // new root
            ],
        ),
    );
    let module_path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Core),
        SymbolNameComponent::Component(symbols::Collections),
        SymbolNameComponent::Component(symbols::Smt),
    ]);
    m.insert(module_path, funcs);
    m
}
