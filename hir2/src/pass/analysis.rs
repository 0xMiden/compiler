use alloc::rc::Rc;
use core::{
    any::{Any, TypeId},
    cell::RefCell,
};

use smallvec::SmallVec;

type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;

use super::{PassInstrumentor, PassTarget};
use crate::{Op, Operation, OperationRef};

/// The [Analysis] trait is used to define an analysis over some operation.
///
/// Analyses must be default-constructible, and `Sized + 'static` to support downcasting.
///
/// An analysis, when requested, is first constructed via its `Default` implementation, and then
/// [Analysis::analyze] is called on the target type in order to compute the analysis results.
/// The analysis type also acts as storage for the analysis results.
///
/// When the IR is changed, analyses are invalidated by default, unless they are specifically
/// preserved via the [PreservedAnalyses] set. When an analysis is being asked if it should be
/// invalidated, via [Analysis::invalidate], it has the opportunity to identify if it actually
/// needs to be invalidated based on what analyses were preserved. If dependent analyses of this
/// analysis haven't been invalidated, then this analysis may be able preserve itself as well,
/// and avoid redundant recomputation.
pub trait Analysis: Default + Any {
    /// The specific type on which this analysis is performed.
    ///
    /// The analysis will only be run when an operation is of this type.
    type Target: ?Sized + PassTarget;

    /// The [TypeId] associated with the concrete underlying [Analysis] implementation
    ///
    /// This is automatically implemented for you, but in some cases, such as wrapping an
    /// analysis in another type, you may want to implement this so that queries against the
    /// type return the expected [TypeId]
    #[inline]
    fn analysis_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Get a `dyn Any` reference to the underlying [Analysis] implementation
    ///
    /// This is automatically implemented for you, but in some cases, such as wrapping an
    /// analysis in another type, you may want to implement this so that queries against the
    /// type return the expected [TypeId]
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    /// Same as [Analysis::as_any], but used specifically for getting a reference-counted handle,
    /// rather than a raw reference.
    #[inline(always)]
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any> {
        self as Rc<dyn Any>
    }

    /// Returns the display name for this analysis
    ///
    /// By default this simply returns the name of the concrete implementation type.
    fn name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    /// Analyze `op` using the provided [AnalysisManager].
    fn analyze(&mut self, op: &Self::Target, analysis_manager: AnalysisManager);

    /// Query this analysis for invalidation.
    ///
    /// Given a preserved analysis set, returns true if it should truly be invalidated. This allows
    /// for more fine-tuned invalidation in cases where an analysis wasn't explicitly marked
    /// preserved, but may be preserved(or invalidated) based upon other properties such as analyses
    /// sets.
    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool;
}

/// A type-erased [Analysis].
///
/// This is automatically derived for all [Analysis] implementations, and is the means by which
/// one can abstract over sets of analyses using dynamic dispatch.
///
/// This essentially just delegates to the underlying [Analysis] implementation, but it also handles
/// converting a raw [OperationRef] to the appropriate target type expected by the underlying
/// [Analysis].
pub trait OperationAnalysis {
    /// The unique type id of this analysis
    fn analysis_id(&self) -> TypeId;

    /// Used for dynamic casting to the underlying [Analysis] type
    fn as_any(&self) -> &dyn Any;

    /// Used for dynamic casting to the underlying [Analysis] type
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any>;

    /// The name of this analysis
    fn name(&self) -> &'static str;

    /// Runs this analysis over `op`.
    ///
    /// NOTE: This is only ever called once per instantiation of the analysis, but in theory can
    /// support multiple calls to re-analyze `op`. Each call should reset any internal state to
    /// ensure that if an analysis is reused in this way, that each analysis gets a clean slate.
    fn analyze(&mut self, op: &OperationRef, am: AnalysisManager);

    /// Query this analysis for invalidation.
    ///
    /// Given a preserved analysis set, returns true if it should truly be invalidated. This allows
    /// for more fine-tuned invalidation in cases where an analysis wasn't explicitly marked
    /// preserved, but may be preserved(or invalidated) based upon other properties such as analyses
    /// sets.
    ///
    /// Invalidated analyses must be removed from `preserved_analyses`.
    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool;
}

