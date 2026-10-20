//! Transaction script which asks a standard (MASM) `rbac` component whether an account holds a
//! role and checks the answer against the one the host expects.
//!
//! `has_role` takes the account id as a `miden::AccountId` parameter, so the script pins the
//! order in which an account id's felts reach a MASM callee.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the methods of the standard `rbac`
/// component, whose interface is derived from the `miden-standards-access-rbac` package
/// manifest.
#[account(miden_standards_access_rbac::Rbac)]
struct Roles;

/// The role query and its expected answer, transported via the `TX_SCRIPT_ARGS` word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The role symbol to query.
    pub role: Felt,
    /// The queried account id's suffix.
    pub account_suffix: Felt,
    /// The queried account id's prefix.
    pub account_prefix: Felt,
    /// The expected answer: 1 when the account holds the role, 0 otherwise.
    pub expected: Felt,
}

/// Queries the role of the account given in the arguments and asserts the expected answer.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut Roles) {
    let queried = AccountId::new(args.account_prefix, args.account_suffix);
    let answer = if account.has_role(args.role, queried) {
        felt!(1)
    } else {
        felt!(0)
    };
    assert_eq(answer, args.expected);
}
