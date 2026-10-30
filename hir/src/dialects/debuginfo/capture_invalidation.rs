//! Deferred slot clears. Original capture witnesses remain immediate; only supplemental kills
//! are solved here, from surviving IR, at an explicit rewrite/observer boundary.
use alloc::{
    rc::{Rc, Weak},
    string::String,
    sync::Arc,
    vec::Vec,
};

use midenc_session::diagnostics::{Diagnostic, miette};

use super::{
    DIBuilder, DebugKill,
    bindings::{SourceBinding, VisibilitySlot},
    mutation::is_under,
};
use crate::{
    Context, FxHashMap, FxHashSet, OperationRef, RegionBranchOpInterface, RegionBranchPoint,
    RegionBranchTerminatorOpInterface, RegionRef, Report, Spanned,
    patterns::{Rewriter, RewriterImpl, RewriterListener},
};

/// The shared original report is also the identity of a cause. Observing it again never
/// registers a new event; independently created reports remain distinct even with equal text.
#[derive(Clone, Debug, Diagnostic)]
#[diagnostic(forward(report))]
struct CaptureError {
    report: Arc<Report>,
    passes: Vec<&'static str>,
}
impl core::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for pass in &self.passes {
            write!(f, "pass '{pass}': ")?;
        }
        core::fmt::Display::fmt(&self.report, f)
    }
}
impl core::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        // The wrapper supplies the original summary, so forward its cause rather than
        // inserting the original summary a second time into the source chain.
        self.report.source()
    }
}

/// A flat aggregate exposes every original diagnostic, including its own related diagnostics.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("capture lifecycle failed with multiple errors")]
struct CaptureErrors(#[related] Vec<CaptureError>);
impl CaptureErrors {
    fn from_report(report: Report) -> Self {
        match report.downcast::<Self>() {
            Ok(errors) => errors,
            Err(report) => match report.downcast::<CaptureError>() {
                Ok(error) => Self(alloc::vec![error]),
                Err(report) => Self(alloc::vec![CaptureError {
                    report: Arc::new(report),
                    passes: Vec::new(),
                }]),
            },
        }
    }

    fn extend(&mut self, errors: Self) {
        for error in errors.0 {
            if let Some(existing) =
                self.0.iter_mut().find(|existing| Arc::ptr_eq(&error.report, &existing.report))
            {
                for pass in error.passes {
                    if !existing.passes.contains(&pass) {
                        existing.passes.push(pass);
                    }
                }
            } else {
                self.0.push(error);
            }
        }
    }

    fn into_report(mut self) -> Report {
        if self.0.len() == 1 {
            Report::new(self.0.pop().unwrap())
        } else {
            Report::new(self)
        }
    }
}

/// Combine failures from a rewrite body and capture cleanup, reporting repeated observations
/// of the same sticky capture failure once. Independent reports always remain independent.
pub fn combine_capture_errors(body: Report, cleanup: Report) -> Report {
    let mut errors = CaptureErrors::from_report(body);
    errors.extend(CaptureErrors::from_report(cleanup));
    errors.into_report()
}

pub(crate) fn capture_error_in_pass(report: Report, pass: &'static str) -> Report {
    let mut errors = CaptureErrors::from_report(report);
    for error in &mut errors.0 {
        if !error.passes.contains(&pass) {
            error.passes.push(pass);
        }
    }
    errors.into_report()
}

#[derive(Default)]
pub(crate) struct CaptureInvalidationState {
    next_id: u64,
    frames: Vec<Frame>,
    pending: FxHashMap<OperationRef, Request>,
    failures: Vec<CaptureError>,
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

    pub fn finish(self) -> Result<bool, Report> {
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
        self.complete(result)
    }

    pub(super) fn finish_with<R: ?Sized + Rewriter>(
        self,
        rewriter: &mut R,
    ) -> Result<bool, Report> {
        let result =
            drain(&self.context, self.id, |binding, anchor, _| emit(rewriter, binding, anchor));
        self.complete(result)
    }

    fn complete(mut self, result: Result<bool, Report>) -> Result<bool, Report> {
        match (result, self.close()) {
            (Err(body), Err(cleanup)) => Err(combine_capture_errors(body, cleanup)),
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(changed), Ok(())) => Ok(changed),
        }
    }

    fn close(&mut self) -> Result<(), Report> {
        let mut state = self.context.capture_invalidation.borrow_mut();
        let out_of_order = state.frames.iter().any(|frame| frame.id == self.id)
            && state.frames.last().is_some_and(|frame| frame.id != self.id);
        state.frames.retain(|frame| frame.id != self.id);
        state.pending.retain(|_, request| request.phase != self.id);
        state.resolving = false;
        self.finished = true;
        drop(state);
        if out_of_order {
            poison(&self.context, "capture invalidation phases completed out of order");
        }
        check(&self.context)
    }
}
impl Drop for CaptureInvalidationPhase {
    fn drop(&mut self) {
        if !self.finished {
            let live = self
                .context
                .capture_invalidation
                .borrow()
                .frames
                .iter()
                .any(|frame| frame.id == self.id);
            if live {
                poison(&self.context, "capture invalidation phase dropped without completion");
            }
            let _ = self.close();
        }
    }
}

