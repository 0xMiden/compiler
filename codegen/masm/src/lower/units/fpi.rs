//! Foreign-procedure invocation as a stored protocol prefix plus a stack payload window.
use alloc::vec;

use midenc_hir::Report;

use super::*;
use crate::{
    lower::{expand::annotate_raw, raw},
    verify::{CallArgument, CallSite, UnitEvidence},
};

/// Closed invocation of the transaction protocol's foreign-procedure helper.
/// Six prefix lanes are loaded from stored inputs; the payload is padded to sixteen lanes.
pub(crate) struct Fpi;

impl Fpi {
    /// Declare the six prefix lanes, at most sixteen payload lanes, and exactly sixteen results.
    /// Evidence fixes the helper target and each physical argument, including padding zeros.
    pub fn select(
        &self,
        id: SelectedStepId,
        source: SourceStepId,
        prefix: [Atom; 6],
        payload: &[Atom],
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        if payload.len() > 16 || outputs.len() != 16 {
            return Err(invalid("FPI requires at most sixteen payload lanes and sixteen results"));
        }
        let stored = prefix
            .into_iter()
            .enumerate()
            .map(|(index, atom)| StoredInput {
                id: StoredInputId(index as u32),
                value: StoredValue::Atom(atom),
            })
            .collect::<alloc::vec::Vec<_>>();
        let unit = selected(self.kind(), id, source, payload, &stored, outputs);
        let abi = ProcedureAbi::new(CallingConvention::SameContext, 22, 16)?;
        let inputs = prefix
            .into_iter()
            .chain(payload.iter().copied())
            .map(|atom| CallArgument::Exact(StoredValue::Atom(atom)))
            .chain(
                (payload.len()..16)
                    .map(|_| CallArgument::Exact(StoredValue::Literal(miden_core::Felt::ZERO))),
            )
            .collect();
        let proof = UnitEvidence {
            calls: vec![CallSite {
                instruction: masm::Instruction::Exec(raw::target(
                    "::miden::protocol::tx::execute_foreign_procedure",
                )),
                abi,
                inputs,
                path: vec![],
                truncate: false,
                outputs: vec![],
                publications: outputs.iter().zip(0u32..).map(|(a, i)| (*a, i)).collect(),
                selector: None,
            }],
            ..Default::default()
        };
        Ok((unit, proof))
    }
}

impl LoweringDefinition for Fpi {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1001)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut offsets = [0; 6];
        for (index, offset) in offsets.iter_mut().enumerate() {
            *offset = frame.resolve(FrameRef::Spill(
                *bindings
                    .stored_inputs
                    .get(&StoredInputId(index as u32))
                    .ok_or_else(|| invalid("missing FPI prefix binding"))?,
            ))?;
        }
        let mut b = raw::Builder::new(Default::default());
        b.extend(raw::fpi_prefix(unit.stack_inputs.len(), offsets, b.span));
        b.exec(raw::target("::miden::protocol::tx::execute_foreign_procedure"));
        Ok(annotate_raw(&b.ops, unit.id))
    }
}
