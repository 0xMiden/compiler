//! Context-changing calls and the explicit phases of indirect dispatch.
//!
//! Selection fixes ABI and selector evidence; expansion only loads assigned inputs, stages
//! declared roots, invokes the chosen instruction, and removes VM context padding.
use alloc::vec;

use midenc_hir::{Report, Spanned};

use super::*;
use crate::{
    lower::{expand::annotate_raw, raw},
    verify::{CallArgument, CallSite, UnitEvidence},
};

/// A registered `call`, `syscall`, or `dyncall` with a source-owned ABI.
/// Dynamic calls store their four-lane root separately from the argument window.
pub(crate) struct ContextCall {
    /// Registered identity shared by the selected unit and its executable definition.
    pub kind: LoweringKind,
    /// Context-call argument/result contract, validated for the invocation form during selection.
    pub abi: ProcedureAbi,
    /// Exact source-authorized invocation, including its target for direct forms.
    pub instruction: masm::Instruction,
    /// Aligned unit-local object used to stage a dynamic root; unused by direct forms.
    pub root: ScratchId,
}
impl ContextCall {
    /// Declare the call's lanes, stored root, scratch, and exact call-site evidence.
    /// Reject unsupported invocation instructions, ABI arity mismatches, or a root supplied for
    /// anything other than `dyncall`. Dynamic root staging may trap if all four lanes are zero.
    pub fn select(
        &self,
        id: SelectedStepId,
        source: SourceStepId,
        arguments: &[Atom],
        root: Option<[Atom; 4]>,
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        let invocation = match self.instruction {
            masm::Instruction::Call(_) => Invocation::Call,
            masm::Instruction::SysCall(_) => Invocation::SysCall,
            masm::Instruction::DynCall => Invocation::DynCall,
            _ => return Err(invalid("context definition requires call, dyncall or syscall")),
        };
        let effect = self.abi.invocation(invocation)?;
        if arguments.len() != effect.arguments as usize
            || outputs.len() != effect.results as usize
            || root.is_some() != matches!(invocation, Invocation::DynCall)
        {
            return Err(invalid("context definition inputs disagree with signature"));
        }
        let stored = root
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, a)| StoredInput {
                id: StoredInputId(i as u32),
                value: StoredValue::Atom(a),
            })
            .collect::<alloc::vec::Vec<_>>();
        let mut unit = selected(self.kind, id, source, arguments, &stored, outputs);
        unit.effects.may_trap = root.is_some();
        let selector = root.map(|root| {
            unit.scratch.push(ScratchRequirement {
                id: self.root,
                size: 4,
                alignment: 4,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Unit(id),
                uses: vec![id],
            });
            for (offset, atom) in root.iter().enumerate() {
                unit.scratch_effects.push(ScratchEffect::Write(ScratchCellFact {
                    object: self.root,
                    offset: offset as u32,
                    content: ContentRequirement::Atom(*atom),
                }));
            }
            crate::verify::SelectorEvidence::FrameRoot {
                reference: FrameRef::Scratch {
                    object: self.root,
                    offset: 0,
                },
                root: root.into_iter().map(StoredValue::Atom).collect(),
            }
        });
        let proof = call_evidence(&self.instruction, self.abi, arguments, outputs, selector);
        Ok((unit, proof))
    }
}
impl LoweringDefinition for ContextCall {
    fn kind(&self) -> LoweringKind {
        self.kind
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut b = raw::Builder::new(Default::default());
        b.extend(raw::pad_inputs(unit.stack_inputs.len(), b.span));
        if matches!(self.instruction, masm::Instruction::DynCall) {
            for i in (0..4).rev() {
                let slot = *bindings
                    .stored_inputs
                    .get(&StoredInputId(i))
                    .ok_or_else(|| invalid("missing root binding"))?;
                b.emit(masm::Instruction::LocLoad(frame.resolve(FrameRef::Spill(slot))?.into()));
            }
            let offset = frame.resolve(FrameRef::Scratch {
                object: self.root,
                offset: 0,
            })?;
            b.extend(raw::stage_root(
                [masm::Instruction::Locaddr(offset.into()), masm::Instruction::MemStoreWLe],
                crate::emit::primop::UNSET_STORED_PROCEDURE_SLOT_MESSAGE,
                b.span,
            ));
            b.emit(masm::Instruction::Locaddr(offset.into()));
        }
        b.emit(self.instruction.clone());
        b.emit_all(context_result_cleanup(self.abi.results() as u8)?);
        Ok(annotate_raw(&b.ops, unit.id))
    }
}

