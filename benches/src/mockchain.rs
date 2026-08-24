use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use miden_core::{Felt, Word, crypto::hash::Poseidon2, mast::MastForest, serde::Serializable};
use miden_debug::{ReplaySnapshot, clone_advice_mutations, flamegraph::FlamegraphProfile};
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_mast_package::Package;
use miden_processor::{
    BaseHost, ExecutionError, ExecutionOptions, ExecutionOutput, FastProcessor, FutureMaybeSend,
    Host, LoadedMastForest, ProcessorState, StackInputs, StackOutputs,
    advice::{AdviceInputs, AdviceMutation},
    event::EventError,
};
use miden_protocol::{
    account::{
        AccountBuilder, AccountComponent, AccountId, AccountType, StorageSlotName,
        auth::{AuthScheme, AuthSecretKey},
        component::InitStorageData,
    },
    assembly::{Path as MasmPath, ProcedureName},
    asset::{Asset, FungibleAsset},
    crypto::rand::{FeltRng, RandomCoin},
    note::{NoteScript, NoteTag, NoteType},
    transaction::{ExecutedTransaction, RawOutputNote, TransactionKernel, TransactionScript},
    vm::{
        DebugSourceNodeId, PackageDebugInfo, PackageDebugInfoError, PackageExport, ProcedureExport,
        Section, SectionId, TargetType,
    },
};
use miden_standards::{account::wallets::BasicWallet, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain, MockTransaction, MockTransactionBuilder};
use miden_tx::{
    ProgramExecutor, TransactionExecutor, TransactionExecutorError, auth::BasicAuthenticator,
};
use miden_tx_script_args::{EncodedScriptArgs, ScriptArgs};
use rand::{SeedableRng, rngs::StdRng};

use super::{BenchmarkRunner, BuildFailure, TransactionBenchmarkResult, load_package};

const AUTH_NO_AUTH: &str = "auth-component-no-auth";
const AUTH_RPO_FALCON512: &str = "auth-component-rpo-falcon512";
const BASIC_WALLET: &str = "basic-wallet";
const BASIC_WALLET_TX_SCRIPT: &str = "basic-wallet-tx-script";
const COUNTER_CONTRACT: &str = "counter-contract";
const COUNTER_NOTE: &str = "counter-note";
const P2ID_NOTE: &str = "p2id-note";
const P2ID_TX_SCRIPT: &str = "p2id-tx-script";
const P2IDE_NOTE: &str = "p2ide-note";
const STORAGE_EXAMPLE: &str = "storage-example";
const STORAGE_TX_SCRIPT: &str = "storage-tx-script";
const COUNTER_STORAGE_KEY: Word = Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
const STORAGE_OWNER_KEY: Word = Word::new([Felt::ONE, Felt::ZERO, Felt::ZERO, Felt::ZERO]);

type ScenarioPackages = BTreeMap<&'static str, Arc<Package>>;

struct TransactionScenario {
    name: &'static str,
    examples: &'static [&'static str],
    packages: &'static [&'static str],
    build: fn(&ScenarioPackages) -> Result<MockTransaction>,
}

#[derive(FromFeltRepr, ToFeltRepr)]
struct BasicWalletScriptArgs {
    tag: miden_field::Felt,
    note_type: miden_field::Felt,
    recipient: miden_field::Word,
    asset_key: miden_field::Word,
    asset_value: miden_field::Word,
}

