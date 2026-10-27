use alloc::rc::Rc;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::{BoolAttr, Overflow, OverflowAttr},
    effects::*,
    traits::*,
    *,
};

use crate::ArithDialect;

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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Add {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: OverflowAttr,
}

infer_return_ty_for_binary_op!(Add);

impl InferIntRangeInterface for Add {
    fn infer_result_ranges(
        &self,
        operands: &[IntegerValueRange],
        set_result_range: &mut dyn FnMut(ValueRef, IntegerValueRange),
    ) {
        let [lhs, rhs] = operands else {
            return;
        };
        let Some(confidence) = lhs.join_confidence(rhs) else {
            return;
        };
        let (Some(lhs), Some(rhs)) = (lhs.range(), rhs.range()) else {
            return;
        };
        let range = infer_add_range(self.result().ty(), lhs, rhs, *self.get_overflow(), confidence);
        set_result_range(self.result().as_value_ref(), range);
    }
}

fn infer_add_range(
    ty: &Type,
    lhs: &ConstantIntRange,
    rhs: &ConstantIntRange,
    overflow: Overflow,
    confidence: IntegerRangeConfidence,
) -> IntegerValueRange {
    let result_top = ConstantIntRange::max_for_type(ty);
    // Checked and wrapping operations establish the result domain independently of their
    // operands. Assumptions cannot prove a narrower bound, but must not hide that guarantee.
    if confidence == IntegerRangeConfidence::Assumed
        && matches!(overflow, Overflow::Checked | Overflow::Wrapping)
    {
        return IntegerValueRange::proven(result_top);
    }
    let precise = match overflow {
        // The one-result Add operation rejects overflowing mode during lowering.
        // AddOverflowing is a separate operation with an additional result.
        Overflow::Overflowing => return IntegerValueRange::max_for_type(ty),
        _ if lhs.width() != result_top.width() || rhs.width() != result_top.width() => None,
        // Felt wrapping is modulo the field order, not modulo its 64-bit storage width.
        // Keep exact unsigned sums only when they remain inside the canonical field domain.
        _ if ty == &Type::Felt => lhs.add_unsigned_without_overflow(rhs),
        // Unchecked operations may use field arithmetic. A signed interpretation that wraps
        // the raw sum cannot establish a bound on the value actually stored.
        Overflow::Unchecked => lhs.add_unsigned_without_overflow(rhs),
        Overflow::Checked if ty.is_signed_integer() => lhs.add_signed_without_overflow(rhs),
        Overflow::Checked => lhs.add_unsigned_without_overflow(rhs),
        Overflow::Wrapping => lhs.add_wrapping(rhs),
    };
    precise
        .filter(|range| result_top.union(range) == result_top)
        .map(|range| IntegerValueRange::with_confidence(range, confidence))
        .unwrap_or_else(|| {
            if overflow.is_unchecked() {
                IntegerValueRange::assumed(result_top)
            } else {
                IntegerValueRange::proven(result_top)
            }
        })
}

/// Two's complement sum with overflow bit
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Sub {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: OverflowAttr,
}

infer_return_ty_for_binary_op!(Sub);

/// Two's complement difference (subtraction) with underflow bit
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Mul {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: OverflowAttr,
}

infer_return_ty_for_binary_op!(Mul);

