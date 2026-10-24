//! Small trusted target rules. Unregistered instructions and unresolved immediates fail closed.
use alloc::{vec, vec::Vec};

use miden_core::Felt;

use super::{DiagnosticCategory, PlanError};
use crate::masm::{Immediate, Instruction, PushValue};

/// Fixed-width memory footprint in native element-address order. Indirect accesses must
/// resolve actual input provenance before being classified as source or compiler storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryRegion {
    /// Statically known local frame interval, in field elements.
    Frame {
        /// First element relative to the local frame base.
        offset: u32,
        /// Number of consecutive elements accessed.
        width: u8,
    },
    /// Source-memory interval whose address has no exact compiler-frame identity.
    SourceUnknown {
        /// Number of consecutive elements accessed.
        width: u8,
    },
    /// Resolve this input token: compiler addresses retain exact provenance.
    Indirect {
        /// Raw input lane containing the address.
        address: u8,
        /// Number of consecutive elements accessed.
        width: u8,
    },
}
/// The exclusive fixed-width memory footprint permitted by a trusted instruction rule.
/// `Read` supplies `LaneEffect::Memory` in increasing address order; `Write` takes the
/// values named by `StackEffect::memory_inputs`. `None` permits no memory access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryEffect {
    None,
    Read(MemoryRegion),
    Write(MemoryRegion),
}
/// Identity of one output lane in a trusted target rule. All lane indices are zero-based
/// raw tuple positions; unknown computed values gain no semantic equality by themselves.
/// `FrameAddress` preserves the exact local-frame address and its compiler/source provenance.
/// `Computed` distinguishes newly computed results within this instruction execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaneEffect {
    Input(u8),
    Literal(Felt),
    Memory { lane: u8 },
    FrameAddress(u32),
    Computed(u8),
}
/// Actual temporary height change in an instruction's VM expansion, used to preserve floor
/// padding behavior even when its overall input/output lane count is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloorStep {
    Push(u8),
    Pop(u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StackEffect {
    /// Replace this prefix with `outputs`; all remaining lanes are preserved.
    pub consumed: u8,
    pub outputs: Vec<LaneEffect>,
    /// Actual temporary height changes, in execution order (prefix replacement is not a pop).
    pub floor: Vec<FloorStep>,
    pub memory: MemoryEffect,
    /// Input lanes written to consecutive cells, in increasing memory-address order.
    pub memory_inputs: Vec<u8>,
}
impl StackEffect {
    fn new(consumed: u8, outputs: Vec<LaneEffect>, floor: Vec<FloorStep>) -> Self {
        Self {
            consumed,
            outputs,
            floor,
            memory: MemoryEffect::None,
            memory_inputs: Vec::new(),
        }
    }

    fn memory(mut self, memory: MemoryEffect) -> Self {
        self.memory = memory;
        self
    }

    fn writes(mut self, inputs: Vec<u8>) -> Self {
        self.memory_inputs = inputs;
        self
    }
}

/// Raw height split into procedure-owned lanes, a protected caller suffix, and VM floor padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloorState {
    pub owned: usize,
    pub protected: usize,
    pub padding: usize,
}
impl FloorState {
    /// Pop owned lanes or reject a crossing into the protected suffix; padding may grow.
    pub fn pop(self, count: usize) -> Result<Self, PlanError> {
        let owned = self
            .owned
            .checked_sub(count)
            .ok_or_else(|| stack_error("pop crosses the protected suffix"))?;
        let padding =
            self.padding.max(16usize.saturating_sub(self.protected.saturating_add(owned)));
        Ok(Self {
            owned,
            padding,
            ..self
        })
    }

    /// Add owned lanes without consuming existing floor padding; reject height overflow.
    pub fn push(self, count: usize) -> Result<Self, PlanError> {
        let owned = self
            .owned
            .checked_add(count)
            .ok_or_else(|| stack_error("raw height overflow"))?;
        Ok(Self { owned, ..self })
    }
}

/// Indexed stack operation requested by planning, before target encoding validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Primitive {
    Dup(u8),
    Swap(u8),
    MoveUp(u8),
    MoveDown(u8),
    DupWord(u8),
    SwapWord(u8),
    Drop,
    Push(Felt),
}
impl Primitive {
    /// Encode a primitive or reject a depth outside the target's legal indexed window.
    pub fn instruction(self) -> Result<Instruction, PlanError> {
        use Instruction::*;
        let instruction = match self {
            Self::Dup(depth) => DUP.get(usize::from(depth)),
            Self::Swap(depth) => depth.checked_sub(1).and_then(|d| SWAP.get(usize::from(d))),
            Self::MoveUp(1) | Self::MoveDown(1) => Some(&Swap1),
            Self::MoveUp(depth) => depth.checked_sub(2).and_then(|d| UP.get(usize::from(d))),
            Self::MoveDown(depth) => depth.checked_sub(2).and_then(|d| DOWN.get(usize::from(d))),
            Self::DupWord(depth) => DUP_WORD.get(usize::from(depth)),
            Self::SwapWord(depth) => {
                depth.checked_sub(1).and_then(|d| SWAP_WORD.get(usize::from(d)))
            }
            Self::Drop => return Ok(Drop),
            Self::Push(value) => return Ok(Push(PushValue::from(value).into())),
        };
        instruction.cloned().ok_or_else(|| stack_error("illegal indexed stack access"))
    }
}