impl dyn OperationAnalysis {
    /// Cast an reference-counted handle to this analysis to its concrete implementation type.
    ///
    /// Returns `None` if the underlying analysis is not of type `T`
    #[inline]
    pub fn downcast<T: 'static>(self: Rc<Self>) -> Option<Rc<T>> {
        self.as_any_rc().downcast::<T>().ok()
    }
}

impl<A> OperationAnalysis for A
where
    A: Analysis,
{
    #[inline]
    fn analysis_id(&self) -> TypeId {
        <A as Analysis>::analysis_id(self)
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        <A as Analysis>::as_any(self)
    }

    #[inline]
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any> {
        <A as Analysis>::as_any_rc(self)
    }

    #[inline]
    fn name(&self) -> &'static str {
        <A as Analysis>::name(self)
    }

    #[inline]
    fn analyze(&mut self, op: &OperationRef, am: AnalysisManager) {
        let op = <<A as Analysis>::Target as PassTarget>::into_target(op);
        <A as Analysis>::analyze(self, &op, am)
    }

    #[inline]
    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool {
        <A as Analysis>::invalidate(self, preserved_analyses)
    }
}

/// Represents a set of analyses that are known to be preserved after a rewrite has been applied.
#[derive(Default)]
pub struct PreservedAnalyses {
    /// The set of preserved analysis type ids
    preserved: SmallVec<[TypeId; 8]>,
}
impl PreservedAnalyses {
    /// Mark all analyses as preserved.
    ///
    /// This is generally only useful when the IR is known not to have changed.
    pub fn preserve_all(&mut self) {
        self.insert(AllAnalyses::TYPE_ID);
    }

    /// Mark the specified [Analysis] type as preserved.
    pub fn preserve<A: 'static>(&mut self) {
        self.insert(TypeId::of::<A>());
    }

    /// Mark a type as preserved using its raw [TypeId].
    ///
    /// Typically it is best to use [Self::preserve] instead, but this can be useful in cases
    /// where you can't express the type in Rust directly.
    pub fn preserve_raw(&mut self, id: TypeId) {
        self.insert(id);
    }

    /// Returns true if the specified type is preserved.
    ///
    /// This will return true if all analyses are marked preserved, even if the specified type was
    /// not explicitly preserved.
    pub fn is_preserved<A: 'static>(&self) -> bool {
        self.preserved.contains(&TypeId::of::<A>()) || self.is_all()
    }

    /// Returns true if the specified [TypeId] is marked preserved.
    ///
    /// This will return true if all analyses are marked preserved, even if the specified type was
    /// not explicitly preserved.
    pub fn is_preserved_raw(&self, ty: &TypeId) -> bool {
        self.preserved.contains(ty) || self.is_all()
    }

    /// Mark a previously preserved type as invalidated.
    ///
    /// This will also remove the "all preserved" flag, if it had been set.
    pub fn unpreserve<A: 'static>(&mut self) {
        // We must also remove the `all` marker, as we have invalidated one of the analyses
        self.remove(&AllAnalyses::TYPE_ID);
        self.remove(&TypeId::of::<A>());
    }

    /// Mark a previously preserved [TypeId] as invalidated.
    ///
    /// This will also remove the "all preserved" flag, if it had been set.
    pub fn unpreserve_raw(&mut self, ty: &TypeId) {
        // We must also remove the `all` marker, as we have invalidated one of the analyses
        self.remove(&AllAnalyses::TYPE_ID);
        self.remove(ty)
    }

    /// Returns true if all analyses are preserved
    pub fn is_all(&self) -> bool {
        self.preserved.contains(&AllAnalyses::TYPE_ID)
    }

    /// Returns true if no analyses are being preserved
    pub fn is_none(&self) -> bool {
        self.preserved.is_empty()
    }

    fn insert(&mut self, id: TypeId) {
        match self.preserved.binary_search_by_key(&id, |probe| *probe) {
            Ok(index) => self.preserved.insert(index, id),
            Err(index) => self.preserved.insert(index, id),
        }
    }

    fn remove(&mut self, id: &TypeId) {
        if let Ok(index) = self.preserved.binary_search_by_key(&id, |probe| probe) {
            self.preserved.remove(index);
        }
    }
}

