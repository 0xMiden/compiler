use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::fmt;

use midenc_hir::{
    CallOpInterface, OperationRef, ProgramPoint, SmallVec, SourceSpan, Spanned, ValueRef,
};
use midenc_hir_analysis::{DataFlowSolver, Lattice, LatticeLike, SparseLattice};

/// The kind of unconstrained value origin tracked by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum AdviceTaintOriginKind {
    /// A value produced by a MASM advice operation.
    Advice,
    /// A value returned by an external call whose body is unavailable to the analysis.
    ExternalCall,
}

/// Provenance for an unconstrained value tracked by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdviceTaintOrigin {
    /// The operation span at which the unconstrained value originated.
    pub span: SourceSpan,
    /// The kind of origin represented by `span`.
    pub kind: AdviceTaintOriginKind,
}

impl AdviceTaintOrigin {
    pub fn advice(span: SourceSpan) -> Self {
        Self {
            span,
            kind: AdviceTaintOriginKind::Advice,
        }
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self {
            span,
            kind: AdviceTaintOriginKind::ExternalCall,
        }
    }
}

/// Sparse taint facts for an SSA value in a single call context.
///
/// Each tracked origin is either still unreported on at least one path, or has already reached an
/// unsafe sink on all paths represented by the value. Keeping reported origins in the lattice lets
/// downstream u32 operations avoid duplicate diagnostics along the same path.
#[derive(Clone, Eq, PartialEq)]
pub struct AdviceTaintValue {
    origins: BTreeMap<AdviceTaintOrigin, OriginState>,
}

impl AdviceTaintValue {
    pub fn clean() -> Self {
        Self {
            origins: BTreeMap::new(),
        }
    }

    pub fn raw(span: SourceSpan) -> Self {
        Self {
            origins: BTreeMap::from([(AdviceTaintOrigin::advice(span), OriginState::Unreported)]),
        }
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self {
            origins: BTreeMap::from([(
                AdviceTaintOrigin::external_call(span),
                OriginState::Unreported,
            )]),
        }
    }

    pub fn is_clean(&self) -> bool {
        self.origins.is_empty()
    }

    pub fn has_unreported_origin(&self) -> bool {
        self.origins.values().any(|state| state.is_unreported())
    }

    pub fn contains_origin(&self, origin: AdviceTaintOrigin) -> bool {
        self.origins.contains_key(&origin)
    }

    pub fn unreported_origins(&self) -> impl Iterator<Item = AdviceTaintOrigin> + '_ {
        self.origins.iter().filter_map(|(origin, state)| {
            if state.is_unreported() {
                Some(*origin)
            } else {
                None
            }
        })
    }

    pub fn mark_reported(&self) -> Self {
        Self {
            origins: self
                .origins
                .keys()
                .copied()
                .map(|origin| (origin, OriginState::Reported))
                .collect(),
        }
    }

    fn mark_origins_reported(&self, origins: &BTreeSet<AdviceTaintOrigin>) -> Self {
        Self {
            origins: self
                .origins
                .iter()
                .map(|(&origin, &state)| {
                    let state = if origins.contains(&origin) {
                        OriginState::Reported
                    } else {
                        state
                    };
                    (origin, state)
                })
                .collect(),
        }
    }
}

impl Default for AdviceTaintValue {
    fn default() -> Self {
        Self::clean()
    }
}

impl fmt::Debug for AdviceTaintValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.origins.iter()).finish()
    }
}

impl LatticeLike for AdviceTaintValue {
    fn join(&self, other: &Self) -> Self {
        let mut joined = self.origins.clone();
        for (&origin, &state) in other.origins.iter() {
            joined
                .entry(origin)
                .and_modify(|current| *current = current.join(state))
                .or_insert(state);
        }
        Self { origins: joined }
    }

    fn meet(&self, other: &Self) -> Self {
        self.join(other)
    }
}

const MAX_CALL_CONTEXT_DEPTH: usize = 4;

type CallContext = SmallVec<[CallContextFrame; MAX_CALL_CONTEXT_DEPTH]>;
pub(super) type AdviceTaintSparseLattice = Lattice<ContextualAdviceTaintValue>;

/// A callsite frame in the bounded call string used by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct CallContextFrame {
    id: usize,
    span: SourceSpan,
}

impl CallContextFrame {
    pub(super) fn new(call: &dyn CallOpInterface) -> Self {
        let op = call.as_operation_ref();
        Self {
            id: OperationRef::as_ptr(&op) as usize,
            span: op.span(),
        }
    }
}

/// Sparse taint facts for an SSA value, partitioned by bounded call context.
///
/// The empty context is the baseline context: facts that hold without assuming any particular
/// caller-provided argument taint. Non-empty contexts represent facts derived from a specific
/// bounded call string. Joining contextual facts unions the context maps and joins the inner taint
/// lattice when the same context appears on both sides.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextualAdviceTaintValue {
    contexts: BTreeMap<CallContext, AdviceTaintValue>,
}

impl ContextualAdviceTaintValue {
    pub fn clean() -> Self {
        Self::from_inner(AdviceTaintValue::clean())
    }

