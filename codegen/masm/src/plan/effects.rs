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
/// A closed read footprint; circuit inputs name raw stack lanes, never SSA values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryRead {
    Fixed(MemoryRegion),
    /// Circuit footprint derived from its address and runtime count inputs.
    Circuit {
        address: u8,
        read_count: u8,
        eval_count: u8,
    },
    /// Host print read using native address and byte-length inputs.
    PrintLn {
        address: u8,
        length: u8,
    },
    /// Paired half-open source bounds observed together, including an empty interval.
    SourceRange {
        start: u8,
        end: u8,
    },
    /// Deferred host read with a tag and runtime chunk count.
    DeferredRegisterData {
        tag: [u8; 4],
        address: u8,
        chunks: u8,
    },
}

/// Input dependencies exposed outside ordinary stack computation, such as host event payloads.
/// Replay checks these for compiler-address escape independently of memory reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    Value(u8),
    /// One nonretaining pair, with indices owned by the SourceRange read contract.
    SourceRangeBounds,
}

/// Raw accesses describe one normal transition: read, construct outputs, then write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MemoryEffects {
    pub read: Option<MemoryRead>,
    pub write: Option<MemoryWrite>,
}

/// Exact normal-completion write footprint and the lane identities written into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryWrite {
    pub region: MemoryRegion,
    /// Values in increasing native memory-address order.
    pub values: Vec<WriteValue>,
}

/// Origin of one written value in the pre-transition inputs or newly constructed outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteValue {
    Input(u8),
    Output(u8),
}

impl MemoryRegion {
    /// Number of consecutive native memory cells touched.
    pub fn width(self) -> u8 {
        match self {
            Self::Frame { width, .. }
            | Self::Indirect { width, .. }
            | Self::SourceUnknown { width } => width,
        }
    }
}

/// Identity of one output lane in a trusted target rule. All lane indices are zero-based
/// raw tuple positions; unknown computed values gain no semantic equality by themselves.
/// `FrameAddress` preserves the exact local-frame address and its compiler/source provenance.
/// `Computed` distinguishes newly computed results within this instruction execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaneEffect {
    Input(u8),
    Literal(Felt),
    /// The predicate must be boolean; candidates are actual input lanes.
    Conditional {
        condition: u8,
        if_true: u8,
        if_false: u8,
    },
    /// Lane from the fixed read footprint, in increasing address order.
    Memory {
        lane: u8,
    },
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
/// Trusted normal-completion rule for one target instruction. The consumed prefix includes
/// all inspected lanes, even when outputs preserve them; the suffix is never inspected.
/// Temporary floor steps and memory/observation effects are checked separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StackEffect {
    /// Replace this prefix with `outputs`; all remaining lanes are preserved.
    pub consumed: u8,
    /// Whether replay must require source authorization for a possible trap.
    pub may_trap: bool,
    pub outputs: Vec<LaneEffect>,
    /// Pre-expansion input lanes exposed to advice/deferred/host state.
    pub observed_inputs: Vec<Observation>,
    /// Actual temporary height changes, in execution order (prefix replacement is not a pop).
    pub floor: Vec<FloorStep>,
    pub memory: MemoryEffects,
}

impl StackEffect {
    fn new(consumed: u8, outputs: Vec<LaneEffect>, floor: Vec<FloorStep>) -> Self {
        Self {
            consumed,
            may_trap: false,
            outputs,
            observed_inputs: vec![],
            floor,
            memory: MemoryEffects::default(),
        }
    }

    fn trapping(mut self) -> Self {
        self.may_trap = true;
        self
    }

    fn computed(consumed: u8, produced: u8, steps: &[i8]) -> Self {
        Self::new(consumed, (0..produced).map(LaneEffect::Computed).collect(), floor(steps))
    }

    fn read(mut self, region: MemoryRegion) -> Self {
        self.memory.read = Some(MemoryRead::Fixed(region));
        self
    }

    fn write(mut self, region: MemoryRegion, values: Vec<WriteValue>) -> Self {
        self.memory.write = Some(MemoryWrite { region, values });
        self
    }

