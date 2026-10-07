use super::types::Asset;
use crate::raw::protocol::faucet as raw;

/// Mints the provided asset for the faucet bound to the current transaction.
pub fn mint(asset: Asset) {
    raw::mint(asset.into());
}

/// Burns the provided asset from the faucet bound to the current transaction.
pub fn burn(asset: Asset) {
    raw::burn(asset.into());
}