    pub fn raw(span: SourceSpan) -> Self {
        Self::from_inner(AdviceTaintValue::raw(span))
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self::from_inner(AdviceTaintValue::external_call(span))
    }

    pub fn is_clean(&self) -> bool {
        self.contexts.values().all(AdviceTaintValue::is_clean)
    }

    pub fn has_unreported_origin(&self) -> bool {
        self.contexts.values().any(AdviceTaintValue::has_unreported_origin)
    }

    pub fn contains_origin(&self, origin: AdviceTaintOrigin) -> bool {
        self.contexts.values().any(|taint| taint.contains_origin(origin))
    }

    pub fn unreported_origins(&self) -> impl Iterator<Item = AdviceTaintOrigin> {
        let mut origins = Vec::new();
        for taint in self.contexts.values() {
            for origin in taint.unreported_origins() {
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
        }
        origins.into_iter()
    }

    pub fn mark_reported(&self) -> Self {
        Self {
            contexts: self
                .contexts
                .iter()
                .map(|(context, taint)| (context.clone(), taint.mark_reported()))
                .collect(),
        }
    }

    pub fn mark_origins_reported(
        &self,
        origins: impl IntoIterator<Item = AdviceTaintOrigin>,
    ) -> Self {
        let origins = origins.into_iter().collect::<BTreeSet<_>>();
        if origins.is_empty() {
            return self.clone();
        }

        Self {
            contexts: self
                .contexts
                .iter()
                .map(|(context, taint)| (context.clone(), taint.mark_origins_reported(&origins)))
                .collect(),
        }
    }

    fn from_inner(taint: AdviceTaintValue) -> Self {
        Self {
            contexts: BTreeMap::from([(CallContext::new(), taint)]),
        }
    }

    pub(super) fn enter_call(&self, frame: CallContextFrame) -> Self {
        let mut contexts = BTreeMap::new();
        for (context, taint) in self.contexts.iter() {
            Self::join_context(&mut contexts, push_call_context(context, frame), taint.clone());
        }
        Self { contexts }
    }

    pub(super) fn exit_call(&self, frame: CallContextFrame) -> Self {
        let mut contexts = BTreeMap::new();
        for (context, taint) in self.contexts.iter() {
            if context.is_empty() {
                Self::join_context(&mut contexts, CallContext::new(), taint.clone());
                continue;
            }

            if context.last() == Some(&frame) {
                let mut caller_context = context.clone();
                caller_context.pop();
                Self::join_context(&mut contexts, caller_context, taint.clone());
            }
        }

        if contexts.is_empty() {
            Self::clean()
        } else {
            Self { contexts }
        }
    }

    pub(super) fn join_all<'a>(values: impl IntoIterator<Item = &'a Self>) -> Self {
        values
            .into_iter()
            .fold(Self::clean(), |acc, value| LatticeLike::join(&acc, value))
    }

    fn join_context(
        contexts: &mut BTreeMap<CallContext, AdviceTaintValue>,
        context: CallContext,
        taint: AdviceTaintValue,
    ) {
        contexts
            .entry(context)
            .and_modify(|current| *current = LatticeLike::join(current, &taint))
            .or_insert(taint);
    }
}

impl Default for ContextualAdviceTaintValue {
    fn default() -> Self {
        Self::clean()
    }
}

impl fmt::Debug for ContextualAdviceTaintValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.contexts.iter()).finish()
    }
}

impl LatticeLike for ContextualAdviceTaintValue {
    fn join(&self, other: &Self) -> Self {
        let mut contexts = self.contexts.clone();
        for (context, taint) in other.contexts.iter() {
            Self::join_context(&mut contexts, context.clone(), taint.clone());
        }
        Self { contexts }
    }

    fn meet(&self, other: &Self) -> Self {
        self.join(other)
    }
}

fn push_call_context(context: &CallContext, frame: CallContextFrame) -> CallContext {
    let mut pushed = context.clone();
    if pushed.len() == MAX_CALL_CONTEXT_DEPTH {
        pushed.remove(0);
    }
    pushed.push(frame);
    pushed
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum OriginState {
    Unreported,
    Reported,
}

impl OriginState {
    fn is_unreported(self) -> bool {
        matches!(self, Self::Unreported)
    }

    fn join(self, other: Self) -> Self {
        if self.is_unreported() || other.is_unreported() {
            Self::Unreported
        } else {
            Self::Reported
        }
    }
}

pub(super) fn required_value_taint(
    value: ValueRef,
    dependent: ProgramPoint,
    solver: &mut DataFlowSolver,
) -> ContextualAdviceTaintValue {
    solver.require::<AdviceTaintSparseLattice, _>(value, dependent).value().clone()
}

pub(super) fn join_value_taint(
    value: ValueRef,
    taint: &ContextualAdviceTaintValue,
    solver: &mut DataFlowSolver,
) {
    let mut lattice = solver.get_or_create_mut::<AdviceTaintSparseLattice, _>(value);
    SparseLattice::join(&mut *lattice, taint);
}

pub(super) fn value_taint(value: ValueRef, solver: &DataFlowSolver) -> ContextualAdviceTaintValue {
    solver
        .get::<AdviceTaintSparseLattice, _>(&value)
        .map(|state| state.value().clone())
        .unwrap_or_default()
}