mod indexed {
    use super::Instruction::{self, *};
    pub(super) const DUP: [Instruction; 16] = [
        Dup0, Dup1, Dup2, Dup3, Dup4, Dup5, Dup6, Dup7, Dup8, Dup9, Dup10, Dup11, Dup12, Dup13,
        Dup14, Dup15,
    ];
    pub(super) const SWAP: [Instruction; 15] = [
        Swap1, Swap2, Swap3, Swap4, Swap5, Swap6, Swap7, Swap8, Swap9, Swap10, Swap11, Swap12,
        Swap13, Swap14, Swap15,
    ];
    pub(super) const UP: [Instruction; 14] = [
        MovUp2, MovUp3, MovUp4, MovUp5, MovUp6, MovUp7, MovUp8, MovUp9, MovUp10, MovUp11, MovUp12,
        MovUp13, MovUp14, MovUp15,
    ];
    pub(super) const DOWN: [Instruction; 14] = [
        MovDn2, MovDn3, MovDn4, MovDn5, MovDn6, MovDn7, MovDn8, MovDn9, MovDn10, MovDn11, MovDn12,
        MovDn13, MovDn14, MovDn15,
    ];
    pub(super) const DUP_WORD: [Instruction; 4] =
        [Instruction::DupW0, Instruction::DupW1, Instruction::DupW2, Instruction::DupW3];
    pub(super) const SWAP_WORD: [Instruction; 3] =
        [Instruction::SwapW1, Instruction::SwapW2, Instruction::SwapW3];
}
use indexed::*;

