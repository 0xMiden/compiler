//! Mock-chain tests for the typed fungible-asset amount API (`AssetAmount`).
//!
//! These tests execute the on-chain `AssetAmount` API inside a real transaction: the note script
//! decodes amounts from kernel-built assets and checks its arithmetic against the kernel's own
//! vault bookkeeping.
//!
//! They also cover the asset shape checks (`Asset::is_fungible`, and `Asset::amount` rejecting
//! non-fungible and out-of-range assets), which read the asset composition through the protocol
//! library and therefore only run on the VM.

use std::{path::Path, sync::Arc};

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    Felt, Word,
    account::auth::AuthScheme,
    asset::{NonFungibleAsset, NonFungibleAssetDetails},
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_integration_test_support::{cargo_proj::Project, project};

use super::support::{
    account_cargo_toml_for, account_miden_project_toml_with_interface,
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    execute_tx_expect_failure, note_cargo_toml_for_dependency,
    note_miden_project_toml_for_dependency, note_script_root,
};

/// Project name of the generated wallet account component.
const WALLET_NAME: &str = "asset-amount-wallet";
/// Miden package name of the generated wallet account component.
const WALLET_PACKAGE: &str = "miden:asset-amount-wallet";

/// Wallet account component consumed by the amount-check note.
///
/// The kernel restricts vault reads to the account context, so the component exposes the typed
/// vault amount as an account procedure; returning `AssetAmount` also exercises the WIT
/// `asset-amount` core type across the component boundary at run time.
const AMOUNT_WALLET_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{Asset, AssetAmount, AssetId, active_account, component, component_storage};

#[component_storage]
struct AmountWalletStorage;

/// API of the amount-check wallet account component.
#[component]
trait AmountWallet {
    /// Adds an asset to the account vault.
    #[account_procedure]
    fn receive_asset(&mut self, asset: Asset);
    /// Returns the typed amount currently held in the vault under `asset_id`.
    #[account_procedure]
    fn vault_amount(&self, asset_id: AssetId) -> AssetAmount;
}

#[component]
impl AmountWallet for AmountWalletStorage {
    fn receive_asset(&mut self, asset: Asset) {
        self.add_asset(asset);
    }

    fn vault_amount(&self, asset_id: AssetId) -> AssetAmount {
        Asset::new(asset_id, active_account::get_asset(asset_id)).amount()
    }
}
"#;

/// On-chain note script exercising the `AssetAmount` API against live kernel state.
///
/// For every note asset it decodes the typed amount from the kernel-built encoding, receives the
/// asset into the wallet, and verifies the vault-amount delta with checked arithmetic,
/// comparisons, and integer conversion. Any violated assertion aborts the transaction.
const ASSET_AMOUNT_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, Word, account, active_note, note};

/// Native account of the note: exposes the amount-wallet component methods.
#[account(asset_amount_wallet::AmountWallet)]
pub struct Wallet;

/// A note that transfers its assets to the consuming account while verifying the typed
/// asset-amount API against the transaction kernel's view of the vault.
#[note]
struct AssetAmountNote;

