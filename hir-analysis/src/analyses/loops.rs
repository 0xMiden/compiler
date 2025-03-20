use core::any::Any;

use crate::{AnalysisState, BuildableAnalysisState, ChangeResult, LatticeAnchor, LatticeAnchorRef};

/// This enumeration represents a lattice of control flow edges that have loop effects.
///
/// The lattice is as follows:
///
/// * `Uninitialized` is the _bottom_ state, and the default state of the lattice
/// * `Unknown` is the _top_ or _overdefined_ state, and represents a state where we are unable to
///   conclude any facts about loop effects along the corresponding control flow edge.
/// * `None` is the "minimal" initialized state of the lattice, i.e. thus far, there are no loop
///   effects known to occur along the current control flow edge
/// * `Enter`, `Latch`, and `Exit` are "maximal" initialized states of the lattice, but are
///   mutually-exclusive determinations. A control flow edge cannot be more than one of these at the
///   same time.
///
/// The partial order (and transitions) when joining states are:
///
/// * `uninitialized -> none -> enter|latch|exit -> unknown`
///
/// Put another way, we can join `None` with `Enter` and get `Enter`, but joining `Enter` with
/// `Exit` will produce `Unknown`.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoopAction {
    /// We have no information about this edge yet
    #[default]
    Uninitialized,
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

impl LoopAction {
    #[inline]
    pub fn is_uninitialized(&self) -> bool {
        matches!(self, Self::Uninitialized)
    }

    #[inline]
    pub fn is_overdefined(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    #[inline]
    pub fn is_known_loop_effect(&self) -> bool {
        matches!(self, Self::Enter | Self::Latch | Self::Exit)
    }
}

/// An [AnalyisState] that associates a [LoopAction] with some anchor ([CfgEdge] for now)
#[derive(Copy, Clone)]
pub struct LoopState {
    anchor: LatticeAnchorRef,
    action: LoopAction,
}

impl LoopState {
    /// What type of action is associated with the anchor
    #[inline]
    pub const fn action(&self) -> LoopAction {
        self.action
    }

    /// Returns true if this state indicates that the anchor is associated with entry to a loop
    pub fn is_entering_loop(&self) -> bool {
        matches!(self.action, LoopAction::Enter)
    }

    /// Returns true if this state indicates that the anchor is associated with exiting a loop
    pub fn is_exiting_loop(&self) -> bool {
        matches!(self.action, LoopAction::Exit)
    }

    /// Returns true if this state indicates that the anchor is associated with a loop latch, i.e.
    /// a block in a loop which has a backedge to the loop header.
    pub fn is_loop_latch(&self) -> bool {
        matches!(self.action, LoopAction::Latch)
    }

    /// Set the loop action for this state, returning whether or not that is a change from the
    /// previous state.
    ///
    /// NOTE: This will set the state to overdefined if `action` conflicts with the current state.
    pub fn set_action(&mut self, action: LoopAction) -> ChangeResult {
        if action.is_uninitialized() {
            if self.action.is_uninitialized() {
                return ChangeResult::Unchanged;
            }
            self.action = action;
            return ChangeResult::Changed;
        }

        if self.action.is_overdefined() {
            return ChangeResult::Unchanged;
        }

        if self.action.is_known_loop_effect() {
            if self.action != action {
                self.action = LoopAction::Unknown;
                return ChangeResult::Changed;
            }
            return ChangeResult::Unchanged;
        }

        if core::mem::replace(&mut self.action, action) == action {
            ChangeResult::Unchanged
        } else {
            ChangeResult::Changed
        }
    }

    /// Joining two loop states will do one of the following:
    ///
    /// * If `Uninitialized`, `action` is determined to be the new loop state
    /// * If `Unknown`, the result is always `Unknown`
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
