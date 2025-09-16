use std::{collections::BTreeMap, num::NonZeroU32, sync::Arc};

use miden_assembly::SourceManager;
use miden_core::Word;
use miden_debug_types::{Location, SourceFile, SourceSpan};
use miden_processor::{
    AdviceInputs, AdviceProvider, BaseHost, EventHandlerRegistry, ExecutionError, MastForest,
    MastForestStore, MemMastForestStore, ProcessState, RowIndex, SyncHost,
};

use super::{TraceEvent, TraceHandler};

/// This is an implementation of [BaseHost] which is essentially [miden_processor::DefaultHost],
/// but extended with additional functionality for debugging, in particular it manages trace
/// events that record the entry or exit of a procedure call frame.
#[derive(Default)]
pub struct DebuggerHost<S: SourceManager> {
    adv_provider: AdviceProvider,
    store: MemMastForestStore,
    tracing_callbacks: BTreeMap<u32, Vec<Box<TraceHandler>>>,
    event_handlers: EventHandlerRegistry,
    on_assert_failed: Option<Box<TraceHandler>>,
    source_manager: Arc<S>,
}
impl<S> DebuggerHost<S>
where
    S: SourceManager,
{
    /// Construct a new instance of [DebuggerHost] with the given advice provider.
    pub fn new(adv_provider: AdviceProvider, source_manager: S) -> Self {
        Self {
            adv_provider,
            store: Default::default(),
            tracing_callbacks: Default::default(),
            event_handlers: EventHandlerRegistry::default(),
            on_assert_failed: None,
            source_manager: Arc::new(source_manager),
        }
    }

    /// Register a trace handler for `event`
    pub fn register_trace_handler<F>(&mut self, event: TraceEvent, callback: F)
    where
        F: FnMut(RowIndex, TraceEvent) + 'static,
    {
        let key = match event {
            TraceEvent::AssertionFailed(None) => u32::MAX,
            ev => ev.into(),
        };
        self.tracing_callbacks.entry(key).or_default().push(Box::new(callback));
    }

    /// Register a handler to be called when an assertion in the VM fails
    pub fn register_assert_failed_tracer<F>(&mut self, callback: F)
    where
        F: FnMut(RowIndex, TraceEvent) + 'static,
    {
        self.on_assert_failed = Some(Box::new(callback));
    }

    /// Load `forest` into the MAST store for this host
    pub fn load_mast_forest(&mut self, forest: Arc<MastForest>) {
        self.store.insert(forest);
    }
}

impl<S> BaseHost for DebuggerHost<S>
where
    S: SourceManager,
{
    fn get_label_and_source_file(
        &self,
        location: &Location,
    ) -> (SourceSpan, Option<Arc<SourceFile>>) {
        let maybe_file = self.source_manager.get_by_uri(location.uri());
        let span = self.source_manager.location_to_span(location.clone()).unwrap_or_default();
        (span, maybe_file)
    }

    fn on_trace(
        &mut self,
        process: &mut ProcessState,
        trace_id: u32,
    ) -> Result<(), ExecutionError> {
        let event = TraceEvent::from(trace_id);
        let clk = process.clk();
        if let Some(handlers) = self.tracing_callbacks.get_mut(&trace_id) {
            for handler in handlers.iter_mut() {
                handler(clk, event);
            }
        }
        Ok(())
    }

    fn on_assert_failed(&mut self, process: &ProcessState, err_code: miden_core::Felt) {
        let clk = process.clk();
        if let Some(handler) = self.on_assert_failed.as_mut() {
            // TODO: We're truncating the error code here, but we may need to handle the full range
            handler(clk, TraceEvent::AssertionFailed(NonZeroU32::new(err_code.as_int() as u32)));
        }
    }
}

impl<S> SyncHost for DebuggerHost<S>
where
    S: SourceManager,
{
    fn on_event(
        &mut self,
        process: &ProcessState,
    ) -> Result<Vec<miden_processor::AdviceMutation>, miden_processor::EventError> {
        Ok(Vec::new())
    }

    fn get_mast_forest(&self, node_digest: &Word) -> Option<Arc<MastForest>> {
        self.store.get(node_digest)
    }
}

impl<S> miden_processor::AsyncHost for DebuggerHost<S>
where
    S: SourceManager,
{
    fn get_mast_forest(
        &self,
        node_digest: &Word,
    ) -> impl miden_processor::FutureMaybeSend<Option<Arc<MastForest>>> {
        core::future::ready(self.store.get(node_digest))
    }

    fn on_event(
        &mut self,
        process: &ProcessState<'_>,
    ) -> impl miden_processor::FutureMaybeSend<
        Result<Vec<miden_processor::AdviceMutation>, miden_processor::EventError>,
    > {
        core::future::ready(Ok(Vec::new()))
    }
}
