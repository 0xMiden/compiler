//! Closed source-owned definitions for fixed-stack intrinsics.
use alloc::{collections::BTreeSet, vec::Vec};

use midenc_hir::{ArrayType, Operation, Report, Spanned, Type, Value};

use super::*;
use crate::{OperandStack, emit::OpEmitter, lower::expand::annotate_raw};

/// Finite registered intrinsic forms with exact source tuple and effect contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Hash,
    HMerge,
    HPerm,
    AdvicePop,
    Caller,
    UnpackWord,
    Clk,
}

pub(crate) struct Intrinsic {
    body: Vec<masm::Op>,
    pub effects: LoweringEffects,
}

impl Intrinsic {
    pub fn from_source(kind: Kind, op: &Operation) -> Result<Self, Report> {
        use Kind::*;
        let inputs =
            op.operands().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let outputs =
            op.results().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let word = Type::from(ArrayType::new(Type::Felt, 4));
        let (expected_inputs, expected_outputs) = match kind {
            Hash => (vec![Type::Felt; 4], vec![Type::Felt; 4]),
            HMerge => (vec![Type::Felt; 8], vec![Type::Felt; 4]),
            HPerm => (vec![Type::Felt; 12], vec![Type::Felt; 12]),
            AdvicePop | Clk => (vec![], vec![Type::Felt]),
            Caller => (vec![], vec![word.clone()]),
            UnpackWord => (vec![word], vec![Type::Felt; 4]),
        };
        if inputs != expected_inputs || outputs != expected_outputs {
            return Err(invalid("fixed intrinsic requires its exact input and result tuple").into());
        }
        let mut stack = OperandStack::new(op.context_rc());
        for ty in inputs.iter().rev() {
            stack.push(ty.clone());
        }
        let mut body = Vec::new();
        let mut invoked = BTreeSet::new();
        let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
        match kind {
            Hash => emitter.hash(op.span()),
            HMerge => emitter.hmerge(op.span()),
            HPerm => emitter.hperm(op.span()),
            AdvicePop => emitter.advice_pop(op.span()),
            Caller => emitter.caller(op.span()),
            UnpackWord => emitter.unpack_word(op.span()),
            Clk => emitter.clk(op.span()),
        }
        if stack.len() != outputs.len()
            || outputs.iter().enumerate().any(|(i, t)| stack[i].ty() != *t)
        {
            return Err(invalid("fixed intrinsic construction disagrees with result tuple").into());
        }
        Ok(Self {
            body,
            effects: LoweringEffects {
                may_trap: kind == AdvicePop,
                ..Default::default()
            },
        })
    }

    /// Annotate the fixed source construction without assigning storage or publishing identities.
    pub fn expand(&self, unit: &SelectedLowering) -> AnnotatedMasmBody {
        annotate_raw(&self.body, unit.id)
    }
}
