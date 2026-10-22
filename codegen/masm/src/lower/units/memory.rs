//! Typed memory constructions and explicit storage phases.
//!
//! Source rules fix memory effects, helper ABIs, and lane relationships before allocation.
//! Ordinary operations defer only source-local addresses to the final frame. Spanning-cell
//! phases instead declare scratch contents explicitly so verification can track them across units.
use alloc::{collections::BTreeMap, vec, vec::Vec};

use midenc_hir::Report;

use super::*;
use crate::{
    lower::{expand::annotate_raw, raw},
    verify::{CallArgument, CallPath, CallSite, LoopPosition, LoopSchema, UnitEvidence},
};

/// Registered overlap-safe byte copy with checked range arithmetic.
/// The raw algorithm uses an aligned element-copy helper or a directional byte loop.
pub(crate) struct ByteMemmove;

impl ByteMemmove {
    /// Require `[src, dst, count]` byte lanes and derive read/write, trap, loop, and call evidence.
    pub fn select(
        &self,
        id: SelectedStepId,
        source: SourceStepId,
        inputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        if inputs.len() != 3 {
            return Err(invalid("byte memmove requires src, dst, count"));
        }
        let mut unit = selected(self.kind(), id, source, inputs, &[], &[]);
        unit.effects = LoweringEffects {
            memory: SourceMemoryEffects::READ_WRITE,
            may_trap: true,
            normal_return: true,
        };
        let proof = UnitEvidence {
            loops: BTreeMap::from([(
                ControlId(2),
                LoopSchema {
                    stack: vec![
                        LoopPosition::Carry(0),
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                    ],
                    cells: vec![],
                },
            )]),
            calls: vec![CallSite {
                instruction: masm::Instruction::Exec(raw::target(
                    "::intrinsics::mem::memmove_elements",
                )),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 3, 0)?,
                inputs: vec![CallArgument::Local; 3],
                path: vec![CallPath::Then(ControlId(0))],
                truncate: false,
                outputs: vec![],
                publications: vec![],
                selector: None,
            }],
            ..Default::default()
        };
        Ok((unit, proof))
    }
}

impl LoweringDefinition for ByteMemmove {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1000)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        Ok(annotate_raw(&raw::byte_memmove(Default::default()), unit.id))
    }
}

/// Explicit one-cell spanning storage phases. Content identities cross selected boundaries;
/// expansion only consumes a final object binding and emits the scalar memory instruction.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SpanningCell {
    /// Consume one lane and establish the declared content identity in the scratch cell.
    Write,
    /// Read the declared content identity and publish it as the single result lane.
    Read,
}
impl SpanningCell {
    /// Declare one phase of a one-element spanning object, including its content obligation.
    /// Reject unit-local lifetimes, non-scalar objects, and mismatched input/result counts.
    pub fn select(
        self,
        id: SelectedStepId,
        source: SourceStepId,
        object: &ScratchRequirement,
        content: ContentId,
        inputs: &[Atom],
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        let write = matches!(self, Self::Write);
        if object.lifetime != ScratchLifetime::Spanning
            || object.size != 1
            || inputs.len() != usize::from(write)
            || outputs.len() != usize::from(!write)
        {
            return Err(invalid("spanning scalar phase contract mismatch"));
        }
        let mut unit = selected(self.kind(), id, source, inputs, &[], outputs);
        unit.scratch = vec![object.clone()];
        let fact = ScratchCellFact {
            object: object.id,
            offset: 0,
            content: ContentRequirement::Content(content),
        };
        unit.scratch_effects = vec![if write {
            ScratchEffect::Write(fact)
        } else {
            ScratchEffect::Read(fact)
        }];
        let proof = UnitEvidence {
            scratch_outputs: if write {
                vec![]
            } else {
                vec![(
                    FrameRef::Scratch {
                        object: object.id,
                        offset: 0,
                    },
                    0,
                )]
            },
            ..Default::default()
        };
        Ok((unit, proof))
    }
}
impl LoweringDefinition for SpanningCell {
    fn kind(&self) -> LoweringKind {
        LoweringKind(if matches!(self, Self::Write) {
            1204
        } else {
            1205
        })
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut body = AnnotatedMasmBody::new();
        body.push_instruction(
            masm::Instruction::Locaddr(
                frame
                    .resolve(FrameRef::Scratch {
                        object: unit.scratch[0].id,
                        offset: 0,
                    })?
                    .into(),
            ),
            unit.id,
        );
        body.push_instruction(
            if matches!(self, Self::Write) {
                masm::Instruction::MemStore
            } else {
                masm::Instruction::MemLoad
            },
            unit.id,
        );
        Ok(body)
    }
}

