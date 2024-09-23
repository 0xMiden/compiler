use smallvec::SmallVec;

use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct Ret : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        value: OpOperand,
    }

    derives Terminator;
}

// TODO(pauls): RetImm

derive! {
    pub struct Br : Op {
        #[dialect]
        dialect: HirDialect,
        #[successor]
        target: Successor,
    }

    derives Terminator;
}

derive! {
    pub struct CondBr : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        condition: OpOperand,
        #[successor]
        then_dest: Successor,
        #[successor]
        else_dest: Successor,
    }

    derives Terminator;
}

derive! {
    pub struct Switch : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        selector: OpOperand,
        #[successors(delegated)]
        cases: SmallVec<[SwitchCase; 2]>,
        #[successor]
        fallback: Successor,
    }

    derives Terminator;
}

// TODO(pauls): Implement `SuccessorInterface` for this type
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: OpSuccessor,
}

impl From<SwitchCase> for OpSuccessor {
    #[inline]
    fn from(value: SwitchCase) -> Self {
        value.successor
    }
}

impl From<&SwitchCase> for OpSuccessor {
    #[inline]
    fn from(value: &SwitchCase) -> Self {
        value.successor.clone()
    }
}

derive! {
    pub struct If : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        condition: OpOperand,
        #[region]
        then_body: Region,
        #[region]
        else_body: Region,
    }

    derives SingleBlock, NoRegionArguments;
}

derive! {
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
    pub struct While : Op {
        #[dialect]
        dialect: HirDialect,
        #[region]
        before: Region,
        #[region]
        after: Region,
    }

    derives SingleBlock;
}

derive! {
    pub struct Condition : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        value: OpOperand,
    }

    derives Terminator, ReturnLike;
}

derive! {
    pub struct Yield : Op {
        #[dialect]
        dialect: HirDialect,
    }

    derives Terminator, ReturnLike;
}
