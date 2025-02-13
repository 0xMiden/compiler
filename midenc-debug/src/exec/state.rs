use std::collections::{BTreeSet, VecDeque};

use miden_core::Word;
use miden_processor::{
    ContextId, ExecutionError, Operation, RowIndex, StackOutputs, VmState, VmStateIterator,
};

use super::ExecutionTrace;
use crate::{CallFrame, CallStack, TestFelt};

/// A special version of [crate::Executor] which provides finer-grained control over execution,
/// and captures a ton of information about the program being executed, so as to make it possible
/// to introspect everything about the program and the state of the VM at a given cycle.
///
/// This is used by the debugger to execute programs, and provide all of the functionality made
/// available by the TUI.
pub struct DebugExecutor {
    /// The underlying [VmStateIterator] being driven
    pub iter: VmStateIterator,
    /// The final outcome of the program being executed
    pub stack_outputs: StackOutputs,
    /// The set of contexts allocated during execution so far
    pub contexts: BTreeSet<ContextId>,
    /// The root context
    pub root_context: ContextId,
    /// The current context at `cycle`
    pub current_context: ContextId,
    /// The current call stack
    pub callstack: CallStack,
    /// A sliding window of the last 5 operations successfully executed by the VM
    pub recent: VecDeque<Operation>,
    /// The most recent [VmState] produced by the [VmStateIterator]
    pub last: Option<VmState>,
    /// The current clock cycle
    pub cycle: usize,
    /// Whether or not execution has terminated
    pub stopped: bool,
}

impl DebugExecutor {
    /// Advance the program state by one cycle.
    ///
    /// If the program has already reached its termination state, it returns the same result
    /// as the previous time it was called.
    ///
    /// Returns the call frame exited this cycle, if any
    pub fn step(&mut self) -> Result<Option<CallFrame>, ExecutionError> {
        if self.stopped {
            return Ok(None);
        }
        match self.iter.next() {
            Some(Ok(state)) => {
                self.cycle += 1;
                if self.current_context != state.ctx {
                    self.contexts.insert(state.ctx);
                    self.current_context = state.ctx;
                }

                if let Some(op) = state.op {
                    if self.recent.len() == 5 {
                        self.recent.pop_front();
                    }
                    self.recent.push_back(op);
                }

                let exited = self.callstack.next(&state);

                self.last = Some(state);

                Ok(exited)
            }
            Some(Err(err)) => {
                self.stopped = true;
                Err(err)
            }
            None => {
                self.stopped = true;
                Ok(None)
            }
        }
    }

    /// Consume the [DebugExecutor], converting it into an [ExecutionTrace] at the current cycle.
    pub fn into_execution_trace(self) -> ExecutionTrace {
        let last_cycle = self.cycle;
        let trace_len_summary = *self.iter.trace_len_summary();
        let (_, _, _, chiplets, _) = self.iter.into_parts();
        ExecutionTrace {
            root_context: self.root_context,
            last_cycle: RowIndex::from(last_cycle),
            chiplets: Chiplets::new(move |context, clk| chiplets.get_mem_state_at(context, clk)),
            outputs: self.stack_outputs,
            trace_len_summary,
        }
    }
}
impl core::iter::FusedIterator for DebugExecutor {}
impl Iterator for DebugExecutor {
    type Item = Result<VmState, ExecutionError>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.stopped {
            return None;
        }
        match self.step() {
            Ok(_) => self.last.clone().map(Ok),
            Err(err) => Some(Err(err)),
        }
    }
}

// Dirty, gross, horrible hack until miden_processor::chiplets::Chiplets is exported
#[allow(clippy::type_complexity)]
pub struct Chiplets(Box<dyn Fn(ContextId, RowIndex) -> Vec<(u64, Word)>>);
impl Chiplets {
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(ContextId, RowIndex) -> Vec<(u64, Word)> + 'static,
    {
        Self(Box::new(callback))
    }

    pub fn get_mem_state_at(&self, context: ContextId, clk: RowIndex) -> Vec<(u64, Word)> {
        (self.0)(context, clk)
    }
}
