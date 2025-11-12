use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "miden::active_account::get_id"]
    pub fn extern_account_get_id(ptr: *mut AccountId);
    #[link_name = "miden::native_account::remove_asset"]
    pub fn extern_account_remove_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut Asset);
    #[link_name = "miden::active_account::get_nonce"]
    pub fn extern_account_get_nonce() -> Felt;
    #[link_name = "miden::native_account::incr_nonce"]
    pub fn extern_account_incr_nonce() -> Felt;
    #[link_name = "miden::active_account::get_initial_commitment"]
    pub fn extern_account_get_initial_commitment(ptr: *mut Word);
    #[link_name = "miden::active_account::compute_commitment"]
    pub fn extern_account_compute_commitment(ptr: *mut Word);
    #[link_name = "miden::native_account::compute_delta_commitment"]
    pub fn extern_account_compute_delta_commitment(ptr: *mut Word);
    #[link_name = "miden::active_account::get_code_commitment"]
    pub fn extern_account_get_code_commitment(ptr: *mut Word);
    #[link_name = "miden::active_account::get_initial_storage_commitment"]
    pub fn extern_account_get_initial_storage_commitment(ptr: *mut Word);
    #[link_name = "miden::active_account::compute_storage_commitment"]
    pub fn extern_account_compute_storage_commitment(ptr: *mut Word);
    #[link_name = "miden::active_account::get_initial_balance"]
    pub fn extern_account_get_initial_balance(
        faucet_id_prefix: Felt,
        faucet_id_suffix: Felt,
    ) -> Felt;
    #[link_name = "miden::active_account::has_non_fungible_asset"]
    pub fn extern_account_has_non_fungible_asset(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
    ) -> Felt;
    #[link_name = "miden::active_account::get_initial_vault_root"]
    pub fn extern_account_get_initial_vault_root(ptr: *mut Word);
    #[link_name = "miden::active_account::get_vault_root"]
    pub fn extern_account_get_vault_root(ptr: *mut Word);
    #[link_name = "miden::active_account::get_num_procedures"]
    pub fn extern_account_get_num_procedures() -> Felt;
    #[link_name = "miden::active_account::get_procedure_root"]
    pub fn extern_account_get_procedure_root(index: Felt, ptr: *mut Word);
    #[link_name = "miden::active_account::has_procedure"]
    pub fn extern_account_has_procedure(
        proc_root_3: Felt,
        proc_root_2: Felt,
        proc_root_1: Felt,
        proc_root_0: Felt,
    ) -> Felt;
    // Resolved via stub rlib at core Wasm link time
    #[link_name = "miden::native_account::add_asset"]
    pub fn extern_account_add_asset(_: Felt, _: Felt, _: Felt, _: Felt, ptr: *mut Asset);
    #[link_name = "miden::active_account::get_balance"]
    pub fn extern_account_get_balance(faucet_id_prefix: Felt, faucet_id_suffix: Felt) -> Felt;
    #[link_name = "miden::native_account::was_procedure_called"]
    pub fn extern_account_was_procedure_called(
        proc_root_3: Felt,
        proc_root_2: Felt,
        proc_root_1: Felt,
        proc_root_0: Felt,
    ) -> Felt;
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

/// Returns the current account nonce.
#[inline]
pub fn get_nonce() -> Felt {
    unsafe { extern_account_get_nonce() }
}

/// Increments the account nonce by one and return the new nonce
#[inline]
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
pub fn compute_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_compute_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Computes and returns the commitment to the native account's delta for this transaction.
#[inline]
pub fn compute_delta_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_compute_delta_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the code commitment of the active account.
#[inline]
pub fn get_code_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_code_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the initial storage commitment of the active account.
#[inline]
pub fn get_initial_storage_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_initial_storage_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Computes the latest storage commitment of the active account.
#[inline]
pub fn compute_storage_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_compute_storage_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the balance of the fungible asset identified by `faucet_id`.
///
/// # Panics
///
/// Propagates kernel errors if the referenced asset is non-fungible or the
/// account vault invariants are violated.
pub fn get_balance(faucet_id: AccountId) -> Felt {
    unsafe { extern_account_get_balance(faucet_id.prefix, faucet_id.suffix) }
}

/// Returns the initial balance of the fungible asset identified by `faucet_id`.
#[inline]
pub fn get_initial_balance(faucet_id: AccountId) -> Felt {
    unsafe { extern_account_get_initial_balance(faucet_id.prefix, faucet_id.suffix) }
}

/// Returns `true` if the active account vault currently contains the specified non-fungible asset.
#[inline]
pub fn has_non_fungible_asset(asset: Asset) -> bool {
    unsafe {
        extern_account_has_non_fungible_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
        ) != Felt::from_u32(0)
    }
}

/// Returns the storage commitment of the active account vault at the beginning of the transaction.
#[inline]
pub fn get_initial_vault_root() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_initial_vault_root(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the current storage commitment of the active account vault.
#[inline]
pub fn get_vault_root() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_vault_root(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the number of procedures exported by the active account.
#[inline]
pub fn get_num_procedures() -> Felt {
    unsafe { extern_account_get_num_procedures() }
}

/// Returns the procedure root for the procedure at `index`.
#[inline]
pub fn get_procedure_root(index: u8) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_account_get_procedure_root(index.into(), ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns `true` if the procedure identified by `proc_root` exists on the active account.
#[inline]
pub fn has_procedure(proc_root: Word) -> bool {
    unsafe {
        extern_account_has_procedure(proc_root[3], proc_root[2], proc_root[1], proc_root[0])
            != Felt::from_u32(0)
    }
}

/// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
#[inline]
pub fn was_procedure_called(proc_root: Word) -> bool {
    unsafe {
        extern_account_was_procedure_called(proc_root[3], proc_root[2], proc_root[1], proc_root[0])
            != Felt::from_u32(0)
    }
}
