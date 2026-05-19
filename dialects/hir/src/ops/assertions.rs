use alloc::rc::Rc;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::{StringAttr, U32Attr},
    effects::*,
    patterns::RewritePatternSet,
    traits::*,
    *,
};

use crate::HirDialect;

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(
        InferTypeOpInterface,
        InferIntRangeInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        ValueRangeAssertionOpInterface,
        OpPrinter
    )
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct Assert {
    #[operand]
    value: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
    #[result]
    result: AnyInteger,
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(
        InferTypeOpInterface,
        InferIntRangeInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        ValueRangeAssertionOpInterface,
        OpPrinter
    )
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct Assertz {
    #[operand]
    value: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
    #[result]
    result: AnyInteger,
}

impl Canonicalizable for Assertz {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::RemoveRepeatedAssertz::new(context));
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
}

/// Assert that the operand is a valid u32 and refine its type on success.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(
        InferTypeOpInterface,
        InferIntRangeInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        ValueRangeAssertionOpInterface,
        OpPrinter
    )
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct AssertU32 {
    #[operand]
    value: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Assert {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.value().ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl OperandRangeRequirementOpInterface for Assert {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // `assert` checks that the input is exactly one; it establishes the range/value contract
        // rather than requiring one before the assertion.
        OperandRangeRequirement::None
    }
}

impl ValueRangeAssertionOpInterface for Assert {
    fn value_range_assertion(&self, result: ValueRef) -> Option<ValueRangeRefinement> {
        let asserted = self.result().as_value_ref();
        (asserted == result).then(|| ValueRangeRefinement {
            input: self.value().as_value_ref(),
            result: asserted,
            constraint: ValueRangeConstraint::Type(Type::I1),
        })
    }
}

impl InferIntRangeInterface for Assert {
    fn infer_result_ranges(
        &self,
        _operands: &[IntegerValueRange],
        set_result_range: &mut dyn FnMut(ValueRef, IntegerValueRange),
    ) {
        let result = self.result().as_value_ref();
        set_result_range(result, exact_zero_or_one_range(self.result().ty(), true));
    }
}

impl InferTypeOpInterface for Assertz {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.value().ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl OperandRangeRequirementOpInterface for Assertz {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // `assertz` checks that the input is exactly zero; it establishes the range/value contract
        // rather than requiring one before the assertion.
        OperandRangeRequirement::None
    }
}

impl ValueRangeAssertionOpInterface for Assertz {
    fn value_range_assertion(&self, result: ValueRef) -> Option<ValueRangeRefinement> {
        let asserted = self.result().as_value_ref();
        (asserted == result).then(|| ValueRangeRefinement {
            input: self.value().as_value_ref(),
            result: asserted,
            constraint: ValueRangeConstraint::Type(Type::I1),
        })
    }
}

impl InferIntRangeInterface for Assertz {
    fn infer_result_ranges(
        &self,
        _operands: &[IntegerValueRange],
        set_result_range: &mut dyn FnMut(ValueRef, IntegerValueRange),
    ) {
        let result = self.result().as_value_ref();
        set_result_range(result, exact_zero_or_one_range(self.result().ty(), false));
    }
}

impl InferTypeOpInterface for AssertU32 {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

impl InferIntRangeInterface for AssertU32 {
    fn infer_result_ranges(
        &self,
        operands: &[IntegerValueRange],
        set_result_range: &mut dyn FnMut(ValueRef, IntegerValueRange),
    ) {
        let result = self.result().as_value_ref();
        let Some(refinement) = self.value_range_assertion(result) else {
            return;
        };
        let range = operands.first().cloned().unwrap_or_else(IntegerValueRange::uninitialized);
        set_result_range(result, range.constrained_by(&refinement.constraint));
    }
}

fn exact_zero_or_one_range(ty: &Type, one: bool) -> IntegerValueRange {
    let width = ConstantIntRange::max_for_type(ty).width();
    if width == 0 {
        IntegerValueRange::max_for_type(ty)
    } else {
        let value = u128::from(one);
        IntegerValueRange::new(ConstantIntRange::from_unsigned(width, value, value))
    }
}

impl OperandRangeRequirementOpInterface for AssertU32 {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // `u32assert` establishes the range contract for its operand; it must not itself be
        // treated as consuming an already-constrained value.
        OperandRangeRequirement::None
    }
}

impl ValueRangeAssertionOpInterface for AssertU32 {
    fn value_range_assertion(&self, result: ValueRef) -> Option<ValueRangeRefinement> {
        let asserted = self.result().as_value_ref();
        (asserted == result).then(|| ValueRangeRefinement {
            input: self.value().as_value_ref(),
            result: asserted,
            constraint: ValueRangeConstraint::Type(Type::U32),
        })
    }
}
