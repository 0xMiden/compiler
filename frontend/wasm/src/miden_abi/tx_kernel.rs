//! Function types and lowering for tx kernel API functions

pub(crate) mod account;
pub(crate) mod note;
pub(crate) mod tx;

use midenc_hir_symbol::sync::LazyLock;

use super::ModuleFunctionTypeMap;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: LazyLock<ModuleFunctionTypeMap> = LazyLock::new(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(account::signatures());
        m.extend(note::signatures());
        m.extend(tx::signatures());
        m
    });
    &TYPES
}
