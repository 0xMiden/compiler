use alloc::rc::Rc;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::{BoolAttr, OverflowAttr},
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
        let confidence = if lhs.is_proven() && rhs.is_proven() {
            IntegerRangeConfidence::Proven
        } else {
            IntegerRangeConfidence::Assumed
        };
        let (Some(lhs), Some(rhs)) = (lhs.range(), rhs.range()) else {
            return;
        };

        let result_top = ConstantIntRange::max_for_type(self.result().ty());
        let range = lhs
            .add_without_overflow(rhs)
            .filter(|range| result_top.union(range) == result_top)
            .map(|range| IntegerValueRange::with_confidence(range, confidence))
            .unwrap_or_else(|| {
                // Unchecked arithmetic can produce a field value outside the
                // declared integer type; only checked/wrapping operations prove
                // the fallback bound at runtime.
                if self.get_overflow().is_unchecked() {
                    IntegerValueRange::assumed(result_top)
                } else {
                    IntegerValueRange::proven(result_top)
                }
            });
        set_result_range(self.result().as_value_ref(), range);
    }
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
                let confidence = if lhs.is_proven() && rhs.is_proven() {
                    IntegerRangeConfidence::Proven
                } else {
                    IntegerRangeConfidence::Assumed
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
    if lhs.is_untracked() || rhs.is_untracked() || lhs.width() != rhs.width() {
        return IntegerValueRange::max_for_type(result_ty);
    }

    if lhs.width() != ConstantIntRange::max_for_type(operand_ty).width() {
        // Unrealized casts preserve the source's bounds. Those signed bounds
        // cannot be used to order values interpreted at a different width.
        return IntegerValueRange::proven(ConstantIntRange::max_for_type(result_ty));
    }

    let outcome = match comparison {
        IntComparison::Eq => infer_eq(lhs, rhs),
        IntComparison::Neq => infer_eq(lhs, rhs).map(|is_equal| !is_equal),
        comparison if is_signed_ordering(operand_ty) => infer_signed_ordering(lhs, rhs, comparison),
        comparison => infer_unsigned_ordering(lhs, rhs, comparison),
    };

    outcome
        .map(|value| exact_bool_range(result_ty, value, confidence))
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

fn infer_signed_ordering(
    lhs: &ConstantIntRange,
    rhs: &ConstantIntRange,
    comparison: IntComparison,
) -> Option<bool> {
    match comparison {
        IntComparison::Lt => {
            if lhs.smax() < rhs.smin() {
                Some(true)
            } else if lhs.smin() >= rhs.smax() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Lte => {
            if lhs.smax() <= rhs.smin() {
                Some(true)
            } else if lhs.smin() > rhs.smax() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Gt => {
            if lhs.smin() > rhs.smax() {
                Some(true)
            } else if lhs.smax() <= rhs.smin() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Gte => {
            if lhs.smin() >= rhs.smax() {
                Some(true)
            } else if lhs.smax() < rhs.smin() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Eq | IntComparison::Neq => unreachable!("handled separately"),
    }
}

fn infer_unsigned_ordering(
    lhs: &ConstantIntRange,
    rhs: &ConstantIntRange,
    comparison: IntComparison,
) -> Option<bool> {
    match comparison {
        IntComparison::Lt => {
            if lhs.umax() < rhs.umin() {
                Some(true)
            } else if lhs.umin() >= rhs.umax() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Lte => {
            if lhs.umax() <= rhs.umin() {
                Some(true)
            } else if lhs.umin() > rhs.umax() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Gt => {
            if lhs.umin() > rhs.umax() {
                Some(true)
            } else if lhs.umax() <= rhs.umin() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Gte => {
            if lhs.umin() >= rhs.umax() {
                Some(true)
            } else if lhs.umax() < rhs.umin() {
                Some(false)
            } else {
                None
            }
        }
        IntComparison::Eq | IntComparison::Neq => unreachable!("handled separately"),
    }
}

fn is_signed_ordering(ty: &Type) -> bool {
    matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
}

fn exact_bool_range(
    ty: &Type,
    value: bool,
    confidence: IntegerRangeConfidence,
) -> IntegerValueRange {
    let width = ConstantIntRange::max_for_type(ty).width();
    if width == 0 {
        IntegerValueRange::max_for_type(ty)
    } else {
        let value = u128::from(value);
        IntegerValueRange::with_confidence(
            ConstantIntRange::from_unsigned(width, value, value),
            confidence,
        )
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
