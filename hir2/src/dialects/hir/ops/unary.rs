use crate::{derive::operation, dialects::hir::HirDialect, traits::*, *};

/// Increment
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Incr {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Negation
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Neg {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Modular inverse
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Inv {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

/// log2(operand)
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Ilog2 {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

/// pow2(operand)
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Pow2 {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Logical NOT
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Not {
    #[operand]
    operand: Bool,
    #[result]
    result: Bool,
}

/// Bitwise NOT
#[operation (
        dialect = HirDialect,
        traits(UnaryOp, SameOperandsAndResultType)
    )]
pub struct Bnot {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// is_odd(operand)
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct IsOdd {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: Bool,
}

/// Count of non-zero bits (population count)
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct Popcnt {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

/// Count Leading Zeros
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct Clz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

/// Count Trailing Zeros
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct Ctz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

/// Count Leading Ones
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct Clo {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

/// Count Trailing Ones
#[operation (
        dialect = HirDialect,
        traits(UnaryOp)
    )]
pub struct Cto {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}
