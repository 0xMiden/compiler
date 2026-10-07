//! Unreachable stubs for the two procedures this crate binds by hand: both are lowered by the
//! compiler itself rather than resolved against a package, so their bindings and stubs are not
//! generated. The protocol's and the standards' stubs are generated (`protocol.rs`,
//! `standards.rs`).
//!
//! Compiled by `build.rs` into a separate archive linked into every dependent, so that the Wasm
//! frontend can recognize the calls by name and lower them. Not part of the crate sources.

#![no_std]
#![feature(optimize_attribute)]

use core::ffi::c_void;

/// `intrinsics::note::script_root` is a compiler intrinsic: the frontend synthesizes the stub
/// body with a `procref` of the crate's `#[note_script]` entrypoint export.
///
/// The exported name must stay in lockstep with `SCRIPT_ROOT_STUB_NAME` in the compiler
/// frontend (`frontend/wasm/src/intrinsics/note.rs`) and the `link_name` in
/// `src/bindings/note.rs`.
#[unsafe(export_name = "intrinsics::note::script_root")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_script_root_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

/// The raw foreign procedure invocation, which the frontend lowers to `hir.exec_fpi`
/// (`frontend/wasm/src/intrinsics/fpi.rs`). It is not an export of the protocol package.
#[unsafe(export_name = "miden::protocol::tx::execute_foreign_procedure_indirect")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_execute_foreign_procedure_indirect(
    _invocation: *const c_void,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
