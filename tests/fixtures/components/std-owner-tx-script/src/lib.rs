//! Transaction script which reads the owner of a standard (MASM) `ownable2step` component and
//! checks it against the owner the host configured the account with.
//!
//! `get_owner` returns a two-field record, so the script pins the order in which a MASM callee's
//! multi-element result comes back.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the methods of the standard `ownable2step`
/// component, whose interface is derived from the `miden-standards-access-ownable2step` package
/// manifest.
#[account(miden_standards_access_ownable2step::Ownable2step)]
struct Ownable;

/// The owner the account is expected to have, transported via the `TX_SCRIPT_ARGS` word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The owner account id's suffix.
    pub owner_suffix: Felt,
    /// The owner account id's prefix.
    pub owner_prefix: Felt,
}

/// Reads the account's owner and asserts it equals the expected one.
///
/// The component's `AccountId` record has the fields `{ suffix, prefix }`, so it is the
/// dependency's own type rather than `miden::AccountId`.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut Ownable) {
    // The expected values are read before the call and compared after it, so they stay live on
    // the caller's side across the call.
    let owner = account.get_owner();
    assert_eq(owner.suffix, args.owner_suffix);
    assert_eq(owner.prefix, args.owner_prefix);
}