/// Closed registry of trusted target semantics used by raw replay. Extending this registry
/// requires matching the VM and assembler expansion, including temporary stack heights.
pub(crate) struct TargetEffects;
impl TargetEffects {
    /// Look up concrete instruction semantics, rejecting unresolved immediates, invalid
    /// encodings, and instructions without a registered rule. Calls use separate ABI rules.
    pub fn instruction(instruction: &Instruction) -> Result<StackEffect, PlanError> {
        use Instruction::*;
        use LaneEffect::{Input, Literal};
        let duplicate = index(instruction, &DUP, 0);
        if let Some(depth) = duplicate {
            return Ok(StackEffect::new(
                depth + 1,
                core::iter::once(Input(depth)).chain((0..=depth).map(Input)).collect(),
                vec![FloorStep::Push(1)],
            ));
        }
        let swap = index(instruction, &SWAP, 1);
        if let Some(depth) = swap {
            let mut outputs = (0..=depth).map(Input).collect::<Vec<_>>();
            outputs.swap(0, usize::from(depth));
            return Ok(StackEffect::new(depth + 1, outputs, vec![]));
        }
        let up = index(instruction, &UP, 2);
        if let Some(depth) = up {
            return Ok(StackEffect::new(
                depth + 1,
                core::iter::once(Input(depth)).chain((0..depth).map(Input)).collect(),
                vec![],
            ));
        }
        let down = index(instruction, &DOWN, 2);
        if let Some(depth) = down {
            return Ok(StackEffect::new(
                depth + 1,
                (1..=depth).map(Input).chain(core::iter::once(Input(0))).collect(),
                vec![],
            ));
        }
        let dup_word = index(instruction, &DUP_WORD, 0);
        if let Some(word) = dup_word {
            let depth = word * 4;
            return Ok(StackEffect::new(
                depth + 4,
                (depth..depth + 4).map(Input).chain((0..depth + 4).map(Input)).collect(),
                vec![FloorStep::Push(4)],
            ));
        }
        let swap_word = index(instruction, &SWAP_WORD, 1);
        if let Some(word) = swap_word {
            let depth = word * 4;
            let mut outputs = (0..depth + 4).map(Input).collect::<Vec<_>>();
            for i in 0..4 {
                outputs.swap(i, usize::from(depth) + i);
            }
            return Ok(StackEffect::new(depth + 4, outputs, vec![]));
        }
        let effect = match instruction {
            Add | Sub | Mul | Eq | Neq => {
                StackEffect::new(2, vec![LaneEffect::Computed(0)], vec![FloorStep::Pop(1)])
            }
            U32WrappingAdd | U32WrappingMul | U32Gt | And | U32Shr | U32Shl | U32And | U32Or => {
                StackEffect::new(2, vec![LaneEffect::Computed(0)], vec![FloorStep::Pop(1)])
            }
            EqImm(Immediate::Value(_)) | U32Not => {
                StackEffect::new(1, vec![LaneEffect::Computed(0)], vec![])
            }
            U32WrappingSubImm(Immediate::Value(_)) => StackEffect::new(
                1,
                vec![LaneEffect::Computed(0)],
                vec![FloorStep::Push(1), FloorStep::Pop(1)],
            ),
            U32OverflowingAdd => {
                StackEffect::new(2, vec![LaneEffect::Computed(0), LaneEffect::Computed(1)], vec![])
            }
            U32DivModImm(Immediate::Value(_)) => StackEffect::new(
                1,
                vec![LaneEffect::Computed(0), LaneEffect::Computed(1)],
                vec![FloorStep::Push(1)],
            ),
            EmitImm(event)
                if *event == crate::Event::FrameStart.into()
                    || *event == crate::Event::FrameEnd.into() =>
            {
                StackEffect::new(0, vec![], vec![])
            }
            MemStoreWLe => {
                StackEffect::new(5, (1..5).map(Input).collect(), vec![FloorStep::Pop(1)])
                    .memory(MemoryEffect::Write(MemoryRegion::Indirect {
                        address: 0,
                        width: 4,
                    }))
                    .writes(vec![1, 2, 3, 4])
            }
            Nop => StackEffect::new(0, vec![], vec![]),
            Drop => StackEffect::new(1, vec![], vec![FloorStep::Pop(1)]),
            DropW => StackEffect::new(4, vec![], vec![FloorStep::Pop(4)]),
            PadW => StackEffect::new(0, vec![Literal(Felt::ZERO); 4], vec![FloorStep::Push(4)]),
            Push(Immediate::Value(value)) => match value.inner() {
                PushValue::Int(value) => StackEffect::new(
                    0,
                    vec![Literal(
                        Felt::new(value.as_int())
                            .map_err(|_| stack_error("literal exceeds the field modulus"))?,
                    )],
                    vec![FloorStep::Push(1)],
                ),
                PushValue::Word(_) => {
                    return Err(stack_error("word literal rule is not registered"));
                }
            },
            Locaddr(offset) => StackEffect::new(
                0,
                vec![LaneEffect::FrameAddress(u32::from(literal(offset)?))],
                vec![FloorStep::Push(1)],
            ),
            LocLoad(offset) => {
                StackEffect::new(0, vec![LaneEffect::Memory { lane: 0 }], vec![FloorStep::Push(1)])
                    .memory(MemoryEffect::Read(MemoryRegion::Frame {
                        offset: u32::from(literal(offset)?),
                        width: 1,
                    }))
            }
            LocStore(offset) => StackEffect::new(
                1,
                vec![],
                vec![FloorStep::Push(1), FloorStep::Pop(1), FloorStep::Pop(1)],
            )
            .memory(MemoryEffect::Write(MemoryRegion::Frame {
                offset: u32::from(literal(offset)?),
                width: 1,
            }))
            .writes(vec![0]),
            LocLoadWLe(offset) | LocLoadWBe(offset) => {
                let offset = word_offset(offset)?;
                let lanes: Vec<_> = if matches!(instruction, LocLoadWLe(_)) {
                    (0..4).collect()
                } else {
                    (0..4).rev().collect()
                };
                StackEffect::new(
                    4,
                    lanes.into_iter().map(|lane| LaneEffect::Memory { lane }).collect(),
                    vec![FloorStep::Push(1), FloorStep::Pop(1)],
                )
                .memory(MemoryEffect::Read(MemoryRegion::Frame {
                    offset: u32::from(offset),
                    width: 4,
                }))
            }
            LocStoreWLe(offset) | LocStoreWBe(offset) => {
                let offset = word_offset(offset)?;
                let inputs = if matches!(instruction, LocStoreWLe(_)) {
                    (0..4).collect()
                } else {
                    (0..4).rev().collect()
                };
                StackEffect::new(
                    4,
                    (0..4).map(Input).collect(),
                    vec![FloorStep::Push(1), FloorStep::Pop(1)],
                )
                .memory(MemoryEffect::Write(MemoryRegion::Frame {
                    offset: u32::from(offset),
                    width: 4,
                }))
                .writes(inputs)
            }
            MemLoad => StackEffect::new(1, vec![LaneEffect::Memory { lane: 0 }], vec![]).memory(
                MemoryEffect::Read(MemoryRegion::Indirect {
                    address: 0,
                    width: 1,
                }),
            ),
            MemStore => StackEffect::new(2, vec![], vec![FloorStep::Pop(2)])
                .memory(MemoryEffect::Write(MemoryRegion::Indirect {
                    address: 0,
                    width: 1,
                }))
                .writes(vec![1]),
            _ => return Err(stack_error("instruction has no registered target-effect rule")),
        };
        Ok(effect)
    }
}
fn literal<T: Copy>(value: &Immediate<T>) -> Result<T, PlanError> {
    match value {
        Immediate::Value(value) => Ok(*value.inner()),
        Immediate::Constant(_) => Err(stack_error("unresolved immediate has no target effect")),
    }
}
fn stack_error(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::IllegalStackAccess, message)
}

fn word_offset(value: &Immediate<u16>) -> Result<u16, PlanError> {
    let offset = literal(value)?;
    if !offset.is_multiple_of(miden_core::WORD_SIZE as u16) {
        return Err(stack_error("unaligned word access"));
    }
    Ok(offset)
}

fn index(instruction: &Instruction, family: &[Instruction], start: u8) -> Option<u8> {
    family
        .iter()
        .position(|candidate| candidate == instruction)
        .map(|index| index as u8 + start)
}
