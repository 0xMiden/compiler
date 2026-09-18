//! Bindings for the protocol's asset-id accessors.
//!
//! These procedures decode the parts of an asset id — the issuing faucet, the asset class and the
//! composition rule — without touching the account vault.

use miden_stdlib_sys::{Felt, WordAligned};

use super::types::{AccountId, AssetClass, AssetComposition, AssetId, RawAccountId, RawAssetClass};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::asset::id_into_faucet_id"]
    fn extern_asset_id_into_faucet_id(
        asset_id_0: Felt,
        asset_id_1: Felt,
        asset_id_2: Felt,
        asset_id_3: Felt,
        ptr: *mut RawAccountId,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::asset::id_into_asset_class"]
    fn extern_asset_id_into_asset_class(
        asset_id_0: Felt,
        asset_id_1: Felt,
        asset_id_2: Felt,
        asset_id_3: Felt,
        ptr: *mut RawAssetClass,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::asset::id_into_composition"]
    fn extern_asset_id_into_composition(
        asset_id_0: Felt,
        asset_id_1: Felt,
        asset_id_2: Felt,
        asset_id_3: Felt,
    ) -> Felt;
}

/// Returns the id of the faucet that issued the asset identified by `asset_id`.
///
/// The faucet id is read out of the asset id without being validated.
pub fn id_into_faucet_id(asset_id: AssetId) -> AccountId {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawAccountId>::uninit());
        let id = asset_id.inner;
        extern_asset_id_into_faucet_id(id[0], id[1], id[2], id[3], ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_account_id()
    }
}

/// Returns the asset class of `asset_id`.
pub fn id_into_asset_class(asset_id: AssetId) -> AssetClass {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawAssetClass>::uninit());
        let id = asset_id.inner;
        extern_asset_id_into_asset_class(id[0], id[1], id[2], id[3], ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_asset_class()
    }
}

/// Returns the composition rule encoded in `asset_id`.
///
/// # Panics
///
/// Panics if the asset id encodes a composition this SDK does not recognize.
pub fn id_into_composition(asset_id: AssetId) -> AssetComposition {
    let id = asset_id.inner;
    let composition = unsafe { extern_asset_id_into_composition(id[0], id[1], id[2], id[3]) };
    AssetComposition::try_from(composition).expect("unrecognized asset composition")
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