/// Two's complement product with overflow bit
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(
        InferTypeOpInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct Exp {
    #[operand]
    lhs: IntFelt,
    #[operand]
    rhs: IntFelt,
    #[attr]
    #[default]
    exponent_must_be_u32: BoolAttr,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_binary_op!(Exp);

impl OperandRangeRequirementOpInterface for Exp {
    fn operand_range_requirement(&self, operand_index: usize) -> OperandRangeRequirement {
        if operand_index == 1 && *self.get_exponent_must_be_u32() {
            OperandRangeRequirement::Required(ValueRangeConstraint::Type(Type::U32))
        } else {
            default_operand_range_requirement(self.as_operation(), operand_index)
                .map(OperandRangeRequirement::Required)
                .unwrap_or(OperandRangeRequirement::None)
        }
    }
}

/// Unsigned integer division, traps on division by zero
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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

/// Bitwise (logical) shift-right
///
/// Shifts larger than the bitwidth of the value will effectively truncate the value to zero.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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

impl Canonicalizable for Rotl {
    fn get_canonicalization_patterns(
        rewrites: &mut patterns::RewritePatternSet,
        context: Rc<Context>,
    ) {
        let name = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Self>();
        rewrites
            .push(crate::canonicalization::CanonicalizeI64RotateBy32ToSwap::for_op(context, name));
    }
}

/// Bitwise rotate-right
///
/// The rotation count must be < the bitwidth of the value type.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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

impl Canonicalizable for Rotr {
    fn get_canonicalization_patterns(
        rewrites: &mut patterns::RewritePatternSet,
        context: Rc<Context>,
    ) {
        let name = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Self>();
        rewrites
            .push(crate::canonicalization::CanonicalizeI64RotateBy32ToSwap::for_op(context, name));
    }
}

#[derive(Copy, Clone)]
enum IntComparison {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

macro_rules! impl_infer_int_range_for_comparison {
    ($Op:ty, $comparison:expr) => {
        impl InferIntRangeInterface for $Op {
            fn infer_result_ranges(
                &self,
                operands: &[IntegerValueRange],
                set_result_range: &mut dyn FnMut(ValueRef, IntegerValueRange),
            ) {
                let [lhs, rhs] = operands else {
                    return;
                };
                let Some(confidence) = lhs.join_confidence(rhs) else {
                    return;
                };
                let (Some(lhs), Some(rhs)) = (lhs.range(), rhs.range()) else {
                    return;
                };
                let operand_ty = self.lhs().ty();
                let range = infer_comparison_range(
                    &operand_ty,
                    lhs,
                    rhs,
                    $comparison,
                    self.result().ty(),
                    confidence,
                );
                set_result_range(self.result().as_value_ref(), range);
            }
        }
    };
}

/// Equality comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Eq, IntComparison::Eq);

impl Canonicalizable for Eq {
    fn get_canonicalization_patterns(
        rewrites: &mut patterns::RewritePatternSet,
        context: Rc<Context>,
    ) {
        let name = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Self>();
        rewrites.push(crate::canonicalization::SimplifyBoolCompare::for_op(context, name));
    }
}

/// Inequality comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Neq, IntComparison::Neq);

impl Canonicalizable for Neq {
    fn get_canonicalization_patterns(
        rewrites: &mut patterns::RewritePatternSet,
        context: Rc<Context>,
    ) {
        let name = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Self>();
        rewrites.push(crate::canonicalization::SimplifyBoolCompare::for_op(context, name));
    }
}

/// Greater-than comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Gt, IntComparison::Gt);

/// Greater-than-or-equal comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Gte, IntComparison::Gte);

/// Less-than comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Lt, IntComparison::Lt);

/// Less-than-or-equal comparison
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, InferIntRangeInterface, MemoryEffectOpInterface, OpPrinter)
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
impl_infer_int_range_for_comparison!(Lte, IntComparison::Lte);

fn infer_comparison_range(
    operand_ty: &Type,
    lhs: &ConstantIntRange,
    rhs: &ConstantIntRange,
    comparison: IntComparison,
    result_ty: &Type,
    confidence: IntegerRangeConfidence,
) -> IntegerValueRange {
    if confidence != IntegerRangeConfidence::Proven
        || lhs.is_untracked()
        || rhs.is_untracked()
        || lhs.width() != rhs.width()
    {
        return IntegerValueRange::proven(ConstantIntRange::max_for_type(result_ty));
    }

    if lhs.width() != ConstantIntRange::max_for_type(operand_ty).width() {
        // Unrealized casts preserve the source's bounds. Those signed bounds
        // cannot be used to order values interpreted at a different width.
        return IntegerValueRange::proven(ConstantIntRange::max_for_type(result_ty));
    }

    let outcome = match comparison {
        IntComparison::Eq => infer_eq(lhs, rhs),
        IntComparison::Neq => infer_eq(lhs, rhs).map(|is_equal| !is_equal),
        comparison if operand_ty.is_signed_integer() => {
            infer_ordering((lhs.smin(), lhs.smax()), (rhs.smin(), rhs.smax()), comparison)
        }
        comparison => {
            infer_ordering((lhs.umin(), lhs.umax()), (rhs.umin(), rhs.umax()), comparison)
        }
    };

    outcome
        .map(|value| IntegerValueRange::exact_bool(result_ty, value))
        .unwrap_or_else(|| IntegerValueRange::proven(ConstantIntRange::max_for_type(result_ty)))
}

fn infer_eq(lhs: &ConstantIntRange, rhs: &ConstantIntRange) -> Option<bool> {
    if lhs.umin() == lhs.umax() && rhs.umin() == rhs.umax() && lhs.umin() == rhs.umin() {
        Some(true)
    } else if lhs.umax() < rhs.umin() || rhs.umax() < lhs.umin() {
        Some(false)
    } else {
        None
    }
}

fn infer_ordering<T: Ord>(lhs: (T, T), rhs: (T, T), comparison: IntComparison) -> Option<bool> {
    let ((lmin, lmax), (rmin, rmax), inclusive) = match comparison {
        IntComparison::Lt => (lhs, rhs, false),
        IntComparison::Lte => (lhs, rhs, true),
        IntComparison::Gt => (rhs, lhs, false),
        IntComparison::Gte => (rhs, lhs, true),
        IntComparison::Eq | IntComparison::Neq => unreachable!("handled separately"),
    };
    if inclusive {
        if lmax <= rmin {
            Some(true)
        } else if lmin > rmax {
            Some(false)
        } else {
            None
        }
    } else if lmax < rmin {
        Some(true)
    } else if lmin >= rmax {
        Some(false)
    } else {
        None
    }
}

/// Select minimum value
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
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

#[cfg(test)]
mod integer_range_tests {
    use super::*;

    #[test]
    fn checked_and_wrapping_add_prove_the_domain_without_proving_assumptions() {
        for lhs in [
            ConstantIntRange::max_for_type(&Type::U32),
            ConstantIntRange::from_unsigned(32, 0, 10),
        ] {
            for rhs in [0, 1] {
                for overflow in [Overflow::Checked, Overflow::Wrapping] {
                    let result = infer_add_range(
                        &Type::U32,
                        &lhs,
                        &ConstantIntRange::constant(Immediate::U32(rhs)),
                        overflow,
                        IntegerRangeConfidence::Assumed,
                    );
                    assert_eq!(
                        result,
                        IntegerValueRange::proven(ConstantIntRange::max_for_type(&Type::U32))
                    );
                }
            }
        }
    }

    #[test]
    fn addition_distinguishes_checked_wrapping_and_field_arithmetic() {
        let cases = [
            (Type::U32, Immediate::U32(0x7fff_ffff), Immediate::U32(1), 0x8000_0000),
            (Type::I32, Immediate::I32(-1), Immediate::I32(1), 0),
        ];
        for (ty, lhs, rhs, expected) in cases {
            for overflow in [Overflow::Checked, Overflow::Wrapping] {
                let result = infer_add_range(
                    &ty,
                    &ConstantIntRange::constant(lhs),
                    &ConstantIntRange::constant(rhs),
                    overflow,
                    IntegerRangeConfidence::Proven,
                );
                assert_eq!(
                    result.range().map(|r| (r.umin(), r.umax())),
                    Some((expected, expected))
                );
                assert!(result.is_proven());
            }
        }
        for (ty, lhs) in [(Type::U32, Immediate::U32(u32::MAX)), (Type::I32, Immediate::I32(-1))] {
            let result = infer_add_range(
                &ty,
                &ConstantIntRange::constant(lhs),
                &ConstantIntRange::constant(Immediate::U32(1)),
                Overflow::Unchecked,
                IntegerRangeConfidence::Proven,
            );
            assert!(!result.is_proven(), "unchecked raw overflow must not acquire a wrapped proof");
        }
        let boolean_overflow = infer_add_range(
            &Type::I1,
            &ConstantIntRange::constant(Immediate::I1(true)),
            &ConstantIntRange::constant(Immediate::I1(true)),
            Overflow::Unchecked,
            IntegerRangeConfidence::Proven,
        );
        assert!(!boolean_overflow.is_proven());
    }

    #[test]
    fn overflowing_mode_is_unsupported_for_one_result_addition() {
        for (ty, lhs, rhs) in [
            (Type::U32, Immediate::U32(1), Immediate::U32(2)),
            (Type::I32, Immediate::I32(-1), Immediate::I32(1)),
            (
                Type::Felt,
                Immediate::Felt(Felt::new(1).unwrap()),
                Immediate::Felt(Felt::new(2).unwrap()),
            ),
        ] {
            let result = infer_add_range(
                &ty,
                &ConstantIntRange::constant(lhs),
                &ConstantIntRange::constant(rhs),
                Overflow::Overflowing,
                IntegerRangeConfidence::Proven,
            );
            assert_eq!(result, IntegerValueRange::max_for_type(&ty));
        }
    }

    #[test]
    fn felt_addition_uses_the_field_modulus_in_supported_modes() {
        let lhs = ConstantIntRange::constant(Immediate::Felt(Felt::new(Felt::ORDER - 1).unwrap()));
        let rhs = ConstantIntRange::constant(Immediate::Felt(Felt::new(1u64 << 32).unwrap()));
        let field_top = ConstantIntRange::max_for_type(&Type::Felt);
        // The raw sum is 2^64, but field addition yields 2^32 - 1 rather than zero.
        for overflow in [Overflow::Wrapping, Overflow::Checked, Overflow::Unchecked] {
            let result =
                infer_add_range(&Type::Felt, &lhs, &rhs, overflow, IntegerRangeConfidence::Proven);
            assert_eq!(result.range(), Some(&field_top));
            assert_eq!(result.is_proven(), !overflow.is_unchecked());
        }
        let one = ConstantIntRange::constant(Immediate::Felt(Felt::new(1).unwrap()));
        let two = ConstantIntRange::constant(Immediate::Felt(Felt::new(2).unwrap()));
        for overflow in [Overflow::Wrapping, Overflow::Checked, Overflow::Unchecked] {
            let result =
                infer_add_range(&Type::Felt, &one, &two, overflow, IntegerRangeConfidence::Proven);
            assert_eq!(
                result,
                IntegerValueRange::proven(ConstantIntRange::constant(Immediate::Felt(
                    Felt::new(3).unwrap()
                )))
            );
        }
    }

    #[test]
    fn comparison_always_proves_boolean_domain_without_proving_assumptions() {
        let top = ConstantIntRange::max_for_type(&Type::U32);
        let zero = ConstantIntRange::constant(Immediate::U32(0));
        for (lhs, rhs, confidence) in [
            (top, zero, IntegerRangeConfidence::Assumed),
            (ConstantIntRange::untracked(), zero, IntegerRangeConfidence::Proven),
            (ConstantIntRange::max_for_type(&Type::U64), zero, IntegerRangeConfidence::Proven),
        ] {
            let result = infer_comparison_range(
                &Type::U32,
                &lhs,
                &rhs,
                IntComparison::Gte,
                &Type::I1,
                confidence,
            );
            assert_eq!(
                result,
                IntegerValueRange::proven(ConstantIntRange::max_for_type(&Type::I1))
            );
        }
        let result = infer_comparison_range(
            &Type::U32,
            &top,
            &zero,
            IntComparison::Gte,
            &Type::I1,
            IntegerRangeConfidence::Proven,
        );
        assert_eq!(result, IntegerValueRange::exact_bool(&Type::I1, true));
    }

    #[test]
    fn signed_comparison_retains_the_interpretation_width_guard() {
        let raw = ConstantIntRange::constant(Immediate::U64(u32::MAX as u64));
        let zero = ConstantIntRange::constant(Immediate::U64(0));
        let result = infer_comparison_range(
            &Type::I32,
            &raw,
            &zero,
            IntComparison::Lt,
            &Type::I1,
            IntegerRangeConfidence::Proven,
        );
        assert_eq!(result, IntegerValueRange::proven(ConstantIntRange::max_for_type(&Type::I1)));
    }

    #[test]
    fn ordering_decisions_match_every_value_in_small_intervals() {
        for lmin in -3..=3 {
            for lmax in lmin..=3 {
                for rmin in -3..=3 {
                    for rmax in rmin..=3 {
                        for comparison in [
                            IntComparison::Lt,
                            IntComparison::Lte,
                            IntComparison::Gt,
                            IntComparison::Gte,
                        ] {
                            if let Some(outcome) =
                                infer_ordering((lmin, lmax), (rmin, rmax), comparison)
                            {
                                for lhs in lmin..=lmax {
                                    for rhs in rmin..=rmax {
                                        let actual = match comparison {
                                            IntComparison::Lt => lhs < rhs,
                                            IntComparison::Lte => lhs <= rhs,
                                            IntComparison::Gt => lhs > rhs,
                                            IntComparison::Gte => lhs >= rhs,
                                            _ => unreachable!(),
                                        };
                                        assert_eq!(outcome, actual);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
