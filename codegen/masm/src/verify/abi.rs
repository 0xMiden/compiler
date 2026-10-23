//! Replay source-registered invocations, including selector provenance and full result tuples.
use alloc::vec::Vec;

use super::{
    replay::{Cursor, Verifier},
    stack::State,
    *,
};
use crate::masm::Instruction;

impl Verifier<'_> {
    /// Match the next call on the active internal path, prove its selector and argument
    /// exposure, and apply its context/storage effects while retaining result obligations.
    pub(super) fn call(
        &mut self,
        instruction: &Instruction,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<(), VerificationError> {
        let active = cursor
            .active
            .as_mut()
            .ok_or_else(|| error("invocation outside registered unit"))?;
        let (site_id, site) = self
            .plan
            .selected
            .evidence(active.id)?
            .calls
            .iter()
            .enumerate()
            .filter(|(_, site)| site.path == active.path)
            .nth(active.call)
            .ok_or_else(|| fail(DiagnosticCategory::AbiMismatch, "unregistered invocation"))?;
        let site = site.clone();
        if *instruction != site.instruction {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "invocation target or family disagrees with registered summary",
            ));
        }
        if site.truncate {
            let boundary = match self.plan.selected.abi.convention() {
                CallingConvention::Root => ReturnBoundary::Root,
                CallingConvention::Context | CallingConvention::Kernel => ReturnBoundary::Context,
                CallingConvention::SameContext => ReturnBoundary::SameContext,
            };
            boundary.validate_truncation()?;
            state.stack.truncate(16);
            state.padding = stack::Padding::new(state.stack.len());
            active.call += 1;
            return Ok(());
        }
        let invocation = match instruction {
            Instruction::Exec(_) => Invocation::Exec,
            Instruction::DynExec => Invocation::DynExec,
            Instruction::Call(_) => Invocation::Call,
            Instruction::DynCall => Invocation::DynCall,
            Instruction::SysCall(_) => Invocation::SysCall,
            _ => return Err(error("non-call instruction in invocation rule")),
        };
        let effect = site.abi.invocation(invocation)?;
        let unit = self.unit(active.id)?.clone();
        if site.inputs.len() != effect.arguments as usize {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "call inputs disagree with target signature",
            ));
        }
        if effect.selector_lanes != 0 {
            let token = state.pop()?;
            let selector = site.selector.as_ref().ok_or_else(|| {
                fail(
                    DiagnosticCategory::AbiMismatch,
                    "dynamic invocation has no registered root binding",
                )
            })?;
            match selector {
                super::SelectorEvidence::FrameRoot {
                    reference,
                    root: required,
                } => {
                    let offset = u32::from(self.frame.resolve(*reference)?);
                    if self.tokens.frame_address(token) != Some(offset) || required.len() != 4 {
                        return Err(fail(
                            DiagnosticCategory::AbiMismatch,
                            "dynamic selector has wrong address provenance",
                        ));
                    }
                    for (lane, value) in required.iter().enumerate() {
                        let expected = memory::stored_token(state, *value, &mut self.tokens)?;
                        if memory::read(state, offset + lane as u32, self.frame)? != expected {
                            return Err(fail(
                                DiagnosticCategory::AbiMismatch,
                                "dynamic root contents are stale or incorrect",
                            ));
                        }
                    }
                }
                super::SelectorEvidence::CheckedTableSlot(lookup) => {
                    if invocation != Invocation::DynExec || state.token(lookup.address)? != token {
                        return Err(fail(
                            DiagnosticCategory::AbiMismatch,
                            "dynamic invocation consumed wrong checked table address",
                        ));
                    }
                }
            }
        }
        let order = self.plan.lowering_bindings[&active.id].input_order;
        let ordered_inputs = self
            .plan
            .selected
            .input_symmetry(active.id)?
            .ordered_inputs(&unit.stack_inputs, order)?;
        let actual = state.prefix(site.inputs.len())?.to_vec();
        for (input, token) in site.inputs.iter().zip(actual) {
            if let CallArgument::Exact(value) = input {
                // Project exact source references through the same whole-value exchange as
                // the entry prefix. Helper argument vectors may interleave the source limbs.
                let value = match *value {
                    StoredValue::Atom(atom) => unit
                        .stack_inputs
                        .iter()
                        .position(|input| *input == atom)
                        .map(|index| StoredValue::Atom(ordered_inputs[index]))
                        .unwrap_or(*value),
                    _ => *value,
                };
                if memory::stored_token(state, value, &mut self.tokens)? != token {
                    return Err(fail(
                        DiagnosticCategory::AbiMismatch,
                        "actual call argument differs from registered identity",
                    ));
                }
            }
            if self.tokens.origins(token).compiler() {
                let address = self.tokens.frame_address(token).ok_or_else(|| {
                    fail(
                        DiagnosticCategory::AbiMismatch,
                        "compiler call argument has unresolved provenance",
                    )
                })?;
                let borrowed = unit.scratch_effects.iter().any(|effect| {
                    let ScratchEffect::Borrow {
                        object,
                        offset,
                        size,
                        retained: false,
                        ..
                    } = effect
                    else {
                        return false;
                    };
                    self.frame
                        .resolve(FrameRef::Scratch {
                            object: *object,
                            offset: *offset,
                        })
                        .is_ok_and(|start| {
                            let start = u32::from(start);
                            (start..start + size).contains(&address)
                        })
                });
                if !borrowed {
                    return Err(fail(
                        DiagnosticCategory::AbiMismatch,
                        "compiler address escapes without a declared nonretaining borrow",
                    ));
                }
            }
        }
        if effect.window.is_some() {
            let window = state.prefix(16)?;
            if window[effect.arguments as usize..]
                .iter()
                .any(|token| self.tokens.constant(*token) != Some(0))
            {
                return Err(fail(
                    DiagnosticCategory::AbiMismatch,
                    "context-call padding is not explicit zero",
                ));
            }
        }
        let shared_memory = effect.memory == CallMemory::SameContext
            || effect.memory == CallMemory::Root
                && matches!(
                    self.plan.selected.abi.convention(),
                    CallingConvention::Root | CallingConvention::Kernel
                );
        if !shared_memory
            && unit.scratch_effects.iter().any(|effect| {
                matches!(
                    effect,
                    ScratchEffect::Borrow { read: true, .. }
                        | ScratchEffect::Borrow { write: true, .. }
                )
            })
        {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "borrowed compiler scratch is not in the callee memory context",
            ));
        }
        for scratch in &unit.scratch_effects {
            match *scratch {
                ScratchEffect::Borrow {
                    object,
                    offset,
                    size,
                    read,
                    write,
                    definite_write,
                    retained,
                } => {
                    if retained {
                        return Err(error("callee retains compiler scratch address"));
                    }
                    let range = memory::object_range(object, offset, size, self.frame)?;
                    for cell in range.clone() {
                        if read {
                            memory::read(state, cell, self.frame)?;
                        }
                        if write && active.readonly.contains_key(&cell) {
                            return Err(fail(
                                DiagnosticCategory::InvalidSpillSlotRead,
                                "call writes stored input",
                            ));
                        }
                    }
                    if write {
                        memory::invalidate(state, range, definite_write, &mut self.tokens);
                    }
                }
                ScratchEffect::PossibleWrite {
                    object,
                    offset,
                    size,
                } => {
                    let range = memory::object_range(object, offset, size, self.frame)?;
                    if range.clone().any(|cell| active.readonly.contains_key(&cell)) {
                        return Err(fail(
                            DiagnosticCategory::InvalidSpillSlotRead,
                            "call writes stored input",
                        ));
                    }
                    memory::invalidate(state, range, false, &mut self.tokens);
                }
                _ => {}
            }
        }
        let results = (0..effect.results).map(|_| self.tokens.fresh()).collect::<Vec<_>>();
        if effect.window.is_some() {
            let mut window = results.clone();
            while window.len() < 16 {
                window.push(self.tokens.fresh());
            }
            state.replace(16, window, &[])?;
        } else {
            let suffix = state.stack.len() - effect.arguments as usize;
            let mut padding = state.padding.clone();
            for (tail, bits) in padding.0.iter_mut().enumerate() {
                let maximum = 16usize.saturating_sub(suffix + tail);
                let previous = *bits;
                *bits = 0;
                for amount in 0..=16 {
                    if previous & (1 << amount) != 0 {
                        for next in amount..=amount.max(maximum) {
                            *bits |= 1 << next;
                        }
                    }
                }
            }
            state.stack.splice(..effect.arguments as usize, results.iter().copied());
            state.padding = padding;
        }
        let mut publications = alloc::collections::BTreeSet::new();
        for (atom, index) in &site.publications {
            if !unit.outputs.contains(atom) || !publications.insert(*atom) {
                return Err(error("call publication names undeclared result"));
            }
            let token = results[super::result_index(*index, results.len())?];
            state
                .results
                .insert((*atom, site_id), super::results::Requirement::equal(token));
        }
        for (reference, result) in &site.outputs {
            let token = results[super::result_index(*result, results.len())?];
            let offset = u32::from(self.frame.resolve(*reference)?);
            let declared = unit.scratch_effects.iter().any(|effect| match (reference, effect) {
                (
                    FrameRef::Scratch { object, offset },
                    ScratchEffect::Borrow {
                        object: borrowed,
                        offset: start,
                        size,
                        write: true,
                        ..
                    },
                ) => object == borrowed && (*start..start + size).contains(offset),
                _ => false,
            });
            if !declared {
                return Err(error("call output relation lacks writable borrow"));
            }
            state.cells.insert(offset, token);
            state.written.insert(offset);
        }
        active.call += 1;
        Ok(())
    }
}
