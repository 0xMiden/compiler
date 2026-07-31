//! This module contains the set of compiler-emitted event codes, and their explanations
use miden_core::events::{EventId, EventName};

/// This event indicates that a procedure call frame is entered
pub const FRAME_START_EVENT: EventName = EventName::new("readonly::miden_debug::frame_start");

/// This event indicates that a procedure call frame is exited
pub const FRAME_END_EVENT: EventName = EventName::new("readonly::miden_debug::frame_end");

/// This event indicates that a line should be printed.
///
/// The bytes representing the string are expected in memory. The executor reads the start address
/// and length from the operand stack.
pub const PRINT_LN_EVENT: EventName = EventName::new("readonly::miden_debug::println");

/// A typed wrapper around the raw events known to the compiler
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum Event {
    FrameStart,
    FrameEnd,
    PrintLn,
    Unknown(EventId),
}
impl Event {
    #[inline(always)]
    pub fn is_frame_start(&self) -> bool {
        matches!(self, Self::FrameStart)
    }

    #[inline(always)]
    pub fn is_frame_end(&self) -> bool {
        matches!(self, Self::FrameEnd)
    }

    pub fn as_event_id(self) -> EventId {
        match self {
            Self::FrameStart => FRAME_START_EVENT.to_event_id(),
            Self::FrameEnd => FRAME_END_EVENT.to_event_id(),
            Self::PrintLn => PRINT_LN_EVENT.to_event_id(),
            Self::Unknown(event) => event,
        }
    }
}

impl From<Event> for EventId {
    fn from(event: Event) -> Self {
        event.as_event_id()
    }
}
