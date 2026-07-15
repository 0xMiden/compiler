//! Mock-chain tests for the typed fungible-asset amount API (`AssetAmount`).
//!
//! Unlike the unit tests in `miden-base-sys`, which decode hand-built asset encodings, these
//! tests execute the on-chain `AssetAmount` API inside a real transaction: the note script
//! decodes amounts from kernel-built assets and checks its arithmetic against the kernel's own
//! balance bookkeeping.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, AssetCallbackFlag, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;
use midenc_integration_test_support::project;

use super::support::{
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency, note_script_root,
    single_note_cycles,
};

/// On-chain note script exercising the `AssetAmount` API against live kernel state.
///
/// For every note asset it decodes the typed amount from the kernel-built encoding, receives the
/// asset into the wallet, and verifies the balance delta with checked arithmetic, comparisons,
/// and integer conversion. Any violated assertion aborts the transaction.
const ASSET_AMOUNT_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, Word, account, active_note, note};

/// Native account of the note: exposes the `basic-wallet` component methods.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// A note that transfers its assets to the consuming account while verifying the typed
/// asset-amount API against the transaction kernel's view of the vault.
#[note]
struct AssetAmountNote;

#[note]
impl AssetAmountNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let assets = active_note::get_assets();
        for asset in assets {
            // Decode the typed amount from the kernel-built fungible asset encoding.
            let amount = asset.amount().unwrap();
            assert!(amount > AssetAmount::ZERO);

            let before = account.get_balance(asset.key);
            account.receive_asset(asset);
            let after = account.get_balance(asset.key);

            // The balance must grow by exactly the decoded amount (checked addition).
            assert_eq!(after, before + amount);
            // Checked subtraction inverts the addition.
            assert_eq!(after - amount, before);
            assert_eq!(after - before, amount);
            // Amounts order and convert like integers.
            assert!(before < after);
            assert_eq!(after.as_u64(), before.as_u64() + amount.as_u64());
        }
    }
}
"#;

/// Compiles the basic-wallet example and returns its package and canonical project root.
///
/// The wallet must be compiled before any dependent note project so that its
/// `target/generated-wit` directory exists.
fn compile_wallet_package() -> (Arc<Package>, PathBuf) {
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let wallet_root = std::fs::canonicalize("../../examples/basic-wallet")
        .expect("failed to canonicalize the basic-wallet example path");
    (wallet_package, wallet_root)
}

/// Generates and compiles a note project with the given source, depending on the basic-wallet
/// example.
fn compile_note_package(note_name: &str, source: &str, wallet_root: &Path) -> Arc<Package> {
    let note_package_name = format!("miden:{note_name}");
    let note_project = project(note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                note_name,
                &note_package_name,
                "miden:basic-wallet",
                wallet_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(
                note_name,
                &note_package_name,
                "miden:basic-wallet",
                wallet_root,
            ),
        )
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(note_project.root(), true)
}

/// Returns a note script performing a fixed chain of four dependent additions through the
/// provided expression body, used to compare the cycle costs of the addition implementations.
///
/// `add_body` is the body of a `fn(a: AssetAmount, b: AssetAmount) -> AssetAmount` helper, e.g.
/// `a + b`.
fn measurement_note_source(add_body: &str) -> String {
    format!(
        r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{{AssetAmount, Word, account, active_note, note}};

/// Native account of the note: exposes the `basic-wallet` component methods.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// The measured addition implementation.
#[inline(always)]
fn checked_add(a: AssetAmount, b: AssetAmount) -> AssetAmount {{
    {add_body}
}}

/// A note performing a fixed chain of checked additions for cycle measurement.
#[note]
struct MeasurementNote;

#[note]
impl MeasurementNote {{
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {{
        let assets = active_note::get_assets();
        for asset in assets {{
            let amount = asset.amount().unwrap();
            let first = checked_add(amount, amount);
            let second = checked_add(first, amount);
            let third = checked_add(second, amount);
            let fourth = checked_add(third, amount);
            assert!(fourth > amount);
            account.receive_asset(asset);
        }}
    }}
}}
"#
    )
}