#[note]
impl AssetAmountNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        let assets = active_note::get_initial_assets();
        for asset in assets {
            // Decode the typed amount from the kernel-built fungible asset encoding.
            let amount = asset.amount();
            assert!(amount > AssetAmount::ZERO);

            let before = account.vault_amount(asset.id);
            account.receive_asset(asset);
            let after = account.vault_amount(asset.id);

            // The vault amount must grow by exactly the decoded amount (checked addition).
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

/// On-chain note script exercising the asset shape checks on an asset taken from its storage.
///
/// The note storage holds `[mode, id0, id1, id2, id3, value0, value1, value2, value3]`: mode `0`
/// asserts the asset is not fungible, mode `1` decodes its fungible amount. Any violated check
/// aborts the transaction.
const ASSET_SHAPE_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{Asset, AssetAmount, Felt, Word, account, active_note, felt, note};

/// Native account of the note; the script does not use it.
#[account(asset_amount_wallet::AmountWallet)]
pub struct Wallet;

/// A note that checks the shape of the asset encoded in its storage.
#[note]
struct AssetShapeNote;

#[note]
impl AssetShapeNote {
    #[note_script]
    pub fn script(self, _arg: Word, _account: &mut Wallet) {
        let storage = active_note::get_storage();
        assert_eq!(storage.len(), 9);
        let mode = storage[0];
        let id: [Felt; 4] = storage[1..5].try_into().unwrap();
        let value: [Felt; 4] = storage[5..9].try_into().unwrap();
        let asset = Asset::new(id, value);

        if mode == felt!(0) {
            assert!(!asset.is_fungible());
        } else if mode == felt!(1) {
            let amount = asset.amount();
            assert!(amount > AssetAmount::ZERO);
        } else {
            panic!();
        }
    }
}
"#;

/// Generates and compiles the wallet account component project.
///
/// The returned [`Project`] keeps the generated directory alive: the dependent note project
/// resolves the wallet dependency from that path.
fn build_wallet_project() -> (Project, Arc<Package>) {
    let wallet_project = project(WALLET_NAME)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                WALLET_NAME,
                WALLET_PACKAGE,
                "amount-wallet",
            ),
        )
        .file("Cargo.toml", &account_cargo_toml_for(WALLET_NAME, WALLET_PACKAGE))
        .file("src/lib.rs", AMOUNT_WALLET_SOURCE)
        .build();
    let wallet_package = compile_rust_package(wallet_project.root(), true);
    (wallet_project, wallet_package)
}

/// Generates and compiles a note project with the given source, depending on the generated
/// wallet component.
fn compile_note_package(note_name: &str, source: &str, wallet_root: &Path) -> Arc<Package> {
    let note_package_name = format!("miden:{note_name}");
    let note_project = project(note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                note_name,
                &note_package_name,
                WALLET_PACKAGE,
                wallet_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(note_name, WALLET_PACKAGE, wallet_root),
        )
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(note_project.root(), true)
}

