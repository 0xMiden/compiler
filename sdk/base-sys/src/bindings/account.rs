use miden_stdlib_sys::Felt;

use super::types::{AccountId, CoreAsset};

#[allow(improper_ctypes)]
#[link(wasm_import_module = "miden:core-base/account@1.0.0")]
extern "C" {
    #[link_name = "get-id"]
    pub fn extern_account_get_id() -> AccountId;
    #[link_name = "add-asset"]
    pub fn extern_account_add_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut CoreAsset);
    #[link_name = "remove-asset"]
    pub fn extern_account_remove_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut CoreAsset);
    #[link_name = "incr-nonce"]
    pub fn extern_account_incr_nonce(value: i32);
}

/// Get the account ID of the currently executing note account.
pub fn get_id() -> AccountId {
    unsafe { extern_account_get_id() }
}

/// Add the specified asset to the vault.
/// Returns the final asset in the account vault defined as follows: If asset is
/// a non-fungible asset, then returns the same as asset. If asset is a
/// fungible asset, then returns the total fungible asset in the account
/// vault after asset was added to it.
///
/// Panics:
/// - If the asset is not valid.
/// - If the total value of two fungible assets is greater than or equal to 2^63.
/// - If the vault already contains the same non-fungible asset.
pub fn add_asset(asset: CoreAsset) -> CoreAsset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<CoreAsset>::uninit();
        extern_account_add_asset(
            asset.inner[0],
            asset.inner[1],
            asset.inner[2],
            asset.inner[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Remove the specified asset from the vault.
///
/// Panics:
/// - The fungible asset is not found in the vault.
/// - The amount of the fungible asset in the vault is less than the amount to be removed.
/// - The non-fungible asset is not found in the vault.
pub fn remove_asset(asset: CoreAsset) -> CoreAsset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<CoreAsset>::uninit();
        extern_account_remove_asset(
            asset.inner[0],
            asset.inner[1],
            asset.inner[2],
            asset.inner[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Increments the account nonce by the provided value.
///
/// Panics:
/// - If the underlying kernel procedure panics.
pub fn incr_nonce(value: u32) {
    unsafe {
        extern_account_incr_nonce(value as i32);
    }
}
