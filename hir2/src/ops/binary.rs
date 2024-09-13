use crate::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BinaryOpcode {
    Add(Overflow),
    Sub(Overflow),
    Mul(Overflow),
    Div,
    Mod,
    DivMod,
    Exp(Overflow),
    And,
    Band,
    Or,
    Bor,
    Xor,
    Bxor,
    Shl,
    Shr,
    Rotl,
    Rotr,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Min,
    Max,
}
impl BinaryOpcode {
    pub fn is_commutative(&self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::Mul
                | Self::Min
                | Self::Max
                | Self::Eq
                | Self::Neq
                | Self::And
                | Self::Band
                | Self::Or
                | Self::Bor
                | Self::Xor
                | Self::Bxor
        )
    }
}

pub struct BinaryOp {
    pub op: Operation,
    pub opcode: BinaryOpcode,
}

pub struct BinaryOpImm {
    pub op: Operation,
    pub opcode: BinaryOpcode,
    pub imm: Immediate,
}
