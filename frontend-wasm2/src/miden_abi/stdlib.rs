//! Function types and lowered signatures for the Miden stdlib API functions

use std::sync::OnceLock;

use super::ModuleFunctionTypeMap;

pub(crate) mod crypto;
pub(crate) mod mem;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: OnceLock<ModuleFunctionTypeMap> = OnceLock::new();
    TYPES.get_or_init(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(crypto::hashes::blake3::signatures());
        m.extend(crypto::dsa::rpo_falcon::signatures());
        m.extend(mem::signatures());
        m
    })
}