/// Closed source vocabulary for ordinary typed memory operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Load,
    Store,
    LoadLocal,
    StoreLocal,
    LocalAddress,
    MemGrow,
    MemSize,
    MemSet,
    MemCpy,
    MemMove,
    I32Load8S,
    I32Load16S,
    I64Load8S,
    I64Load16S,
    I64Load32S,
}

/// Source-owned typed memory construction with a deferred source-local offset when needed.
/// Build from a registered source operation and bind its lane identities before using evidence.
/// Helper selection and memory effects remain source facts; expansion only resolves final locals.
pub(crate) struct Memory {
    body: Vec<masm::Op>,
    local: Option<u32>,
    /// Source-derived helper contracts, completed with exact selected atoms by `bind`.
    pub proof: UnitEvidence,
    /// Source-memory and trap behavior authorized for this operation.
    pub effects: LoweringEffects,
    argument_lanes: Vec<(usize, usize, usize)>,
    direct_results: Option<usize>,
}

impl Memory {
    /// Construct a compiler-owned global write using the ordinary typed immediate store.
    /// The linked byte address must satisfy the source type's alignment and complete range.
    /// This accepts semantic type/context parameters rather than fabricating a HIR Store;
    /// callers must bind its exact value lanes and register the generated terminal write.
    pub fn store_immediate(
        context: alloc::rc::Rc<midenc_hir::Context>,
        ty: midenc_hir::Type,
        address: u32,
        span: midenc_hir::SourceSpan,
    ) -> Result<Self, Report> {
        use crate::{NativePtr, OperandStack, emit::OpEmitter};
        let width = ValueLayout::new(ty.clone())?.lanes() as usize;
        if !storable(&ty)
            || !u64::from(address).is_multiple_of(ty.min_alignment() as u64)
            || u64::from(address) + ty.size_in_bytes() as u64 > (1u64 << 32)
        {
            return Err(invalid("global store type, alignment or byte range is unsupported").into());
        }
        let pointer = NativePtr::from_ptr(address);
        if ty == midenc_hir::Type::Felt && !pointer.is_element_aligned() {
            return Err(invalid("global felt store must be element aligned").into());
        }
        let mut stack = OperandStack::new(context);
        stack.push(ty.clone());
        let mut body = vec![];
        let mut invoked = alloc::collections::BTreeSet::new();
        OpEmitter::new(&mut invoked, &mut body, &mut stack).store_imm(address, span);
        let direct = (pointer.is_element_aligned() && ty.size_in_bits() >= 32)
            || (ty.size_in_bits() < 32
                && usize::from(pointer.offset) * 8 + ty.size_in_bits() <= 32
                && !(ty.size_in_bits() == 16 && !pointer.is_element_aligned()));
        let proof = if direct {
            UnitEvidence::default()
        } else {
            memory_contract(Kind::Store, &ty, false)?
        };
        check_construction(&body, &proof).map_err(|mut error| {
            error.message = alloc::format!("{}; immediate store body: {body:?}", error.message);
            error
        })?;
        let mut argument_lanes = vec![];
        if scalar(&ty) || ty.size_in_bits() == 32 {
            for (site, call) in proof.calls.iter().enumerate() {
                if call.inputs.len() == width + 2 {
                    argument_lanes.extend((0..width).map(|lane| (site, 2 + lane, lane)));
                }
            }
        }
        Ok(Self {
            body,
            local: None,
            proof,
            effects: LoweringEffects {
                memory: if ty.size_in_bits() < 32 || !pointer.is_element_aligned() {
                    SourceMemoryEffects::READ_WRITE
                } else {
                    SourceMemoryEffects::WRITE
                },
                may_trap: true,
                normal_return: true,
            },
            argument_lanes,
            direct_results: None,
        })
    }

