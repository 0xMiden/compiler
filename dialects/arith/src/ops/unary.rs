use alloc::rc::Rc;

use midenc_hir::{
    attributes::{BoolLikeAttr, IntegerLikeAttr},
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::ImmediateAttr,
    effects::MemoryEffectOpInterface,
    matchers::Matcher,
    traits::*,
    *,
};

use crate::*;

macro_rules! infer_return_ty_for_unary_op {
    ($Op:ty) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                let lhs = self.operand().ty().clone();
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
}

/// Increment
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Incr {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Incr);

/// Negation
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Neg {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Neg);

/// Modular inverse
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Inv {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_unary_op!(Inv);

/// log2(operand)
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        name = "ilog_2",
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Ilog2 {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_unary_op!(Ilog2);

/// pow2(operand)
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        name = "pow_2",
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Pow2 {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Pow2);

/// Logical NOT
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)

    )]
pub struct Not {
    #[operand]
    operand: Bool,
    #[result]
    result: Bool,
}

infer_return_ty_for_unary_op!(Not);

impl Foldable for Not {
    /// Folds a constant operand, and `not (not x)` to `x`.
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        let operand = self.operand().as_value_ref();
        if let Some(inner) = operand.borrow().get_defining_op()
            && let Some(inner) = inner.borrow().downcast_ref::<Not>()
        {
            results.push(OpFoldResult::Value(inner.operand().as_value_ref()));
            return FoldResult::Ok(());
        }
        let operand = matchers::foldable_operand().matches(&self.operand().as_operand_ref());
        self.fold_with(&[operand], results)
    }

    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        let Some(operand) = &operands[0] else {
            return FoldResult::Failed;
        };
        let operand = operand.borrow();
        let attr = operand.as_attr();
        let Some(value) =
            attr.as_trait::<dyn BoolLikeAttr>().map(|attr| attr.as_bool()).or_else(|| {
                attr.as_trait::<dyn IntegerLikeAttr>()
                    .and_then(|attr| attr.as_immediate().as_bool())
            })
        else {
            return FoldResult::Failed;
        };
        let result = self
            .as_operation()
            .context_rc()
            .create_attribute::<ImmediateAttr, _>(Immediate::I1(!value));
        results.push(OpFoldResult::Attribute(result));
        FoldResult::Ok(())
    }
}

impl Canonicalizable for Not {
    fn get_canonicalization_patterns(
        rewrites: &mut patterns::RewritePatternSet,
        context: Rc<Context>,
    ) {
        rewrites.push(crate::canonicalization::SimplifyNotCompare::new(context));
    }
}

/// Bitwise NOT
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameTypeOperands, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Bnot {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Bnot);

/// is_odd(operand)
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct IsOdd {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_unary_op!(IsOdd as Type::I1);

/// Count of non-zero bits (population count)
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Popcnt {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Popcnt as Type::U32);

/// Count Leading Zeros
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Clz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Clz as Type::U32);

/// Count Trailing Zeros
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Ctz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Ctz as Type::U32);

/// Count Leading Ones
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Clo {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Clo as Type::U32);

/// Count Trailing Ones
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
    )]
pub struct Cto {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Cto as Type::U32);
