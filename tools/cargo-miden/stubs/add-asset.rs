#![no_std]

use core::ffi::c_void;

/// Unreachable stub for `add-asset` import (extern_account_add_asset).
/// Signature matches the Wasm lowering used by the SDK: (f32, f32, f32, f32, i32)
#[export_name = "miden::account::add_asset"]
pub extern "C" fn add_asset_plain(_a0: f32, _a1: f32, _a2: f32, _a3: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