    /// Validate a registered operation's type tuple and local ownership, then capture its body.
    /// `kind` must match the operation's registered rule. Unsupported memory representations,
    /// malformed local attributes, and inconsistent helper constructions are errors.
    pub fn from_source(kind: Kind, op: &midenc_hir::Operation) -> Result<Self, Report> {
        use Kind::*;
        use midenc_hir::{Spanned, Type, Value};

        use crate::{OperandStack, emit::OpEmitter, hir};
        let inputs =
            op.operands().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let outputs =
            op.results().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let local_attr = match kind {
            LoadLocal => Some(op.downcast_ref::<hir::LoadLocal>().unwrap().local()),
            StoreLocal => Some(op.downcast_ref::<hir::StoreLocal>().unwrap().local()),
            LocalAddress => Some(op.downcast_ref::<hir::LocalAddress>().unwrap().local()),
            _ => None,
        };
        let (local, local_ty) =
            if let Some(attr) = local_attr {
                let function = op
                    .nearest_parent_op::<midenc_hir::dialects::builtin::Function>()
                    .ok_or_else(|| invalid("source local has no enclosing function"))?;
                let index = attr.as_usize();
                let function_borrow = function.borrow();
                let ty =
                    function_borrow.locals().iter().nth(index).cloned().ok_or_else(|| {
                        invalid("source local index is outside the function table")
                    })?;
                if attr.is_bound() && attr.function() != function {
                    return Err(invalid("source local belongs to a different function").into());
                }
                if attr.local_type() != ty {
                    return Err(
                        invalid("source local attribute disagrees with its function type").into()
                    );
                }
                (Some(index as u32), Some(ty))
            } else {
                (None, None)
            };
        let pointee = inputs.first().and_then(Type::pointee).cloned();
        let signed = match kind {
            I32Load8S => Some((Type::I8, Type::I32)),
            I32Load16S => Some((Type::I16, Type::I32)),
            I64Load8S => Some((Type::I8, Type::I64)),
            I64Load16S => Some((Type::I16, Type::I64)),
            I64Load32S => Some((Type::I32, Type::I64)),
            _ => None,
        };
        let ty = local_ty.clone().or(pointee.clone()).unwrap_or(Type::U32);
        let valid = match kind {
            Load => {
                inputs.len() == 1
                    && pointee.as_ref() == outputs.first()
                    && outputs.len() == 1
                    && scalar(&ty)
            }
            Store => {
                inputs.len() == 2
                    && pointee.as_ref() == inputs.get(1)
                    && outputs.is_empty()
                    && storable(&ty)
            }
            LoadLocal => inputs.is_empty() && outputs == [ty.clone()] && scalar(&ty),
            StoreLocal => inputs == [ty.clone()] && outputs.is_empty() && storable(&ty),
            LocalAddress => {
                inputs.is_empty()
                    && outputs
                        == [Type::from(midenc_hir::PointerType::new_with_address_space(
                            ty.clone(),
                            midenc_hir::AddressSpace::Element,
                        ))]
            }
            MemGrow => inputs == [Type::U32] && outputs == [Type::U32],
            MemSize => inputs.is_empty() && outputs == [Type::U32],
            MemSet => {
                inputs.len() == 3
                    && inputs[1] == Type::U32
                    && pointee.as_ref() == inputs.get(2)
                    && outputs.is_empty()
                    && storable(&ty)
            }
            MemCpy | MemMove => {
                inputs.len() == 3
                    && inputs[0] == inputs[1]
                    && inputs[2] == Type::U32
                    && outputs.is_empty()
                    && matches!(&inputs[0], Type::Ptr(p) if p.is_byte_pointer())
                    && u32::try_from(ty.size_in_bytes()).is_ok()
                    && (ty.size_in_bytes() == 1
                        || scalar(&ty)
                        || (ty.size_in_bytes() >= 16 && ty.size_in_bytes().is_multiple_of(16)))
            }
            _ => signed.as_ref().is_some_and(|(src, dst)| {
                inputs.len() == 1 && pointee.as_ref() == Some(src) && outputs == [dst.clone()]
            }),
        };
        if !valid {
            return Err(invalid("unsupported memory source type, arity or local contract").into());
        }
        for t in inputs.iter().chain(&outputs) {
            ValueLayout::new(t.clone())?;
        }
        let mut stack = OperandStack::new(op.context_rc());
        for t in inputs.iter().rev() {
            stack.push(t.clone());
        }
        let mut invoked = alloc::collections::BTreeSet::new();
        let mut body = vec![];
        let span = op.span();
        {
            let mut e = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            match kind {
                Load => e.load(ty.clone(), span),
                Store => e.store(span),
                LoadLocal => {
                    e.local_address_typed(ty.clone(), 0, span);
                    e.load(ty.clone(), span);
                }
                StoreLocal => {
                    e.local_address_typed(ty.clone(), 0, span);
                    e.store(span);
                }
                LocalAddress => e.local_address_typed(ty.clone(), 0, span),
                MemGrow => e.mem_grow(span),
                MemSize => e.mem_size(span),
                MemSet => e.memset(span),
                MemCpy => e.memcpy(span),
                MemMove => e.memmove(span),
                _ => {
                    e.load(ty.clone(), span);
                    e.sext(&signed.as_ref().unwrap().1, span);
                }
            }
        }
        if stack.len() != outputs.len()
            || outputs.iter().enumerate().any(|(i, t)| stack[i].ty() != *t)
        {
            return Err(invalid("memory construction disagrees with exact result tuple").into());
        }
        let element =
            local.is_some() || matches!(inputs.first(), Some(Type::Ptr(p)) if !p.is_byte_pointer());
        let proof = memory_contract(kind, &ty, element)?;
        check_construction(&body, &proof)?;
        let mut argument_lanes = vec![];
        if kind == MemGrow {
            argument_lanes.push((0, 0, 0));
        }
        // Original store/fill lanes are copied into the final helper arguments without renaming.
        if matches!(kind, Store | StoreLocal | MemSet) {
            let start = match kind {
                Store => 1,
                StoreLocal => 0,
                _ => 2,
            };
            let width = ValueLayout::new(ty.clone())?.lanes() as usize;
            for (site, call) in proof.calls.iter().enumerate() {
                if matches!(call.instruction, masm::Instruction::Exec(ref target) if *target == raw::target("::intrinsics::mem::store_dw") || *target == raw::target("::intrinsics::mem::store_qw") || *target == raw::target("::intrinsics::mem::store_sw") || *target == raw::target("::intrinsics::mem::store_felt") || *target == raw::target("::intrinsics::mem::store_u16"))
                {
                    // Packed crossing merges construct a new value; its helper argument is local.
                    if scalar(&ty) || ty.size_in_bits() == 32 {
                        for lane in 0..width {
                            argument_lanes.push((site, 2 + lane, start + lane));
                        }
                    }
                }
            }
        }
        let direct_results =
            if matches!(kind, Load | LoadLocal | MemGrow | MemSize) && proof.calls.len() == 1 {
                Some(0)
            } else {
                None
            };
        let effects = LoweringEffects {
            memory: match kind {
                LocalAddress => SourceMemoryEffects::default(),
                Store | StoreLocal if element && scalar(&ty) && ty.size_in_bits() == 32 => {
                    SourceMemoryEffects::WRITE
                }
                Load | LoadLocal | MemSize | I32Load8S | I32Load16S | I64Load8S | I64Load16S
                | I64Load32S => SourceMemoryEffects::READ,
                _ => SourceMemoryEffects::READ_WRITE,
            },
            may_trap: !matches!(kind, LocalAddress),
            normal_return: true,
        };
        Ok(Self {
            body,
            local,
            proof,
            effects,
            argument_lanes,
            direct_results,
        })
    }

