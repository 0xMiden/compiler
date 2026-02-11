use miden_stdlib_sys::{Felt, Word};

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::native_account::add_asset"]
    fn extern_native_account_add_asset(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
        ptr: *mut Asset,
    );
    #[link_name = "miden::protocol::native_account::remove_asset"]
    fn extern_native_account_remove_asset(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
        ptr: *mut Asset,
    );
    #[link_name = "miden::protocol::native_account::incr_nonce"]
    fn extern_native_account_incr_nonce() -> Felt;
    #[link_name = "miden::protocol::native_account::compute_delta_commitment"]
    fn extern_native_account_compute_delta_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::native_account::was_procedure_called"]
    fn extern_native_account_was_procedure_called(
        proc_root_3: Felt,
        proc_root_2: Felt,
        proc_root_1: Felt,
        proc_root_0: Felt,
    ) -> Felt;
}

/// Add the specified asset to the vault.
///
/// Returns the final asset in the account vault defined as follows: If `asset` is
/// a non-fungible asset, then returns the same as `asset`. If `asset` is a
/// fungible asset, then returns the total fungible asset in the account
/// vault after `asset` was added to it.
///
/// Panics:
/// - If the asset is not valid.
/// - If the total value of two fungible assets is greater than or equal to 2^63.
/// - If the vault already contains the same non-fungible asset.
///
/// # Examples
///
/// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
///
/// ```rust,ignore
/// use miden::{component, native_account::NativeAccount, Asset};
///
/// #[component]
/// struct MyAccount;
///
/// #[component]
/// impl MyAccount {
///     pub fn receive_asset(&mut self, asset: Asset) {
///         self.add_asset(asset);
///     }
/// }
/// ```
pub fn add_asset(asset: Asset) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_native_account_add_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
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
        extern_native_account_remove_asset(
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
#[inline]
pub fn incr_nonce() -> Felt {
    unsafe { extern_native_account_incr_nonce() }
}

/// Computes and returns the commitment to the native account's delta for this transaction.
#[inline]
pub fn compute_delta_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_native_account_compute_delta_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
#[inline]
pub fn was_procedure_called(proc_root: Word) -> bool {
    unsafe {
        extern_native_account_was_procedure_called(
            proc_root[3],
            proc_root[2],
            proc_root[1],
            proc_root[0],
        ) != Felt::from_u32(0)
    }
}

/// Trait that provides native account operations for components.
///
/// This trait is automatically implemented for types marked with the `#[component]` macro.
pub trait NativeAccount {
    /// Add the specified asset to the vault.
    ///
    /// Returns the final asset in the account vault defined as follows: If `asset` is
    /// a non-fungible asset, then returns the same as `asset`. If `asset` is a
    /// fungible asset, then returns the total fungible asset in the account
    /// vault after `asset` was added to it.
    ///
    /// # Panics
    ///
    /// - If the asset is not valid.
    /// - If the total value of two fungible assets is greater than or equal to 2^63.
    /// - If the vault already contains the same non-fungible asset.
    ///
    /// # Examples
    ///
    /// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
    ///
    /// ```rust,ignore
    /// use miden::{component, native_account::NativeAccount, Asset};
    ///
    /// #[component]
    /// struct MyAccount;
    ///
    /// #[component]
    /// impl MyAccount {
    ///     pub fn receive_asset(&mut self, asset: Asset) {
    ///         self.add_asset(asset);
    ///     }
    /// }
    /// ```
    #[inline]
    fn add_asset(&mut self, asset: Asset) -> Asset {
        add_asset(asset)
    }

    /// Remove the specified asset from the vault.
    ///
    /// # Panics
    ///
    /// - The fungible asset is not found in the vault.
    /// - The amount of the fungible asset in the vault is less than the amount to be removed.
    /// - The non-fungible asset is not found in the vault.
    #[inline]
    fn remove_asset(&mut self, asset: Asset) -> Asset {
        remove_asset(asset)
    }

    /// Increments the account nonce by one and returns the new nonce.
    #[inline]
    fn incr_nonce(&mut self) -> Felt {
        incr_nonce()
    }

    /// Computes and returns the commitment to the native account's delta for this transaction.
    #[inline]
    fn compute_delta_commitment(&self) -> Word {
        compute_delta_commitment()
    }

    /// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
    #[inline]
    fn was_procedure_called(&self, proc_root: Word) -> bool {
        was_procedure_called(proc_root)
    }
}
