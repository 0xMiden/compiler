//! Contains intrinsics for advice operations with the advice provider.

use crate::{Felt, Word};

#[link(wasm_import_module = "miden:core-intrinsics/intrinsics-advice@1.0.0")]
extern "C" {
    /// Pushes a list of field elements onto the advice stack.
    /// The list is looked up in the advice map using `key` as the key.
    /// Returns the number of elements pushed on the advice stack.
    #[link_name = "adv-push-mapvaln"]
    fn extern_adv_push_mapvaln(key0: Felt, key1: Felt, key2: Felt, key3: Felt) -> Felt;
}

/// Pushes a list of field elements onto the advice stack.
/// The list is looked up in the advice map using `key` as the key.
/// Returns the number of elements pushed on the advice stack.
#[inline]
pub fn adv_push_mapvaln(key: Word) -> Felt {
    unsafe { extern_adv_push_mapvaln(key[3], key[2], key[1], key[0]) }
}