    /// Replace source-relative helper argument/result positions with exact selected lane atoms.
    /// Lane counts and ordering must match the tuple validated by `from_source`.
    pub fn bind(&mut self, inputs: &[Atom], outputs: &[Atom]) {
        for &(site, argument, lane) in &self.argument_lanes {
            self.proof.calls[site].inputs[argument] =
                CallArgument::Exact(StoredValue::Atom(inputs[lane]));
        }
        if let Some(site) = self.direct_results {
            self.proof.calls[site].publications =
                outputs.iter().enumerate().map(|(i, a)| (*a, i as u32)).collect();
        }
    }

    /// Resolve any source-local address and annotate the fixed body for this selected unit.
    /// Missing final local bindings or an invalid captured local-address prefix are errors.
    pub fn expand(
        &self,
        unit: &SelectedLowering,
        frame: &FinalFrameLayout,
    ) -> Result<AnnotatedMasmBody, Report> {
        let mut body = self.body.clone();
        if let Some(index) = self.local {
            let offset = frame.resolve(FrameRef::SourceLocal(index))?;
            let masm::Op::Inst(inst) = &mut body[0] else {
                return Err(invalid("missing source local address instruction").into());
            };
            *inst = masm::Span::new(inst.span(), masm::Instruction::Locaddr(offset.into()));
        }
        Ok(annotate_raw(&body, unit.id))
    }
}

