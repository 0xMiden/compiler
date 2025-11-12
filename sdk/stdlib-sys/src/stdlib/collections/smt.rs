//! Bindings for the `std::collections::smt` module, which exposes sparse Merkle tree
//! functionality from the Miden standard library.

use crate::intrinsics::{Felt, Word, WordAligned};

/// Result of [`smt_get`], containing the retrieved `value` and the (unchanged) `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtGetResponse {
    pub value: Word,
    pub root: Word,
}

/// Result of [`smt_set`], containing the `old_value` and the updated `new_root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtSetResponse {
    pub old_value: Word,
    pub new_root: Word,
}

#[allow(improper_ctypes)]
extern "C" {
    /// Returns the value located under the specified `key` in the sparse Merkle tree defined by
    /// the specified `root`.
    ///
    /// This maps to the `std::collections::smt::get` procedure.
    ///
    /// Inputs: `[key, root, ...]`
    /// Outputs: `[value, root, ...]`
    ///
    /// Fails if the tree with the specified `root` does not exist in the VM's advice provider. When
    /// no value has previously been inserted under `key`, the procedure returns the empty word.
    #[link_name = "std::collections::smt::get"]
    fn extern_smt_get(
        k3: Felt,
        k2: Felt,
        k1: Felt,
        k0: Felt,
        r3: Felt,
        r2: Felt,
        r1: Felt,
        r0: Felt,
        ptr: *mut (Word, Word),
    );

    /// Inserts `value` under `key` in the sparse Merkle tree defined by `root`.
    ///
    /// This maps to the `std::collections::smt::set` procedure.
    ///
    /// Inputs: `[value, key, root, ...]`
    /// Outputs: `[old_value, new_root, ...]`
    ///
    /// On success, the prior value stored under `key` is returned along with the updated root. If
    /// `value` is the empty word, the new tree state is equivalent to omitting the update.
    ///
    /// Fails if the tree with the specified `root` does not exist in the VM's advice provider.
    #[link_name = "std::collections::smt::set"]
    fn extern_smt_set(
        v3: Felt,
        v2: Felt,
        v1: Felt,
        v0: Felt,
        k3: Felt,
        k2: Felt,
        k1: Felt,
        k0: Felt,
        r3: Felt,
        r2: Felt,
        r1: Felt,
        r0: Felt,
        ptr: *mut (Word, Word),
    );
}

/// Returns the value associated with `key` in the sparse Merkle tree rooted at `root` as tracked by
/// the VM's advice provider. The returned [`SmtGetResponse`] contains the retrieved value and the
/// (unchanged) root returned by the ABI.
/// Fails if the tree with the specified `root` does not exist in the VM's advice provider. When
/// no value has previously been inserted under `key`, the procedure returns the empty word.
#[inline]
pub fn smt_get(key: Word, root: Word) -> SmtGetResponse {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<(Word, Word)>>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut (Word, Word);
        extern_smt_get(key[3], key[2], key[1], key[0], root[3], root[2], root[1], root[0], ptr);
        let (value, returned_root) = ret_area.assume_init().into_inner();
        SmtGetResponse {
            value: value.reverse(),
            root: returned_root.reverse(),
        }
    }
}

/// Inserts `value` at `key` in the sparse Merkle tree rooted at `root`, returning the prior value
/// stored at `key` along with the new root. The returned [`SmtSetResponse`] contains
/// the previous value stored under `key` and the updated root.
/// Fails if the tree with the specified `root` does not exist in the VM's advice provider.
#[inline]
pub fn smt_set(value: Word, key: Word, root: Word) -> SmtSetResponse {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<(Word, Word)>>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut (Word, Word);
        extern_smt_set(
            value[3], value[2], value[1], value[0], key[3], key[2], key[1], key[0], root[3],
            root[2], root[1], root[0], ptr,
        );
        let (old_value, new_root) = ret_area.assume_init().into_inner();
        SmtSetResponse {
            old_value: old_value.reverse(),
            new_root: new_root.reverse(),
        }
    }
}
