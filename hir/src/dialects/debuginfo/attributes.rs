mod compile_unit;
mod expression;
mod inline_call;
mod subprogram;
mod variable;

pub use self::{
    compile_unit::{CompileUnit, CompileUnitAttr},
    expression::{Expression, ExpressionAttr, ExpressionOp, FrameBase, ResolvedFrameBase},
    inline_call::{
        INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr, InlineCallFrame,
    },
    subprogram::{Subprogram, SubprogramAttr},
    variable::{Variable, VariableAttr},
};
