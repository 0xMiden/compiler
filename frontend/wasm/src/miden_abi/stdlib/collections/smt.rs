use midenc_hir::{
    CallConv, FunctionType, SymbolNameComponent, SymbolPath,
    Type::*,
    interner::{Symbol, symbols},
};

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub(crate) const MODULE_ID: &str = "std::collections::smt";

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
        SymbolNameComponent::Component(symbols::Std),
        SymbolNameComponent::Component(Symbol::intern("collections")),
        SymbolNameComponent::Component(Symbol::intern("smt")),
    ]);
    m.insert(module_path, funcs);
    let mut env_funcs: FunctionTypeMap = Default::default();
    env_funcs.insert(
        Symbol::intern("std::collections::smt::get"),
        FunctionType::new(
            CallConv::Wasm,
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
            [Felt, Felt, Felt, Felt, Felt, Felt, Felt, Felt],
        ),
    );
    env_funcs.insert(
        Symbol::intern("std::collections::smt::set"),
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
    let env_module_path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(Symbol::intern("env")),
    ]);
    m.insert(env_module_path, env_funcs);
    m
}
