use std::{collections::BTreeMap, sync::Arc};

use miden_core::crypto::hash::RpoDigest;
use miden_processor::{
    AdviceProvider, ExecutionError, Host, MastForest, MastForestStore, MemAdviceProvider,
    MemMastForestStore, ProcessState, RowIndex,
};

use super::{TraceEvent, TraceHandler};

/// This is an implementation of [Host] which is essentially [miden_processor::DefaultHost],
/// but extended with additional functionality for debugging, in particular it manages trace
/// events that record the entry or exit of a procedure call frame.
#[derive(Default)]
pub struct DebuggerHost {
    adv_provider: MemAdviceProvider,
    store: MemMastForestStore,
    tracing_callbacks: BTreeMap<u32, Vec<Box<TraceHandler>>>,
    on_assert_failed: Option<Box<TraceHandler>>,
}
impl DebuggerHost {
    /// Construct a new instance of [DebuggerHost] with the given advice provider.
    pub fn new(adv_provider: MemAdviceProvider) -> Self {
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
        self.store.insert(forest);
    }
}

impl Host for DebuggerHost {
    type AdviceProvider = MemAdviceProvider;

    fn get_mast_forest(&self, node_digest: &RpoDigest) -> Option<Arc<MastForest>> {
        self.store.get(node_digest)
    }

    fn on_trace(&mut self, process: ProcessState, trace_id: u32) -> Result<(), ExecutionError> {
        let event = TraceEvent::from(trace_id);
        let clk = process.clk();
        if let Some(handlers) = self.tracing_callbacks.get_mut(&trace_id) {
            for handler in handlers.iter_mut() {
                handler(clk, event);
            }
        }
        Ok(())
    }

    fn on_assert_failed(&mut self, process: ProcessState, err_code: u32) -> ExecutionError {
        let clk = process.clk();
        if let Some(handler) = self.on_assert_failed.as_mut() {
            handler(clk, TraceEvent::AssertionFailed(core::num::NonZeroU32::new(err_code)));
        }
        let err_msg = match err_code {
            midenc_dialect_hir::assertions::ASSERT_FAILED_ALIGNMENT => Some(
                "failed alignment: use of memory address violates minimum alignment requirements \
                 for that use"
                    .to_string(),
            ),
            _ => None,
        };
        ExecutionError::FailedAssertion {
            clk,
            err_code,
            err_msg,
        }
    }

    fn advice_provider(&self) -> &Self::AdviceProvider {
        &self.adv_provider
    }

    fn advice_provider_mut(&mut self) -> &mut Self::AdviceProvider {
        &mut self.adv_provider
    }
}
