use alloc::rc::Rc;
use core::{
    any::{Any, TypeId},
    hash::Hash,
};

use midenc_session::Session;
use rustc_hash::FxBuildHasher;

use crate::{diagnostics::Report, FxHashMap, FxHashSet};

/// A convenient type alias for `Result<T, AnalysisError>`
pub type AnalysisResult<T> = Result<T, Report>;

#[doc(hidden)]
pub trait PreservableAnalysis: Any {
    /// Called to determine if this analysis should be invalidated after a pass is run
    ///
    /// By default, all analyses are always invalidated after a pass, unless that pass
    /// specifically marks an analysis as preserved.
    ///
    /// If overridden, implementors must ensure that they use the provided
    /// [PreservedAnalyses] set to determine if any dependencies were invalidated,
    /// and return `true` if so.
    fn is_invalidated(&self, _analyses: &PreservedAnalyses) -> bool {
        true
    }
}
impl<A: Analysis + Any> PreservableAnalysis for A {
    fn is_invalidated(&self, analyses: &PreservedAnalyses) -> bool {
        <A as Analysis>::is_invalidated(self, analyses)
    }
}

/// An [Analysis] computes information about some compiler entity, e.g. a module.
///
/// Analyses are cached, and associated with a unique key derived from the entity
/// to which they were applied. These cached analyses can then be queried via the
/// [AnalysisManager] by requesting a specific concrete [Analysis] type using that
/// key.
///
/// For example, a module is typically associated with a unique identifier. Thus
/// to obtain the analysis for a specific module, you would request the specific
/// analysis using the module id, see [AnalysisManager::get].
pub trait Analysis: Sized + Any {
    /// The entity to which this analysis applies
    type Entity: AnalysisKey;

    /// Analyze `entity`, using the provided [AnalysisManager] to query other
    /// analyses on which this one depends; and the provided [Session] to
    /// configure this analysis based on the current compilation session.
    fn analyze(
        entity: &Self::Entity,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> AnalysisResult<Self>;

    /// Called to determine if this analysis should be invalidated after a pass is run
    ///
    /// By default, all analyses are always invalidated after a pass, unless that pass
    /// specifically marks an analysis as preserved.
    ///
    /// If overridden, implementors must ensure that they use the provided
    /// [PreservedAnalyses] set to determine if any dependencies were invalidated,
    /// and return `true` if so.
    fn is_invalidated(&self, _analyses: &PreservedAnalyses) -> bool {
        true
    }
}

/// The [AnalysisKey] trait is implemented for compiler entities that are targeted
/// for one or more [Analysis], and have a stable, unique identifier which can be
/// used to cache the results of each analysis computed for that entity.
///
/// You must ensure that the key uniquely identifies the entity to which it applies,
/// or incorrect analysis results will be returned from the [AnalysisManager]. Note,
/// however, that it is not necessary to ensure that the key reflect changes to the
/// underlying entity (see [AnalysisManager::invalidate] for how invalidation is done).
pub trait AnalysisKey: 'static {
    /// The type of the unique identifier associated with `Self`
    type Key: Hash + PartialEq + Eq;

    /// Get the key to associate with the current entity
    fn key(&self) -> Self::Key;
}

/// This type is used as a cache key for analyses cached by the [AnalysisManager].
///
/// It pairs the [TypeId] of the [Analysis], with the hash of the entity type and
/// the unique key associated with the specific instance of the entity type to which
/// the analysis applies. This ensures that for any given analysis/entity combination,
/// no two cache entries will have the same key unless the analysis key for the entity
/// matches.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CachedAnalysisKey {
    ty: TypeId,
    key: u64,
}
impl CachedAnalysisKey {
    /// Get a new cache key from the given type information and analysis key
    fn new<A>(key: &<<A as Analysis>::Entity as AnalysisKey>::Key) -> Self
    where
        A: Analysis,
    {
        let ty = TypeId::of::<A>();
        let key = Self::entity_key::<<A as Analysis>::Entity>(key);
        Self { ty, key }
    }

