use miden_stdlib_sys::Word;

use super::types::{AccountId, Asset, AssetId, Nonce};
use crate::raw::protocol::native_account as raw;

/// Adds the specified asset to the vault and returns the resulting asset value word stored under
/// that asset id.
///
/// Panics:
/// - If the asset is not valid.
/// - If the total value of two fungible assets is greater than
///   [`AssetAmount::MAX_U64`](super::types::AssetAmount::MAX_U64).
/// - If the vault already contains the same non-fungible asset.
///
/// # Examples
///
/// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
///
/// ```rust,ignore
/// use miden::{component, component_storage, native_account::NativeAccount, Asset};
///
/// #[component_storage]
/// struct MyAccountStorage;
///
/// #[component]
/// trait MyAccount {
///     fn receive_asset(&mut self, asset: Asset);
/// }
///
/// #[component]
/// impl MyAccount for MyAccountStorage {
///     fn receive_asset(&mut self, asset: Asset) {
///         self.add_asset(asset);
///     }
/// }
/// ```
pub fn add_asset(asset: Asset) -> Word {
    raw::add_asset(asset.into())
}

/// Removes the specified asset from the vault and returns the resulting asset value word.
///
/// Panics:
/// - The fungible asset is not found in the vault.
/// - The amount of the fungible asset in the vault is less than the amount to be removed.
/// - The non-fungible asset is not found in the vault.
pub fn remove_asset(asset: Asset) -> Word {
    raw::remove_asset(asset.into())
}

/// Returns the native account ID for the current transaction.
pub fn get_id() -> AccountId {
    raw::get_id().into()
}

/// Increments the account nonce by one and returns the new nonce.
#[inline]
pub fn incr_nonce() -> Nonce {
    Nonce {
        inner: raw::incr_nonce(),
    }
}

/// Computes and returns the commitment to the native account's current state.
///
/// # Panics
///
/// - If the invocation does not originate from the account context (a note or transaction
///   script cannot call it directly).
/// - If the active account is not the native account (a foreign account reached through FPI).
#[inline]
pub fn compute_commitment() -> Word {
    raw::compute_commitment()
}

/// Computes and returns the commitment to the native account's delta for this transaction.
#[inline]
pub fn compute_delta_commitment() -> Word {
    raw::compute_delta_commitment()
}

/// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
#[inline]
pub fn was_procedure_called(proc_root: Word) -> bool {
    raw::was_procedure_called(proc_root)
}

/// Returns the native account's commitment at the beginning of the transaction.
#[inline]
pub fn get_initial_commitment() -> Word {
    raw::get_initial_commitment()
}

/// Returns the native account's storage commitment at the beginning of the transaction.
#[inline]
pub fn get_initial_storage_commitment() -> Word {
    raw::get_initial_storage_commitment()
}

/// Returns the native account's vault root at the beginning of the transaction.
#[inline]
pub fn get_initial_vault_root() -> Word {
    raw::get_initial_vault_root()
}

/// Returns the native account's initial value stored under the specified `asset_id` in the vault.
pub fn get_initial_asset(asset_id: AssetId) -> Word {
    raw::get_initial_asset(asset_id.inner)
}

/// Returns `true` if the native account's state has changed since the transaction began.
///
/// Unlike [`compute_delta_commitment`], this may be called before the authentication procedure
/// increments the nonce.
#[inline]
pub fn has_state_changed() -> bool {
    raw::has_state_changed()
}

/// Returns `true` if the native account's vault held an asset with the specified asset id at the
/// beginning of the transaction.
#[inline]
pub fn has_initial_asset(asset_id: AssetId) -> bool {
    raw::has_initial_asset(asset_id.inner)
}

/// Trait that provides native account operations for components.
///
/// This trait is automatically implemented for the storage struct marked with the
/// `#[component_storage]` macro.
pub trait NativeAccount {
    /// Adds the specified asset to the vault and returns the resulting asset value word stored
    /// under that asset id.
    ///
    /// # Panics
    ///
    /// - If the asset is not valid.
    /// - If the total value of two fungible assets is greater than
    ///   [`AssetAmount::MAX_U64`](super::types::AssetAmount::MAX_U64).
    /// - If the vault already contains the same non-fungible asset.
    ///
    /// # Examples
    ///
    /// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
    ///
    /// ```rust,ignore
    /// use miden::{component, component_storage, native_account::NativeAccount, Asset};
    ///
    /// #[component_storage]
    /// struct MyAccountStorage;
    ///
    /// #[component]
    /// trait MyAccount {
    ///     fn receive_asset(&mut self, asset: Asset);
    /// }
    ///
    /// #[component]
    /// impl MyAccount for MyAccountStorage {
    ///     fn receive_asset(&mut self, asset: Asset) {
    ///         self.add_asset(asset);
    ///     }
    /// }
    /// ```
    #[inline]
    fn add_asset(&mut self, asset: Asset) -> Word {
        add_asset(asset)
    }

    /// Removes the specified asset from the vault and returns the resulting asset value word.
    ///
    /// # Panics
    ///
    /// - The fungible asset is not found in the vault.
    /// - The amount of the fungible asset in the vault is less than the amount to be removed.
    /// - The non-fungible asset is not found in the vault.
    #[inline]
    fn remove_asset(&mut self, asset: Asset) -> Word {
        remove_asset(asset)
    }

    /// Increments the account nonce by one and returns the new nonce.
    #[inline]
    fn incr_nonce(&mut self) -> Nonce {
        incr_nonce()
    }

    /// Computes and returns the commitment to the native account's current state.
    ///
    /// # Panics
    ///
    /// - If the invocation does not originate from the account context (a note or transaction
    ///   script cannot call it directly).
    /// - If the active account is not the native account (a foreign account reached through
    ///   FPI).
    #[inline]
    fn compute_commitment(&self) -> Word {
        compute_commitment()
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

    /// Returns `true` if the native account's state has changed since the transaction began.
    ///
    /// Unlike [`NativeAccount::compute_delta_commitment`], this may be called before the
    /// authentication procedure increments the nonce.
    #[inline]
    fn has_state_changed(&self) -> bool {
        has_state_changed()
    }

    /// Returns `true` if the native account's vault held an asset with the specified asset id at
    /// the beginning of the transaction.
    #[inline]
    fn has_initial_asset(&self, asset_id: AssetId) -> bool {
        has_initial_asset(asset_id)
    }
}
