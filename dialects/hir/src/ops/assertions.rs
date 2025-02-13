use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects)
)]
pub struct Assert {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: u32,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects)
)]
pub struct Assertz {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: u32,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, Commutative, SameTypeOperands)
)]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects)
)]
pub struct AssertEqImm {
    #[operand]
    lhs: AnyInteger,
    #[attr]
    rhs: Immediate,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, Terminator)
)]
pub struct Unreachable {}

#[operation(
    dialect = HirDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface)
)]
pub struct Poison {
    #[attr]
    ty: Type,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for Poison {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let poison_ty = self.ty().clone();
        self.result_mut().set_type(poison_ty);
        Ok(())
    }
}
