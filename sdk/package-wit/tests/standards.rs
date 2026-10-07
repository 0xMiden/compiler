//! The WIT interfaces of the `miden-standards` account components, pinned as snapshots.
//!
//! Each component that has an interface has its snapshot at `tests/standards/<package name>.wit`;
//! run with `UPDATE_BINDINGS=1` to regenerate them.

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
use midenc_package_wit::{Error, Generated, Options, Skipped, generate};

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

/// The components whose every interface procedure is left out, so they have no interface: each
/// procedure's results occupy more than one stack element.
const WITHOUT_INTERFACE: [&str; 4] = [
    "miden-standards-faucets-policies-mint-allow-all",
    "miden-standards-faucets-policies-mint-owner-controlled-owner-only",
    "miden-standards-fees-policies-basic-constant-fee",
    "miden-standards-inspection-schema-commitment",
];

/// The generation result of every component, keyed by package name.
fn results() -> Vec<(String, Result<Generated, Error>)> {
    components()
        .into_iter()
        .map(|package| {
            let name = package.name.to_string();
            (name, generate(&PackageInterface::from_package(package), &Options::default()))
        })
        .collect()
}

/// Generate the interface of every component that has one, keyed by package name.
fn generated() -> Vec<(String, Generated)> {
    results()
        .into_iter()
        .filter(|(name, _)| !WITHOUT_INTERFACE.contains(&name.as_str()))
        .map(|(name, result)| {
            let generated = result.unwrap_or_else(|err| panic!("{name}: {err}"));
            (name, generated)
        })
        .collect()
}

/// The procedures left out of each component in [`WITHOUT_INTERFACE`].
fn left_out_entirely() -> Vec<(String, Vec<Skipped>)> {
    results()
        .into_iter()
        .filter(|(name, _)| WITHOUT_INTERFACE.contains(&name.as_str()))
        .map(|(name, result)| match result {
            Err(Error::EverythingSkipped(skipped)) => (name, skipped),
            other => panic!("{name}: expected every procedure to be left out, got {other:?}"),
        })
        .collect()
}

/// Every standard component's generated WIT matches its snapshot, with no stale snapshots.
#[test]
fn every_component_matches_its_snapshot() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let generated = generated();
    assert_eq!(generated.len(), 33 - WITHOUT_INTERFACE.len());
    for (name, generated) in &generated {
        check_generated(manifest_dir, &format!("tests/standards/{name}.wit"), &generated.wit);
    }
    // A snapshot of a component renamed or removed upstream is stale, too.
    let mut expected: Vec<String> =
        generated.iter().map(|(name, _)| format!("{name}.wit")).collect();
    expected.sort();
    let mut snapshots: Vec<String> = std::fs::read_dir(manifest_dir.join("tests/standards"))
        .expect("the snapshot directory exists")
        .map(|entry| entry.expect("a snapshot entry").file_name().to_string_lossy().into_owned())
        .collect();
    snapshots.sort();
    assert_eq!(snapshots, expected, "the snapshot files are exactly the generated interfaces");
}

/// Standard components without an interface report their left-out procedures and no snapshot.
#[test]
fn components_without_an_interface_list_their_procedures() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let left_out = left_out_entirely();
    assert_eq!(left_out.len(), WITHOUT_INTERFACE.len());
    for (name, skipped) in left_out {
        assert!(!skipped.is_empty(), "{name}");
        assert!(
            !manifest_dir.join(format!("tests/standards/{name}.wit")).exists(),
            "{name} has no interface, so no snapshot"
        );
    }
}

/// Every generated interface parses and each function has a standards procedure external id.
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

/// The standards yield the expected function and left-out counts, all for multi-element results.
#[test]
fn generated_and_skipped_totals() {
    let generated = generated();
    let functions: usize = generated
        .iter()
        .map(|(_, generated)| generated.wit.matches("@external-id(").count())
        .sum();
    let left_out = left_out_entirely();
    let skipped: Vec<&Skipped> = generated
        .iter()
        .flat_map(|(_, generated)| &generated.skipped)
        .chain(left_out.iter().flat_map(|(_, skipped)| skipped))
        .collect();
    assert_eq!((functions, skipped.len()), (93, 27));
    // Every standard procedure has a typed signature the mapping covers; the ones left out all
    // have results occupying more than one stack element (a word, an asset, a struct, or several
    // results).
    for skipped in skipped {
        assert!(
            skipped.reason.starts_with("results occupy "),
            "{}: {}",
            skipped.path,
            skipped.reason
        );
    }
}
