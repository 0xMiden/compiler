//! Bindings for the protocol's asset-id accessors.
//!
//! These procedures decode the parts of an asset id — the issuing faucet, the asset class and the
//! composition rule — without touching the account vault.

use super::types::{AccountId, AssetClass, AssetComposition, AssetId};
use crate::raw::protocol::asset as raw;

/// Returns the id of the faucet that issued the asset identified by `asset_id`.
///
/// The faucet id is read out of the asset id without being validated.
pub fn id_into_faucet_id(asset_id: AssetId) -> AccountId {
    raw::id_into_faucet_id(asset_id.inner).into()
}

/// Returns the asset class of `asset_id`.
pub fn id_into_asset_class(asset_id: AssetId) -> AssetClass {
    raw::id_into_asset_class(asset_id.inner).into()
}

/// Returns the composition rule encoded in `asset_id`.
///
/// # Panics
///
/// Panics if the asset id encodes a composition this SDK does not recognize.
pub fn id_into_composition(asset_id: AssetId) -> AssetComposition {
    raw::id_into_composition(asset_id.inner).into()
}

impl AssetId {
    /// Returns the id of the faucet that issued this asset.
    ///
    /// The faucet id is read out of the asset id without being validated.
    #[inline]
    pub fn faucet_id(self) -> AccountId {
        id_into_faucet_id(self)
    }

    /// Returns this asset's class, which distinguishes it from the other assets issued by the same
    /// faucet.
    #[inline]
    pub fn asset_class(self) -> AssetClass {
        id_into_asset_class(self)
    }

    /// Returns the composition rule encoded in this asset id.
    ///
    /// # Panics
    ///
    /// Panics if the asset id encodes a composition this SDK does not recognize.
    #[inline]
    pub fn composition(self) -> AssetComposition {
        id_into_composition(self)
    }
}
