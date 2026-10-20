//! Transaction script which pauses an account through the standard (MASM) `pausable-manager`
//! component and reads the paused state through the standard `pausable` component.
//!
//! The two components' namespaces nest (`...::access::pausable` and
//! `...::access::pausable::manager`), so the script pins that both can be dependencies of one
//! consumer.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the methods of the standard `pausable` and
/// `pausable-manager` components, whose interfaces are derived from the
/// `miden-standards-access-pausable` and `miden-standards-access-pausable-manager` package
/// manifests.
#[account(
    miden_standards_access_pausable::Pausable,
    miden_standards_access_pausable_manager::Manager
)]
struct PausableAccount;

/// Asserts the account is not paused, pauses it and asserts it is paused.
#[tx_script]
fn run(_arg: Word, account: &mut PausableAccount) {
    assert!(!account.is_paused());
    account.pause();
    assert!(account.is_paused());
}
