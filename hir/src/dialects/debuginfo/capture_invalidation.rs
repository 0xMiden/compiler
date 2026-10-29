//! Deferred slot clears. Original capture witnesses remain immediate; only supplemental kills
//! are solved here, from surviving IR, at an explicit rewrite/observer boundary.
use alloc::{
    format,
    rc::{Rc, Weak},
    string::String,
    vec::Vec,
};

use super::{
    DIBuilder, DebugKill,
    bindings::{SourceBinding, VisibilitySlot},
};
use crate::{
    Context, FxHashMap, FxHashSet, OperationRef, RegionBranchOpInterface, RegionBranchPoint,
    RegionBranchTerminatorOpInterface, RegionRef, Report, Spanned,
    patterns::{Rewriter, RewriterImpl, RewriterListener},
};

#[derive(Default)]
pub(crate) struct CaptureInvalidationState {
    next_id: u64,
    frames: Vec<Frame>,
    pending: FxHashMap<OperationRef, Request>,
    failure: Option<String>,
    resolving: bool,
}
struct Frame {
    id: u64,
    root: OperationRef,
    anchors: Vec<OperationRef>,
    listener: Option<Weak<dyn RewriterListener>>,
}
#[derive(Clone)]
struct Request {
    phase: u64,
    binding: SourceBinding,
    function_parent: Option<OperationRef>,
}

/// A rewrite phase must finish even when its body returns an error. Nested invocations flush
/// their caller before observing IR. The strong listener lives only as long as this handle.
#[must_use]
pub struct CaptureInvalidationPhase {
    context: Rc<Context>,
    id: u64,
    _listener: Option<Rc<dyn RewriterListener>>,
    finished: bool,
}
impl CaptureInvalidationPhase {
    pub fn begin(
        root: OperationRef,
        listener: Option<Rc<dyn RewriterListener>>,
    ) -> Result<Self, Report> {
        let context = root.borrow().context_rc();
        capture_invalidation_barrier(&context)?;
        let mut state = context.capture_invalidation.borrow_mut();
        state.next_id += 1;
        let id = state.next_id;
        state.frames.push(Frame {
            id,
            root,
            anchors: Vec::new(),
            listener: listener.as_ref().map(Rc::downgrade),
        });
        drop(state);
        Ok(Self {
            context,
            id,
            _listener: listener,
            finished: false,
        })
    }

    pub fn finish(mut self) -> Result<bool, Report> {
        let result = drain(&self.context, self.id, |binding, anchor, listener| {
            let mut rewriter =
                RewriterImpl::<crate::patterns::NoopRewriterListener>::new(
                    anchor.borrow().context_rc(),
                )
                .with_listener(listener.ok_or_else(|| {
                    Report::msg("capture phase requires its originating listener")
                })?);
            emit(&mut rewriter, binding, anchor)
        });
        self.close();
        check(&self.context)?;
        result
    }

    pub(super) fn finish_with<R: ?Sized + Rewriter>(
        mut self,
        rewriter: &mut R,
    ) -> Result<bool, Report> {
        let result =
            drain(&self.context, self.id, |binding, anchor, _| emit(rewriter, binding, anchor));
        self.close();
        check(&self.context)?;
        result
    }

    fn close(&mut self) {
        let mut state = self.context.capture_invalidation.borrow_mut();
        if state.frames.last().is_some_and(|frame| frame.id != self.id) {
            state
                .failure
                .get_or_insert("capture invalidation phases completed out of order".into());
        }
        state.frames.retain(|frame| frame.id != self.id);
        state.pending.retain(|_, request| request.phase != self.id);
        state.resolving = false;
        self.finished = true;
    }
}
impl Drop for CaptureInvalidationPhase {
    fn drop(&mut self) {
        if !self.finished {
            poison(&self.context, "capture invalidation phase dropped without completion");
            self.close();
        }
    }
}

