//! Raw-lane procedure and invocation contracts, including context-window normalization.
use alloc::vec::Vec;

use super::{DiagnosticCategory, PlanError, Primitive};
use crate::masm::Instruction;

/// Physical lane count of the VM context boundary, independent of semantic value widths.
pub(crate) const STACK_WINDOW: u8 = miden_core::program::MIN_STACK_DEPTH as u8;

/// Procedure boundary that determines argument exposure, return shape, and suffix ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallingConvention {
    /// Execute in the caller's context while preserving its opaque stack suffix.
    SameContext,
    /// Enter a fresh memory context through a fixed stack window.
    Context,
    /// Enter the root memory context through a system call.
    Kernel,
    /// Whole-program entry and exit, modeled with the complete VM window.
    Root,
}

/// Concrete invocation family; compatibility is checked against the target convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Invocation {
    Exec,
    DynExec,
    Call,
    DynCall,
    SysCall,
}

/// Memory context visible to a callee, used when checking source and compiler-storage effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallMemory {
    SameContext,
    Fresh,
    Root,
}

/// Signature in raw lane counts. Construction checks context capacity; invocation derives
/// the physical call effect for a compatible instruction family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcedureAbi {
    convention: CallingConvention,
    arguments: u32,
    results: u32,
}

impl ProcedureAbi {
    /// Reject context and kernel signatures that exceed the VM window. Same-context and
    /// root shape obligations are checked at their respective verification boundaries.
    pub fn new(
        convention: CallingConvention,
        arguments: u32,
        results: u32,
    ) -> Result<Self, PlanError> {
        if matches!(convention, CallingConvention::Context | CallingConvention::Kernel)
            && (arguments > u32::from(STACK_WINDOW) || results > u32::from(STACK_WINDOW))
        {
            return Err(abi_error("context signatures exceed the sixteen-lane window"));
        }
        Ok(Self {
            convention,
            arguments,
            results,
        })
    }

    /// Boundary rules this signature uses.
    pub fn convention(&self) -> CallingConvention {
        self.convention
    }

    /// Semantic argument lanes, excluding padding and any indirect selector.
    pub fn arguments(&self) -> u32 {
        self.arguments
    }

    /// Semantic result lanes, excluding unspecified context-window contents.
    pub fn results(&self) -> u32 {
        self.results
    }

    /// Derive call exposure, padding, and memory rules, or reject an incompatible family.
    /// Root procedures have no supported invocation family.
    pub fn invocation(&self, invocation: Invocation) -> Result<CallEffect, PlanError> {
        let memory = match (invocation, self.convention) {
            (Invocation::Exec | Invocation::DynExec, CallingConvention::SameContext) => {
                CallMemory::SameContext
            }
            (Invocation::Call | Invocation::DynCall, CallingConvention::Context) => {
                CallMemory::Fresh
            }
            (Invocation::SysCall, CallingConvention::Kernel) => CallMemory::Root,
            _ => return Err(abi_error("invocation family is incompatible with target convention")),
        };
        let context = memory != CallMemory::SameContext;
        Ok(CallEffect {
            invocation,
            arguments: self.arguments,
            results: self.results,
            window: context.then_some(STACK_WINDOW),
            zero_padding: if context {
                STACK_WINDOW - self.arguments as u8
            } else {
                0
            },
            clobbered: if context {
                STACK_WINDOW - self.results as u8
            } else {
                0
            },
            preserves_suffix: !context,
            preserves_overflow: true,
            memory,
            selector_lanes: u8::from(matches!(
                invocation,
                Invocation::DynExec | Invocation::DynCall
            )),
        })
    }
}

/// Physical obligations at one call boundary, derived from a validated target ABI.
/// Counts exclude the indirect selector, which is consumed before argument exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallEffect {
    pub invocation: Invocation,
    pub arguments: u32,
    pub results: u32,
    /// Fixed physical argument/result window for context-changing calls.
    pub window: Option<u8>,
    /// Argument-window lanes that must be zero beyond the declared arguments.
    pub zero_padding: u8,
    /// Result-window lanes beyond the declared results whose identities are unspecified.
    pub clobbered: u8,
    /// Whether lanes below the semantic arguments retain their ordinary identities.
    pub preserves_suffix: bool,
    /// Whether overflow beneath a context window survives the invocation.
    pub preserves_overflow: bool,
    pub memory: CallMemory,
    /// Indirect address is consumed before exposing the argument window.
    pub selector_lanes: u8,
}

/// Enclosing procedure boundary used to authorize return-stack normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnBoundary {
    SameContext,
    Context,
    Root,
}
impl ReturnBoundary {
    /// Permit truncation only after caller-suffix obligations have ended at a context or root exit.
    pub fn validate_truncation(self) -> Result<(), PlanError> {
        match self {
            Self::Context | Self::Root => Ok(()),
            Self::SameContext => {
                Err(abi_error("return normalization violates the procedure boundary"))
            }
        }
    }
}

/// Remove arbitrary nonresult lanes while preserving the result prefix and caller overflow.
/// Reject result prefixes wider than the context window; returned instructions use only
/// legal indexed stack accesses and do not perform the invocation itself.
pub(crate) fn context_result_cleanup(results: u8) -> Result<Vec<Instruction>, PlanError> {
    if results > STACK_WINDOW {
        return Err(abi_error("result prefix exceeds context window"));
    }
    let mut instructions = Vec::new();
    for _ in results..STACK_WINDOW {
        if results == 1 {
            instructions.push(Primitive::Swap(1).instruction()?);
        } else if results > 1 {
            instructions.push(Primitive::MoveUp(results).instruction()?);
        }
        instructions.push(Instruction::Drop);
    }
    Ok(instructions)
}

fn abi_error(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::AbiMismatch, message)
}