/// Register a newly detected violation, including on callbacks and Drop paths without a Result.
pub(crate) fn poison(context: &Context, message: impl Into<String>) {
    let errors = CaptureErrors::from_report(Report::msg(message.into()));
    record(context, errors);
}

fn record(context: &Context, errors: CaptureErrors) {
    let mut state = context.capture_invalidation.borrow_mut();
    let mut failures = CaptureErrors(core::mem::take(&mut state.failures));
    failures.extend(errors);
    state.failures = failures.0;
}

/// Register original reports while reusing identities already registered by an inner boundary.
pub(super) fn fail(context: &Context, report: Report) -> Report {
    record(context, CaptureErrors::from_report(report));
    check(context).unwrap_err()
}

/// Observe registered causes. Construct the Report only after releasing the Context borrow.
pub(crate) fn check(context: &Context) -> Result<(), Report> {
    let failures = context.capture_invalidation.borrow().failures.clone();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(CaptureErrors(failures).into_report())
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
        drop(state);
        poison(context, "capture rewrite invoked during materialization");
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
        drop(state);
        return Err(fail(context, Report::msg("invalid pending capture replacement")));
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
        return Err(fail(context, Report::msg("capture observer invoked during materialization")));
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
    let abandoned = state.frames.iter().any(|frame| frame.id > since);
    if abandoned {
        state.frames.retain(|frame| frame.id <= since);
        state.pending.retain(|_, request| request.phase <= since);
    }
    drop(state);
    if abandoned {
        poison(context, "capture invalidation phase outlived its audit");
    }
    check(context)
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
                || !is_under(anchor, function)
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
    match result {
        Err(error) => Err(fail(context, error)),
        Ok(changed) => check(context).map(|()| changed),
    }
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

#[cfg(test)]
pub(super) mod tests {
    use alloc::{format, string::ToString, vec};

    use super::*;
    use crate::{
        SourceSpan, Type, ValueRef,
        dialects::{builtin::BuiltinOpBuilder, debuginfo::attributes::Variable},
        testing::Test,
    };

    #[derive(Debug, thiserror::Error, Diagnostic)]
    #[error("structured root failure")]
    #[diagnostic(
        code(capture::structured),
        help("structured recovery advice"),
        url("https://example.com/capture")
    )]
    struct StructuredFailure {
        #[source_code]
        source: miette::NamedSource<String>,
        #[label("structured source label")]
        span: miette::SourceSpan,
        #[related]
        related: Vec<Detail>,
        #[source]
        #[diagnostic_source]
        cause: Detail,
    }

    #[derive(Debug, thiserror::Error, Diagnostic)]
    #[error("{0}")]
    struct Detail(&'static str);

    pub(crate) fn structured_report() -> Report {
        Report::new(StructuredFailure {
            source: miette::NamedSource::new("capture.rs", String::from("let capture = invalid;")),
            span: (14, 7).into(),
            related: alloc::vec![Detail("structured related detail")],
            cause: Detail("structured underlying cause"),
        })
    }

    pub(crate) fn render(report: &Report) -> String {
        let mut rendered = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .with_links(false)
            .render_report(&mut rendered, report.as_ref())
            .unwrap();
        rendered
    }

    pub(crate) fn assert_structured(report: &Report) {
        let rendered = render(report);
        for expected in [
            "structured root failure",
            "capture::structured",
            "structured recovery advice",
            "capture.rs",
            "let capture = invalid;",
            "structured source label",
            "structured related detail",
            "structured underlying cause",
        ] {
            assert_eq!(
                rendered.matches(expected).count(),
                1,
                "missing or repeated {expected}: {rendered}"
            );
        }
        fn inspect(diagnostic: &dyn Diagnostic) -> bool {
            if diagnostic.code().is_some_and(|code| code.to_string() == "capture::structured") {
                assert_eq!(diagnostic.url().unwrap().to_string(), "https://example.com/capture");
                assert_eq!(diagnostic.source().unwrap().to_string(), "structured underlying cause");
                assert_eq!(
                    diagnostic.diagnostic_source().unwrap().to_string(),
                    "structured underlying cause"
                );
                return true;
            }
            diagnostic.related().is_some_and(|mut related| related.any(inspect))
        }
        assert!(inspect(report.as_ref()), "structured diagnostic not exposed");
    }

    #[test]
    fn structured_drain_failure_survives_sticky_observation_and_new_events() -> Result<(), Report> {
        let mut test = Test::new("structured_drain", &[Type::U32], &[Type::U32]);
        let (first, second) = {
            let mut builder = test.function_builder();
            let value = builder.current_block().borrow().arguments()[0] as ValueRef;
            let mut record = |line| {
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_value(
                        value,
                        Variable::new("x".into(), "capture.rs".into(), line, None),
                        SourceSpan::UNKNOWN,
                    )
                    .map(|op| op.as_operation_ref())
            };
            let first = record(1)?;
            let second = record(2)?;
            builder.ret([value], SourceSpan::UNKNOWN)?;
            (first, second)
        };
        let root = test.function().as_operation_ref();
        let phase = CaptureInvalidationPhase::begin(root, None)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        super::super::transform::invalidate_debug_captures(second, &mut rewriter)?;
        assert!(first.parent_op().is_some());
        let error =
            drain(&test.context_rc(), phase.id, |_, _, _| Err(structured_report())).unwrap_err();
        poison(test.context(), "independent subsequent violation");
        poison(test.context(), "independent subsequent violation");
        let error = combine_capture_errors(error, check(test.context()).unwrap_err());
        let error = combine_capture_errors(error, phase.finish().unwrap_err());
        assert_structured(&error);
        let rendered = render(&error);
        assert_eq!(rendered.matches("independent subsequent violation").count(), 2, "{rendered}");
        assert!(!active(test.context()));
        assert!(capture_invalidation_barrier(&test.context_rc()).is_err());
        Ok(())
    }

    #[test]
    fn dropping_a_retired_phase_only_observes_the_audit_failure() -> Result<(), Report> {
        let test = Test::new("retired_drop", &[], &[]);
        let phase = CaptureInvalidationPhase::begin(test.function().as_operation_ref(), None)?;
        let error = audit_boundary(test.context(), 0).unwrap_err();
        drop(phase);
        let error = combine_capture_errors(error, check(test.context()).unwrap_err());
        let rendered = render(&error);
        assert_eq!(
            rendered.matches("capture invalidation phase outlived its audit").count(),
            1,
            "{rendered}"
        );
        assert!(!rendered.contains("dropped without completion"), "{rendered}");
        Ok(())
    }

    #[test]
    fn void_poison_and_drop_keep_independent_events_after_first_failure() -> Result<(), Report> {
        let test = Test::new("void_poison", &[], &[]);
        let root = test.function().as_operation_ref();
        let outer = CaptureInvalidationPhase::begin(root, None)?;
        let inner = CaptureInvalidationPhase::begin(root, None)?;
        poison(test.context(), "initial poison");
        drop(outer);
        drop(inner);
        let rendered = render(&check(test.context()).unwrap_err());
        assert_eq!(rendered.matches("initial poison").count(), 1, "{rendered}");
        assert_eq!(
            rendered
                .matches("capture invalidation phase dropped without completion")
                .count(),
            2,
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("capture invalidation phases completed out of order").count(),
            1,
            "{rendered}"
        );
        assert!(!active(test.context()));
        Ok(())
    }
}