pub(crate) fn poison(context: &Context, message: impl Into<String>) {
    let message = message.into();
    context.capture_invalidation.borrow_mut().failure.get_or_insert(message);
}
pub(crate) fn check(context: &Context) -> Result<(), Report> {
    match &context.capture_invalidation.borrow().failure {
        Some(error) => Err(Report::msg(error.clone())),
        None => Ok(()),
    }
}
pub(super) fn active(context: &Context) -> bool {
    !context.capture_invalidation.borrow().frames.is_empty()
}
pub(super) fn validate_anchor(anchor: OperationRef) -> bool {
    let state = anchor.context_ref().capture_invalidation.borrow();
    state.pending.get(&anchor).is_none_or(|request| {
        anchor.name().is::<DebugKill>()
            && SourceBinding::of(anchor).as_ref() == Some(&request.binding)
            && request.binding.function.and_then(|function| function.parent_op())
                == request.function_parent
    })
}
pub(super) fn expected_binding(anchor: OperationRef) -> Option<SourceBinding> {
    anchor
        .context_ref()
        .capture_invalidation
        .borrow()
        .pending
        .get(&anchor)
        .map(|request| request.binding.clone())
}
pub(super) fn has_pending(context: &Context) -> bool {
    !context.capture_invalidation.borrow().pending.is_empty()
}
pub fn is_pending_capture(anchor: OperationRef) -> bool {
    anchor.context_ref().capture_invalidation.borrow().pending.contains_key(&anchor)
}
pub(super) fn submit(anchor: OperationRef) {
    let context = anchor.context_ref();
    let mut state = context.capture_invalidation.borrow_mut();
    if state.resolving {
        state
            .failure
            .get_or_insert("capture rewrite invoked during materialization".into());
        return;
    }
    if state.pending.contains_key(&anchor) {
        return;
    }
    let frame = state.frames.last_mut().expect("capture clear requires a phase");
    frame.anchors.push(anchor);
    let phase = frame.id;
    state.pending.insert(
        anchor,
        Request {
            phase,
            binding: SourceBinding::of(anchor).unwrap(),
            function_parent: super::bindings::enclosing_function(anchor)
                .and_then(|f| f.parent_op()),
        },
    );
    #[cfg(test)]
    stats::COUNTS.with(|counts| counts.borrow_mut().requests += 1);
}

/// Transfer only exact kill effects; salvage must never change a pending clear's slot.
pub(super) fn transfer(original: OperationRef, replacement: OperationRef) -> Result<(), Report> {
    let context = original.context_ref();
    let mut state = context.capture_invalidation.borrow_mut();
    let Some(request) = state.pending.get(&original).cloned() else {
        return Ok(());
    };
    if !replacement.name().is::<DebugKill>()
        || SourceBinding::of(replacement).as_ref() != Some(&request.binding)
    {
        state.failure.get_or_insert("invalid pending capture replacement".into());
        return Err(Report::msg("invalid pending capture replacement"));
    }
    state.pending.remove(&original);
    if !state.pending.contains_key(&replacement) {
        let frame = state.frames.iter_mut().find(|frame| frame.id == request.phase).unwrap();
        frame.anchors.push(replacement);
        state.pending.insert(replacement, request);
    }
    Ok(())
}
pub(super) fn relocated(anchor: OperationRef, binding: SourceBinding) {
    if let Some(request) =
        anchor.context_ref().capture_invalidation.borrow_mut().pending.get_mut(&anchor)
    {
        request.function_parent = binding.function.and_then(|f| f.parent_op());
        request.binding = binding;
    }
}
pub(super) fn cancel(anchor: OperationRef) {
    anchor.context_ref().capture_invalidation.borrow_mut().pending.remove(&anchor);
}

/// Flush before a nested driver, instrumentation, or an audit observes ordinary public IR.
/// The empty path does not walk a root, construct a graph, or examine any function.
pub fn capture_invalidation_barrier(context: &Rc<Context>) -> Result<bool, Report> {
    check(context)?;
    if context.capture_invalidation.borrow().resolving {
        poison(context, "capture observer invoked during materialization");
        return Err(Report::msg("capture observer invoked during materialization"));
    }
    let ids = {
        let state = context.capture_invalidation.borrow();
        if state.pending.is_empty() {
            return Ok(false);
        }
        state.frames.iter().map(|frame| frame.id).collect::<Vec<_>>()
    };
    let mut changed = false;
    for id in ids {
        changed |= drain(context, id, |binding, anchor, listener| {
            let listener = listener.ok_or_else(|| {
                Report::msg("one-shot capture phase crossed an observer boundary")
            })?;
            let mut rewriter =
                RewriterImpl::<crate::patterns::NoopRewriterListener>::new(context.clone())
                    .with_listener(listener);
            emit(&mut rewriter, binding, anchor)
        })?;
    }
    Ok(changed)
}

