use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::active_account::get_id"]
    fn extern_active_account_get_id(ptr: *mut AccountId);
    #[link_name = "miden::protocol::active_account::get_nonce"]
    fn extern_active_account_get_nonce() -> Felt;
    #[link_name = "miden::protocol::active_account::get_initial_commitment"]
    fn extern_active_account_get_initial_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::compute_commitment"]
    fn extern_active_account_compute_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::get_code_commitment"]
    fn extern_active_account_get_code_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::get_initial_storage_commitment"]
    fn extern_active_account_get_initial_storage_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::compute_storage_commitment"]
    fn extern_active_account_compute_storage_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::get_balance"]
    fn extern_active_account_get_balance(faucet_id_prefix: Felt, faucet_id_suffix: Felt) -> Felt;
    #[link_name = "miden::protocol::active_account::get_initial_balance"]
    fn extern_active_account_get_initial_balance(
        faucet_id_prefix: Felt,
        faucet_id_suffix: Felt,
    ) -> Felt;
    #[link_name = "miden::protocol::active_account::has_non_fungible_asset"]
    fn extern_active_account_has_non_fungible_asset(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
    ) -> Felt;
    #[link_name = "miden::protocol::active_account::get_initial_vault_root"]
    fn extern_active_account_get_initial_vault_root(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::get_vault_root"]
    fn extern_active_account_get_vault_root(ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::get_num_procedures"]
    fn extern_active_account_get_num_procedures() -> Felt;
    #[link_name = "miden::protocol::active_account::get_procedure_root"]
    fn extern_active_account_get_procedure_root(index: Felt, ptr: *mut Word);
    #[link_name = "miden::protocol::active_account::has_procedure"]
    fn extern_active_account_has_procedure(
        proc_root_3: Felt,
        proc_root_2: Felt,
        proc_root_1: Felt,
        proc_root_0: Felt,
    ) -> Felt;
}

/// Returns the account ID of the active account.
pub fn get_id() -> AccountId {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<AccountId>::uninit();
        extern_active_account_get_id(ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}

/// Returns the nonce of the active account.
#[inline]
pub fn get_nonce() -> Felt {
    unsafe { extern_active_account_get_nonce() }
}

/// Returns the active account commitment at the beginning of the transaction.
#[inline]
pub fn get_initial_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_initial_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Computes and returns the commitment of the current account data.
#[inline]
pub fn compute_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_compute_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the code commitment of the active account.
#[inline]
pub fn get_code_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_code_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the initial storage commitment of the active account.
#[inline]
pub fn get_initial_storage_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_initial_storage_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Computes the latest storage commitment of the active account.
#[inline]
pub fn compute_storage_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_compute_storage_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the balance of the fungible asset identified by `faucet_id`.
///
/// # Panics
///
/// Propagates kernel errors if the referenced asset is non-fungible or the
/// account vault invariants are violated.
pub fn get_balance(faucet_id: AccountId) -> Felt {
    unsafe { extern_active_account_get_balance(faucet_id.prefix, faucet_id.suffix) }
}

/// Returns the initial balance of the fungible asset identified by `faucet_id`.
#[inline]
pub fn get_initial_balance(faucet_id: AccountId) -> Felt {
    unsafe { extern_active_account_get_initial_balance(faucet_id.prefix, faucet_id.suffix) }
}

/// Returns `true` if the active account vault currently contains the specified non-fungible asset.
#[inline]
pub fn has_non_fungible_asset(asset: Asset) -> bool {
    unsafe {
        extern_active_account_has_non_fungible_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
        ) != Felt::new(0)
    }
}

/// Returns the vault root of the active account at the beginning of the transaction.
#[inline]
pub fn get_initial_vault_root() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_initial_vault_root(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the current vault root of the active account.
#[inline]
pub fn get_vault_root() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_vault_root(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the number of procedures exported by the active account.
#[inline]
pub fn get_num_procedures() -> Felt {
    unsafe { extern_active_account_get_num_procedures() }
}

/// Returns the procedure root for the procedure at `index`.
#[inline]
pub fn get_procedure_root(index: u8) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_active_account_get_procedure_root(Felt::new(index as u64), ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns `true` if the procedure identified by `proc_root` exists on the active account.
#[inline]
pub fn has_procedure(proc_root: Word) -> bool {
    unsafe {
        extern_active_account_has_procedure(proc_root[3], proc_root[2], proc_root[1], proc_root[0])
            != Felt::new(0)
    }
}

/// Trait that provides active account operations for components.
///
/// This trait is automatically implemented for types marked with the `#[component]` macro.
pub trait ActiveAccount {
    /// Returns the account ID of the active account.
    #[inline]
    fn get_id(&self) -> AccountId {
        get_id()
    }

    /// Returns the nonce of the active account.
    #[inline]
    fn get_nonce(&self) -> Felt {
        get_nonce()
    }

    /// Returns the active account commitment at the beginning of the transaction.
    #[inline]
    fn get_initial_commitment(&self) -> Word {
        get_initial_commitment()
    }

    /// Computes and returns the commitment of the current account data.
    #[inline]
    fn compute_commitment(&self) -> Word {
        compute_commitment()
    }

    /// Returns the code commitment of the active account.
    #[inline]
    fn get_code_commitment(&self) -> Word {
        get_code_commitment()
    }

    /// Returns the initial storage commitment of the active account.
    #[inline]
    fn get_initial_storage_commitment(&self) -> Word {
        get_initial_storage_commitment()
    }

    /// Computes the latest storage commitment of the active account.
    #[inline]
    fn compute_storage_commitment(&self) -> Word {
        compute_storage_commitment()
    }

    /// Returns the balance of the fungible asset identified by `faucet_id`.
    ///
    /// # Panics
    ///
    /// Propagates kernel errors if the referenced asset is non-fungible or the
    /// account vault invariants are violated.
    #[inline]
    fn get_balance(&self, faucet_id: AccountId) -> Felt {
        get_balance(faucet_id)
    }

    /// Returns the initial balance of the fungible asset identified by `faucet_id`.
    #[inline]
    fn get_initial_balance(&self, faucet_id: AccountId) -> Felt {
        get_initial_balance(faucet_id)
    }

    /// Returns `true` if the active account vault currently contains the specified non-fungible asset.
    #[inline]
    fn has_non_fungible_asset(&self, asset: Asset) -> bool {
        has_non_fungible_asset(asset)
    }

    /// Returns the vault root of the active account at the beginning of the transaction.
    #[inline]
    fn get_initial_vault_root(&self) -> Word {
        get_initial_vault_root()
    }

    /// Returns the current vault root of the active account.
    #[inline]
    fn get_vault_root(&self) -> Word {
        get_vault_root()
    }

    /// Returns the number of procedures exported by the active account.
    #[inline]
    fn get_num_procedures(&self) -> Felt {
        get_num_procedures()
    }

    /// Returns the procedure root for the procedure at `index`.
    #[inline]
    fn get_procedure_root(&self, index: u8) -> Word {
        get_procedure_root(index)
    }

    /// Returns `true` if the procedure identified by `proc_root` exists on the active account.
    #[inline]
    fn has_procedure(&self, proc_root: Word) -> bool {
        has_procedure(proc_root)
    }
}