fn scalar(ty: &midenc_hir::Type) -> bool {
    use midenc_hir::Type::*;
    matches!(
        ty,
        I1 | I8 | U8 | I16 | U16 | I32 | U32 | I64 | U64 | I128 | U128 | Felt | Ptr(_)
    )
}

fn storable(ty: &midenc_hir::Type) -> bool {
    scalar(ty)
        || (!ty.is_zst()
            && ty.size_in_bytes() <= 4
            && ValueLayout::new(ty.clone()).is_ok_and(|l| l.lanes() == 1))
}

fn call(name: &str, args: u32, results: u32, path: &[CallPath]) -> Result<CallSite, PlanError> {
    Ok(CallSite {
        instruction: masm::Instruction::Exec(raw::target(name)),
        abi: ProcedureAbi::new(CallingConvention::SameContext, args, results)?,
        inputs: vec![CallArgument::Local; args as usize],
        path: path.to_vec(),
        truncate: false,
        outputs: vec![],
        publications: vec![],
        selector: None,
    })
}

/// Exact helper targets and signatures are selected from the checked source type, never from raw code.
fn access_contract(
    ty: &midenc_hir::Type,
    store: bool,
    element: bool,
    path: &[CallPath],
    next: &mut u32,
) -> Result<Vec<CallSite>, PlanError> {
    use midenc_hir::Type;
    if element && scalar(ty) && ty.size_in_bytes() <= 4 {
        return Ok(vec![]);
    }
    let (name, args, results) = if store {
        match ty {
            Type::I128 | Type::U128 => ("store_qw", 6, 0),
            Type::I64 | Type::U64 => ("store_dw", 4, 0),
            Type::Felt => ("store_felt", 3, 0),
            _ if ty.size_in_bits() == 32 => ("store_sw", 3, 0),
            _ if ty.size_in_bits() == 16 => ("store_u16", 3, 0),
            _ if ty.size_in_bits() <= 8 => return Ok(vec![]),
            _ => {
                let mut p = path.to_vec();
                p.push(CallPath::Then(ControlId(*next)));
                *next += 1;
                return Ok(vec![
                    call("::intrinsics::mem::load_sw", 2, 1, &p)?,
                    call("::intrinsics::mem::store_sw", 3, 0, &p)?,
                ]);
            }
        }
    } else {
        match ty {
            Type::I128 | Type::U128 => ("load_qw", 2, 4),
            Type::I64 | Type::U64 => ("load_dw", 2, 2),
            Type::Felt => ("load_felt", 2, 1),
            Type::I32 | Type::U32 | Type::Ptr(_) => ("load_sw", 2, 1),
            Type::I16 | Type::U16 => ("load_u16", 2, 1),
            _ => return Ok(vec![]),
        }
    };
    Ok(vec![call(&alloc::format!("::intrinsics::mem::{name}"), args, results, path)?])
}