const TRANSACTION_SCENARIOS: &[TransactionScenario] = &[
    TransactionScenario {
        name: AUTH_NO_AUTH,
        examples: &[AUTH_NO_AUTH],
        packages: &[AUTH_NO_AUTH, P2ID_NOTE],
        build: build_no_auth_transaction,
    },
    TransactionScenario {
        name: AUTH_RPO_FALCON512,
        examples: &[AUTH_RPO_FALCON512],
        packages: &[AUTH_RPO_FALCON512, P2ID_NOTE],
        build: build_rpo_auth_transaction,
    },
    TransactionScenario {
        name: COUNTER_NOTE,
        examples: &[COUNTER_CONTRACT, COUNTER_NOTE],
        packages: &[COUNTER_CONTRACT, COUNTER_NOTE],
        build: build_counter_transaction,
    },
    TransactionScenario {
        name: P2ID_NOTE,
        examples: &[BASIC_WALLET, P2ID_NOTE],
        packages: &[BASIC_WALLET, P2ID_NOTE],
        build: build_p2id_consumption,
    },
    TransactionScenario {
        name: BASIC_WALLET_TX_SCRIPT,
        examples: &[BASIC_WALLET_TX_SCRIPT],
        packages: &[BASIC_WALLET, BASIC_WALLET_TX_SCRIPT, P2ID_NOTE],
        build: build_basic_wallet_script_transaction,
    },
    TransactionScenario {
        name: P2ID_TX_SCRIPT,
        examples: &[P2ID_TX_SCRIPT],
        packages: &[BASIC_WALLET, P2ID_NOTE, P2ID_TX_SCRIPT],
        build: build_p2id_script_transaction,
    },
    TransactionScenario {
        name: P2IDE_NOTE,
        examples: &[P2IDE_NOTE],
        packages: &[BASIC_WALLET, P2IDE_NOTE],
        build: build_p2ide_consumption,
    },
    TransactionScenario {
        name: STORAGE_EXAMPLE,
        examples: &[STORAGE_EXAMPLE],
        packages: &[BASIC_WALLET, STORAGE_EXAMPLE, STORAGE_TX_SCRIPT],
        build: build_storage_transaction,
    },
];

#[cfg(test)]
pub(super) fn transaction_scenario_examples() -> Vec<&'static str> {
    TRANSACTION_SCENARIOS
        .iter()
        .flat_map(|scenario| scenario.examples.iter().copied())
        .collect()
}

impl BenchmarkRunner {
    pub(super) fn run_transaction_benchmarks(&self) -> Result<Vec<TransactionBenchmarkResult>> {
        let mut benchmarks = Vec::new();
        let mut optimized_packages = ScenarioPackages::new();
        let mut debug_packages = ScenarioPackages::new();
        for scenario in TRANSACTION_SCENARIOS {
            eprintln!("Benchmarking {}", scenario.name);
            match self.run_transaction_scenario(
                scenario,
                &mut optimized_packages,
                &mut debug_packages,
            ) {
                Ok(results) => benchmarks.extend(results),
                Err(err) if self.skip_failed_builds && err.is::<BuildFailure>() => {
                    eprintln!("Skipping {}: {err:#}", scenario.name);
                }
                Err(err) => return Err(err),
            }
        }
        Ok(benchmarks)
    }

    fn run_transaction_scenario(
        &self,
        scenario: &TransactionScenario,
        optimized_cache: &mut ScenarioPackages,
        debug_cache: &mut ScenarioPackages,
    ) -> Result<Vec<TransactionBenchmarkResult>> {
        let optimized_packages =
            self.load_scenario_packages(scenario.packages, "none", true, optimized_cache)?;
        let optimized_tx = (scenario.build)(&optimized_packages)?;
        let optimized_tx = block_on(optimized_tx.execute())
            .with_context(|| format!("optimized {} transaction execution failed", scenario.name))?;
        let cycles = optimized_tx.measurements().total_cycles();

        let debug_packages =
            self.load_scenario_packages(scenario.packages, "full", false, debug_cache)?;
        let debug_tx = (scenario.build)(&debug_packages)?;
        let capture = ReplayCapture::default();
        execute_with_replay_capture(debug_tx, capture.clone())?;

        let snapshot = capture
            .take()
            .ok_or_else(|| anyhow!("transaction execution did not produce a replay snapshot"))?;
        let relative_replay = format!("replays/{}.mdsnap", scenario.name);
        snapshot
            .write_to_file(self.output_dir.join(&relative_replay))
            .context("failed to write transaction replay snapshot")?;

        let profile = FlamegraphProfile::collect_replay(snapshot)
            .context("failed to collect transaction replay flamegraph")?;
        let relative_flamegraph = format!("flamegraphs/{}.svg", scenario.name);
        profile
            .write_svg(self.output_dir.join(&relative_flamegraph))
            .map_err(|err| anyhow!(err.to_string()))?;

        Ok(scenario
            .examples
            .iter()
            .map(|name| TransactionBenchmarkResult {
                name: (*name).into(),
                cycles,
                flamegraph: relative_flamegraph.clone(),
                replay: relative_replay.clone(),
            })
            .collect())
    }

