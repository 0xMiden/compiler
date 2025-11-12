//! Function types and lowering for tx kernel API functions

pub(crate) mod account;
pub(crate) mod active_note;
pub(crate) mod asset;
pub(crate) mod faucet;
pub(crate) mod input_note;
pub(crate) mod output_note;
pub(crate) mod tx;

use midenc_hir_symbol::sync::LazyLock;

use super::ModuleFunctionTypeMap;

pub(crate) fn signatures() -> &'static ModuleFunctionTypeMap {
    static TYPES: LazyLock<ModuleFunctionTypeMap> = LazyLock::new(|| {
        let mut m: ModuleFunctionTypeMap = Default::default();
        m.extend(account::signatures());
        m.extend(active_note::signatures());
        m.extend(asset::signatures());
        m.extend(faucet::signatures());
        m.extend(input_note::signatures());
        m.extend(output_note::signatures());
        m.extend(tx::signatures());
        m
    });
    &TYPES
}
