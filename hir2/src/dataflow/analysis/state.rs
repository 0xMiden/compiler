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
use crate::{
    dataflow::{LatticeAnchor, LatticeAnchorRef, ProgramPoint},
    InsertionPoint,
};

/// The identifier for a uniqued [AnalysisStateKeyImpl]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalysisStateKey(u64);

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
