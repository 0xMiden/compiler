//! This module contains the set of compiler-emitted event codes, and their explanations
use miden_core::events::{EventId, EventName};

/// This event indicates that a line should be printed.
///
/// The bytes representing the string are expected in memory. The executor reads the start address
/// and length from the operand stack.
pub const PRINT_LN_EVENT: EventName = EventName::new("readonly::miden_debug::println");

/// A typed wrapper around the raw events known to the compiler
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum Event {
    PrintLn,
    Unknown(EventId),
}
impl Event {
    pub fn as_event_id(self) -> EventId {
        match self {
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

impl From<Event> for miden_assembly_syntax::ast::EventImmediate {
    fn from(event: Event) -> Self {
        miden_assembly_syntax::ast::ImmFelt::from(event.as_event_id().as_felt()).into()
    }
}
