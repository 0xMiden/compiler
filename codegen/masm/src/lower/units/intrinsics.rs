//! Closed source-owned definitions for fixed-stack intrinsics and events.
//!
//! Exact input/result tuples, retained lanes, immediates, and source-memory effects are fixed
//! before the unit is allocated. Typed helpers construct a local raw body; that construction
//! cannot supply its own authority to preserve an identity or access source memory.
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
    HornerBase,
    HornerExt,
    LogDeferred,
    FriExt2Fold4,
    MemStream,
    AdvicePipe,
    CryptoStream,
    EvalCircuit,
    SystemEvent,
    MTreeGet,
    MTreeSet,
    MTreeMerge,
    MTreeVerify,
    AdvicePop,
    AdviceLoadWord,
    Caller,
    UnpackWord,
    Clk,
}

/// Captured intrinsic instructions and source-established lane/effect relationships.
/// Retained lane positions are bound to selected atoms separately from executable expansion.
pub(crate) struct Intrinsic {
    body: Vec<masm::Op>,
    retained: Vec<(usize, usize)>,
    /// Source-memory and trap behavior allowed by the checked intrinsic or event descriptor.
    pub effects: LoweringEffects,
}
impl Intrinsic {
    /// Validate the exact source tuple and immediate/descriptor parameters, then capture code.
    /// `kind` must be the operation's registered selection rule. Unknown system events,
    /// unsupported tuples, and helper constructions with inconsistent results are rejected.
    pub fn from_source(kind: Kind, op: &Operation) -> Result<Self, Report> {
        use Kind::*;
        let inputs =
            op.operands().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let outputs =
            op.results().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let event = if kind == SystemEvent {
            let source = op
                .downcast_ref::<midenc_dialect_hir::SystemEvent>()
                .ok_or_else(|| invalid("system event requires its source operation"))?;
            let midenc_hir::Immediate::Felt(id) = *source.get_event_id() else {
                return Err(invalid("system event ID requires a Felt immediate").into());
            };
            let descriptor = crate::events::FixedSystemEvent::from_id(id)
                .ok_or_else(|| invalid("system event has no fixed-window contract"))?;
            Some((id, usize::from(descriptor.payload_lanes)))
        } else {
            None
        };
        let word = Type::from(ArrayType::new(Type::Felt, 4));
        let (expected_inputs, expected_outputs) = match kind {
            Hash => (vec![Type::Felt; 4], vec![Type::Felt; 4]),
            HMerge => (vec![Type::Felt; 8], vec![Type::Felt; 4]),
            HPerm | LogDeferred => (vec![Type::Felt; 12], vec![Type::Felt; 12]),
            HornerBase | HornerExt => (vec![Type::Felt; 16], vec![Type::Felt; 16]),
            AdvicePipe | MemStream => (vec![Type::Felt; 13], vec![Type::Felt; 13]),
            EvalCircuit => (vec![Type::Felt; 3], vec![Type::Felt; 3]),
            SystemEvent => {
                let (_, n) = event.unwrap();
                (vec![Type::Felt; n], vec![Type::Felt; n])
            }
            CryptoStream => (vec![Type::Felt; 14], vec![Type::Felt; 14]),
            FriExt2Fold4 => (vec![Type::Felt; 17], vec![Type::Felt; 16]),
            MTreeGet => (vec![Type::Felt; 6], vec![Type::Felt; 8]),
            MTreeSet => (vec![Type::Felt; 10], vec![Type::Felt; 8]),
            MTreeMerge => (vec![Type::Felt; 8], vec![Type::Felt; 4]),
            MTreeVerify => (vec![Type::Felt; 10], vec![Type::Felt; 10]),
            AdvicePop | Clk => (vec![], vec![Type::Felt]),
            AdviceLoadWord => {
                if inputs.len() != 4 {
                    return Err(invalid("advice word requires four input operands").into());
                }
                for ty in &inputs {
                    if ValueLayout::new(ty.clone())?.lanes() != 1 {
                        return Err(
                            invalid("advice word inputs require one nonempty raw lane").into()
                        );
                    }
                }
                (inputs.clone(), vec![Type::Felt; 4])
            }
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
            HornerBase => emitter.horner_base(op.span()),
            HornerExt => emitter.horner_ext(op.span()),
            LogDeferred => emitter.log_deferred(op.span()),
            FriExt2Fold4 => emitter.fri_ext2fold4(op.span()),
            MemStream => emitter.mem_stream(op.span()),
            AdvicePipe => emitter.advice_pipe(op.span()),
            CryptoStream => emitter.crypto_stream(op.span()),
            EvalCircuit => emitter.eval_circuit(op.span()),
            SystemEvent => {
                let (id, n) = event.unwrap();
                emitter.system_event(id, n, op.span());
            }
            MTreeGet => emitter.mtree_get(op.span()),
            MTreeSet => emitter.mtree_set(op.span()),
            MTreeMerge => emitter.mtree_merge(op.span()),
            MTreeVerify => {
                let source = op
                    .downcast_ref::<midenc_dialect_hir::MTreeVerify>()
                    .ok_or_else(|| invalid("Merkle verification requires its source operation"))?;
                emitter.mtree_verify(Some(source.get_message().as_str()), op.span());
            }
            AdvicePop => emitter.advice_pop(op.span()),
            AdviceLoadWord => emitter.advice_load_word(op.span())?,
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
            retained: match kind {
                MTreeGet => (4..8).map(|i| (i, i - 2)).collect(),
                EvalCircuit => (0..3).map(|i| (i, i)).collect(),
                SystemEvent => (0..inputs.len()).map(|i| (i, i)).collect(),
                MTreeVerify => (0..10).map(|i| (i, i)).collect(),
                HornerBase | HornerExt => (0..14).map(|i| (i, i)).collect(),
                AdvicePipe | CryptoStream | MemStream => (8..12).map(|i| (i, i)).collect(),
                FriExt2Fold4 => vec![(11, 8), (15, 16)],
                _ => vec![],
            },
            effects: LoweringEffects {
                may_trap: matches!(
                    kind,
                    AdvicePop
                        | AdviceLoadWord
                        | MTreeGet
                        | MTreeSet
                        | MTreeMerge
                        | MTreeVerify
                        | HornerBase
                        | HornerExt
                        | LogDeferred
                        | FriExt2Fold4
                        | MemStream
                        | AdvicePipe
                        | CryptoStream
                        | EvalCircuit
                        | SystemEvent
                ),
                memory: if matches!(kind, CryptoStream) {
                    SourceMemoryEffects::READ_WRITE
                } else if matches!(kind, AdvicePipe) {
                    SourceMemoryEffects::WRITE
                } else if matches!(kind, HornerBase | HornerExt | MemStream | EvalCircuit) {
                    SourceMemoryEffects::READ
                } else {
                    SourceMemoryEffects::default()
                },
                ..Default::default()
            },
        })
    }

    /// Bind only the unconditional identities reconstructed from the source kind.
    pub fn bind_evidence(&self, inputs: &[Atom], outputs: &[Atom]) -> crate::verify::UnitEvidence {
        crate::verify::UnitEvidence {
            aliases: LaneAliases::new(
                self.retained
                    .iter()
                    .map(|&(output, input)| (outputs[output], inputs[input]))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    /// Annotate the fixed source construction without assigning storage or publishing identities.
    pub fn expand(&self, unit: &SelectedLowering) -> AnnotatedMasmBody {
        annotate_raw(&self.body, unit.id)
    }
}