    fn load_scenario_packages(
        &self,
        names: &'static [&'static str],
        debug: &str,
        persist: bool,
        cache: &mut ScenarioPackages,
    ) -> Result<ScenarioPackages> {
        let mut packages = ScenarioPackages::new();
        for name in names {
            let package = if let Some(package) = cache.get(name) {
                Arc::clone(package)
            } else {
                let package = self.load_scenario_package(name, debug, persist)?;
                cache.insert(name, Arc::clone(&package));
                package
            };
            packages.insert(name, package);
        }
        Ok(packages)
    }

    fn load_scenario_package(
        &self,
        name: &str,
        debug: &str,
        persist: bool,
    ) -> Result<Arc<Package>> {
        let saved = self.output_dir.join("packages").join(format!("{name}.masp"));
        if persist && saved.is_file() {
            return load_package(&saved);
        }

        let project_dir = if name == STORAGE_TX_SCRIPT {
            self.workspace_root.join("benches/fixtures/storage-tx-script")
        } else {
            self.workspace_root.join("examples").join(name)
        };
        let compiled = self
            .compile(&project_dir, debug)
            .map_err(|err| anyhow::Error::new(BuildFailure(err)))?;
        if persist {
            std::fs::copy(&compiled, &saved).with_context(|| {
                format!("failed to copy {} to {}", compiled.display(), saved.display())
            })?;
            load_package(&saved)
        } else {
            load_package(&compiled)
        }
    }
}

fn required_package<'a>(packages: &'a ScenarioPackages, name: &str) -> Result<&'a Arc<Package>> {
    packages.get(name).ok_or_else(|| anyhow!("scenario package {name} is missing"))
}

fn build_no_auth_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let auth_component = AccountComponent::from_package(
        required_package(packages, AUTH_NO_AUTH)?,
        &InitStorageData::default(),
    )?;
    let account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_component(auth_component)
        .with_component(BasicWallet)
        .build_existing()?;
    let mut builder = MockChain::builder();
    builder.add_account(account.clone())?;
    let note_package = required_package(packages, P2ID_NOTE)?;
    let note_root: Word = NoteScript::from_package(note_package)?.root().into();
    let note = NoteBuilder::new(account.id(), RandomCoin::new(note_root))
        .package((**note_package).clone())
        .note_storage(vec![account.id().prefix().as_felt(), account.id().suffix()])?
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(note.clone()));
    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    chain.build_transaction(account).authenticated_input_note(note.id()).build()
}

fn build_rpo_auth_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let mut rng = StdRng::seed_from_u64(1);
    let secret_key = AuthSecretKey::new_falcon512_poseidon2_with_rng(&mut rng);
    let public_key: Word = secret_key.public_key().to_commitment().into();
    let mut auth_storage = InitStorageData::default();
    auth_storage.insert_value(
        "auth_component_rpo_falcon512::auth_component::owner_public_key",
        public_key,
    )?;
    let auth_component = AccountComponent::from_package(
        required_package(packages, AUTH_RPO_FALCON512)?,
        &auth_storage,
    )?;
    let account = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(auth_component)
        .with_component(BasicWallet)
        .build_existing()?;
    let mut builder = MockChain::builder();
    builder.add_account(account.clone())?;
    let note_package = required_package(packages, P2ID_NOTE)?;
    let note_root: Word = NoteScript::from_package(note_package)?.root().into();
    let note = NoteBuilder::new(account.id(), RandomCoin::new(note_root))
        .package((**note_package).clone())
        .note_storage(vec![account.id().prefix().as_felt(), account.id().suffix()])?
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(note.clone()));
    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    chain
        .build_transaction(account)
        .authenticated_input_note(note.id())
        .authenticator(Some(BasicAuthenticator::new(std::slice::from_ref(&secret_key))))
        .build()
}

