//! AccountId wrapper for felt representation serialization.

use alloc::vec::Vec;

use miden_core::Felt;
use miden_objects::account::AccountId;

use crate::ToFeltRepr;

/// Wrapper around `AccountId` that implements `ToFeltRepr`.
///
/// This wrapper serializes the account ID into its felt representation,
/// matching the memory layout expected by on-chain deserialization.
pub struct AccountIdFeltRepr<'a>(pub &'a AccountId);

impl<'a> From<&'a AccountId> for AccountIdFeltRepr<'a> {
    fn from(account_id: &'a AccountId) -> Self {
        Self(account_id)
    }
}

impl ToFeltRepr for AccountIdFeltRepr<'_> {
    fn to_felt_repr(&self) -> Vec<Felt> {
        Vec::from([self.0.prefix().as_felt(), self.0.suffix()])
    }
}
