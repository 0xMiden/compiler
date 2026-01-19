//! Contains intrinsics for advice operations with the advice provider.

use crate::intrinsics::{Felt, Word};

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
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
#[cfg(all(target_family = "wasm", miden))]
pub fn adv_push_mapvaln(key: Word) -> Felt {
    unsafe { extern_adv_push_mapvaln(key[3], key[2], key[1], key[0]) }
}

#[inline]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn adv_push_mapvaln(_key: Word) -> Felt {
    unimplemented!("advice intrinsics are only available when targeting the Miden VM")
}

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    /// Emits an event to request a Falcon signature for the provided message/public key.
    /// This maps to the MASM instruction: `emit.AUTH_REQUEST_EVENT`.
    #[link_name = "intrinsics::advice::emit_falcon_sig_to_stack"]
    fn extern_emit_falcon_sig_to_stack(
        msg0: Felt,
        msg1: Felt,
        msg2: Felt,
        msg3: Felt,
        pk0: Felt,
        pk1: Felt,
        pk2: Felt,
        pk3: Felt,
    );
}

/// Emits an event to request a Falcon signature for the current message/public key.
/// Host is expected to push the signature onto the advice stack in response.
/// This is a workaround until migrating to VM v0.18 where the `emit` op reads the value from the stack.
#[inline]
#[cfg(all(target_family = "wasm", miden))]
pub fn emit_falcon_sig_to_stack(msg: Word, pub_key: Word) {
    unsafe {
        extern_emit_falcon_sig_to_stack(
            msg[3], msg[2], msg[1], msg[0], pub_key[3], pub_key[2], pub_key[1], pub_key[0],
        );
    }
}

#[inline]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn emit_falcon_sig_to_stack(_msg: Word, _pub_key: Word) {
    unimplemented!("advice intrinsics are only available when targeting the Miden VM")
}

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    /// Inserts values from memory into the advice map using the provided key and memory range.
    /// Maps to the VM op: adv.insert_mem
    /// Signature: (key0..key3, start_addr, end_addr)
    #[link_name = "intrinsics::advice::adv_insert_mem"]
    fn extern_adv_insert_mem(
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        start_addr: u32,
        end_addr: u32,
    );
}

/// Insert memory region [start, end) into advice map under the given key.
#[inline]
#[cfg(all(target_family = "wasm", miden))]
pub fn adv_insert_mem(key: Word, start_addr: u32, end_addr: u32) {
    unsafe { extern_adv_insert_mem(key[3], key[2], key[1], key[0], start_addr, end_addr) }
}

/// Insert memory region [start, end) into advice map under the given key.
#[inline]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn adv_insert_mem(_key: Word, _start_addr: u32, _end_addr: u32) {
    unimplemented!("advice intrinsics are only available when targeting the Miden VM")
}

/// Insert values into advice map under the given key.
#[cfg(all(target_family = "wasm", miden))]
pub fn adv_insert(key: Word, values: &[Word]) {
    let rust_ptr = values.as_ptr() as u32;
    let miden_ptr = rust_ptr / 4;
    let end_addr = miden_ptr + values.len() as u32 * 4;
    adv_insert_mem(key, miden_ptr, end_addr);
}

/// Insert values into advice map under the given key.
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn adv_insert(_key: Word, _values: &[Word]) {
    unimplemented!("advice intrinsics are only available when targeting the Miden VM")
}