/// A marker type that is used to represent all possible [Analysis] types
pub struct AllAnalyses;
impl AllAnalyses {
    const TYPE_ID: TypeId = TypeId::of::<AllAnalyses>();
}

/// This type wraps all analyses stored in an [AnalysisMap], and handles some of the boilerplate
/// details around invalidation by intercepting calls to [Analysis::invalidate] and wrapping it
/// with extra logic. Notably, ensuring that invalidated analyses are removed from the
/// [PreservedAnalyses] set is handled by this wrapper.
///
/// It is a transparent wrapper around `A`, and otherwise acts as a simple proxy to `A`'s
/// implementation of the [Analysis] trait.
#[repr(transparent)]
struct AnalysisWrapper<A> {
    analysis: A,
}
impl<A: Analysis> AnalysisWrapper<A> {
    fn new(op: &<A as Analysis>::Target, am: AnalysisManager) -> Self {
        let mut analysis = A::default();
        analysis.analyze(op, am);

        Self { analysis }
    }
}
impl<A: Default> Default for AnalysisWrapper<A> {
    fn default() -> Self {
        Self {
            analysis: Default::default(),
        }
    }
}
impl<A: Analysis> Analysis for AnalysisWrapper<A> {
    type Target = <A as Analysis>::Target;

    #[inline]
    fn analysis_id(&self) -> TypeId {
        self.analysis.analysis_id()
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self.analysis.as_any()
    }

    #[inline]
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any> {
        // SAFETY: This transmute is safe because AnalysisWrapper is a transparent wrapper
        // around A, so a pointer to the former is a pointer to the latter
        let ptr = Rc::into_raw(self);
        unsafe { Rc::<A>::from_raw(ptr.cast()) as Rc<dyn Any> }
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.analysis.name()
    }

    #[inline]
    fn analyze(&mut self, op: &Self::Target, am: AnalysisManager) {
        self.analysis.analyze(op, am);
    }

    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool {
        let invalidated = self.analysis.invalidate(preserved_analyses);
        if invalidated {
            preserved_analyses.unpreserve::<A>();
        }
        invalidated
    }
}

/// An [AnalysisManager] is the primary entrypoint for performing analysis on a specific operation
/// instance that it is constructed for.
///
/// It is used to manage and cache analyses for the operation, as well as those of child operations,
/// via nested [AnalysisManager] instances.
///
/// This type is a thin wrapper around a pointer, and is meant to be passed by value. It can be
/// cheaply cloned.
#[derive(Clone)]
#[repr(transparent)]
pub struct AnalysisManager {
    analyses: Rc<NestedAnalysisMap>,
}
impl AnalysisManager {
    /// Create a new top-level [AnalysisManager] for `op`
    pub fn new(op: OperationRef, instrumentor: Option<Rc<PassInstrumentor>>) -> Self {
        Self {
            analyses: Rc::new(NestedAnalysisMap::new(op, instrumentor)),
        }
    }

    /// Query for a cached analysis on the given parent operation. The analysis may not exist and if
    /// it does it may be out-of-date.
    pub fn get_cached_parent_analysis<A>(&self, parent: &OperationRef) -> Option<Rc<A>>
    where
        A: Analysis,
    {
        let mut current_parent = self.analyses.parent();
        while let Some(parent_am) = current_parent.take() {
            if &parent_am.get_operation() == parent {
                return parent_am.analyses().get_cached::<A>();
            }
            current_parent = parent_am.parent();
        }
        None
    }

    /// Query for the given analysis for the current operation.
    pub fn get_analysis<A>(&self) -> Rc<A>
    where
        A: Analysis<Target = Operation>,
    {
        self.analyses.analyses.borrow_mut().get(self.pass_instrumentor(), self.clone())
    }

