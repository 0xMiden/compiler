use std::fmt;

use midenc_hir::{Felt, FunctionIdent, OperandStack};

use super::{Addr, InstructionPointer, InstructionWithOp};

/// Represents basic information about a frame on the call stack
pub struct CallFrame {
    pub function: FunctionIdent,
    pub fp: Addr,
    pub ip: Option<InstructionPointer>,
}

/// Represents the current state of the program being executed for use in debugging/troubleshooting
pub struct DebugInfo<'a> {
    /// The current cycle count
    pub cycle: usize,
    /// The current function being executed
    pub function: FunctionIdent,
    /// The address at which locals for the current function begin
    pub fp: Addr,
    /// The current instruction pointer metadata, if one is pending
    pub ip: Option<InstructionWithOp>,
    /// The current state of the operand stack
    pub stack: &'a OperandStack<Felt>,
}
impl DebugInfo<'_> {
    pub fn to_owned(self) -> DebugInfoWithStack {
        let stack = self.stack.clone();
        DebugInfoWithStack {
            cycle: self.cycle,
            function: self.function,
            fp: self.fp,
            ip: self.ip,
            stack,
        }
    }
}
impl fmt::Debug for DebugInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use midenc_hir::Stack;

        f.debug_struct("DebugInfo")
            .field("cycle", &self.cycle)
            .field("function", &self.function)
            .field("fp", &self.fp)
            .field("ip", &self.ip)
            .field("stack", &self.stack.debug())
            .finish()
    }
}

/// Same as [DebugInfo], but takes a clone of the operand stack, rather than a reference
pub struct DebugInfoWithStack {
    /// The current cycle count
    pub cycle: usize,
    /// The current function being executed
    pub function: FunctionIdent,
    /// The address at which locals for the current function begin
    pub fp: Addr,
    /// The current instruction pointer metadata, if one is pending
    pub ip: Option<InstructionWithOp>,
    /// The current state of the operand stack
    pub stack: OperandStack<Felt>,
}
impl fmt::Debug for DebugInfoWithStack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use midenc_hir::Stack;

        f.debug_struct("DebugInfo")
            .field("cycle", &self.cycle)
            .field("function", &self.function)
            .field("fp", &self.fp)
            .field("ip", &self.ip)
            .field("stack", &self.stack.debug())
            .finish()
    }
}
