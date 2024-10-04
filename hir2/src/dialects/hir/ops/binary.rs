use crate::{derive::operation, dialects::hir::HirDialect, traits::*, *};

/// Two's complement sum
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Add {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Add {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement sum with overflow bit
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct AddOverflowing {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    overflowed: Bool,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for AddOverflowing {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement difference (subtraction)
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Sub {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Sub {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement difference (subtraction) with underflow bit
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct SubOverflowing {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    overflowed: Bool,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for SubOverflowing {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement product
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Mul {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Mul {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement product with overflow bit
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands),
        implements(InferTypeOpInterface)
    )]
pub struct MulOverflowing {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    overflowed: Bool,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for MulOverflowing {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.lhs().ty().clone();
        {
            let rhs = self.rhs();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Exponentiation for field elements
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Exp {
    #[operand]
    lhs: IntFelt,
    #[operand]
    rhs: IntFelt,
    #[result]
    result: IntFelt,
}

/// Unsigned integer division, traps on division by zero
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Div {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Signed integer division, traps on division by zero or dividing the minimum signed value by -1
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Sdiv {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Unsigned integer Euclidean modulo, traps on division by zero
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Mod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Signed integer Euclidean modulo, traps on division by zero
///
/// The result has the same sign as the dividend (lhs)
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Smod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Combined unsigned integer Euclidean division and remainder (modulo).
///
/// Traps on division by zero.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Divmod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    remainder: AnyInteger,
    #[result]
    quotient: AnyInteger,
}

/// Combined signed integer Euclidean division and remainder (modulo).
///
/// Traps on division by zero.
///
/// The remainder has the same sign as the dividend (lhs)
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Sdivmod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    remainder: AnyInteger,
    #[result]
    quotient: AnyInteger,
}

/// Logical AND
///
/// Operands must be boolean.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct And {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

/// Logical OR
///
/// Operands must be boolean.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Or {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

/// Logical XOR
///
/// Operands must be boolean.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Xor {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

/// Bitwise AND
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Band {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Bitwise OR
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Bor {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Bitwise XOR
///
/// Operands must be boolean.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Bxor {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Bitwise shift-left
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp),
    )]
pub struct Shl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

/// Bitwise (logical) shift-right
///
/// Shifts larger than the bitwidth of the value will effectively truncate the value to zero.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp),
    )]
pub struct Shr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

/// Arithmetic (signed) shift-right
///
/// The result of shifts larger than the bitwidth of the value depend on the sign of the value;
/// for positive values, it rounds to zero; for negative values, it rounds to MIN.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp),
    )]
pub struct Ashr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

/// Bitwise rotate-left
///
/// The rotation count must be < the bitwidth of the value type.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp),
    )]
pub struct Rotl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

/// Bitwise rotate-right
///
/// The rotation count must be < the bitwidth of the value type.
#[operation(
        dialect = HirDialect,
        traits(BinaryOp),
    )]
pub struct Rotr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

/// Equality comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands),
    )]
pub struct Eq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Inequality comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands),
    )]
pub struct Neq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Greater-than comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands),
    )]
pub struct Gt {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Greater-than-or-equal comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands),
    )]
pub struct Gte {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Less-than comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands),
    )]
pub struct Lt {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Less-than-or-equal comparison
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, SameTypeOperands),
    )]
pub struct Lte {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

/// Select minimum value
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Min {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

/// Select maximum value
#[operation(
        dialect = HirDialect,
        traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    )]
pub struct Max {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}