    /// Query for the given analysis for the current operation of a specific derived operation type.
    ///
    /// NOTE: This will panic if the current operation is not of type `O`.
    pub fn get_analysis_for<A, O>(&self) -> Rc<A>
    where
        A: Analysis<Target = O>,
        O: 'static,
    {
        self.analyses
            .analyses
            .borrow_mut()
            .get_analysis_for::<A, O>(self.pass_instrumentor(), self.clone())
    }

    /// Query for a cached entry of the given analysis on the current operation.
    pub fn get_cached_analysis<A>(&self) -> Option<Rc<A>>
    where
        A: Analysis,
    {
        self.analyses.analyses().get_cached::<A>()
    }

    /// Query for an analysis of a child operation, constructing it if necessary.
    pub fn get_child_analysis<A>(&self, op: &OperationRef) -> Rc<A>
    where
        A: Analysis<Target = Operation>,
    {
        self.clone().nest(op).get_analysis::<A>()
    }

    /// Query for an analysis of a child operation of a specific derived operation type,
    /// constructing it if necessary.
    ///
    /// NOTE: This will panic if `op` is not of type `O`.
    pub fn get_child_analysis_for<A, O>(&self, op: &O) -> Rc<A>
    where
        A: Analysis<Target = O>,
        O: Op,
    {
        self.clone()
            .nest(&op.as_operation().as_operation_ref())
            .get_analysis_for::<A, O>()
    }

    /// Query for a cached analysis of a child operation, or return `None`.
    pub fn get_cached_child_analysis<A>(&self, child: &OperationRef) -> Option<Rc<A>>
    where
        A: Analysis,
    {
        assert!(child.borrow().parent_op().unwrap() == self.analyses.get_operation());
        let child_analyses = self.analyses.child_analyses.borrow();
        let child_analyses = child_analyses.get(child)?;
        let child_analyses = child_analyses.analyses.borrow();
        child_analyses.get_cached::<A>()
    }

    /// Get an analysis manager for the given operation, which must be a proper descendant of the
    /// current operation represented by this analysis manager.
    pub fn nest(&self, op: &OperationRef) -> AnalysisManager {
        let current_op = self.analyses.get_operation();
        assert!(
            current_op.borrow().is_proper_ancestor_of(&op.borrow()),
            "expected valid descendant op"
        );

        // Check for the base case where the provided operation is immediately nested
        if current_op == op.borrow().parent_op().expect("expected `op` to have a parent") {
            return self.nest_immediate(op.clone());
        }

        // Otherwise, we need to collect all ancestors up to the current operation
        let mut ancestors = SmallVec::<[OperationRef; 4]>::default();
        let mut next_op = op.clone();
        while next_op != current_op {
            ancestors.push(next_op.clone());
            next_op = next_op.borrow().parent_op().unwrap();
        }

        let mut manager = self.clone();
        while let Some(op) = ancestors.pop() {
            manager = manager.nest_immediate(op);
        }
        manager
    }

    fn nest_immediate(&self, op: OperationRef) -> AnalysisManager {
        use hashbrown::hash_map::Entry;

        assert!(
            Some(self.analyses.get_operation()) == op.borrow().parent_op(),
            "expected immediate child operation"
        );
        let parent = self.analyses.clone();
        let mut child_analyses = self.analyses.child_analyses.borrow_mut();
        match child_analyses.entry(op.clone()) {
            Entry::Vacant(entry) => {
                let analyses = entry.insert(Rc::new(parent.nest(op)));
                AnalysisManager {
                    analyses: Rc::clone(analyses),
                }
            }
            Entry::Occupied(entry) => AnalysisManager {
                analyses: Rc::clone(entry.get()),
            },
        }
    }

