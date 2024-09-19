use smallvec::SmallVec;

use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct Ret : Op implements Terminator {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        value: OpOperand,
    }
}

// TODO(pauls): RetImm

// TODO(pauls): Implement support for:
//
// * `#[successor]` to represent a single `Successor` of this op
derive! {
    pub struct Br : Op implements Terminator {
        #[dialect]
        dialect: HirDialect,
        #[successor]
        target: Successor,
    }
}

derive! {
    pub struct CondBr : Op implements Terminator {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        condition: OpOperand,
        #[successor]
        then_dest: Successor,
        #[successor]
        else_dest: Successor,
    }
}

// TODO(pauls): Implement support for:
//
// * `SuccessorInterface` for custom types which represent a `Successor`
// * `#[successors]` to represent variadic successors of an op
// * `#[successors(interface)]` to indicate that the successor info should be obtained from this field via `SuccessorInterface`
derive! {
    pub struct Switch : Op implements Terminator {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        selector: OpOperand,
        #[successors(delegated)]
        cases: SmallVec<[SwitchCase; 2]>,
        #[successor]
        fallback: Successor,
    }
}

// TODO(pauls): Implement `SuccessorInterface` for this type
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: Successor,
}

// TODO(pauls): Implement:
//
// * `region` attribute
derive! {
    pub struct If : Op implements SingleBlock, NoRegionArguments {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        condition: OpOperand,
        #[region]
        then_body: Region,
        #[region]
        else_body: Region,
    }
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

derive! {
    pub struct While : Op implements SingleBlock {
        #[dialect]
        dialect: HirDialect,
        #[region]
        before: Region,
        #[region]
        after: Region,
    }
}

derive! {
    pub struct Condition : Op implements Terminator, ReturnLike {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        value: OpOperand,
    }
}

derive! {
    pub struct Yield : Op implements Terminator, ReturnLike {
        #[dialect]
        dialect: HirDialect,
    }
}
