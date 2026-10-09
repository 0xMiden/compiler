//! Transaction script which reads the token configuration of a standard (MASM) fungible faucet
//! and checks it against the values the host configured the faucet with.
//!
//! `get_token_config` returns a four-field record, so the script pins the order in which a MASM
//! callee's multi-element result comes back.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the methods of the standard fungible faucet
/// component, whose interface is derived from the `miden-standards-faucets-fungible-faucet`
/// package manifest.
#[account(miden_standards_faucets_fungible_faucet::FungibleFaucet)]
struct Faucet;

/// The token configuration the faucet is expected to have, transported via the `TX_SCRIPT_ARGS`
/// word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The token supply.
    pub supply: Felt,
    /// The maximum token supply.
    pub max_supply: Felt,
    /// The token decimals.
    pub decimals: u8,
    /// The encoded token symbol.
    pub symbol: Felt,
}

/// Reads the faucet's token configuration and asserts each field equals the expected one.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut Faucet) {
    // The expected values are read before the call and compared after it, so they stay live on
    // the caller's side across the call.
    let config = account.get_token_config();
    assert_eq(config.supply, args.supply);
    assert_eq(config.max_supply, args.max_supply);
    assert!(config.decimals == args.decimals, "the decimals must match");
    assert_eq(config.symbol, args.symbol);
}