    /// Invalidate any non preserved analyses.
    #[inline]
    pub fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) {
        Rc::clone(&self.analyses).invalidate(preserved_analyses)
    }

    /// Clear any held analyses.
    #[inline]
    pub fn clear(&mut self) {
        self.analyses.clear();
    }

    /// Clear any held analyses when the returned guard is dropped.
    #[inline]
    pub fn defer_clear(&self) -> ResetAnalysesOnDrop {
        ResetAnalysesOnDrop {
            analyses: self.analyses.clone(),
        }
    }

    /// Returns a [PassInstrumentor] for the current operation, if one was installed.
    #[inline]
    pub fn pass_instrumentor(&self) -> Option<Rc<PassInstrumentor>> {
        self.analyses.pass_instrumentor()
    }
}

#[must_use]
#[doc(hidden)]
pub struct ResetAnalysesOnDrop {
    analyses: Rc<NestedAnalysisMap>,
}
impl Drop for ResetAnalysesOnDrop {
    fn drop(&mut self) {
        self.analyses.clear()
    }
}

/// An analysis map that contains a map for the current operation, and a set of maps for any child
/// operations.
struct NestedAnalysisMap {
    parent: Option<Rc<NestedAnalysisMap>>,
    instrumentor: Option<Rc<PassInstrumentor>>,
    analyses: RefCell<AnalysisMap>,
    child_analyses: RefCell<FxHashMap<OperationRef, Rc<NestedAnalysisMap>>>,
}
impl NestedAnalysisMap {
    /// Create a new top-level [NestedAnalysisMap] for `op`, with the given optional pass
    /// instrumentor.
    pub fn new(op: OperationRef, instrumentor: Option<Rc<PassInstrumentor>>) -> Self {
        Self {
            parent: None,
            instrumentor,
            analyses: RefCell::new(AnalysisMap::new(op)),
            child_analyses: Default::default(),
        }
    }

    /// Create a new [NestedAnalysisMap] for `op` nested under `self`.
    pub fn nest(self: Rc<Self>, op: OperationRef) -> Self {
        let instrumentor = self.instrumentor.clone();
        Self {
            parent: Some(self),
            instrumentor,
            analyses: RefCell::new(AnalysisMap::new(op)),
            child_analyses: Default::default(),
        }
    }

    /// Get the parent [NestedAnalysisMap], or `None` if this is a top-level map.
    pub fn parent(&self) -> Option<Rc<NestedAnalysisMap>> {
        self.parent.clone()
    }

    /// Return a [PassInstrumentor] for the current operation, if one was installed.
    pub fn pass_instrumentor(&self) -> Option<Rc<PassInstrumentor>> {
        self.instrumentor.clone()
    }

    /// Get the operation for this analysis map.
    #[inline]
    pub fn get_operation(&self) -> OperationRef {
        self.analyses.borrow().get_operation()
    }

    fn analyses(&self) -> core::cell::Ref<'_, AnalysisMap> {
        self.analyses.borrow()
    }

    /// Invalidate any non preserved analyses.
    pub fn invalidate(self: Rc<Self>, preserved_analyses: &mut PreservedAnalyses) {
        // If all analyses were preserved, then there is nothing to do
        if preserved_analyses.is_all() {
            return;
        }

        // Invalidate the analyses for the current operation directly
        self.analyses.borrow_mut().invalidate(preserved_analyses);

        // If no analyses were preserved, then just simply clear out the child analysis results
        if preserved_analyses.is_none() {
            self.child_analyses.borrow_mut().clear();
        }

        // Otherwise, invalidate each child analysis map
        let mut to_invalidate = SmallVec::<[Rc<NestedAnalysisMap>; 8]>::from_iter([self]);
        while let Some(map) = to_invalidate.pop() {
            map.child_analyses.borrow_mut().retain(|_op, nested_analysis_map| {
                Rc::clone(nested_analysis_map).invalidate(preserved_analyses);
                if nested_analysis_map.child_analyses.borrow().is_empty() {
                    false
                } else {
                    to_invalidate.push(Rc::clone(nested_analysis_map));
                    true
                }
            });
        }
    }

    pub fn clear(&self) {
        self.child_analyses.borrow_mut().clear();
        self.analyses.borrow_mut().clear();
    }
}

