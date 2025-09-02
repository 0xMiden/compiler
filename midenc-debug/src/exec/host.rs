use std::{collections::BTreeMap, num::NonZeroU32, sync::Arc};

use miden_core::Word;
use miden_debug_types::{Location, SourceFile, SourceSpan};
use miden_processor::{
    AdviceInputs, AdviceProvider, BaseHost, ExecutionError, KvMap, MastForest, MastForestStore,
    MemMastForestStore, ProcessState, RowIndex,
};

use super::{TraceEvent, TraceHandler};

/// This is an implementation of [BaseHost] which is essentially [miden_processor::DefaultHost],
/// but extended with additional functionality for debugging, in particular it manages trace
/// events that record the entry or exit of a procedure call frame.
#[derive(Default)]
pub struct DebuggerHost {
    adv_provider: AdviceProvider,
    store: MemMastForestStore,
    tracing_callbacks: BTreeMap<u32, Vec<Box<TraceHandler>>>,
    on_assert_failed: Option<Box<TraceHandler>>,
}
impl DebuggerHost {
    /// Construct a new instance of [DebuggerHost] with the given advice provider.
    pub fn new(adv_provider: AdviceProvider) -> Self {
        Self {
            adv_provider,
            store: Default::default(),
            tracing_callbacks: Default::default(),
            on_assert_failed: None,
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
        // Extract and load the advice map from the forest before putting it into the store.
        let advice_map = forest.advice_map();
        for (digest, values) in advice_map.iter() {
            self.adv_provider.insert_into_map(*digest, values.to_vec().clone());
        }
        self.store.insert(forest);
    }
}

impl BaseHost for DebuggerHost {
    fn get_mast_forest(&self, node_digest: &Word) -> Option<Arc<MastForest>> {
        self.store.get(node_digest)
    }

    fn get_label_and_source_file(
        &self,
        location: &Location,
    ) -> (SourceSpan, Option<Arc<SourceFile>>) {
        // TODO: Left for later
        todo!()
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