    /// Validate closed metadata before replay can index any raw token vector.
    pub fn validate(&self) -> Result<(), PlanError> {
        let input = |index: u8| index < self.consumed;
        let mut observed = alloc::collections::BTreeSet::new();
        let mut pairs = 0;
        for observation in &self.observed_inputs {
            match *observation {
                Observation::Value(index) => {
                    if !input(index) || !observed.insert(index) {
                        return Err(stack_error("invalid or repeated observed input dependency"));
                    }
                }
                Observation::SourceRangeBounds => pairs += 1,
            }
        }
        let fixed_read = match self.memory.read {
            Some(MemoryRead::Fixed(region)) => Some(region),
            Some(MemoryRead::Circuit {
                address,
                read_count,
                eval_count,
            }) => {
                if ![address, read_count, eval_count].into_iter().all(input) {
                    return Err(stack_error("invalid circuit read input dependency"));
                }
                None
            }
            Some(MemoryRead::PrintLn { address, length }) => {
                if ![address, length].into_iter().all(input)
                    || address == length
                    || self.observed_inputs
                        != [Observation::Value(address), Observation::Value(length)]
                    || self.memory.write.is_some()
                {
                    return Err(stack_error("invalid PrintLn observation/read roles"));
                }
                None
            }
            Some(MemoryRead::SourceRange { start, end }) => {
                if ![start, end].into_iter().all(input)
                    || start == end
                    || pairs != 1
                    || !observed.insert(start)
                    || !observed.insert(end)
                    || observed.len() != usize::from(self.consumed)
                    || self.memory.write.is_some()
                {
                    return Err(stack_error("invalid source range observation/read roles"));
                }
                None
            }
            Some(MemoryRead::DeferredRegisterData {
                tag,
                address,
                chunks,
            }) => {
                if !tag.into_iter().chain([address, chunks]).all(input) {
                    return Err(stack_error("invalid deferred data read input dependency"));
                }
                None
            }
            None => None,
        };
        if pairs != 0 && !matches!(self.memory.read, Some(MemoryRead::SourceRange { .. })) {
            return Err(stack_error("source range bounds require a source range read"));
        }
        for region in fixed_read.into_iter().chain(self.memory.write.as_ref().map(|w| w.region)) {
            if region.width() == 0
                || matches!(region, MemoryRegion::Indirect { address, .. } if !input(address))
            {
                return Err(stack_error("invalid memory region width or address input"));
            }
        }
        for lane in &self.outputs {
            let valid = match *lane {
                LaneEffect::Input(i) => input(i),
                LaneEffect::Conditional {
                    condition,
                    if_true,
                    if_false,
                } => input(condition) && input(if_true) && input(if_false),
                LaneEffect::Memory { lane } => fixed_read.is_some_and(|r| lane < r.width()),
                _ => true,
            };
            if !valid {
                return Err(stack_error("invalid output lane dependency"));
            }
        }
        if let Some(write) = &self.memory.write
            && (write.values.len() != usize::from(write.region.width())
                || write.values.iter().any(|value| match *value {
                    WriteValue::Input(i) => !input(i),
                    WriteValue::Output(i) => usize::from(i) >= self.outputs.len(),
                }))
        {
            return Err(stack_error("invalid memory write value dependency"));
        }
        Ok(())
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
    /// Closed generated-state observation, requiring sealed generic source authority.
    /// This does not grant operand-memory permissions or strict typed observations.
    pub fn generated_event(immediate: bool) -> StackEffect {
        if immediate {
            StackEffect::new(0, vec![], vec![FloorStep::Push(1), FloorStep::Pop(1)]).trapping()
        } else {
            StackEffect::new(1, vec![LaneEffect::Input(0)], vec![]).trapping()
        }
    }

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
            // Pinned assembler macros push their complete temporary words before cleanup.
            Hash => StackEffect::computed(
                4,
                4,
                &[1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1, -1],
            ),
            HMerge => StackEffect::computed(8, 4, &[1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1, -1]),
            HPerm => StackEffect::computed(12, 12, &[]),
            HornerBase | HornerExt => StackEffect::new(
                16,
                (0..14).map(Input).chain((0..2).map(LaneEffect::Computed)).collect(),
                vec![],
            )
            .trapping()
            .read(MemoryRegion::Indirect {
                address: 13,
                width: 4,
            }),
            EvalCircuit => {
                let mut effect =
                    StackEffect::new(3, (0..3).map(Input).collect(), vec![]).trapping();
                effect.memory.read = Some(MemoryRead::Circuit {
                    address: 0,
                    read_count: 1,
                    eval_count: 2,
                });
                effect
            }
            LogDeferred => StackEffect::computed(12, 12, &[]).trapping(),
            FriExt2Fold4 => StackEffect::new(
                17,
                (0..16)
                    .map(|lane| match lane {
                        11 => Input(8),
                        15 => Input(16),
                        _ => LaneEffect::Computed(lane),
                    })
                    .collect(),
                vec![FloorStep::Pop(1)],
            )
            .trapping(),
            MemStream => StackEffect::new(
                13,
                (0..8)
                    .map(|lane| LaneEffect::Memory { lane })
                    .chain((8..12).map(Input))
                    .chain([LaneEffect::Computed(0)])
                    .collect(),
                vec![],
            )
            .trapping()
            .read(MemoryRegion::Indirect {
                address: 12,
                width: 8,
            }),
            AdvPipe | CryptoStream => {
                let crypto = matches!(instruction, CryptoStream);
                let consumed = if crypto { 14 } else { 13 };
                let mut effect = StackEffect::new(
                    consumed,
                    (0..8)
                        .map(LaneEffect::Computed)
                        .chain((8..12).map(Input))
                        .chain((12..consumed).map(LaneEffect::Computed))
                        .collect(),
                    vec![],
                )
                .trapping()
                .write(
                    MemoryRegion::Indirect {
                        address: consumed - 1,
                        width: 8,
                    },
                    (0..8).map(WriteValue::Output).collect(),
                );
                if crypto {
                    effect = effect.read(MemoryRegion::Indirect {
                        address: 12,
                        width: 8,
                    });
                }
                effect
            }
            MTreeGet => StackEffect::new(
                6,
                (0..4).map(LaneEffect::Computed).chain((2..6).map(Input)).collect(),
                floor(&[1, -1, 1, 1, 1, 1, -1, -1]),
            )
            .trapping(),
            MTreeSet => StackEffect::computed(
                10,
                8,
                &[1, -1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
            )
            .trapping(),
            MTreeMerge => {
                StackEffect::computed(8, 4, &[1, -1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1, -1])
                    .trapping()
            }
            MTreeVerify | MTreeVerifyWithError(Immediate::Value(_)) => {
                StackEffect::new(10, (0..10).map(Input).collect(), vec![]).trapping()
            }
            Caller => StackEffect::computed(4, 4, &[]),
            Clk => StackEffect::computed(0, 1, &[1]),
            // The assembler pushes the resolved root in reverse order: root[0] ends on top.
            ProcRef(_) => StackEffect::computed(0, 4, &[1, 1, 1, 1]),
            AdvPush => StackEffect::computed(0, 1, &[1]).trapping(),
            AdvLoadW => StackEffect::computed(4, 4, &[]).trapping(),
            Add | Sub | Mul | Eq | Neq => StackEffect::computed(2, 1, &[-1]),
            Div => StackEffect::computed(2, 1, &[-1]).trapping(),
            Neg | Incr => StackEffect::computed(1, 1, &[]),
            Inv | Not => StackEffect::computed(1, 1, &[]).trapping(),
            And | Or => StackEffect::computed(2, 1, &[-1]).trapping(),
            Xor => StackEffect::computed(2, 1, &[1, 1, -1, -1, -1]).trapping(),
            // Pinned field_ops::{lt,lte,gt,gte}: two splits, then three reductions.
            Lt | Lte | Gt | Gte => StackEffect::computed(2, 1, &[1, 1, -1, -1, -1]),
            IsOdd => StackEffect::computed(1, 1, &[1, -1, 1, -1]),
            EqImm(value) | NeqImm(value) => StackEffect::computed(
                1,
                1,
                if literal(value)? == Felt::ZERO {
                    &[]
                } else {
                    &[1, -1]
                },
            ),
            AddImm(value) => {
                let value = literal(value)?.as_canonical_u64();
                if value == 0 {
                    StackEffect::new(1, vec![Input(0)], vec![])
                } else {
                    StackEffect::computed(1, 1, if value <= 2 { &[] } else { &[1, -1] })
                }
            }
            SubImm(value) => {
                if literal(value)? == Felt::ZERO {
                    StackEffect::new(1, vec![Input(0)], vec![])
                } else {
                    StackEffect::computed(1, 1, &[1, -1])
                }
            }
            MulImm(value) | DivImm(value) => {
                let value = literal(value)?.as_canonical_u64();
                if matches!(instruction, DivImm(_)) && value == 0 {
                    return Err(stack_error("division immediate is zero"));
                }
                match value {
                    0 => StackEffect::new(1, vec![Literal(Felt::ZERO)], floor(&[-1, 1])),
                    1 => StackEffect::new(1, vec![Input(0)], vec![]),
                    _ => StackEffect::computed(1, 1, &[1, -1]),
                }
            }
            Pow2 => StackEffect::computed(1, 1, &[1, 1, 1, -1, -1, -1]).trapping(),
            Exp | ExpBitLength(_) => {
                if let ExpBitLength(bits) = instruction
                    && *bits > 63
                {
                    return Err(stack_error("exponent bit length exceeds target limit"));
                }
                StackEffect::computed(2, 1, &[1, 1, -1, -1, -1]).trapping()
            }
            ExpImm(value) => {
                let pow = literal(value)?.as_canonical_u64();
                match pow {
                    0 => StackEffect::new(1, vec![Literal(Felt::ONE)], floor(&[-1, 1])),
                    1 => StackEffect::new(1, vec![Input(0)], vec![]),
                    2..=7 => {
                        let mut steps = vec![1; (pow - 1) as usize];
                        steps.extend(core::iter::repeat_n(-1, (pow - 1) as usize));
                        StackEffect::computed(1, 1, &steps)
                    }
                    _ => StackEffect::computed(1, 1, &[1, 1, 1, -1, -1, -1]),
                }
            }
            // Closed native arithmetic instructions include their internal system-event
            // push/drop and advice verification. Raw advice/events remain unregistered.
            ILog2 => StackEffect::computed(
                1,
                1,
                &[
                    1, -1, 1, 1, 1, 1, 1, -1, -1, -1, 1, 1, 1, 1, -1, -1, -1, -1, 1, -1, 1, 1, -1,
                    -1, -1, 1, 1, -1, -1, -1,
                ],
            )
            .trapping(),
            Eqw => StackEffect::new(
                8,
                core::iter::once(LaneEffect::Computed(0)).chain((0..8).map(Input)).collect(),
                floor(&[1, 1, -1, 1, 1, -1, -1, 1, 1, -1, -1, 1, 1, -1, -1]),
            ),
            Assert | Assertz | AssertWithError(_) | AssertzWithError(_) => {
                StackEffect::computed(1, 0, &[-1]).trapping()
            }
            AssertEq | AssertEqWithError(_) => StackEffect::computed(2, 0, &[-1, -1]).trapping(),
            AssertEqw | AssertEqwWithError(_) => StackEffect::computed(8, 0, &[-1; 8]).trapping(),
            CDrop | CDropW => {
                let width = if matches!(instruction, CDropW) { 4 } else { 1 };
                StackEffect::new(
                    1 + 2 * width,
                    (0..width)
                        .map(|lane| LaneEffect::Conditional {
                            condition: 0,
                            if_true: 1 + lane,
                            if_false: 1 + width + lane,
                        })
                        .collect(),
                    floor(&vec![-1; usize::from(1 + width)]),
                )
                .trapping()
            }
            U32Assert | U32AssertWithError(_) => {
                StackEffect::new(1, vec![Input(0)], floor(&[1, -1])).trapping()
            }
            U32Assert2 | U32Assert2WithError(_) => {
                StackEffect::new(2, (0..2).map(Input).collect(), vec![]).trapping()
            }
            U32AssertW | U32AssertWWithError(_) => {
                StackEffect::new(4, (0..4).map(Input).collect(), vec![]).trapping()
            }
            U32Split => StackEffect::computed(1, 2, &[1]),
            U32Cast => StackEffect::computed(1, 1, &[1, -1]),
            U32And | U32Xor | U32WrappingAdd | U32WrappingSub | U32WrappingMul | U32Lt | U32Lte
            | U32Gt | U32Gte | U32Div | U32Mod => StackEffect::computed(2, 1, &[-1]).trapping(),
            U32Or => StackEffect::computed(2, 1, &[1, 1, -1, -1, -1]).trapping(),
            U32Min | U32Max => StackEffect::computed(2, 1, &[1, 1, -1, -1, -1]).trapping(),
            U32Not => StackEffect::computed(1, 1, &[1, -1]).trapping(),
            U32OverflowingAdd | U32OverflowingSub | U32WideningMul | U32DivMod => {
                StackEffect::computed(2, 2, &[]).trapping()
            }
            U32WrappingAdd3 | U32WrappingMadd => StackEffect::computed(3, 1, &[-1, -1]).trapping(),
            U32WrappingAddImm(value) | U32WrappingSubImm(value) | U32WrappingMulImm(value) => {
                literal(value)?;
                StackEffect::computed(1, 1, &[1, -1]).trapping()
            }
            U32OverflowingAddImm(value)
            | U32OverflowingSubImm(value)
            | U32WideningMulImm(value) => {
                literal(value)?;
                StackEffect::computed(1, 2, &[1]).trapping()
            }
            U32DivImm(value) | U32ModImm(value) | U32DivModImm(value) => {
                if literal(value)? == 0 {
                    return Err(stack_error("division immediate is zero"));
                }
                let outputs = if matches!(instruction, U32DivModImm(_)) {
                    2
                } else {
                    1
                };
                StackEffect::computed(1, outputs, if outputs == 2 { &[1] } else { &[1, -1] })
                    .trapping()
            }
            U32Shl | U32Shr | U32Rotl => {
                StackEffect::computed(2, 1, &[1, 1, 1, -1, -1, -1, -1]).trapping()
            }
            U32Rotr => {
                StackEffect::computed(2, 1, &[1, -1, 1, 1, 1, -1, -1, -1, -1, 1, -1]).trapping()
            }
            U32ShlImm(value) | U32ShrImm(value) | U32RotlImm(value) | U32RotrImm(value) => {
                let shift = literal(value)?;
                if shift > 31 {
                    return Err(stack_error("shift immediate exceeds target limit"));
                }
                if shift == 0 {
                    StackEffect::new(1, vec![Input(0)], vec![])
                } else {
                    StackEffect::computed(1, 1, &[1, -1]).trapping()
                }
            }
            U32Popcnt => StackEffect::computed(
                1,
                1,
                &[
                    1, 1, -1, 1, -1, -1, 1, 1, -1, 1, -1, 1, -1, -1, 1, 1, -1, -1, 1, -1, 1, -1, 1,
                    -1,
                ],
            )
            .trapping(),
            U32Clz => StackEffect::computed(
                1,
                1,
                &[
                    1, -1, 1, 1, 1, -1, 1, 1, 1, -1, -1, -1, 1, -1, 1, -1, 1, 1, -1, 1, -1, 1, 1,
                    -1, 1, -1, -1, -1, -1, -1,
                ],
            )
            .trapping(),
            U32Clo => StackEffect::computed(
                1,
                1,
                &[
                    1, -1, 1, 1, 1, -1, 1, 1, 1, -1, -1, -1, 1, -1, 1, 1, -1, 1, -1, -1, 1, -1, -1,
                    -1,
                ],
            )
            .trapping(),
            U32Ctz | U32Cto => StackEffect::computed(
                1,
                1,
                &[1, -1, 1, 1, 1, 1, 1, -1, -1, -1, 1, 1, -1, 1, -1, 1, -1, -1, -1, -1],
            )
            .trapping(),
            Ext2Add | Ext2Sub | Ext2Mul => StackEffect::computed(4, 2, &[-1, -1]),
            Ext2Neg => StackEffect::computed(2, 2, &[]),
            Ext2Div => StackEffect::computed(4, 2, &[1, -1, 1, 1, -1, -1, -1, -1]).trapping(),
            Ext2Inv => StackEffect::computed(2, 2, &[1, -1, 1, 1, -1, -1]).trapping(),
            EmitImm(event)
                if *event == crate::Event::FrameStart.into()
                    || *event == crate::Event::FrameEnd.into() =>
            {
                StackEffect::new(0, vec![], vec![])
            }
            EmitImm(event) if *event == crate::Event::PrintLn.into() => {
                let mut effect = StackEffect::new(
                    2,
                    vec![Input(0), Input(1)],
                    vec![FloorStep::Push(1), FloorStep::Pop(1)],
                )
                .trapping();
                effect.observed_inputs = vec![Observation::Value(0), Observation::Value(1)];
                effect.memory.read = Some(MemoryRead::PrintLn {
                    address: 0,
                    length: 1,
                });
                effect
            }
            EmitImm(crate::masm::EventImmediate::Immediate(id)) => {
                let descriptor = crate::events::SystemEventDescriptor::from_id(literal(id)?)
                    .ok_or_else(|| stack_error("event has no finite target contract"))?;
                let n = descriptor.payload_lanes;
                let mut effect = StackEffect::new(
                    n,
                    (0..n).map(Input).collect(),
                    vec![FloorStep::Push(1), FloorStep::Pop(1)],
                )
                .trapping();
                effect.observed_inputs = (0..n).map(Observation::Value).collect();
                effect.memory.read = descriptor.read.map(|read| match read {
                    crate::events::SystemEventRead::SourceRange => {
                        effect.observed_inputs = (0..4)
                            .map(Observation::Value)
                            .chain([Observation::SourceRangeBounds])
                            .collect();
                        MemoryRead::SourceRange { start: 4, end: 5 }
                    }
                    crate::events::SystemEventRead::DeferredRegisterData => {
                        MemoryRead::DeferredRegisterData {
                            tag: [0, 1, 2, 3],
                            address: 4,
                            chunks: 5,
                        }
                    }
                });
                effect
            }
            MemStoreWLe => {
                StackEffect::new(5, (1..5).map(Input).collect(), vec![FloorStep::Pop(1)]).write(
                    MemoryRegion::Indirect {
                        address: 0,
                        width: 4,
                    },
                    (0..4).map(WriteValue::Output).collect(),
                )
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
                PushValue::Word(value) => StackEffect::new(
                    0,
                    value.0.iter().copied().map(Literal).collect(),
                    floor(&[1; 4]),
                ),
            },
            Locaddr(offset) => StackEffect::new(
                0,
                vec![LaneEffect::FrameAddress(u32::from(literal(offset)?))],
                vec![FloorStep::Push(1)],
            ),
            LocLoad(offset) => {
                StackEffect::new(0, vec![LaneEffect::Memory { lane: 0 }], vec![FloorStep::Push(1)])
                    .read(MemoryRegion::Frame {
                        offset: u32::from(literal(offset)?),
                        width: 1,
                    })
            }
            LocStore(offset) => StackEffect::new(
                1,
                vec![],
                vec![FloorStep::Push(1), FloorStep::Pop(1), FloorStep::Pop(1)],
            )
            .write(
                MemoryRegion::Frame {
                    offset: u32::from(literal(offset)?),
                    width: 1,
                },
                vec![WriteValue::Input(0)],
            ),
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
                .read(MemoryRegion::Frame {
                    offset: u32::from(offset),
                    width: 4,
                })
            }
            LocStoreWLe(offset) | LocStoreWBe(offset) => {
                let offset = word_offset(offset)?;
                let inputs: Vec<u8> = if matches!(instruction, LocStoreWLe(_)) {
                    (0..4).collect()
                } else {
                    (0..4).rev().collect()
                };
                StackEffect::new(
                    4,
                    (0..4).map(Input).collect(),
                    vec![FloorStep::Push(1), FloorStep::Pop(1)],
                )
                .write(
                    MemoryRegion::Frame {
                        offset: u32::from(offset),
                        width: 4,
                    },
                    inputs.into_iter().map(WriteValue::Output).collect(),
                )
            }
            MemLoadImm(address) => {
                literal(address)?;
                StackEffect::new(0, vec![LaneEffect::Memory { lane: 0 }], vec![FloorStep::Push(1)])
                    .read(MemoryRegion::SourceUnknown { width: 1 })
            }
            MemStoreImm(address) => {
                literal(address)?;
                StackEffect::new(1, vec![], floor(&[1, -1, -1]))
                    .write(MemoryRegion::SourceUnknown { width: 1 }, vec![WriteValue::Input(0)])
            }
            MemStoreWLeImm(address) => {
                let address = literal(address)?;
                if !address.is_multiple_of(miden_core::WORD_SIZE as u32) {
                    return Err(stack_error("unaligned source word access"));
                }
                StackEffect::new(4, (0..4).map(Input).collect(), floor(&[1, -1])).write(
                    MemoryRegion::SourceUnknown { width: 4 },
                    (0..4).map(WriteValue::Input).collect(),
                )
            }
            MemLoad => StackEffect::new(1, vec![LaneEffect::Memory { lane: 0 }], vec![]).read(
                MemoryRegion::Indirect {
                    address: 0,
                    width: 1,
                },
            ),
            MemStore => StackEffect::new(2, vec![], vec![FloorStep::Pop(2)]).write(
                MemoryRegion::Indirect {
                    address: 0,
                    width: 1,
                },
                vec![WriteValue::Input(1)],
            ),
            _ => return Err(stack_error("instruction has no registered target-effect rule")),
        };
        effect.validate()?;
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

/// Ordered height changes in the pinned assembler expansion. Zero-height operations
/// are omitted; each entry is one actual VM stack push/pop, never a net delta.
fn floor(steps: &[i8]) -> Vec<FloorStep> {
    steps
        .iter()
        .map(|step| match step {
            1 => FloorStep::Push(1),
            -1 => FloorStep::Pop(1),
            _ => unreachable!("target floor steps are single VM pushes/pops"),
        })
        .collect()
}