/// This class represents a cache of analyses for a single operation.
///
/// All computation, caching, and invalidation of analyses takes place here.
struct AnalysisMap {
    analyses: FxHashMap<TypeId, Rc<dyn OperationAnalysis>>,
    ir: OperationRef,
}
impl AnalysisMap {
    pub fn new(ir: OperationRef) -> Self {
        Self {
            analyses: Default::default(),
            ir,
        }
    }

    /// Get an analysis for the current IR unit, computing it if necessary.
    pub fn get<A>(&mut self, pi: Option<Rc<PassInstrumentor>>, am: AnalysisManager) -> Rc<A>
    where
        A: Analysis<Target = Operation>,
    {
        Self::get_analysis_impl::<A, Operation>(
            &mut self.analyses,
            pi,
            &self.ir.borrow(),
            &self.ir,
            am,
        )
    }

    /// Get a cached analysis instance if one exists, otherwise return `None`.
    pub fn get_cached<A>(&self) -> Option<Rc<A>>
    where
        A: Analysis,
    {
        self.analyses.get(&TypeId::of::<A>()).cloned().and_then(|a| a.downcast::<A>())
    }

    /// Get an analysis for the current IR unit, assuming it's of the specified type, computing it
    /// if necessary.
    ///
    /// NOTE: This will panic if the current operation is not of type `O`.
    pub fn get_analysis_for<A, O>(
        &mut self,
        pi: Option<Rc<PassInstrumentor>>,
        am: AnalysisManager,
    ) -> Rc<A>
    where
        A: Analysis<Target = O>,
        O: 'static,
    {
        let ir = <<A as Analysis>::Target as PassTarget>::into_target(&self.ir);
        Self::get_analysis_impl::<A, O>(&mut self.analyses, pi, &*ir, &self.ir, am)
    }

    fn get_analysis_impl<A, O>(
        analyses: &mut FxHashMap<TypeId, Rc<dyn OperationAnalysis>>,
        pi: Option<Rc<PassInstrumentor>>,
        ir: &O,
        op: &OperationRef,
        am: AnalysisManager,
    ) -> Rc<A>
    where
        A: Analysis<Target = O>,
    {
        use hashbrown::hash_map::Entry;

        let id = TypeId::of::<A>();
        match analyses.entry(id) {
            Entry::Vacant(entry) => {
                // We don't have a cached analysis for the operation, compute it directly and
                // add it to the cache.
                if let Some(pi) = pi.as_deref() {
                    pi.run_before_analysis(core::any::type_name::<A>(), &id, op);
                }

                let analysis = entry.insert(Self::construct_analysis::<A, O>(am, ir));

                if let Some(pi) = pi.as_deref() {
                    pi.run_after_analysis(core::any::type_name::<A>(), &id, op);
                }

                Rc::clone(analysis).downcast::<A>().unwrap()
            }
            Entry::Occupied(entry) => Rc::clone(entry.get()).downcast::<A>().unwrap(),
        }
    }

    fn construct_analysis<A, O>(am: AnalysisManager, op: &O) -> Rc<dyn OperationAnalysis>
    where
        A: Analysis<Target = O>,
    {
        Rc::new(AnalysisWrapper::<A>::new(op, am)) as Rc<dyn OperationAnalysis>
    }

    /// Returns the operation that this analysis map represents.
    pub fn get_operation(&self) -> OperationRef {
        self.ir.clone()
    }

    /// Clear any held analyses.
    pub fn clear(&mut self) {
        self.analyses.clear();
    }

    /// Invalidate any cached analyses based upon the given set of preserved analyses.
    pub fn invalidate(&mut self, preserved_analyses: &mut PreservedAnalyses) {
        // Remove any analyses that were invalidated.
        //
        // Using `retain`, we preserve the original insertion order, and dependencies always go
        // before users, so we need only a single pass through.
        self.analyses.retain(|_, a| !a.invalidate(preserved_analyses));
    }
}