fn build_counter_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let counter_slot = StorageSlotName::new("counter_contract::counter_contract::count_map")?;
    let mut counter_storage = InitStorageData::default();
    counter_storage.insert_map_entry(counter_slot.clone(), COUNTER_STORAGE_KEY, 1_u64)?;
    let counter_component = AccountComponent::from_package(
        required_package(packages, COUNTER_CONTRACT)?,
        &counter_storage,
    )?;

    let mut builder = MockChain::builder();
    let counter_account = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([2_u8; 32])
            .account_type(AccountType::Public)
            .with_component(BasicWallet)
            .with_component(counter_component),
        AccountState::Exists,
    )?;

    let note_package = required_package(packages, COUNTER_NOTE)?;
    let note_root: Word = NoteScript::from_package(note_package)?.root().into();
    let counter_note = NoteBuilder::new(counter_account.id(), RandomCoin::new(note_root))
        .package((**note_package).clone())
        .tag(NoteTag::with_account_target(counter_account.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(counter_note.clone()));

    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    chain
        .build_transaction(counter_account.clone())
        .authenticated_input_note(counter_note.id())
        .build()
}

fn build_p2id_consumption(packages: &ScenarioPackages) -> Result<MockTransaction> {
    build_note_consumption(packages, P2ID_NOTE, |account_id| {
        vec![account_id.prefix().as_felt(), account_id.suffix()]
    })
}

fn build_p2ide_consumption(packages: &ScenarioPackages) -> Result<MockTransaction> {
    build_note_consumption(packages, P2IDE_NOTE, |account_id| {
        vec![account_id.suffix(), account_id.prefix().as_felt(), Felt::ZERO, Felt::ZERO]
    })
}

fn build_note_consumption(
    packages: &ScenarioPackages,
    note_name: &'static str,
    storage: impl FnOnce(AccountId) -> Vec<Felt>,
) -> Result<MockTransaction> {
    let wallet_component = AccountComponent::from_package(
        required_package(packages, BASIC_WALLET)?,
        &InitStorageData::default(),
    )?;
    let mut builder = MockChain::builder();
    let faucet = builder.add_existing_basic_faucet(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        "BENCH",
        1_000_000,
        None,
    )?;
    let account = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([3_u8; 32])
            .account_type(AccountType::Public)
            .with_component(wallet_component),
        AccountState::Exists,
    )?;
    let asset: Asset = FungibleAsset::new(faucet.id(), 100)?.into();
    let note_package = required_package(packages, note_name)?;
    let note_root: Word = NoteScript::from_package(note_package)?.root().into();
    let note = NoteBuilder::new(faucet.id(), RandomCoin::new(note_root))
        .package((**note_package).clone())
        .add_assets([asset])
        .note_storage(storage(account.id()))?
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(note.clone()));

    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    chain
        .build_transaction(account)
        .authenticated_input_note(note.id())
        .foreign_accounts(vec![chain.get_foreign_account_inputs(faucet.id())?])
        .build()
}

fn build_basic_wallet_script_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let mut context = build_wallet_transfer_context(packages)?;
    let serial_number = context.random_coin.draw_word();
    let output_note = build_p2id_note(&context, serial_number)?;
    let asset_elements = context.asset.as_elements();
    let args = BasicWalletScriptArgs {
        tag: to_field_felt(Felt::ZERO),
        note_type: to_field_felt(Felt::from(NoteType::Public)),
        recipient: to_field_word(output_note.recipient().digest().into()),
        asset_key: to_field_word(asset_elements[..4].try_into()?),
        asset_value: to_field_word(asset_elements[4..].try_into()?),
    };
    let script =
        TransactionScript::from_package(required_package(packages, BASIC_WALLET_TX_SCRIPT)?)?;
    let builder = context
        .chain
        .build_transaction(context.sender_id)
        .foreign_accounts(vec![context.chain.get_foreign_account_inputs(context.faucet_id)?])
        .tx_script(script);
    apply_script_args(builder, &args)
        .expected_output_notes(vec![RawOutputNote::Full(output_note)])
        .build()
}

