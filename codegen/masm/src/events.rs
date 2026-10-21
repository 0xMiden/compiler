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

impl From<Event> for miden_assembly_syntax::ast::EventImmediate {
    fn from(event: Event) -> Self {
        miden_assembly_syntax::ast::ImmFelt::from(event.as_event_id().as_felt()).into()
    }
}

/// The operand window and possible memory read of a typed VM system event.
///
/// Selection uses this descriptor to require explicit SSA uses for every observed payload lane;
/// target replay uses the same closed description to check the actual stack and read footprint.
/// It describes dedicated system-event operations, not generic host event dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SystemEventDescriptor {
    /// Number of retained payload lanes, excluding the transient event ID pushed by `EmitImm`.
    pub payload_lanes: u8,
    /// Source-memory read requiring a footprint check in addition to payload observation.
    pub read: Option<SystemEventRead>,
}

/// Closed memory-read shapes for the system handlers whose payload includes addresses.
///
/// These select target rules with different guard and provenance requirements; they are not
/// interchangeable permissions to read arbitrary memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemEventRead {
    /// A half-open native-cell range whose exclusive endpoint is used only as a bound.
    SourceRange,
    /// A tag-dependent payload consisting of a runtime number of fixed-size chunks.
    DeferredRegisterData,
}

impl SystemEventDescriptor {
    /// Look up a supported typed-system-event contract using its exact VM ID.
    ///
    /// Returns `None` for IDs without a closed typed contract, including trace events. This lookup
    /// neither validates generic event IDs nor rewrites named events into numerical IDs.
    pub fn from_id(id: miden_core::Felt) -> Option<Self> {
        use miden_core::events::SystemEvent::*;
        let event = miden_core::events::SystemEvent::from_event_id(EventId::from_felt(id))?;
        let payload_lanes = match event {
            MapValueToStack
            | MapValueCountToStack
            | MapValueToStackN0
            | MapValueToStackN4
            | MapValueToStackN8
            | HasMapKey
            | DeferredEvaluate
            | DeferredEvaluateTag
            | DeferredEvaluatePayload => 4,
            MerkleNodeToStack => 6,
            HdwordToMap | MerkleNodeMerge => 8,
            HdwordToMapWithDomain => 9,
            HqwordToMap => 16,
            HpermToMap | DeferredRegister => 12,
            Ext2Inv => 2,
            U32Clz | U32Ctz | U32Clo | U32Cto | ILog2 => 1,
            MemToMap | DeferredRegisterData => 6,
            TraceEvent => return None,
        };
        let read = match event {
            MemToMap => Some(SystemEventRead::SourceRange),
            DeferredRegisterData => Some(SystemEventRead::DeferredRegisterData),
            _ => None,
        };
        Some(Self {
            payload_lanes,
            read,
        })
    }
}
