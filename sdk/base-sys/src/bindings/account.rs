use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "miden::account::get_id"]
    pub fn extern_account_get_id(ptr: *mut AccountId);
    #[link_name = "miden::account::remove_asset"]
    pub fn extern_account_remove_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut Asset);
    #[link_name = "miden::account::incr_nonce"]
    pub fn extern_account_incr_nonce() -> Felt;
    #[link_name = "miden::account::get_initial_commitment"]
    pub fn extern_account_get_initial_commitment(ptr: *mut Word);
    #[link_name = "miden::account::compute_current_commitment"]
    pub fn extern_account_compute_current_commitment(ptr: *mut Word);
    // Resolved via stub rlib at core Wasm link time
    #[link_name = "miden::account::add_asset"]
    pub fn extern_account_add_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut Asset);
}

/// Get the account ID of the currently executing note account.
pub fn get_id() -> AccountId {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<AccountId>::uninit();

        // The MASM procedure returns the account ID on the stack.
        // Inputs:  []
        // Outputs: [acct_id_prefix, acct_id_suffix]
        extern_account_get_id(ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
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
pub fn add_asset(asset: Asset) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_account_add_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
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
pub fn remove_asset(asset: Asset) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_account_remove_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Increments the account nonce by one and returns the new nonce.
pub fn incr_nonce() -> Felt {
    unsafe { extern_account_incr_nonce() }
}

/// Returns the native account commitment at the beginning of the transaction.
#[inline]
pub fn get_initial_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_initial_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Computes and returns the commitment of the current account data.
#[inline]
pub fn compute_current_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_compute_current_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}
