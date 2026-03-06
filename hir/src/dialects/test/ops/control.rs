use crate::{
    OpPrinter,
    derive::{OpParser, OpPrinter, operation},
    dialects::test::TestDialect,
    traits::*,
};

/// Returns from the enclosing function with the provided operands as its results.
#[derive(OpPrinter, OpParser)]
#[operation(
    dialect = TestDialect,
    traits(Terminator, ReturnLike),
    implements(OpPrinter)
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}
