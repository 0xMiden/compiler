use crate::{derive::operation, dialects::test::TestDialect, traits::*};

/// Returns from the enclosing function with the provided operands as its results.
#[operation(
    dialect = TestDialect,
    traits(Terminator, ReturnLike)
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}
