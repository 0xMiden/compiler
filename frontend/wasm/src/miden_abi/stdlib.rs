//! Function types and lowered signatures for the Miden stdlib API functions

use midenc_hir_symbol::sync::LazyLock;

use super::ModuleFunctionTypeMap;

pub(crate) mod crypto;
pub(crate) mod mem;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: LazyLock<ModuleFunctionTypeMap> = LazyLock::new(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(crypto::hashes::blake3::signatures());
        m.extend(crypto::dsa::rpo_falcon::signatures());
        m.extend(mem::signatures());
        m
    });
    &TYPES
}
