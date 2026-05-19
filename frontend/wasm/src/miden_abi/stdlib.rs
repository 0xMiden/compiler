//! Function types and lowered signatures for the Miden stdlib API functions

use midenc_hir::{SmallVec, SymbolPath, interner::Symbol, smallvec};
use midenc_hir_symbol::{symbols, sync::LazyLock};

use super::ModuleFunctionTypeMap;
use crate::intrinsics::IntrinsicEffect;

pub(crate) mod collections;
pub(crate) mod crypto;
pub(crate) mod mem;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: LazyLock<ModuleFunctionTypeMap> = LazyLock::new(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(collections::smt::signatures());
        m.extend(crypto::hashes::blake3::signatures());
        m.extend(crypto::hashes::sha256::signatures());
        m.extend(crypto::hashes::poseidon2::signatures());
        m.extend(crypto::dsa::rpo_falcon512::signatures());
        m.extend(mem::signatures());
        m
    });
    &TYPES
}

pub(crate) fn function_effects(
    module_path: &SymbolPath,
    function: Symbol,
) -> Option<SmallVec<[IntrinsicEffect; 2]>> {
    let mut components = module_path.components().peekable();
    components.next_if_eq(&midenc_hir::SymbolNameComponent::Root);
    if components.next()?.as_symbol_name() != symbols::Miden {
        return None;
    }
    if components.next()?.as_symbol_name() != symbols::Core {
        return None;
    }

    match components.next()?.as_symbol_name() {
        symbols::Mem => mem::function_effects(function),
        _ => Some(smallvec![]),
    }
}