fn build_p2id_script_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let mut context = build_wallet_transfer_context(packages)?;
    let serial_number = context.random_coin.draw_word();
    let output_note = build_p2id_note(&context, serial_number)?;
    let mut args = vec![
        Felt::ZERO,
        Felt::from(NoteType::Public),
        context.recipient_id.prefix().as_felt(),
        context.recipient_id.suffix(),
    ];
    let serial_elements: [Felt; 4] = serial_number.into();
    args.extend(serial_elements);
    args.extend(context.asset.as_elements());
    let args_key = Poseidon2::hash_elements(&args);
    let script = transaction_script_with_dependencies(
        required_package(packages, P2ID_TX_SCRIPT)?,
        &[required_package(packages, P2ID_NOTE)?.as_ref()],
    )?;
    context
        .chain
        .build_transaction(context.sender_id)
        .foreign_accounts(vec![context.chain.get_foreign_account_inputs(context.faucet_id)?])
        .tx_script(script)
        .tx_script_args(args_key)
        .add_advice_map_entry(args_key, args)
        .expected_output_notes(vec![RawOutputNote::Full(output_note)])
        .build()
}

struct WalletTransferContext {
    chain: MockChain,
    sender_id: AccountId,
    recipient_id: AccountId,
    faucet_id: AccountId,
    asset: Asset,
    random_coin: RandomCoin,
    note: Arc<Package>,
}

fn build_wallet_transfer_context(packages: &ScenarioPackages) -> Result<WalletTransferContext> {
    let wallet_component = AccountComponent::from_package(
        required_package(packages, BASIC_WALLET)?,
        &InitStorageData::default(),
    )?;
    let mut builder = MockChain::builder();
    let faucet = builder.add_existing_basic_faucet(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        "BENCH",
        1_000_000,
        None,
    )?;
    let asset: Asset = FungibleAsset::new(faucet.id(), 100)?.into();
    let sender = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([4_u8; 32])
            .account_type(AccountType::Public)
            .with_component(wallet_component.clone()),
        AccountState::Exists,
    )?;
    let recipient = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([5_u8; 32])
            .account_type(AccountType::Public)
            .with_component(wallet_component),
        AccountState::Exists,
    )?;
    let note = Arc::clone(required_package(packages, P2ID_NOTE)?);
    let note_root: Word = NoteScript::from_package(&note)?.root().into();
    let mut random_coin = RandomCoin::new(note_root);
    let funding_note = NoteBuilder::new(faucet.id(), &mut random_coin)
        .package((*note).clone())
        .add_assets([asset])
        .note_storage(vec![sender.id().prefix().as_felt(), sender.id().suffix()])?
        .tag(NoteTag::with_account_target(sender.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(funding_note.clone()));
    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    let funding_tx = chain
        .build_transaction(sender.id())
        .authenticated_input_note(funding_note.id())
        .foreign_accounts(vec![chain.get_foreign_account_inputs(faucet.id())?])
        .build()?;
    execute_and_commit(&mut chain, funding_tx)?;
    Ok(WalletTransferContext {
        chain,
        sender_id: sender.id(),
        recipient_id: recipient.id(),
        faucet_id: faucet.id(),
        asset,
        random_coin,
        note,
    })
}

fn execute_and_commit(
    chain: &mut MockChain,
    transaction: MockTransaction,
) -> Result<ExecutedTransaction> {
    let transaction = block_on(transaction.execute())?;
    chain.add_pending_executed_transaction(&transaction)?;
    chain.prove_next_block()?;
    Ok(transaction)
}

fn apply_script_args<'a>(
    builder: MockTransactionBuilder<'a>,
    args: &impl ScriptArgs,
) -> MockTransactionBuilder<'a> {
    match args.encode() {
        EncodedScriptArgs::Word(word) => builder.tx_script_args(from_field_word(word)),
        EncodedScriptArgs::Preimage(values) => {
            let values = from_field_felts(&values);
            let commitment = Poseidon2::hash_elements(&values);
            builder.tx_script_args(commitment).add_advice_map_entry(commitment, values)
        }
    }
}

fn to_field_felt(value: Felt) -> miden_field::Felt {
    miden_field::Felt::new(value.as_canonical_u64()).expect("protocol felt must be canonical")
}

fn to_field_word(values: [Felt; 4]) -> miden_field::Word {
    miden_field::Word::new(values.map(to_field_felt))
}

