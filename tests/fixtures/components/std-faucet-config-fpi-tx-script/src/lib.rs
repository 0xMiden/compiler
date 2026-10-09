//! Transaction script which reads the token configuration of a standard (MASM) fungible faucet
//! through foreign procedure invocation and checks it against the values the host configured the
//! faucet with.
//!
//! The script runs on another account, so `get_token_config`'s four-field record comes back
//! across the FPI boundary, which pins the order of a foreign MASM callee's multi-element result.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Foreign account of the transaction script: exposes the methods of the standard fungible faucet
/// component, whose interface is derived from the `miden-standards-faucets-fungible-faucet`
/// package manifest.
#[account(miden_standards_faucets_fungible_faucet::FungibleFaucet)]
struct Faucet;

/// The faucet to read and the token configuration it is expected to have, transported via the
/// `TX_SCRIPT_ARGS` word as a commitment to the advice-provided felts.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The faucet account id's suffix.
    pub faucet_suffix: Felt,
    /// The faucet account id's prefix.
    pub faucet_prefix: Felt,
    /// The token supply.
    pub supply: Felt,
    /// The maximum token supply.
    pub max_supply: Felt,
    /// The token decimals.
    pub decimals: u8,
    /// The encoded token symbol.
    pub symbol: Felt,
}

/// Reads the foreign faucet's token configuration and asserts each field equals the expected one.
#[tx_script]
fn run(args: TxScriptArgs) {
    let faucet = Faucet::new(AccountId::new(args.faucet_prefix, args.faucet_suffix));
    let config = faucet.get_token_config();
    assert_eq(config.supply, args.supply);
    assert_eq(config.max_supply, args.max_supply);
    assert!(config.decimals == args.decimals, "the decimals must match");
    assert_eq(config.symbol, args.symbol);
}
