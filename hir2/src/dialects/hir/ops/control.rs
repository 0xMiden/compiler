use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct RetImm {
    #[attr]
    value: Immediate,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator)
)]
pub struct Br {
    #[successor]
    target: Successor,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator)
)]
pub struct CondBr {
    #[operand]
    condition: Bool,
    #[successor]
    then_dest: Successor,
    #[successor]
    else_dest: Successor,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator)
)]
pub struct Switch {
    #[operand]
    selector: UInt32,
    #[successors(keyed)]
    cases: SwitchCase,
    #[successor]
    fallback: Successor,
}

// TODO(pauls): Implement `SuccessorInterface` for this type
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: BlockRef,
    pub arguments: Vec<ValueRef>,
}

pub struct SwitchCaseRef<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRange<'a>,
}

pub struct SwitchCaseMut<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRangeMut<'a>,
}

impl KeyedSuccessor for SwitchCase {
    type Key = u32;
    type Repr<'a> = SwitchCaseRef<'a>;
    type ReprMut<'a> = SwitchCaseMut<'a>;

    fn key(&self) -> &Self::Key {
        &self.value
    }

    fn into_parts(self) -> (Self::Key, BlockRef, Vec<ir::ValueRef>) {
        (self.value, self.successor, self.arguments)
    }

    fn into_repr(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRange<'_>,
    ) -> Self::Repr<'_> {
        SwitchCaseRef {
            value: key,
            successor: block,
            arguments: operands,
        }
    }

    fn into_repr_mut(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRangeMut<'_>,
    ) -> Self::ReprMut<'_> {
        SwitchCaseMut {
            value: key,
            successor: block,
            arguments: operands,
        }
    }
}

#[operation(
    dialect = HirDialect,
    traits(SingleBlock, NoRegionArguments)
)]
pub struct If {
    #[operand]
    condition: Bool,
    #[region]
    then_body: Region,
    #[region]
    else_body: Region,
}

/// A while is a loop structure composed of two regions: a "before" region, and an "after" region.
///
/// The "before" region's entry block parameters correspond to the operands expected by the
/// operation, and can be used to compute the condition that determines whether the "after" body
/// is executed or not, or simply forwarded to the "after" region. The "before" region must
/// terminate with a [Condition] operation, which will be evaluated to determine whether or not
/// to continue the loop.
///
/// The "after" region corresponds to the loop body, and must terminate with a [Yield] operation,
/// whose operands must be of the same arity and type as the "before" region's argument list. In
/// this way, the "after" body can feed back input to the "before" body to determine whether to
/// continue the loop.
#[operation(
    dialect = HirDialect,
    traits(SingleBlock)
)]
pub struct While {
    #[region]
    before: Region,
    #[region]
    after: Region,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct Condition {
    #[operand]
    value: Bool,
}

#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct Yield {}