fn from_field_felts(values: &[miden_field::Felt]) -> Vec<Felt> {
    values
        .iter()
        .map(|value| Felt::new_unchecked(value.as_canonical_u64()))
        .collect()
}

fn from_field_word(value: miden_field::Word) -> Word {
    Word::new([
        Felt::new_unchecked(value[0].as_canonical_u64()),
        Felt::new_unchecked(value[1].as_canonical_u64()),
        Felt::new_unchecked(value[2].as_canonical_u64()),
        Felt::new_unchecked(value[3].as_canonical_u64()),
    ])
}

fn build_p2id_note(
    context: &WalletTransferContext,
    serial_number: Word,
) -> Result<miden_protocol::note::Note> {
    Ok(NoteBuilder::new(context.sender_id, RandomCoin::new(serial_number))
        .serial_number(serial_number)
        .package((*context.note).clone())
        .note_storage(vec![context.recipient_id.prefix().as_felt(), context.recipient_id.suffix()])?
        .add_assets([context.asset])
        .tag(0)
        .build()?)
}

fn transaction_script_with_dependencies(
    package: &Package,
    dependencies: &[&Package],
) -> Result<TransactionScript> {
    let base = TransactionScript::from_package(package)?;
    let entrypoint_digest: Word = base.root().into();
    let mut forests = vec![base.mast()];
    forests.extend(dependencies.iter().map(|package| package.mast_forest().clone()));
    let (merged, _) = MastForest::merge(forests.iter().map(Arc::as_ref))?;
    let entrypoint = merged
        .find_procedure_root(entrypoint_digest)
        .ok_or_else(|| anyhow!("transaction script entrypoint was removed while linking"))?;
    Ok(TransactionScript::from_parts(Arc::new(merged), entrypoint))
}

fn build_storage_transaction(packages: &ScenarioPackages) -> Result<MockTransaction> {
    let mut storage = InitStorageData::default();
    storage.insert_value("storage_example::foo::owner_public_key", STORAGE_OWNER_KEY)?;
    let component =
        AccountComponent::from_package(required_package(packages, STORAGE_EXAMPLE)?, &storage)?;
    let wallet = AccountComponent::from_package(
        required_package(packages, BASIC_WALLET)?,
        &InitStorageData::default(),
    )?;
    let mut builder = MockChain::builder();
    let account = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([6_u8; 32])
            .account_type(AccountType::Public)
            .with_component(wallet)
            .with_component(component),
        AccountState::Exists,
    )?;
    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    let script = transaction_script_with_dependencies(
        required_package(packages, STORAGE_TX_SCRIPT)?,
        &[required_package(packages, STORAGE_EXAMPLE)?.as_ref()],
    )?;
    chain.build_transaction(account).tx_script(script).build()
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build MockChain benchmark runtime")
        .block_on(future)
}

#[derive(Clone, Default)]
struct ReplayCapture {
    snapshot: Arc<Mutex<Option<ReplaySnapshot>>>,
}

impl ReplayCapture {
    fn record(&self, snapshot: ReplaySnapshot) {
        *self.snapshot.lock().expect("replay capture poisoned") = Some(snapshot);
    }

    fn take(&self) -> Option<ReplaySnapshot> {
        self.snapshot.lock().expect("replay capture poisoned").take()
    }
}

thread_local! {
    static ACTIVE_REPLAY_CAPTURES: RefCell<Vec<ReplayCapture>> = const { RefCell::new(Vec::new()) };
}

struct ReplayCaptureGuard;

impl ReplayCaptureGuard {
    fn install(capture: ReplayCapture) -> Self {
        ACTIVE_REPLAY_CAPTURES.with(|captures| captures.borrow_mut().push(capture));
        Self
    }
}

impl Drop for ReplayCaptureGuard {
    fn drop(&mut self) {
        ACTIVE_REPLAY_CAPTURES.with(|captures| {
            captures.borrow_mut().pop().expect("replay capture stack is empty");
        });
    }
}

fn execute_with_replay_capture(
    transaction: MockTransaction,
    capture: ReplayCapture,
) -> Result<ExecutedTransaction> {
    let _guard = ReplayCaptureGuard::install(capture);
    block_on(execute_recorded_transaction(transaction))
        .map_err(|err| anyhow!("recorded MockChain transaction execution failed: {err}"))
}