    fn entity_key<T>(key: &<T as AnalysisKey>::Key) -> u64
    where
        T: AnalysisKey,
    {
        use core::hash::Hasher;

        let mut hasher = rustc_hash::FxHasher::default();
        let entity_ty = TypeId::of::<T>();
        entity_ty.hash(&mut hasher);
        key.hash(&mut hasher);
        hasher.finish()
    }
}

/// [PreservedAnalyses] represents the set of analyses which will be preserved for the next pass.
///
/// You may mark an analysis as preserved using [AnalysisManager::mark_preserved].
#[derive(Default)]
pub struct PreservedAnalyses {
    current_entity_key: u64,
    preserved: FxHashMap<CachedAnalysisKey, Rc<dyn PreservableAnalysis>>,
}
impl PreservedAnalyses {
    fn new(
        current_entity_key: u64,
        mut cached: FxHashMap<CachedAnalysisKey, Rc<dyn PreservableAnalysis>>,
        preserve: FxHashSet<CachedAnalysisKey>,
    ) -> Self {
        // Since we know which analyses are definitely preserved,
        // build the initial preserved analyses set from those.
        let mut preserved = Self::with_capacity(current_entity_key, preserve.len());
        for key in preserve.into_iter() {
            if let Some(analysis) = cached.remove(&key) {
                preserved.insert(key, analysis);
            }
        }

        // Preserve all analyses for other entities
        let mut worklist = vec![];
        for (key, analysis) in cached.into_iter() {
            if key.key != current_entity_key {
                preserved.insert(key, analysis);
                continue;
            }
            worklist.push((key, analysis));
        }

        // Ask all remaining analyses if they should indeed be invalidated.
        //
        // We iterate to a fixpoint here to ensure that any new preserved analyses
        // are taken into account by the remaining analyses pending invalidation
        // when their `is_invalidated` method is called. If those analyses depend
        // on a newly preserved analysis, they may be able to avoid being invalidated
        // if they have no other invalidated dependencies.
        let mut q = vec![];
        let mut changed = false;
        loop {
            while let Some((key, analysis)) = worklist.pop() {
                if analysis.is_invalidated(&preserved) {
                    q.push((key, analysis));
                    continue;
                } else {
                    changed = true;
                    preserved.insert(key, analysis);
                }
            }
            if !changed {
                break;
            }
            changed = false;
            core::mem::swap(&mut worklist, &mut q);
        }

        preserved
    }

    /// Returns true if the analysis associated with the given type and key is preserved
    pub fn is_preserved<A>(&self) -> bool
    where
        A: Analysis,
    {
        let ty = TypeId::of::<A>();
        let key = CachedAnalysisKey {
            ty,
            key: self.current_entity_key,
        };
        self.preserved.contains_key(&key)
    }

    #[inline]
    fn insert(&mut self, key: CachedAnalysisKey, analysis: Rc<dyn PreservableAnalysis>) {
        self.preserved.insert(key, analysis);
    }

    fn with_capacity(current_entity_key: u64, cap: usize) -> Self {
        Self {
            current_entity_key,
            preserved: FxHashMap::with_capacity_and_hasher(cap, FxBuildHasher),
        }
    }
}

