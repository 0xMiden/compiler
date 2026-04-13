//! This module contains the set of compiler-emitted event codes, and their explanations
use core::num::NonZeroU32;

/// This event is emitted via `trace`, and indicates that a procedure call frame is entered
///
/// The mnemonic here is F = frame, 0 = open
pub const TRACE_FRAME_START: u32 = 0xf0;

/// This event is emitted via `trace`, and indicates that a procedure call frame is exited
///
/// The mnemonic here is F = frame, C = close
pub const TRACE_FRAME_END: u32 = 0xfc;

/// This event is emitted via `trace`, and indicates that a line should be printed.
///
/// The bytes representing the string are expected in memory. The executor reads the start address
/// and length from the operand stack.
///
/// The mnemonic here is ASCII `PLN`.
pub const TRACE_PRINT_LN: u32 = 0x50_4c_4e;

/// A typed wrapper around the raw trace events known to the compiler
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum TraceEvent {
    FrameStart,
    FrameEnd,
    PrintLn,
    AssertionFailed(Option<NonZeroU32>),
    Unknown(u32),
}
impl TraceEvent {
    #[inline(always)]
    pub fn is_frame_start(&self) -> bool {
        matches!(self, Self::FrameStart)
    }

    #[inline(always)]
    pub fn is_frame_end(&self) -> bool {
        matches!(self, Self::FrameEnd)
    }

    pub fn as_u32(self) -> u32 {
        match self {
            Self::FrameStart => TRACE_FRAME_START,
            Self::FrameEnd => TRACE_FRAME_END,
            Self::PrintLn => TRACE_PRINT_LN,
            Self::AssertionFailed(None) => 0,
            Self::AssertionFailed(Some(code)) => code.get(),
            Self::Unknown(event) => event,
        }
    }
}
impl From<u32> for TraceEvent {
    fn from(raw: u32) -> Self {
        match raw {
            TRACE_FRAME_START => Self::FrameStart,
            TRACE_FRAME_END => Self::FrameEnd,
            TRACE_PRINT_LN => Self::PrintLn,
            _ => Self::Unknown(raw),
        }
    }
}
impl From<TraceEvent> for u32 {
    fn from(event: TraceEvent) -> Self {
        match event {
            TraceEvent::FrameStart => TRACE_FRAME_START,
            TraceEvent::FrameEnd => TRACE_FRAME_END,
            TraceEvent::PrintLn => TRACE_PRINT_LN,
            TraceEvent::AssertionFailed(None) => 0,
            TraceEvent::AssertionFailed(Some(code)) => code.get(),
            TraceEvent::Unknown(code) => code,
        }
    }
}