async fn execute_recorded_transaction(
    transaction: MockTransaction,
) -> Result<ExecutedTransaction, TransactionExecutorError> {
    let account_id = transaction.account().id();
    let block_num = transaction.tx_inputs().block_header().block_num();
    let notes = transaction.input_notes().clone();
    let tx_args = transaction.tx_args().clone();
    let mut executor = TransactionExecutor::new(&transaction)
        .with_source_manager(transaction.source_manager())
        .with_program_executor::<TransactionReplayExecutor>();
    if let Some(authenticator) = transaction.authenticator() {
        executor = executor.with_authenticator(authenticator);
    }
    executor.execute_transaction(account_id, block_num, notes, tx_args).await
}

struct TransactionReplayExecutor {
    stack_inputs: StackInputs,
    advice_inputs: AdviceInputs,
    options: ExecutionOptions,
    capture: ReplayCapture,
}

impl ProgramExecutor for TransactionReplayExecutor {
    fn new(
        stack_inputs: StackInputs,
        advice_inputs: AdviceInputs,
        options: ExecutionOptions,
    ) -> Self {
        let capture = ACTIVE_REPLAY_CAPTURES.with(|captures| {
            captures
                .borrow()
                .last()
                .cloned()
                .expect("TransactionReplayExecutor used without an active replay capture")
        });
        Self {
            stack_inputs,
            advice_inputs,
            options,
            capture,
        }
    }

    fn execute<H: Host + Send>(
        self,
        program: &miden_processor::Program,
        host: &mut H,
    ) -> impl FutureMaybeSend<Result<ExecutionOutput, ExecutionError>> {
        let package = build_transaction_package(program, &PackageDebugInfo::default(), None);
        self.execute_package(package, host)
    }

    fn execute_with_package_debug_info<H: Host + Send>(
        self,
        program: &miden_processor::Program,
        package_debug_info: &PackageDebugInfo,
        entrypoint_source_node: Option<DebugSourceNodeId>,
        host: &mut H,
    ) -> impl FutureMaybeSend<Result<ExecutionOutput, ExecutionError>> {
        let package =
            build_transaction_package(program, package_debug_info, entrypoint_source_node);
        self.execute_package(package, host)
    }
}

impl TransactionReplayExecutor {
    fn execute_package<H: Host + Send>(
        self,
        package: Result<Arc<Package>, ExecutionError>,
        host: &mut H,
    ) -> impl FutureMaybeSend<Result<ExecutionOutput, ExecutionError>> {
        async move {
            let package = package?;
            self.execute_package_inner(package, host).await
        }
    }

    async fn execute_package_inner<H: Host + Send>(
        self,
        package: Arc<Package>,
        host: &mut H,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let Self {
            stack_inputs,
            advice_inputs,
            options,
            capture,
        } = self;
        let events = Arc::new(Mutex::new(Vec::new()));
        let forests = Arc::new(Mutex::new(Vec::new()));
        let mut processor =
            FastProcessor::new_with_options(stack_inputs, advice_inputs.clone(), options)
                .expect("transaction executor supplied invalid advice inputs");
        let mut host = RecordingHost {
            inner: host,
            events: Arc::clone(&events),
            forests: Arc::clone(&forests),
        };

        let result = match processor.get_initial_resume_context_for_package(Arc::clone(&package)) {
            Ok(mut context) => loop {
                let debug_info = context.debug_info();
                let step = if let Some(debug_info) = debug_info.as_deref() {
                    processor.step_with_package_debug_info(&mut host, context, debug_info).await
                } else {
                    processor.step(&mut host, context).await
                };
                match step {
                    Ok(Some(next)) => context = next,
                    Ok(None) => break Ok(()),
                    Err(error) => break Err(error),
                }
            },
            Err(error) => Err(error),
        };

        drop(host);
        capture.record(ReplaySnapshot {
            package,
            stack_inputs,
            advice_inputs,
            options,
            mast_forests: forests.lock().expect("forest recording poisoned").clone(),
            event_log: std::mem::take(&mut *events.lock().expect("event recording poisoned")),
        });
        result?;

        let stack_top = processor.stack_top().iter().rev().copied().collect::<Vec<_>>();
        let stack = StackOutputs::new(&stack_top)
            .unwrap_or_else(|_| StackOutputs::new(&[]).expect("empty stack output is valid"));
        let deferred_state = processor.deferred_state().clone();
        let (advice, memory) = processor.into_parts();
        Ok(ExecutionOutput {
            stack,
            advice,
            memory,
            deferred_state,
        })
    }
}