/// The [AnalysisManager] is used to query and compute analyses required during compilation.
///
/// Each thread gets its own analysis manager, and may query any analysis, as long as the
/// caller has the key used for caching that analysis (e.g. module identifier).
///
/// To compute an analysis, one must have a reference to the entity on which the analysis
/// is applied, and request that the analysis be computed.
///
/// Analyses are cached, and assumed valid until explicitly invalidated. An analysis should
/// be invalidated any time the underlying entity changes, unless the analysis is known to
/// be preserved even with those changes.
#[derive(Default)]
pub struct AnalysisManager {
    /// We store the analysis results as `Rc` so that we can freely hand out references
    /// to the analysis results without having to concern ourselves with too much lifetime
    /// management.
    ///
    /// Since an [AnalysisManager] is scoped to a single thread, the reference counting
    /// overhead is essentially irrelevant.
    cached: FxHashMap<CachedAnalysisKey, Rc<dyn PreservableAnalysis>>,
    /// The set of analyses to preserve after the current pass is run
    preserve: FxHashSet<CachedAnalysisKey>,
    /// The set of entity keys that have had `preserve_none` set
    preserve_none: FxHashSet<u64>,
    /// The set of entity keys that have had `preserve_all` set
    preserve_all: FxHashSet<u64>,
}
impl AnalysisManager {
    /// Get a new, empty [AnalysisManager].
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the given analysis has been computed and is in the cache
    pub fn is_available<A>(&self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key) -> bool
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        self.cached.contains_key(&key)
    }

    /// Get a reference to the analysis of the requested type, for the given entity, if available
    pub fn get<A>(&self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key) -> Option<Rc<A>>
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        self.cached.get(&key).cloned().map(preservable_analysis_to_concrete)
    }

    /// Get a reference to the analysis of the requested type, for the given entity, or panics with
    /// `msg`
    pub fn expect<A>(&self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key, msg: &str) -> Rc<A>
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        self.cached.get(&key).cloned().map(preservable_analysis_to_concrete).expect(msg)
    }

    /// Get a reference to the analysis of the requested type, or the default value, for the given
    /// entity, if available
    ///
    /// If unavailable, and the default value is returned, that value is not cached.
    pub fn get_or_default<A>(&self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key) -> Rc<A>
    where
        A: Analysis + Default,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        self.cached
            .get(&key)
            .cloned()
            .map(preservable_analysis_to_concrete)
            .unwrap_or_else(|| Rc::new(A::default()))
    }

    /// Get a reference to the analysis of the requested type, computing it if necessary
    ///
    /// If computing the analysis fails, `Err` is returned.
    pub fn get_or_compute<A>(
        &mut self,
        entity: &<A as Analysis>::Entity,
        session: &Session,
    ) -> AnalysisResult<Rc<A>>
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(&entity.key());
        if let Some(cached) = self.cached.get(&key).cloned() {
            return Ok(preservable_analysis_to_concrete(cached));
        }
        let analysis = Rc::new(A::analyze(entity, self, session)?);
        let any = Rc::clone(&analysis);
        self.cached.insert(key, any);
        Ok(analysis)
    }

    /// If an analysis of the requested type has been computed, take ownership of it,
    /// and return the owned object to the caller.
    ///
    /// If there are outstanding references to the cached analysis data, then the data
    /// will be cloned so that the caller gets an owning reference.
    ///
    /// If the analysis has not been computed, returns `None`
    pub fn take<A>(&mut self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key) -> Option<A>
    where
        A: Analysis + Clone,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        let cached = preservable_analysis_to_concrete(self.cached.remove(&key)?);
        Some(match Rc::try_unwrap(cached) {
            Ok(analysis) => analysis,
            Err(cached) => (*cached).clone(),
        })
    }

    /// Insert an analysis into the manager with the given key
    pub fn insert<A>(&mut self, key: <<A as Analysis>::Entity as AnalysisKey>::Key, analysis: A)
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(&key);
        self.cached.insert(key, Rc::new(analysis));
    }

    /// Mark all analyses as invalidated, unless otherwise preserved, forcing recomputation
    /// of those analyses the next time they are requested.
    ///
    /// This clears any preservation markers that were set prior to calling this function,
    /// e.g. with `mark_preserved`. When this function returns, all analyses are assumed
    /// to be invalidated the next time this function is called, unless otherwise indicated.
    pub fn invalidate<T>(&mut self, key: &<T as AnalysisKey>::Key)
    where
        T: AnalysisKey,
    {
        let current_entity_key = CachedAnalysisKey::entity_key::<T>(key);

        if self.preserve_none.remove(&current_entity_key) {
            self.cached.retain(|k, _| k.key != current_entity_key);
            return;
        }

        if self.preserve_all.remove(&current_entity_key) {
            return;
        }

        let mut to_preserve = vec![];
        for key in self.preserve.iter() {
            if key.key == current_entity_key {
                to_preserve.push(*key);
            }
        }

        let mut to_invalidate = vec![];
        for key in self.cached.keys() {
            if key.key == current_entity_key {
                to_invalidate.push(*key);
            }
        }

        let mut preserve = FxHashSet::default();
        for key in to_preserve.into_iter() {
            preserve.insert(self.preserve.take(&key).unwrap());
        }

        let mut cached = FxHashMap::with_capacity_and_hasher(to_invalidate.len(), FxBuildHasher);
        for key in to_invalidate.into_iter() {
            let (key, value) = self.cached.remove_entry(&key).unwrap();
            cached.insert(key, value);
        }

        let preserved = PreservedAnalyses::new(current_entity_key, cached, preserve);
        self.cached.extend(preserved.preserved);
    }

    /// Mark the given analysis as no longer valid (due to changes to the analyzed entity)
    ///
    /// You should invalidate analyses any time you modify the IR for that entity, unless
    /// you can guarantee that the specific analysis is preserved.
    pub fn mark_invalid<A>(&mut self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key)
    where
        A: Analysis,
    {
        let key = CachedAnalysisKey::new::<A>(key);
        self.preserve.remove(&key);
        self.cached.remove(&key);
    }

    /// When called, the current pass is signalling that all analyses should be invalidated
    /// after it completes, regardless of any other configuration.
    pub fn mark_none_preserved<T>(&mut self, key: &<T as AnalysisKey>::Key)
    where
        T: AnalysisKey,
    {
        let preserve_entity_key = CachedAnalysisKey::entity_key::<T>(key);
        self.preserve_none.insert(preserve_entity_key);
        self.preserve_all.remove(&preserve_entity_key);
    }

    /// When called, the current pass is signalling that all analyses will still be valid
    /// after it completes, i.e. it makes no modifications that would invalidate an analysis.
    ///
    /// Care must be taken when doing this, to ensure that the pass actually does not do
    /// anything that would invalidate any analysis results, or miscompiles are likely to
    /// occur.
    pub fn mark_all_preserved<T>(&mut self, key: &<T as AnalysisKey>::Key)
    where
        T: AnalysisKey,
    {
        let preserve_entity_key = CachedAnalysisKey::entity_key::<T>(key);
        self.preserve_all.insert(preserve_entity_key);
        self.preserve_none.remove(&preserve_entity_key);
    }

    /// When called, the current pass is signalling that the given analysis identified by `key`,
    /// will still be valid after it completes.
    ///
    /// This should only be called when the caller can guarantee that the analysis is truly
    /// preserved by the pass, otherwise miscompiles are likely to occur.
    pub fn mark_preserved<A>(&mut self, key: &<<A as Analysis>::Entity as AnalysisKey>::Key)
    where
        A: Analysis,
    {
        // If we're preserving everything, or preserving nothing, this is a no-op
        let key = CachedAnalysisKey::new::<A>(key);
        if self.preserve_all.contains(&key.key) || self.preserve_none.contains(&key.key) {
            return;
        }

        self.preserve.insert(key);
    }
}

fn preservable_analysis_to_concrete<A, T>(pa: Rc<dyn PreservableAnalysis>) -> Rc<A>
where
    T: AnalysisKey,
    A: Analysis<Entity = T>,
{
    let any: Rc<dyn Any> = pa;
    any.downcast::<A>().expect("invalid cached analysis key")
}
