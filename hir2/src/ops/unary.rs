use crate::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnaryOpcode {
    PtrToInt,
    IntToPtr,
    Cast,
    Bitcast,
    Trunc,
    Zext,
    Sext,
    Test,
    Neg,
    Inv,
    Incr,
    Ilog2,
    Pow2,
    Popcnt,
    Clz,
    Ctz,
    Clo,
    Cto,
    Not,
    Bnot,
    IsOdd,
}

pub struct UnaryOp {
    pub op: Operation,
    pub opcode: UnaryOpcode,
}

pub struct UnaryOpImm {
    pub op: Operation,
    pub opcode: UnaryOpcode,
    pub imm: Immediate,
}
