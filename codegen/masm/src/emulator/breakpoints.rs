use std::collections::BTreeSet;

use midenc_hir::{FunctionIdent, FxHashMap, FxHashSet};

use super::{Addr, BreakpointEvent, EmulatorEvent, Instruction, InstructionPointer};
use crate::BlockId;

/// A breakpoint can be used to force the emulator to suspend
/// execution when a specific event or condition is reached.
///
/// When hit, control is handed back to the owner of the emulator
/// so that they can inspect the state, potentially make changes,
/// and then resume execution if desired.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Breakpoint {
    /// Break after each cycle
    All,
    /// Break when the cycle count reaches N
    Cycle(usize),
    /// Step until control reaches the given instruction pointer value
    At(InstructionPointer),
    /// Break at loop instructions
    ///
    /// The break will start on the looping instruction itself, and when
    /// execution resumes, will break either at the next nested loop, or
    /// if a complete iteration is reached, one of two places depending on
    /// the type of looping instruction we're in:
    ///
    /// * `while.true` will break at the `while.true` on each iteration
    /// * `repeat.n` will break at the top of the loop body on each iteration
    Loops,
    /// Break when the given function is called
    Called(FunctionIdent),
    /// Break when the given watchpoint is hit
    ///
    /// This is also referred to as a watchpoint
    Watch(WatchpointId),
}

/// The unique identifier associated with an active [Watchpoint]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WatchpointId(usize);
impl WatchpointId {
    #[inline]
    const fn index(self) -> usize {
        self.0
    }
}

/// A [Watchpoint] specifies a region of memory that will trigger
/// a breakpoint in the emulator when it is written to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Watchpoint {
    pub addr: u32,
    pub size: u32,
    mode: WatchMode,
}
impl Watchpoint {
    pub const fn new(addr: u32, size: u32, mode: WatchMode) -> Self {
        Self { addr, size, mode }
    }

