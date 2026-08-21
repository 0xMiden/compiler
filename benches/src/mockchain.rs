use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow, ensure};
use miden_core::{Felt, Word, serde::Serializable};
use miden_debug::{
    Executor, ReplaySnapshot, clone_advice_mutations, flamegraph::FlamegraphProfile,
};
use miden_mast_package::Package;
use miden_processor::{
    BaseHost, ExecutionError, ExecutionOptions, ExecutionOutput, FastProcessor, FutureMaybeSend,
    Host, LoadedMastForest, ProcessorState, StackInputs, StackOutputs,
    advice::{AdviceInputs, AdviceMutation},
    event::EventError,
};
use miden_protocol::{
    account::{
        AccountBuilder, AccountComponent, AccountId, AccountType, StorageMapKey, StorageSlotName,
        auth::AuthScheme, component::InitStorageData,
    },
    assembly::{Path as MasmPath, ProcedureName},
    crypto::rand::RandomCoin,
    note::{NoteScript, NoteTag},
    transaction::{ExecutedTransaction, RawOutputNote, TransactionKernel},
    vm::{
        DebugSourceNodeId, PackageDebugInfo, PackageDebugInfoError, PackageExport, ProcedureExport,
        Section, SectionId, TargetType,
    },
};
use miden_standards::{account::wallets::BasicWallet, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain, MockTransaction};
use miden_tx::{ProgramExecutor, TransactionExecutor, TransactionExecutorError};

use super::{BenchmarkRunner, BuildFailure, TransactionBenchmarkResult, load_package};

const COUNTER_NOTE_NO_AUTH: &str = "counter-note-no-auth";
const COUNTER_CONTRACT: &str = "counter-contract";
const COUNTER_NOTE: &str = "counter-note";
const NO_AUTH_COMPONENT: &str = "auth-component-no-auth";
const COUNTER_STORAGE_KEY: Word = Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

struct ScenarioPackages {
    counter: Arc<Package>,
    note: Arc<Package>,
    no_auth: Arc<Package>,
}

impl BenchmarkRunner {
    pub(super) fn run_transaction_benchmarks(&self) -> Result<Vec<TransactionBenchmarkResult>> {
        eprintln!("Benchmarking MockChain transaction {COUNTER_NOTE_NO_AUTH}");

        let optimized_packages = self.load_scenario_packages("none", true)?;
        let (mut optimized_chain, optimized_tx, counter_id, counter_slot) =
            build_counter_note_no_auth(&optimized_packages)?;
        let optimized_tx = block_on(optimized_tx.execute())
            .context("optimized MockChain transaction execution failed")?;
        let cycles = optimized_tx.measurements().total_cycles();
        commit_and_validate_counter(
            &mut optimized_chain,
            &optimized_tx,
            counter_id,
            &counter_slot,
        )?;

        let debug_packages = self.load_scenario_packages("full", false)?;
        let (mut debug_chain, debug_tx, debug_counter_id, debug_counter_slot) =
            build_counter_note_no_auth(&debug_packages)?;
        let capture = ReplayCapture::default();
        let debug_tx = execute_with_replay_capture(debug_tx, capture.clone())?;
        commit_and_validate_counter(
            &mut debug_chain,
            &debug_tx,
            debug_counter_id,
            &debug_counter_slot,
        )?;

        let snapshot = capture
            .take()
            .ok_or_else(|| anyhow!("transaction execution did not produce a replay snapshot"))?;
        let relative_replay = format!("replays/{COUNTER_NOTE_NO_AUTH}.mdsnap");
        snapshot
            .write_to_file(self.output_dir.join(&relative_replay))
            .context("failed to write transaction replay snapshot")?;

        let profile = collect_replay_profile(snapshot)
            .context("failed to collect transaction replay flamegraph")?;
        let relative_flamegraph = format!("flamegraphs/{COUNTER_NOTE_NO_AUTH}.svg");
        profile
            .write_svg(self.output_dir.join(&relative_flamegraph))
            .map_err(|err| anyhow!(err.to_string()))?;

        Ok(vec![TransactionBenchmarkResult {
            name: COUNTER_NOTE_NO_AUTH.into(),
            cycles,
            flamegraph: relative_flamegraph,
            replay: relative_replay,
        }])
    }

