//! The WIT interfaces of the `miden-standards` account components, pinned as snapshots.
//!
//! Each snapshot is `tests/standards/<package name>.wit`; run with `UPDATE_BINDINGS=1` to
//! regenerate them.

use std::path::Path;

use miden_standards::account::{
    access::{Authority, Ownable2Step, Pausable, PausableManager, RoleBasedAccessControl},
    auth::{
        AuthGuardedMultisig, AuthMultisig, AuthMultisigSmart, AuthNetworkAccount, AuthSingleSig,
        AuthTxFeeCollector, NoAuth,
    },
    faucets::{FungibleFaucet, NonFungibleFaucet},
    fees::{BasicConstantFeePolicy, ConstantFeeManager},
    inspection::{AccountSchemaCommitment, CodeInspection},
    note_creator::NoteCreator,
    oracle::PriceOracle,
    policies::{
        AllowlistManager, BasicAllowlist, BasicBlocklist, BlocklistManager, BurnAllowAll,
        BurnOwnerOnly, MinBurnAmount, MintAllowAll, MintOwnerOnly, TokenPolicyManager,
        TransferAllowAll,
    },
    upgrade::UpgradeManager,
    wallets::BasicWallet,
};
use midenc_integration_test_support::testing::bindings::check_generated;
use midenc_package_interface::PackageInterface;
use midenc_package_wit::{Generated, Options, generate};

/// The SDK's core-types package, which every generated document `use`s.
const MIDEN_WIT: &str = include_str!("../../base-macros/wit/miden.wit");

/// Every account component of `miden-standards`.
fn components() -> Vec<&'static miden_mast_package::Package> {
    [
        Authority::code(),
        Ownable2Step::code(),
        Pausable::code(),
        PausableManager::code(),
        RoleBasedAccessControl::code(),
        AuthGuardedMultisig::code(),
        AuthMultisig::code(),
        AuthMultisigSmart::code(),
        AuthNetworkAccount::code(),
        AuthSingleSig::code(),
        AuthTxFeeCollector::code(),
        NoAuth::code(),
        FungibleFaucet::code(),
        NonFungibleFaucet::code(),
        BasicConstantFeePolicy::code(),
        ConstantFeeManager::code(),
        AccountSchemaCommitment::code(),
        CodeInspection::code(),
        NoteCreator::code(),
        PriceOracle::code(),
        AllowlistManager::code(),
        BasicAllowlist::code(),
        BasicBlocklist::code(),
        BlocklistManager::code(),
        BurnAllowAll::code(),
        BurnOwnerOnly::code(),
        MinBurnAmount::code(),
        MintAllowAll::code(),
        MintOwnerOnly::code(),
        TokenPolicyManager::code(),
        TransferAllowAll::code(),
        UpgradeManager::code(),
        BasicWallet::code(),
    ]
    .into_iter()
    .map(|code| code.as_package())
    .collect()
}

/// Generate the interface of every component, keyed by package name.
fn generated() -> Vec<(String, Generated)> {
    components()
        .into_iter()
        .map(|package| {
            let name = package.name.to_string();
            let generated = generate(&PackageInterface::from_package(package), &Options::default())
                .unwrap_or_else(|err| panic!("{name}: {err}"));
            (name, generated)
        })
        .collect()
}

#[test]
fn every_component_matches_its_snapshot() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let generated = generated();
    assert_eq!(generated.len(), 33);
    for (name, generated) in &generated {
        check_generated(manifest_dir, &format!("tests/standards/{name}.wit"), &generated.wit);
    }
}

#[test]
fn every_interface_parses_with_external_ids() {
    for (name, generated) in generated() {
        let mut resolve = wit_parser::Resolve::new();
        resolve.push_str("miden.wit", MIDEN_WIT).expect("the core types parse");
        let package = resolve
            .push_str(format!("{name}.wit"), &generated.wit)
            .unwrap_or_else(|err| panic!("{name}: generated WIT must parse: {err:?}"));
        let package = &resolve.packages[package];
        assert_eq!(package.name.to_string(), generated.package_id, "{name}");
        let interface = package.interfaces[&generated.interface];
        assert!(package.worlds.contains_key(&generated.world), "{name}");
        for function in resolve.interfaces[interface].functions.values() {
            let external_id = function
                .external_id
                .as_deref()
                .unwrap_or_else(|| panic!("{name}: `{}` has no external id", function.name));
            let procedure = external_id.strip_prefix("miden::standards::components::");
            assert!(
                procedure.is_some_and(|procedure| !procedure.is_empty()),
                "{name}: `{}` is `{external_id}`",
                function.name
            );
        }
    }
}

#[test]
fn generated_and_skipped_totals() {
    let generated = generated();
    let functions: usize = generated
        .iter()
        .map(|(_, generated)| generated.wit.matches("@external-id(").count())
        .sum();
    let skipped: Vec<_> = generated.iter().flat_map(|(_, generated)| &generated.skipped).collect();
    assert_eq!((functions, skipped.len()), (93, 27));
    // Every standard procedure has a typed signature the mapping covers; the ones left out all
    // return more than one value (a word, an asset, a struct, or several results).
    for skipped in skipped {
        assert!(
            skipped.reason.starts_with("results flatten to "),
            "{}: {}",
            skipped.path,
            skipped.reason
        );
    }
}
