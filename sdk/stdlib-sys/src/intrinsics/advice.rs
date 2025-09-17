//! Contains intrinsics for advice operations with the advice provider.

use crate::{Felt, Word};

extern "C" {
    /// Pushes a list of field elements onto the advice stack.
    /// The list is looked up in the advice map using `key` as the key.
    /// Returns the number of elements pushed on the advice stack.
    #[link_name = "intrinsics::advice::adv_push_mapvaln"]
    fn extern_adv_push_mapvaln(key0: Felt, key1: Felt, key2: Felt, key3: Felt) -> Felt;
}

/// Pushes a list of field elements onto the advice stack.
/// The list is looked up in the advice map using `key` as the key.
/// Returns the number of elements pushed on the advice stack.
#[inline]
pub fn adv_push_mapvaln(key: Word) -> Felt {
    unsafe { extern_adv_push_mapvaln(key[3], key[2], key[1], key[0]) }
}

extern "C" {
    /// Emits an event to request a Falcon signature for the current message/public key.
    /// This maps to a single MASM instruction: `emit.131087`.
    /// No inputs/outputs.
    #[link_name = "intrinsics::advice::emit_falcon_sig_to_stack"]
    fn extern_emit_falcon_sig_to_stack();
}

/// Emits an event to request a Falcon signature for the current message/public key.
/// Host is expected to push the signature onto the advice stack in response.
/// This is a workaround until migrate to use the VM v0.18 where the `emit` op reads the value from the stack.
#[inline]
pub fn emit_falcon_sig_to_stack() {
    unsafe { extern_emit_falcon_sig_to_stack() }
}
