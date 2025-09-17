use std::{ops::Deref, path::Path, str::FromStr};

use glob::Pattern;
use miden_processor::VmState;

use super::ResolvedLocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: u8,
    pub creation_cycle: usize,
    pub ty: BreakpointType,
}

impl Default for Breakpoint {
    fn default() -> Self {
        Self {
            id: 0,
            creation_cycle: 0,
            ty: BreakpointType::Step,
        }
    }
}

impl Breakpoint {
    /// Return the number of cycles this breakpoint indicates we should skip, or `None` if the
    /// number of cycles is context-specific, or the breakpoint is triggered by something other
    /// than cycle count.
    pub fn cycles_to_skip(&self, current_cycle: usize) -> Option<usize> {
        let cycles_passed = current_cycle - self.creation_cycle;
        match &self.ty {
            BreakpointType::Step => Some(1),
            BreakpointType::StepN(n) => Some(n.saturating_sub(cycles_passed)),
            BreakpointType::StepTo(to) if to >= &current_cycle => Some(to.abs_diff(current_cycle)),
            _ => None,
        }
    }
}
impl Deref for Breakpoint {
    type Target = BreakpointType;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.ty
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointType {
    /// Break at next cycle
    Step,
    /// Skip N cycles
    StepN(usize),
    /// Break at a given cycle
    StepTo(usize),
    /// Break at the first cycle of the next instruction
    Next,
    /// Break when we exit the current call frame
    Finish,
    /// Break when any cycle corresponds to a source location whose file matches PATTERN
    File(Pattern),
    /// Break when any cycle corresponds to a source location whose file matches PATTERN and occurs
    /// on LINE
    Line { pattern: Pattern, line: u32 },
    /// Break anytime the given operation occurs
    Opcode(miden_core::Operation),
    /// Break when any cycle causes us to push a frame for PROCEDURE on the call stack
    Called(Pattern),
}
impl BreakpointType {
    /// Return true if this breakpoint indicates we should break for `current_op`
    pub fn should_break_for(&self, current_op: &miden_core::Operation) -> bool {
        match self {
            Self::Opcode(op) => current_op == op,
            _ => false,
        }
    }

    /// Return true if this breakpoint indicates we should break on entry to `procedure`
    pub fn should_break_in(&self, procedure: &str) -> bool {
        match self {
            Self::Called(pattern) => pattern.matches(procedure),
            _ => false,
        }
    }

    /// Return true if this breakpoint indicates we should break at `loc`
    pub fn should_break_at(&self, loc: &ResolvedLocation) -> bool {
        match self {
            Self::File(pattern) => {
                pattern.matches_path(Path::new(loc.source_file.deref().content().uri().as_str()))
            }
            Self::Line { pattern, line } if line == &loc.line => {
                pattern.matches_path(Path::new(loc.source_file.deref().content().uri().as_str()))
            }
            _ => false,
        }
    }

    /// Returns true if this breakpoint is internal to the debugger (i.e. not creatable via :b)
    pub fn is_internal(&self) -> bool {
        matches!(self, BreakpointType::Next | BreakpointType::Step | BreakpointType::Finish)
    }

    /// Returns true if this breakpoint is removed upon being hit
    pub fn is_one_shot(&self) -> bool {
        matches!(
            self,
            BreakpointType::Next
                | BreakpointType::Finish
                | BreakpointType::Step
                | BreakpointType::StepN(_)
                | BreakpointType::StepTo(_)
        )
    }
}

impl FromStr for BreakpointType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // b next
        // b finish
        // b after {n}
        // b for {opcode}
        // b at {cycle}
        // b in {procedure}
        // b {file}[:{line}]
        if s == "next" {
            return Ok(BreakpointType::Next);
        }
        if s == "finish" {
            return Ok(BreakpointType::Finish);
        }
        if let Some(n) = s.strip_prefix("after ") {
            let n = n.trim().parse::<usize>().map_err(|err| {
                format!("invalid breakpoint expression: could not parse cycle count: {err}")
            })?;
            return Ok(BreakpointType::StepN(n));
        }
        if let Some(_opcode) = s.strip_prefix("for ") {
            todo!()
        }
        if let Some(cycle) = s.strip_prefix("at ") {
            let cycle = cycle.trim().parse::<usize>().map_err(|err| {
                format!("invalid breakpoint expression: could not parse cycle value: {err}")
            })?;
            return Ok(BreakpointType::StepTo(cycle));
        }
        if let Some(procedure) = s.strip_prefix("in ") {
            let pattern = Pattern::new(procedure.trim())
                .map_err(|err| format!("invalid breakpoint expression: bad pattern: {err}"))?;
            return Ok(BreakpointType::Called(pattern));
        }
        match s.split_once(':') {
            Some((file, line)) => {
                let pattern = Pattern::new(file.trim())
                    .map_err(|err| format!("invalid breakpoint expression: bad pattern: {err}"))?;
                let line = line.trim().parse::<u32>().map_err(|err| {
                    format!("invalid breakpoint expression: could not parse line: {err}")
                })?;
                Ok(BreakpointType::Line { pattern, line })
            }
            None => {
                let pattern = Pattern::new(s.trim())
                    .map_err(|err| format!("invalid breakpoint expression: bad pattern: {err}"))?;
                Ok(BreakpointType::File(pattern))
            }
        }
    }
}
