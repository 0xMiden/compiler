use crate::{derive::operation, dialects::hir::HirDialect, traits::*, *};

// Implement `derive(InferTypeOpInterface)` with `#[infer]` helper attribute:
//
// * `#[infer]` on a result field indicates its type should be inferred from the type of the first
//   operand field
// * `#[infer(from = field)]` on a result field indicates its type should be inferred from
//   the given field. The field is expected to implement `AsRef<Type>`
// * `#[infer(type = I1)]` on a field indicates that the field should always be inferred to have the given type
// * `#[infer(with = path::to::function)]` on a field indicates that the given function should be called to
//   compute the inferred type for that field
macro_rules! infer_return_ty_for_binary_op {
    ($Op:ty) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                let lhs = self.lhs().ty().clone();
                self.result_mut().set_type(lhs);
                Ok(())
            }
        }
    };


    ($Op:ty as $manually_specified_ty:expr) => {
        paste::paste! {
            impl InferTypeOpInterface for $Op {
                fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                    self.result_mut().set_type($manually_specified_ty);
                    Ok(())
                }
            }
        }
    };

    ($Op:ty, $($manually_specified_field_name:ident : $manually_specified_field_ty:expr),+) => {
        paste::paste! {
            impl InferTypeOpInterface for $Op {
                fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                    let lhs = self.lhs().ty().clone();
                    self.result_mut().set_type(lhs);
                    $(
                        self.[<$manually_specified_field_name _mut>]().set_type($manually_specified_field_ty);
                    )*
                    Ok(())
                }
            }
        }
    };
}

/// Two's complement sum
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
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

infer_return_ty_for_binary_op!(Add);

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

infer_return_ty_for_binary_op!(AddOverflowing, overflowed: Type::I1);

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

infer_return_ty_for_binary_op!(Sub);

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

infer_return_ty_for_binary_op!(SubOverflowing, overflowed: Type::I1);

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

infer_return_ty_for_binary_op!(Mul);

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

infer_return_ty_for_binary_op!(MulOverflowing, overflowed: Type::I1);

/// Exponentiation for field elements
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Exp {
    #[operand]
    lhs: IntFelt,
    #[operand]
    rhs: IntFelt,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_binary_op!(Exp);

/// Unsigned integer division, traps on division by zero
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Div {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Div);

/// Signed integer division, traps on division by zero or dividing the minimum signed value by -1
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Sdiv {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Sdiv);

/// Unsigned integer Euclidean modulo, traps on division by zero
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Mod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Mod);

/// Signed integer Euclidean modulo, traps on division by zero
///
/// The result has the same sign as the dividend (lhs)
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Smod {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Smod);

/// Combined unsigned integer Euclidean division and remainder (modulo).
///
/// Traps on division by zero.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
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

impl InferTypeOpInterface for Divmod {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.remainder_mut().set_type(lhs.clone());
        self.quotient_mut().set_type(lhs);
        Ok(())
    }
}

/// Combined signed integer Euclidean division and remainder (modulo).
///
/// Traps on division by zero.
///
/// The remainder has the same sign as the dividend (lhs)
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
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

impl InferTypeOpInterface for Sdivmod {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.remainder_mut().set_type(lhs.clone());
        self.quotient_mut().set_type(lhs);
        Ok(())
    }
}

/// Logical AND
///
/// Operands must be boolean.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct And {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(And);

/// Logical OR
///
/// Operands must be boolean.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Or {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Or);

/// Logical XOR
///
/// Operands must be boolean.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Xor {
    #[operand]
    lhs: Bool,
    #[operand]
    rhs: Bool,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Xor);

/// Bitwise AND
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Band {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Band);

/// Bitwise OR
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Bor {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Bor);

/// Bitwise XOR
///
/// Operands must be boolean.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Bxor {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Bxor);

/// Bitwise shift-left
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Shl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Shl);

/// Bitwise shift-left by immediate
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface)
)]
pub struct ShlImm {
    #[operand]
    lhs: AnyInteger,
    #[attr]
    shift: u32,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for ShlImm {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.lhs().ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

/// Bitwise (logical) shift-right
///
/// Shifts larger than the bitwidth of the value will effectively truncate the value to zero.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Shr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Shr);

/// Arithmetic (signed) shift-right
///
/// The result of shifts larger than the bitwidth of the value depend on the sign of the value;
/// for positive values, it rounds to zero; for negative values, it rounds to MIN.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Ashr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Ashr);

/// Bitwise rotate-left
///
/// The rotation count must be < the bitwidth of the value type.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Rotl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Rotl);

/// Bitwise rotate-right
///
/// The rotation count must be < the bitwidth of the value type.
#[operation(
    dialect = HirDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Rotr {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Rotr);

/// Equality comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Eq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Eq as Type::I1);

/// Inequality comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Neq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Neq as Type::I1);

/// Greater-than comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Gt {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Gt as Type::I1);

/// Greater-than-or-equal comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Gte {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Gte as Type::I1);

/// Less-than comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Lt {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Lt as Type::I1);

/// Less-than-or-equal comparison
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Lte {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Lte as Type::I1);

/// Select minimum value
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Min {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Min);

/// Select maximum value
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Max {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Max);
