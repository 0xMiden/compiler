//! Bindings for the `std::collections::smt` module, which exposes sparse Merkle tree
//! functionality from the Miden standard library.

use crate::{intrinsics::Word, raw::core::collections::smt};

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

/// Returns the value associated with `key` in the sparse Merkle tree rooted at `root` as tracked by
/// the VM's advice provider. The returned [`SmtGetResponse`] contains the retrieved value and the
/// (unchanged) root returned by the ABI.
/// Fails if the tree with the specified `root` does not exist in the VM's advice provider. When
/// no value has previously been inserted under `key`, the procedure returns the empty word.
#[inline]
pub fn smt_get(key: Word, root: Word) -> SmtGetResponse {
    let (value, root) = smt::get(key, root);
    SmtGetResponse { value, root }
}

/// Inserts `value` at `key` in the sparse Merkle tree rooted at `root`, returning the prior value
/// stored at `key` along with the new root. The returned [`SmtSetResponse`] contains
/// the previous value stored under `key` and the updated root.
/// Fails if the tree with the specified `root` does not exist in the VM's advice provider.
#[inline]
pub fn smt_set(value: Word, key: Word, root: Word) -> SmtSetResponse {
    let (old_value, new_root) = smt::set(value, key, root);
    SmtSetResponse {
        old_value,
        new_root,
    }
}
