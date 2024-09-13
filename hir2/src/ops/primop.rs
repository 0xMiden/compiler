use crate::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PrimOpcode {
    MemGrow,
    MemSize,
    MemSet,
    MemCpy,
}

pub struct PrimOp {
    pub op: Operation,
}

pub struct PrimOpImm {
    pub op: Operation,
    pub imm: Immediate,
}

pub struct Unreachable {
    pub op: Operation,
}