pub(crate) fn generation(context: &Context) -> u64 {
    context.capture_invalidation.borrow().next_id
}

/// Every phase opened during an audit must complete before that audit exits. The generation
/// cutoff preserves caller-owned phases without relying on roots that rewrites can move or erase.
/// Retire leaked state even after another failure; retained handles may only observe the poison.
pub(crate) fn audit_boundary(context: &Context, since: u64) -> Result<(), Report> {
    let mut state = context.capture_invalidation.borrow_mut();
    if state.frames.iter().any(|frame| frame.id > since) {
        state.frames.retain(|frame| frame.id <= since);
        state.pending.retain(|_, request| request.phase <= since);
        state
            .failure
            .get_or_insert("capture invalidation phase outlived its audit".into());
    }
    drop(state);
    check(context)
}
pub(super) fn under(mut op: OperationRef, root: OperationRef) -> bool {
    loop {
        if op == root {
            return true;
        }
        let Some(parent) = op.parent_op() else {
            return false;
        };
        op = parent;
    }
}

pub(super) fn emit<R: ?Sized + Rewriter>(
    rewriter: &mut R,
    binding: &SourceBinding,
    anchor: OperationRef,
) -> Result<(), Report> {
    let ip = *rewriter.insertion_point();
    let chain = binding.identity.inline_calls.clone().map(|chain| {
        rewriter
            .context_rc()
            .create_attribute::<super::attributes::InlineCallChainAttr, _>(chain)
            .as_attribute_ref()
    });
    let previous = rewriter.replace_inline_call_chain(chain);
    rewriter.set_insertion_point_before(anchor);
    let result = rewriter.debug_kill(binding.identity.variable.clone(), anchor.borrow().span());
    rewriter.restore_insertion_point(ip);
    rewriter.replace_inline_call_chain(previous);
    result.map(|_| ())
}

fn drain(
    context: &Rc<Context>,
    id: u64,
    mut insert: impl FnMut(
        &SourceBinding,
        OperationRef,
        Option<Rc<dyn RewriterListener>>,
    ) -> Result<(), Report>,
) -> Result<bool, Report> {
    check(context)?;
    let (root, requests, listener) = {
        let mut state = context.capture_invalidation.borrow_mut();
        let frame = state.frames.iter_mut().find(|frame| frame.id == id).expect("live phase");
        let root = frame.root;
        let anchors = core::mem::take(&mut frame.anchors);
        let listener = frame.listener.as_ref().and_then(Weak::upgrade);
        let requests = anchors
            .into_iter()
            .filter_map(|anchor| state.pending.remove(&anchor).map(|request| (anchor, request)))
            .collect::<Vec<_>>();
        (root, requests, listener)
    };
    if requests.is_empty() {
        return Ok(false);
    }
    context.capture_invalidation.borrow_mut().resolving = true;
    let result = (|| {
        let mut functions = Vec::<(OperationRef, Vec<(OperationRef, SourceBinding)>)>::new();
        let mut indices = FxHashMap::default();
        for (anchor, request) in requests {
            let function = request.binding.function.unwrap_or(root);
            if !anchor.name().is::<DebugKill>()
                || !under(anchor, function)
                || SourceBinding::of(anchor).as_ref() != Some(&request.binding)
                || request.binding.function.and_then(|f| f.parent_op()) != request.function_parent
            {
                return Err(Report::msg(
                    "pending capture anchor erased, detached, or changed without a mapping",
                ));
            }
            let index = *indices.entry(function).or_insert_with(|| {
                functions.push((function, Vec::new()));
                functions.len() - 1
            });
            functions[index].1.push((anchor, request.binding));
        }
        let mut changed = false;
        for (function, requests) in functions {
            let mut graph = FlowGraph::new(requests);
            graph.function(function);
            #[cfg(test)]
            stats::COUNTS.with(|counts| {
                let mut counts = counts.borrow_mut();
                counts.bindings += graph.bindings.len();
                counts.nodes += graph.nodes.len();
                counts.edges += graph.nodes.iter().map(|n| n.edges.len()).sum::<usize>();
            });
            changed |= graph.solve(|binding, anchor| insert(binding, anchor, listener.clone()))?;
        }
        Ok(changed)
    })();
    context.capture_invalidation.borrow_mut().resolving = false;
    if let Err(error) = &result {
        poison(context, format!("capture invalidation failed: {error}"));
    }
    check(context)?;
    result
}

