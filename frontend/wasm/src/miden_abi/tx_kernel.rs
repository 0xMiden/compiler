//! Function types and lowering for tx kernel API functions

pub(crate) mod account;
pub(crate) mod note;
pub(crate) mod tx;

use std::sync::OnceLock;

use super::ModuleFunctionTypeMap;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: OnceLock<ModuleFunctionTypeMap> = OnceLock::new();
    TYPES.get_or_init(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(account::signatures());
        m.extend(note::signatures());
        m.extend(tx::signatures());
        m
    })
}