    pub fn mode(&self) -> WatchMode {
        self.mode
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WatchMode {
    /// Treat the watchpoint like a breakpoint
    Break,
    /// Raise a warning when the watchpoint is hit, but do not break
    Warn,
    /// Publish an event when this watchpoint is hit, but do not break
    Event,
    /// The watchpoint is inactive
    Disabled,
}

/// The [BreakpointManager] is responsible for tracking what break-
/// and watchpoints have been created, activated/deactivated, and for
/// informing the emulator when a breakpoint was hit.
#[derive(Debug, Default)]
pub struct BreakpointManager {
    /// True if we should break every cycle
    break_every_cycle: bool,
    /// True if we should break when returning from the current function
    pub break_on_return: bool,
    /// True if we should break at every loop instruction
    break_loops: bool,
    /// The set of cycle counts at which a breakpoint will be triggered
    break_at_cycles: BTreeSet<usize>,
    /// The set of functions we should break on when called
    break_on_calls: FxHashSet<FunctionIdent>,
    /// The set of address ranges that will trigger a watchpoint
    break_on_writes: Vec<Watchpoint>,
    /// A mapping of blocks to instruction indices which will trigger a breakpoint
    break_on_reached: FxHashMap<BlockId, FxHashSet<usize>>,
}
impl BreakpointManager {
    /// Returns all watchpoints that are currently managed by this [BreakpointManager]
    pub fn watchpoints(&self) -> impl Iterator<Item = Watchpoint> + '_ {
        self.break_on_writes.iter().copied()
    }

    #[allow(unused)]
    pub fn has_watchpoints(&self) -> bool {
        !self.break_on_writes.is_empty()
    }

    pub fn has_break_on_reached(&self) -> bool {
        !self.break_on_reached.is_empty()
    }

    /// Returns all breakpoints that are currently managed by this [BreakpointManager]
    pub fn breakpoints(&self) -> impl Iterator<Item = Breakpoint> {
        BreakpointIter::new(self)
    }

    /// Create a [Watchpoint] that monitors the specified memory region, using `mode`
    /// to determine how writes to that region should be handled by the watchpoint
    pub fn watch(&mut self, addr: u32, size: u32, mode: WatchMode) -> WatchpointId {
        let id = WatchpointId(self.break_on_writes.len());
        self.break_on_writes.push(Watchpoint { addr, size, mode });
        id
    }

    /// Set the watch mode for a [Watchpoint] using the identifier returned by [watch]
    pub fn watch_mode(&mut self, id: WatchpointId, mode: WatchMode) {
        self.break_on_writes[id.index()].mode = mode;
    }

    /// Disables a [Watchpoint] using the identifier returned by [watch]
    pub fn unwatch(&mut self, id: WatchpointId) {
        self.break_on_writes[id.index()].mode = WatchMode::Disabled;
    }

    /// Clears all watchpoints
    pub fn unwatch_all(&mut self) {
        self.break_on_writes.clear();
    }

    /// Set the given breakpoint
    pub fn set(&mut self, bp: Breakpoint) {
        use hashbrown::hash_map::Entry;

        match bp {
            Breakpoint::All => {
                self.break_every_cycle = true;
            }
            Breakpoint::Cycle(cycle) => {
                self.break_at_cycles.insert(cycle);
            }
            Breakpoint::At(ip) => match self.break_on_reached.entry(ip.block) {
                Entry::Vacant(entry) => {
                    entry.insert(FxHashSet::from_iter([ip.index]));
                }
                Entry::Occupied(mut entry) => {
                    entry.get_mut().insert(ip.index);
                }
            },
            Breakpoint::Loops => {
                self.break_loops = true;
            }
            Breakpoint::Called(id) => {
                self.break_on_calls.insert(id);
            }
            Breakpoint::Watch(id) => {
                self.break_on_writes[id.index()].mode = WatchMode::Break;
            }
        }
    }

    /// Unset/disable the given breakpoint
    pub fn unset(&mut self, bp: Breakpoint) {
        match bp {
            Breakpoint::All => {
                self.break_every_cycle = false;
            }
            Breakpoint::Cycle(cycle) => {
                self.break_at_cycles.remove(&cycle);
            }
            Breakpoint::At(ip) => {
                if let Some(indices) = self.break_on_reached.get_mut(&ip.block) {
                    indices.remove(&ip.index);
                }
            }
            Breakpoint::Loops => {
                self.break_loops = false;
            }
            Breakpoint::Called(id) => {
                self.break_on_calls.remove(&id);
            }
            Breakpoint::Watch(id) => {
                self.unwatch(id);
            }
        }
    }

    /// Clear all breakpoints, but leaves watchpoints in place
    pub fn unset_all(&mut self) {
        self.break_every_cycle = false;
        self.break_at_cycles.clear();
        self.break_loops = false;
        self.break_on_calls.clear();
        self.break_on_reached.clear();
    }

    /// Clear all breakpoints and watchpoints
    pub fn clear(&mut self) {
        self.unset_all();
        self.unwatch_all();
    }

    /// Force the emulator to break the next time we return from a function
    pub fn break_on_return(&mut self, value: bool) {
        self.break_on_return = value;
    }

    /// Respond to emulator events, and return true if at least one breakpoint was hit
    pub fn handle_event(
        &mut self,
        event: EmulatorEvent,
        ip: Option<Instruction>,
    ) -> Option<BreakpointEvent> {
        use core::cmp::Ordering;

        match event {
            EmulatorEvent::EnterFunction(id) => {
                if self.break_on_calls.contains(&id) {
                    Some(BreakpointEvent::Called(id))
                } else {
                    None
                }
            }
            EmulatorEvent::EnterLoop(block) if self.break_loops => {
                Some(BreakpointEvent::Loop(block))
            }
            EmulatorEvent::EnterLoop(block) => {
                if self.should_break_at(block, 0) {
                    Some(BreakpointEvent::Reached(InstructionPointer::new(block)))
                } else {
                    None
                }
            }
            EmulatorEvent::CycleStart(cycle) => {
                let mut cycle_hit = false;
                self.break_at_cycles.retain(|break_at_cycle| match cycle.cmp(break_at_cycle) {
                    Ordering::Equal => {
                        cycle_hit = true;
                        false
                    }
                    Ordering::Greater => false,
                    Ordering::Less => true,
                });
                if cycle_hit {
                    Some(BreakpointEvent::ReachedCycle(cycle))
                } else if self.break_every_cycle {
                    Some(BreakpointEvent::Step)
                } else {
                    None
                }
            }
            EmulatorEvent::ExitFunction(_) if self.break_on_return => {
                Some(BreakpointEvent::StepOut)
            }
            EmulatorEvent::ExitFunction(_)
            | EmulatorEvent::ExitLoop(_)
            | EmulatorEvent::Jump(_) => match ip {
                Some(Instruction { ip, .. }) => {
                    let break_at_current_ip = self.should_break_at(ip.block, ip.index);
                    if break_at_current_ip {
                        Some(BreakpointEvent::Reached(ip))
                    } else if self.break_every_cycle {
                        Some(BreakpointEvent::Step)
                    } else {
                        None
                    }
                }
                None => {
                    if self.break_every_cycle {
                        Some(BreakpointEvent::Step)
                    } else {
                        None
                    }
                }
            },
            EmulatorEvent::MemoryWrite { addr, size } => {
                self.matches_watchpoint(addr, size).copied().map(BreakpointEvent::Watch)
            }
            EmulatorEvent::Stopped | EmulatorEvent::Suspended => None,
            EmulatorEvent::Breakpoint(bp) => Some(bp),
        }
    }

    pub fn should_break_at(&self, block: BlockId, index: usize) -> bool {
        self.break_on_reached
            .get(&block)
            .map(|indices| indices.contains(&index))
            .unwrap_or(false)
    }

    #[inline]
    #[allow(unused)]
    pub fn should_break_on_write(&self, addr: Addr, size: u32) -> bool {
        self.matches_watchpoint(addr, size).is_some()
    }

    fn matches_watchpoint(&self, addr: Addr, size: u32) -> Option<&Watchpoint> {
        let end_addr = addr + size;
        self.break_on_writes.iter().find(|wp| {
            let wp_end = wp.addr + wp.size;
            if let WatchMode::Break = wp.mode {
                addr <= wp_end && end_addr >= wp.addr
            } else {
                false
            }
        })
    }
}

struct BreakpointIter {
    bps: Vec<Breakpoint>,
}
impl BreakpointIter {
    fn new(bpm: &BreakpointManager) -> Self {
        let mut iter = BreakpointIter {
            bps: Vec::with_capacity(4),
        };
        iter.bps.extend(bpm.break_on_writes.iter().enumerate().filter_map(|(i, wp)| {
            if wp.mode == WatchMode::Break {
                Some(Breakpoint::Watch(WatchpointId(i)))
            } else {
                None
            }
        }));
        iter.bps.extend(bpm.break_at_cycles.iter().copied().map(Breakpoint::Cycle));
        for (block, indices) in bpm.break_on_reached.iter() {
            if indices.is_empty() {
                continue;
            }
            let block = *block;
            for index in indices.iter().copied() {
                iter.bps.push(Breakpoint::At(InstructionPointer { block, index }))
            }
        }
        if bpm.break_loops {
            iter.bps.push(Breakpoint::Loops);
        }
        if bpm.break_every_cycle {
            iter.bps.push(Breakpoint::All);
        }
        iter
    }
}
impl Iterator for BreakpointIter {
    type Item = Breakpoint;

    fn next(&mut self) -> Option<Self::Item> {
        self.bps.pop()
    }
}
impl core::iter::FusedIterator for BreakpointIter {}