#[derive(Default)]
struct Node {
    edges: Vec<usize>,
    event: Event,
}
#[derive(Default)]
enum Event {
    #[default]
    None,
    Kill(SourceBinding),
    Clear(OperationRef, SourceBinding),
}
struct FlowGraph {
    nodes: Vec<Node>,
    clears: FxHashMap<OperationRef, SourceBinding>,
    slots: FxHashSet<VisibilitySlot>,
    bindings: Vec<(SourceBinding, Vec<usize>)>,
    binding_index: FxHashMap<SourceBinding, usize>,
}
impl FlowGraph {
    fn new(requests: Vec<(OperationRef, SourceBinding)>) -> Self {
        Self {
            nodes: Vec::new(),
            slots: requests.iter().map(|(_, b)| b.visibility()).collect(),
            clears: requests.into_iter().collect(),
            bindings: Vec::new(),
            binding_index: FxHashMap::default(),
        }
    }

    fn node(&mut self) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::default());
        id
    }

    fn edge(&mut self, from: usize, to: usize) {
        self.nodes[from].edges.push(to);
    }

    fn function(&mut self, root: OperationRef) {
        // A callable is entered once; returns never flow back to its entry.
        let exit = self.node();
        let regions = root.borrow().regions().iter().map(|r| r.as_region_ref()).collect::<Vec<_>>();
        for region in regions {
            self.region(region, exit, None);
        }
    }

    fn region(
        &mut self,
        region: RegionRef,
        exit: usize,
        destinations: Option<(&FxHashMap<RegionRef, usize>, usize)>,
    ) -> usize {
        let entry = self.node();
        let blocks = region.borrow().body().iter().map(|b| b.as_block_ref()).collect::<Vec<_>>();
        let block_nodes =
            blocks.iter().map(|&block| (block, self.node())).collect::<FxHashMap<_, _>>();
        let graph = region.borrow().may_be_graph_region();
        if graph {
            self.edge(entry, exit);
        }
        if let Some(block) = blocks.first() {
            self.edge(entry, block_nodes[block]);
        } else {
            self.edge(entry, exit);
        }
        for block in blocks {
            let mut previous = block_nodes[&block];
            if graph {
                self.edge(entry, previous);
            }
            let ops =
                block.borrow().body().iter().map(|op| op.as_operation_ref()).collect::<Vec<_>>();
            for op in ops {
                let (start, end) = self.operation(op);
                self.edge(if graph { entry } else { previous }, start);
                previous = end;
                if graph {
                    self.edge(end, entry);
                }
                let operation = op.borrow();
                for successor in operation.successors().iter() {
                    if let Some(&dest) = block_nodes.get(&successor.block.borrow().successor()) {
                        self.edge(end, dest);
                    }
                }
                if let Some(branch) = operation.as_trait::<dyn RegionBranchTerminatorOpInterface>()
                {
                    if let Some((destinations, returning)) = destinations {
                        let unknown = alloc::vec![None; operation.num_operands()];
                        for successor in branch.get_successor_regions(&unknown) {
                            let dest = successor
                                .successor()
                                .region()
                                .and_then(|r| destinations.get(&r).copied())
                                .unwrap_or(returning);
                            self.edge(end, dest);
                        }
                    } else {
                        self.edge(end, exit);
                    }
                }
            }
            // For a CFG terminator the explicit successors are complete. Unknown exits still
            // reach the enclosing region's conservative continuation.
            let term = block.borrow().body().back().as_pointer();
            if term.is_none_or(|op| {
                op.borrow().num_successors() == 0
                    && !op.borrow().implements::<dyn RegionBranchTerminatorOpInterface>()
            }) {
                self.edge(previous, exit);
            }
        }
        entry
    }

    fn operation(&mut self, op: OperationRef) -> (usize, usize) {
        let start = self.node();
        if let Some(binding) = self.clears.get(&op) {
            self.nodes[start].event = Event::Clear(op, binding.clone());
        } else if let Some(binding) = SourceBinding::of(op) {
            if op.name().is::<DebugKill>() {
                self.nodes[start].event = Event::Kill(binding);
            } else if self.slots.contains(&binding.visibility()) {
                let index = *self.binding_index.entry(binding.clone()).or_insert_with(|| {
                    self.bindings.push((binding, Vec::new()));
                    self.bindings.len() - 1
                });
                self.bindings[index].1.push(start);
            }
        }
        if op.name().is::<crate::dialects::builtin::Function>() {
            return (start, start);
        }
        let operation = op.borrow();
        let regions = operation.regions().iter().map(|r| r.as_region_ref()).collect::<Vec<_>>();
        if regions.is_empty() {
            return (start, start);
        }
        let end = self.node();
        if let Some(branch) = operation.as_trait::<dyn RegionBranchOpInterface>() {
            let entries = regions.iter().map(|&r| (r, self.node())).collect::<FxHashMap<_, _>>();
            let unknown = alloc::vec![None; operation.num_operands()];
            for successor in branch.get_entry_successor_regions(&unknown) {
                self.edge(
                    start,
                    successor.successor().and_then(|r| entries.get(&r).copied()).unwrap_or(end),
                );
            }
            for region in regions {
                let fallback = self.node();
                for successor in branch.get_successor_regions(RegionBranchPoint::Child(region)) {
                    self.edge(
                        fallback,
                        successor.successor().and_then(|r| entries.get(&r).copied()).unwrap_or(end),
                    );
                }
                let child = self.region(region, fallback, Some((&entries, end)));
                // Terminator returns must target the owner's continuation, not its fallback
                // successors. Add the conservative continuation for unknown exits as well.
                self.edge(fallback, end);
                self.edge(entries[&region], child);
            }
        } else {
            // Unknown region execution can repeat or select regions in arbitrary order.
            // One hub represents this without an all-pairs region edge matrix.
            let hub = self.node();
            self.edge(start, hub);
            self.edge(hub, end);
            for region in regions {
                let child = self.region(region, hub, None);
                self.edge(hub, child);
            }
        }
        (start, end)
    }

    fn solve(
        &self,
        mut emit: impl FnMut(&SourceBinding, OperationRef) -> Result<(), Report>,
    ) -> Result<bool, Report> {
        let mut visited = alloc::vec![0usize; self.nodes.len()];
        let mut worklist = Vec::new();
        let mut changed = false;
        for (index, (binding, sources)) in self.bindings.iter().enumerate() {
            let generation = index + 1;
            worklist.clear();
            for &source in sources {
                worklist.extend(self.nodes[source].edges.iter().copied());
            }
            let slot = binding.visibility();
            while let Some(node) = worklist.pop() {
                if visited[node] == generation {
                    continue;
                }
                visited[node] = generation;
                match &self.nodes[node].event {
                    Event::Kill(killed) if killed == binding => continue,
                    Event::Clear(anchor, killed) if killed.visibility() == slot => {
                        if killed != binding {
                            emit(binding, *anchor)?;
                            changed = true;
                        }
                        continue;
                    }
                    _ => worklist.extend(self.nodes[node].edges.iter().copied()),
                }
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
pub(super) mod stats {
    #[derive(Default, Debug)]
    pub struct Counts {
        pub requests: usize,
        pub bindings: usize,
        pub nodes: usize,
        pub edges: usize,
    }
    std::thread_local! { pub static COUNTS: core::cell::RefCell<Counts> = Default::default(); }
}
