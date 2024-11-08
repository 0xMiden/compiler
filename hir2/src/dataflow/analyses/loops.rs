use core::any::Any;

use crate::dataflow::{
    AnalysisState, BuildableAnalysisState, ChangeResult, LatticeAnchor, LatticeAnchorRef,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoopAction {
    /// We do not know what loop action might be taking place along this edge
    Unknown,
    /// No loop action is taking place along the associated control flow edge
    None,
    /// The associated control flow edge enters a loop
    Enter,
    /// The associated control flow edge loops back to the loop header
    Latch,
    /// The associated control flow edge exits the current loop
    Exit,
}

pub struct LoopState {
    anchor: LatticeAnchorRef,
    action: LoopAction,
}

impl LoopState {
    #[inline]
    pub const fn action(&self) -> LoopAction {
        self.action
    }

    pub fn is_entering_loop(&self) -> bool {
        matches!(self.action, LoopAction::Enter)
    }

    pub fn is_exiting_loop(&self) -> bool {
        matches!(self.action, LoopAction::Exit)
    }

    pub fn is_loop_latch(&self) -> bool {
        matches!(self.action, LoopAction::Latch)
    }

    /// Set the loop action for this state, returning whether or not that is a change from the
    /// previous state.
    pub fn set_action(&mut self, action: LoopAction) -> ChangeResult {
        if core::mem::replace(&mut self.action, action) == action {
            ChangeResult::Unchanged
        } else {
            ChangeResult::Changed
        }
    }

    pub fn join(&mut self, action: LoopAction) -> ChangeResult {
        self.set_action(action)
    }
}

impl BuildableAnalysisState for LoopState {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            action: LoopAction::Unknown,
        }
    }
}

impl AnalysisState for LoopState {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        &self.anchor
    }
}