/// Tests the on-chain `AssetAmount` API (`Asset::amount`, checked `+`/`-`, ordering, `as_u64`)
/// against kernel-built assets and vault amounts on a mock chain.
///
/// Flow:
/// - The faucet emits two amount-check notes carrying different fungible amounts
/// - The wallet consumes both notes in one transaction, so the note script checks the typed
///   arithmetic once against a zero starting vault amount and once against a non-zero one
/// - The committed vault must hold the sum of both amounts
#[test]
fn asset_amount_api_matches_kernel_balances() {
    // Compile the contracts first (before creating any runtime)
    let (wallet_project, wallet_package) = build_wallet_project();
    let note_package =
        compile_note_package("asset-amount-note", ASSET_AMOUNT_NOTE_SOURCE, wallet_project.root());

    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

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
        let mint_asset = FungibleAsset::new(faucet_id, amount).unwrap();
        NoteBuilder::new(faucet_id, &mut note_rng)
            .package((*note_package).clone())
            .add_assets([Asset::from(mint_asset)])
            .build()
            .unwrap()
    });

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script = build_send_notes_script(&faucet_account, &notes);
    let mint_tx = chain
        .build_transaction(faucet_id)
        .send_notes_script(&mint_tx_script)
        .expected_output_notes(notes.iter().cloned().map(RawOutputNote::Full).collect::<Vec<_>>())
        .build()
        .unwrap();
    execute_tx(&mut chain, mint_tx);

    eprintln!("\n=== Step 2: Alice consumes both notes; the scripts assert the amount API ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_notes([notes[0].id(), notes[1].id()])
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    execute_tx(&mut chain, consume_tx);

    eprintln!("\n=== Step 3: Checking Alice's committed vault holds the checked sum ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, first_amount + second_amount);
}

/// The fixed VM assertion code every guest panic reports, so the failing shape checks cannot be
/// satisfied by an unrelated kernel failure.
const GUEST_PANIC_CODE: &str = "assertion failed with error code: 10154102372021603817";

/// Builds the nine-felt shape-note storage `[mode, id word, value word]`.
fn shape_note_storage(mode: u64, id: Word, value: Word) -> Vec<Felt> {
    let mut storage = vec![Felt::new(mode).unwrap()];
    storage.extend(id.iter().copied());
    storage.extend(value.iter().copied());
    storage
}

/// Tests the on-chain asset shape checks (`Asset::is_fungible`, `Asset::amount`) against
/// protocol-built assets on a mock chain.
///
/// Flow:
/// - Genesis holds four shape-check notes, each encoding one asset in its storage: a
///   non-fungible asset, a valid fungible asset, and the same two with checks that must fail
/// - Alice consumes each note in its own transaction
/// - `is_fungible()` on the non-fungible asset and `amount()` on the fungible one succeed
/// - `amount()` on the non-fungible asset and on a fungible asset whose amount exceeds the
///   protocol maximum abort the transaction
#[test]
fn asset_shape_checks_match_the_kernel_on_chain() {
    // Compile the contracts first (before creating any runtime)
    let (wallet_project, wallet_package) = build_wallet_project();
    let note_package =
        compile_note_package("asset-shape-note", ASSET_SHAPE_NOTE_SOURCE, wallet_project.root());

    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let faucet_id = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            1_000_000_000,
            None,
        )
        .unwrap()
        .id();
    let nft_faucet_id = builder
        .add_existing_non_fungible_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "NFT",
        )
        .unwrap()
        .id();
    let alice_id = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap()
        .id();

    let non_fungible = Asset::from(NonFungibleAsset::new(&NonFungibleAssetDetails::new(
        nft_faucet_id,
        vec![1, 2, 3, 4],
    )));
    let fungible = Asset::from(FungibleAsset::new(faucet_id, 42).unwrap());
    let oversized_amount = Felt::new(FungibleAsset::MAX_AMOUNT.as_u64() + 1).unwrap();
    let oversized_value = Word::from([oversized_amount, Felt::ZERO, Felt::ZERO, Felt::ZERO]);

    let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let mut shape_note = |storage: Vec<Felt>| {
        let note = NoteBuilder::new(alice_id, &mut note_rng)
            .package((*note_package).clone())
            .note_storage(storage)
            .unwrap()
            .build()
            .unwrap();
        builder.add_output_note(RawOutputNote::Full(note.clone()));
        note
    };
    let non_fungible_is_not_fungible = shape_note(shape_note_storage(
        0,
        non_fungible.id().to_word(),
        non_fungible.to_value_word(),
    ));
    let fungible_amount =
        shape_note(shape_note_storage(1, fungible.id().to_word(), fungible.to_value_word()));
    let non_fungible_amount = shape_note(shape_note_storage(
        1,
        non_fungible.id().to_word(),
        non_fungible.to_value_word(),
    ));
    let oversized_fungible_amount =
        shape_note(shape_note_storage(1, fungible.id().to_word(), oversized_value));

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Consuming the notes whose shape checks must succeed ===");
    for note in [&non_fungible_is_not_fungible, &fungible_amount] {
        let tx = chain
            .build_transaction(alice_id)
            .authenticated_input_notes([note.id()])
            .build()
            .unwrap();
        execute_tx(&mut chain, tx);
    }

    eprintln!("\n=== Step 2: Consuming the notes whose shape checks must abort ===");
    for (label, note) in [
        ("amount() on a non-fungible asset", &non_fungible_amount),
        ("amount() above the maximum", &oversized_fungible_amount),
    ] {
        let tx = chain
            .build_transaction(alice_id)
            .authenticated_input_notes([note.id()])
            .build()
            .unwrap();
        let err = execute_tx_expect_failure(tx);
        eprintln!("{label} aborted the transaction: {err}");
        assert!(err.contains(GUEST_PANIC_CODE), "{label} failed for an unexpected reason: {err}");
    }
}
