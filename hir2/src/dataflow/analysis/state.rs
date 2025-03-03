mod guard;
mod info;
mod raw;

use core::any::Any;

pub(in crate::dataflow) use self::raw::{
    AnalysisStateDescriptor, RawAnalysisStateInfo, RawAnalysisStateInfoHandle,
};
pub use self::{
    guard::AnalysisStateGuard,
    info::{
        AnalysisStateInfo, AnalysisStateSubscription, AnalysisStateSubscriptionBehavior, Revision,
    },
};
use super::DataFlowAnalysis;
use crate::dataflow::{LatticeAnchor, LatticeAnchorRef, ProgramPoint};

/// The identifier for a uniqued [AnalysisStateKeyImpl]
#[derive(Copy, Clone)]
pub struct AnalysisStateKey {
    type_id: core::any::TypeId,
    anchor: LatticeAnchorRef,
}
impl AnalysisStateKey {
    pub fn new<T>(anchor: LatticeAnchorRef) -> Self
    where
        T: BuildableAnalysisState,
    {
        Self {
            type_id: core::any::TypeId::of::<T>(),
            anchor,
        }
    }
}
impl Eq for AnalysisStateKey {}
impl PartialEq for AnalysisStateKey {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && self.anchor == other.anchor
    }
}
impl core::hash::Hash for AnalysisStateKey {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.type_id.hash(state);
        self.anchor.hash(state);
    }
}
impl core::fmt::Debug for AnalysisStateKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AnalysisStateKey")
            .field("type_id", &self.type_id)
            .field_with("anchor", |f| write!(f, "{}", &self.anchor))
            .finish()
    }
}
impl core::fmt::Display for AnalysisStateKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", &self.anchor)
    }
}

pub trait BuildableAnalysisState: AnalysisState + Any {
    fn create(anchor: LatticeAnchorRef) -> Self;
}

pub trait AnalysisState {
    fn as_any(&self) -> &dyn Any;
    fn type_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }
    fn anchor(&self) -> &dyn LatticeAnchor;
}

impl dyn AnalysisState {
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        self.as_any().is::<T>()
    }

    #[inline]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}