    fn load_scenario_packages(&self, debug: &str, persist: bool) -> Result<ScenarioPackages> {
        Ok(ScenarioPackages {
            counter: self.load_scenario_package(COUNTER_CONTRACT, debug, persist)?,
            note: self.load_scenario_package(COUNTER_NOTE, debug, persist)?,
            no_auth: self.load_scenario_package(NO_AUTH_COMPONENT, debug, persist)?,
        })
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

        let project_dir = self.workspace_root.join("examples").join(name);
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

fn build_counter_note_no_auth(
    packages: &ScenarioPackages,
) -> Result<(MockChain, MockTransaction, AccountId, StorageSlotName)> {
    let counter_slot = StorageSlotName::new("counter_contract::counter_contract::count_map")?;
    let mut counter_storage = InitStorageData::default();
    counter_storage.insert_map_entry(counter_slot.clone(), COUNTER_STORAGE_KEY, 1_u64)?;
    let counter_component = AccountComponent::from_package(&packages.counter, &counter_storage)?;
    let auth_component =
        AccountComponent::from_package(&packages.no_auth, &InitStorageData::default())?;

    let mut builder = MockChain::builder();
    let counter_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_component(auth_component)
        .with_component(BasicWallet)
        .with_component(counter_component)
        .build_existing()?;
    builder.add_account(counter_account.clone())?;

    let sender = builder.add_account_from_builder(
        Auth::BasicAuth {
            auth_scheme: AuthScheme::Falcon512Poseidon2,
        },
        AccountBuilder::new([1_u8; 32])
            .account_type(AccountType::Public)
            .with_component(BasicWallet),
        AccountState::Exists,
    )?;

    let note_root: Word = NoteScript::from_package(&packages.note)?.root().into();
    let counter_note = NoteBuilder::new(sender.id(), RandomCoin::new(note_root))
        .package((*packages.note).clone())
        .tag(NoteTag::with_account_target(counter_account.id()).into())
        .build()?;
    builder.add_output_note(RawOutputNote::Full(counter_note.clone()));

    let mut chain = builder.build()?;
    chain.prove_next_block()?;
    chain.prove_next_block()?;
    let transaction = chain
        .build_transaction(counter_account.clone())
        .authenticated_input_note(counter_note.id())
        .build()?;

    Ok((chain, transaction, counter_account.id(), counter_slot))
}

fn commit_and_validate_counter(
    chain: &mut MockChain,
    transaction: &ExecutedTransaction,
    counter_id: AccountId,
    counter_slot: &StorageSlotName,
) -> Result<()> {
    chain.add_pending_executed_transaction(transaction)?;
    chain.prove_next_block()?;
    let value = chain
        .committed_account(counter_id)?
        .storage()
        .get_map_item(counter_slot, StorageMapKey::from_raw(COUNTER_STORAGE_KEY))?[0]
        .as_canonical_u64();
    ensure!(value == 2, "counter transaction produced {value}, expected 2");
    Ok(())
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build MockChain benchmark runtime")
        .block_on(future)
}

fn collect_replay_profile(snapshot: ReplaySnapshot) -> Result<FlamegraphProfile, ExecutionError> {
    let ReplaySnapshot {
        package,
        stack_inputs,
        advice_inputs,
        options,
        mast_forests,
        event_log,
    } = snapshot;
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(miden_assembly::DefaultSourceManager::default());
    let executor = Executor::from_config(miden_debug::ExecutionConfig {
        inputs: stack_inputs,
        advice_inputs,
        options,
    });
    let mut executor =
        executor.into_debug_with_replay(package, source_manager, mast_forests, event_log.into());
    FlamegraphProfile::collect(&mut executor)
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