fn memory_contract(
    kind: Kind,
    ty: &midenc_hir::Type,
    element: bool,
) -> Result<UnitEvidence, PlanError> {
    use Kind::*;
    let mut proof = UnitEvidence::default();
    let mut next = 0;
    match kind {
        LocalAddress => {}
        MemSize | MemGrow => proof.calls.push(call(
            if kind == MemSize {
                "::intrinsics::mem::memory_size"
            } else {
                "::intrinsics::mem::memory_grow"
            },
            u32::from(kind == MemGrow),
            1,
            &[],
        )?),
        MemSet => {
            let path = [CallPath::Loop(ControlId(0))];
            next = 1;
            proof.calls = access_contract(ty, true, false, &path, &mut next)?;
            let width = ValueLayout::new(ty.clone())?.lanes() as usize;
            let mut stack = vec![LoopPosition::Invariant; 3 + width];
            stack[0] = LoopPosition::Carry(0);
            proof.loops.insert(
                ControlId(0),
                LoopSchema {
                    stack,
                    cells: vec![],
                },
            );
        }
        MemMove if ty.size_in_bytes() == 1 => {
            return Ok(ByteMemmove
                .select(
                    SelectedStepId(0),
                    SourceStepId(0),
                    &[Atom {
                        value: ValueId(0),
                        lane: 0,
                    }; 3],
                )?
                .1);
        }
        MemCpy | MemMove => {
            let size = ty.size_in_bytes();
            if size >= 16 && size.is_multiple_of(16) {
                proof.calls.push(call(
                    if kind == MemMove {
                        "::intrinsics::mem::memmove_words"
                    } else {
                        "::miden::core::mem::memcopy_words"
                    },
                    3,
                    0,
                    &[],
                )?);
            } else {
                let mut path = vec![];
                if size == 1 {
                    proof.calls.push(call(
                        "::miden::core::mem::memcopy_elements",
                        3,
                        0,
                        &[CallPath::Then(ControlId(0))],
                    )?);
                    path.push(CallPath::Else(ControlId(0)));
                    next = 1;
                }
                if kind == MemMove {
                    next += 1;
                }
                let id = ControlId(next);
                next += 1;
                path.push(CallPath::Loop(id));
                let scalar_ty = if size == 1 {
                    midenc_hir::Type::U8
                } else {
                    ty.clone()
                };
                proof.calls.extend(access_contract(&scalar_ty, false, false, &path, &mut next)?);
                proof.calls.extend(access_contract(&scalar_ty, true, false, &path, &mut next)?);
                proof.loops.insert(
                    id,
                    LoopSchema {
                        stack: vec![
                            LoopPosition::Carry(0),
                            LoopPosition::Invariant,
                            LoopPosition::Invariant,
                            LoopPosition::Invariant,
                            LoopPosition::Invariant,
                        ],
                        cells: vec![],
                    },
                );
            }
        }
        Store | StoreLocal => proof.calls = access_contract(ty, true, element, &[], &mut next)?,
        _ => proof.calls = access_contract(ty, false, element, &[], &mut next)?,
    }
    Ok(proof)
}

/// A scan checks construction against already closed source contracts; it never grants authority.
fn check_construction(body: &[masm::Op], proof: &UnitEvidence) -> Result<(), PlanError> {
    fn walk(
        body: &[masm::Op],
        next: &mut u32,
        path: &mut Vec<CallPath>,
        calls: &mut Vec<(masm::Instruction, Vec<CallPath>)>,
    ) -> Result<(), PlanError> {
        for op in body {
            match op {
                masm::Op::Inst(i) => {
                    if matches!(i.inner(), masm::Instruction::Exec(_)) {
                        calls.push((i.inner().clone(), path.clone()));
                    } else {
                        TargetEffects::instruction(i.inner())?;
                    }
                }
                masm::Op::If {
                    then_blk, else_blk, ..
                } => {
                    let id = ControlId(*next);
                    *next += 1;
                    path.push(CallPath::Then(id));
                    walk(&then_blk.iter().cloned().collect::<Vec<_>>(), next, path, calls)?;
                    path.pop();
                    path.push(CallPath::Else(id));
                    walk(&else_blk.iter().cloned().collect::<Vec<_>>(), next, path, calls)?;
                    path.pop();
                }
                masm::Op::While { body, .. } => {
                    let id = ControlId(*next);
                    *next += 1;
                    path.push(CallPath::Loop(id));
                    walk(&body.iter().cloned().collect::<Vec<_>>(), next, path, calls)?;
                    path.pop();
                }
                _ => return Err(invalid("unregistered memory control construction")),
            }
        }
        Ok(())
    }
    let mut actual = vec![];
    walk(body, &mut 0, &mut vec![], &mut actual)?;
    if actual.len() != proof.calls.len()
        || actual
            .iter()
            .zip(&proof.calls)
            .any(|((i, p), c)| *i != c.instruction || *p != c.path)
    {
        return Err(invalid(
            "memory construction disagrees with source-owned call targets, order or paths",
        ));
    }
    Ok(())
}