struct RecordingHost<'a, H> {
    inner: &'a mut H,
    events: Arc<Mutex<Vec<Vec<AdviceMutation>>>>,
    forests: Arc<Mutex<Vec<LoadedMastForest>>>,
}

impl<H: Host> BaseHost for RecordingHost<'_, H> {
    fn get_label_and_source_file(
        &self,
        location: &miden_debug::debug_types::Location,
    ) -> (
        miden_debug::debug_types::SourceSpan,
        Option<Arc<miden_debug::debug_types::SourceFile>>,
    ) {
        self.inner.get_label_and_source_file(location)
    }

    fn resolve_event(
        &self,
        event_id: miden_core::events::EventId,
    ) -> Option<&miden_core::events::EventName> {
        self.inner.resolve_event(event_id)
    }
}

impl<H: Host> Host for RecordingHost<'_, H> {
    fn get_mast_forest(
        &self,
        node_digest: &Word,
    ) -> impl FutureMaybeSend<Option<LoadedMastForest>> {
        let forests = Arc::clone(&self.forests);
        let future = self.inner.get_mast_forest(node_digest);
        async move {
            let forest = future.await;
            if let Some(forest) = forest.as_ref() {
                let mut forests = forests.lock().expect("forest recording poisoned");
                if !forests
                    .iter()
                    .any(|existing| Arc::ptr_eq(existing.mast_forest(), forest.mast_forest()))
                {
                    forests.push(forest.clone());
                }
            }
            forest
        }
    }

    fn on_event(
        &mut self,
        process: &ProcessorState<'_>,
    ) -> impl FutureMaybeSend<Result<Vec<AdviceMutation>, EventError>> {
        let events = Arc::clone(&self.events);
        let future = self.inner.on_event(process);
        async move {
            let result = future.await;
            if let Ok(mutations) = result.as_ref() {
                events
                    .lock()
                    .expect("event recording poisoned")
                    .push(clone_advice_mutations(mutations));
            }
            result
        }
    }
}

fn build_transaction_package(
    program: &miden_processor::Program,
    debug_info: &PackageDebugInfo,
    entrypoint_source_node: Option<DebugSourceNodeId>,
) -> Result<Arc<Package>, ExecutionError> {
    let entrypoint: Arc<MasmPath> =
        MasmPath::exec_path().join(ProcedureName::MAIN_PROC_NAME).into();
    let export = ProcedureExport::new(entrypoint, Some(program.entrypoint()), program.hash(), None)
        .with_source_node(entrypoint_source_node);
    let kernel = TransactionKernel::package();
    let mut package = Package::create(
        "midenc-contract-benchmark".into(),
        kernel.version.clone(),
        TargetType::Executable,
        program.mast_forest().clone(),
        [PackageExport::Procedure(export)],
        [kernel.to_dependency()],
    )
    .map_err(package_construction_error)?;
    package.sections.push(Section::new(SectionId::KERNEL, kernel.to_bytes()));
    package
        .sections
        .push(Section::new(SectionId::DEBUG_INFO, debug_info.to_bytes()));
    package.debug_info()?;
    Ok(Arc::new(package))
}

fn package_construction_error(error: impl ToString) -> ExecutionError {
    PackageDebugInfoError::InvalidReference {
        message: format!("failed to construct benchmark package: {}", error.to_string()),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_capture_stack_is_scoped() {
        let capture = ReplayCapture::default();
        {
            let _guard = ReplayCaptureGuard::install(capture);
            ACTIVE_REPLAY_CAPTURES.with(|captures| assert_eq!(captures.borrow().len(), 1));
        }
        ACTIVE_REPLAY_CAPTURES.with(|captures| assert!(captures.borrow().is_empty()));
    }
}
