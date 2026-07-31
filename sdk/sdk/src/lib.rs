#![no_std]
#![deny(warnings)]

pub mod debug;

pub use miden_base::*;
pub use miden_base_macros::{
    account, account_procedure, auth_script, component, component_storage, export_type, generate,
    note, note_constructor, note_script, tx_script,
};
pub use miden_base_sys::bindings::*;
/// Unified `Felt` and related helpers.
pub use miden_field;
/// Felt representation helpers.
pub use miden_field_repr as felt_repr;
// Re-export under the crate's own name so the `FromFeltRepr`/`ToFeltRepr` derive expansions
// (which reference `miden_field_repr::...`) resolve in crates that only depend on `miden`
// and glob-import it.
pub use miden_field_repr;
pub use miden_sdk_alloc::BumpAlloc;
pub use miden_stdlib_sys::*;
pub use miden_tx_script_args::{
    EncodedScriptArgs, ScriptArgs, ScriptArgsError, ScriptArgsResult, decode_preimage,
};
// Re-export since `wit_bindgen::generate!` is used in `generate!`
pub use wit_bindgen;