/// Closed phase algorithms selected by final-HIR indirect-call rules.
/// Keeping lookup, root staging, and dispatch separate makes intervening storage and checks
/// visible to allocation and verification, instead of hiding them in an emitter callback.
pub(crate) enum Indirect {
    /// Check a table index and signature tag, producing the exact slot address for dispatch.
    Lookup { slots: u32, base: u32, tag: u32 },
    /// Reject an unset root and write its four lanes to an aligned spanning scratch object.
    StageRoot { object: ScratchId },
    /// Invoke a previously staged root with context padding and result cleanup.
    DispatchRoot { object: ScratchId, results: u32 },
    /// Load the previously checked table address from stored input zero and execute it.
    DispatchTable,
}
impl Indirect {
    /// Emit this phase using a source-selected contract and already validated bindings.
    /// Missing stored addresses, invalid result counts, or unresolved scratch references fail.
    pub fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        let mut b = raw::Builder::new(Default::default());
        match self {
            Self::Lookup { slots, base, tag } => {
                b.extend(raw::checked_table_address(*slots, *base, *tag, b.span))
            }
            Self::StageRoot { object } => {
                let offset = frame.resolve(FrameRef::Scratch {
                    object: *object,
                    offset: 0,
                })?;
                b.extend(raw::stage_root(
                    [masm::Instruction::Locaddr(offset.into()), masm::Instruction::MemStoreWLe],
                    crate::emit::primop::UNSET_STORED_PROCEDURE_SLOT_MESSAGE,
                    b.span,
                ));
            }
            Self::DispatchRoot { object, results } => {
                b.extend(raw::pad_inputs(unit.stack_inputs.len(), b.span));
                b.emit(masm::Instruction::Locaddr(
                    frame
                        .resolve(FrameRef::Scratch {
                            object: *object,
                            offset: 0,
                        })?
                        .into(),
                ));
                b.emit(masm::Instruction::DynCall);
                b.emit_all(context_result_cleanup(
                    u8::try_from(*results).map_err(|_| invalid("context results exceed window"))?,
                )?);
            }
            Self::DispatchTable => {
                let slot = bindings
                    .stored_inputs
                    .get(&StoredInputId(0))
                    .ok_or_else(|| invalid("missing checked table address"))?;
                b.emit(masm::Instruction::LocLoad(frame.resolve(FrameRef::Spill(*slot))?.into()));
                b.emit(masm::Instruction::EmitImm(crate::Event::FrameStart.into()));
                b.emit(masm::Instruction::DynExec);
                b.emit(masm::Instruction::EmitImm(crate::Event::FrameEnd.into()));
            }
        }
        Ok(annotate_raw(&b.ops, unit.id))
    }
}

/// A resolved direct invocation. Context calls share the window construction used by dynamic calls.
pub(crate) struct Direct {
    /// Canonical target signature and invocation convention.
    pub abi: ProcedureAbi,
    /// Accessible source name retained independently of the canonical target identity.
    pub instruction: masm::Instruction,
}
impl Direct {
    /// Declare exact argument and result lanes, rejecting incompatible invocation families.
    pub fn select(
        &self,
        kind: LoweringKind,
        id: SelectedStepId,
        source: SourceStepId,
        arguments: &[Atom],
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        if !matches!(self.instruction, masm::Instruction::Exec(_)) {
            return ContextCall {
                kind,
                abi: self.abi,
                instruction: self.instruction.clone(),
                root: ScratchId(0),
            }
            .select(id, source, arguments, None, outputs);
        }
        self.abi.invocation(Invocation::Exec)?;
        if arguments.len() != self.abi.arguments() as usize
            || outputs.len() != self.abi.results() as usize
        {
            return Err(invalid("direct inputs disagree with signature"));
        }
        let unit = selected(kind, id, source, arguments, &[], outputs);
        let proof = call_evidence(&self.instruction, self.abi, arguments, outputs, None);
        Ok((unit, proof))
    }

    /// Expand using the already bound call contract; no signature inference occurs here.
    pub fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        env: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        if !matches!(self.instruction, masm::Instruction::Exec(_)) {
            return ContextCall {
                kind: unit.kind,
                abi: self.abi,
                instruction: self.instruction.clone(),
                root: ScratchId(0),
            }
            .expand(unit, bindings, frame, env);
        }
        let mut body = AnnotatedMasmBody::new();
        body.push_instruction(masm::Instruction::EmitImm(crate::Event::FrameStart.into()), unit.id);
        body.push_instruction(self.instruction.clone(), unit.id);
        body.push_instruction(masm::Instruction::EmitImm(crate::Event::FrameEnd.into()), unit.id);
        Ok(body)
    }
}

/// Resolve canonical analysis identity while retaining the accessible emission name.
pub(crate) fn resolve_direct(
    call: &dyn midenc_hir::CallOpInterface,
    env: &crate::select::SelectEnv<'_>,
) -> Result<(midenc_hir::ResolvedSymbolCallee, masm::InvocationTarget), Report> {
    let resolved = super::super::lowering::resolve_invocation_callee(call)?;
    let target = super::super::lowering::invocation_target_from_symbol(
        resolved.named_symbol(),
        if call
            .as_operation()
            .nearest_parent_op::<midenc_hir::dialects::builtin::GlobalVariable>()
            .is_some()
        {
            None
        } else {
            call.as_operation().nearest_symbol_table()
        },
        call.as_operation().span(),
    );
    let target = if let Some(owners) = env.owners {
        owners.target(target)?
    } else {
        target
    };
    Ok((resolved, target))
}

/// Kernel procedures belong to the exact root kernel module, independently of source convention.
pub(crate) fn is_kernel(symbol: &dyn midenc_hir::Symbol) -> bool {
    super::super::lowering::symbol_module_path(symbol).is_kernel_path()
}

/// The sole depth-sensitive external helper in the supported core ABI.
pub(crate) fn is_truncate(symbol: &dyn midenc_hir::Symbol) -> bool {
    super::super::lowering::qualified_invocation_target(symbol, Default::default())
        == raw::target("::miden::core::sys::truncate_stack")
}

/// Build the single exact contract shared by direct and staged context constructions.
fn call_evidence(
    instruction: &masm::Instruction,
    abi: ProcedureAbi,
    arguments: &[Atom],
    outputs: &[Atom],
    selector: Option<crate::verify::SelectorEvidence>,
) -> UnitEvidence {
    UnitEvidence {
        calls: vec![CallSite {
            instruction: instruction.clone(),
            abi,
            inputs: arguments.iter().map(|a| CallArgument::Exact(StoredValue::Atom(*a))).collect(),
            path: vec![],
            truncate: false,
            outputs: vec![],
            publications: outputs.iter().copied().zip(0u32..).collect(),
            selector,
        }],
        ..Default::default()
    }
}