/// Tests the on-chain `AssetAmount` API (`Asset::amount`, checked `+`/`-`, ordering, `as_u64`)
/// against kernel-built assets and balances on a mock chain.
///
/// Flow:
/// - The faucet emits two amount-check notes carrying different fungible amounts
/// - The wallet consumes both notes in one transaction, so the note script checks the typed
///   arithmetic once against a zero starting balance and once against a non-zero one
/// - The committed vault must hold the sum of both amounts
#[test]
fn asset_amount_api_matches_kernel_balances() {
    // Compile the contracts first (before creating any runtime)
    let (wallet_package, wallet_root) = compile_wallet_package();
    let note_package =
        compile_note_package("asset-amount-note", ASSET_AMOUNT_NOTE_SOURCE, &wallet_root);

    let wallet_component =
        AccountComponent::from_package(&wallet_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            max_supply,
            None,
        )
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting two amount-check notes from the faucet ===");
    let first_amount = 100_000u64;
    let second_amount = 25_000u64;
    let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let notes = [first_amount, second_amount].map(|amount| {
        let mint_asset = FungibleAsset::new(faucet_id, amount)
            .unwrap()
            .with_callbacks(AssetCallbackFlag::Enabled);
        NoteBuilder::new(faucet_id, &mut note_rng)
            .package((*note_package).clone())
            .add_assets([Asset::from(mint_asset)])
            .build()
            .unwrap()
    });

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script = build_send_notes_script(&faucet_account, &notes);
    let mint_tx_context_builder = chain
        .build_tx_context(faucet_id, &[], &[])
        .unwrap()
        .tx_script(mint_tx_script)
        .extend_expected_output_notes(
            notes.iter().cloned().map(RawOutputNote::Full).collect::<Vec<_>>(),
        );
    execute_tx(&mut chain, mint_tx_context_builder);

    eprintln!("\n=== Step 2: Alice consumes both notes; the scripts assert the amount API ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx_context_builder = chain
        .build_tx_context(alice_id, &[notes[0].id(), notes[1].id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    execute_tx(&mut chain, consume_tx_context_builder);

    eprintln!("\n=== Step 3: Checking Alice's committed balance is the checked sum ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, first_amount + second_amount);
}

/// A note script with the measurement-script shape but no additions, isolating the fixed
/// note-execution overhead (note setup, `get_assets`, the amount decode, `receive_asset`).
/// Subtracting its cycle count from the measurement note's yields the absolute cost of the
/// note's four additions.
const BASELINE_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, Word, account, active_note, note};

/// Native account of the note: exposes the `basic-wallet` component methods.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// A note performing no additions, used as the cycle-measurement baseline.
#[note]
struct BaselineNote;

#[note]
impl BaselineNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let assets = active_note::get_assets();
        for asset in assets {
            let amount = asset.amount().unwrap();
            assert!(amount > AssetAmount::ZERO);
            account.receive_asset(asset);
        }
    }
}
"#;

/// Measures the note-execution cycle cost of the panicking felt-native `+` operator.
///
/// The measurement note performs four dependent additions; the addition-free baseline note
/// isolates the fixed note-execution overhead, so the absolute cost of the four additions is
/// the measurement count minus the baseline. Each note is consumed by a fresh wallet account,
/// so the vault bookkeeping costs are symmetric.
#[test]
fn asset_amount_add_cycles() {
    // Compile the contracts first (before creating any runtime)
    let (wallet_package, wallet_root) = compile_wallet_package();
    let add_note_package = compile_note_package(
        "asset-amount-add-note",
        &measurement_note_source("a + b"),
        &wallet_root,
    );
    let baseline_note_package =
        compile_note_package("asset-amount-baseline-note", BASELINE_NOTE_SOURCE, &wallet_root);

    let wallet_component =
        AccountComponent::from_package(&wallet_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            max_supply,
            None,
        )
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component.clone()],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let bob_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let bob_id = bob_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting the measurement and baseline notes ===");
    let amount = 100_000u64;
    let build_note = |note_package: &Arc<Package>| {
        let mint_asset = FungibleAsset::new(faucet_id, amount)
            .unwrap()
            .with_callbacks(AssetCallbackFlag::Enabled);
        let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
        NoteBuilder::new(faucet_id, &mut note_rng)
            .package((**note_package).clone())
            .add_assets([Asset::from(mint_asset)])
            .build()
            .unwrap()
    };
    let add_note = build_note(&add_note_package);
    let baseline_note = build_note(&baseline_note_package);

    let notes = [add_note.clone(), baseline_note.clone()];
    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script = build_send_notes_script(&faucet_account, &notes);
    let mint_tx_context_builder = chain
        .build_tx_context(faucet_id, &[], &[])
        .unwrap()
        .tx_script(mint_tx_script)
        .extend_expected_output_notes(
            notes.iter().cloned().map(RawOutputNote::Full).collect::<Vec<_>>(),
        );
    execute_tx(&mut chain, mint_tx_context_builder);

    eprintln!("\n=== Step 2: Alice consumes the addition note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let add_tx_context_builder = chain
        .build_tx_context(alice_id, &[add_note.id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    let add_measurements = execute_tx(&mut chain, add_tx_context_builder);

    eprintln!("\n=== Step 3: Bob consumes the addition-free baseline note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let baseline_tx_context_builder = chain
        .build_tx_context(bob_id, &[baseline_note.id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    let baseline_measurements = execute_tx(&mut chain, baseline_tx_context_builder);

    let add_cycles = single_note_cycles(&add_measurements);
    let baseline_cycles = single_note_cycles(&baseline_measurements);
    let add_total = add_cycles.parse::<i64>().unwrap() - baseline_cycles.parse::<i64>().unwrap();
    eprintln!(
        "\n=== Note execution: baseline (no additions) = {baseline_cycles} cycles, `+` = \
         {add_cycles} cycles; four additions cost {add_total} cycles ({} per add) ===",
        add_total / 4,
    );

    expect!["6977"].assert_eq(add_cycles);
    expect!["6200"].assert_eq(baseline_cycles);

    // Both wallets received their asset, so both scripts ran to completion.
    assert_account_has_fungible_asset(
        chain.committed_account(alice_id).unwrap(),
        faucet_id,
        amount,
    );
    assert_account_has_fungible_asset(chain.committed_account(bob_id).unwrap(), faucet_id, amount);
}
