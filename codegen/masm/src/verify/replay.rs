//! Replay physical actions and lowering instructions with independent value and memory state.
//! Boundary markers constrain the action cursor; they never establish executable effects alone.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use super::{
    liveness::{Live, Liveness},
    stack::State,
    tokens::{Identity, Token, Tokens},
    *,
};
use crate::{
    lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp, Boundary},
    masm::{Immediate, Instruction},
    plan::actions::{PlanAction, ProcedurePlan},
};

/// State needed to check one unit's normal publication against its entry and registered calls.
#[derive(Clone)]
pub(super) struct ActiveUnit {
    pub id: SelectedStepId,
    /// Snapshot used to prove preservation of the unit's untouched suffix and inputs.
    pub incoming: State,
    /// Protected initialized cells whose identity must survive the unit.
    pub readonly: BTreeMap<u32, Token>,
    /// Next call ordinal within the current internal path.
    pub call: usize,
    pub path: Vec<CallPath>,
}

/// Position in the expected segment/edge action stream and any currently executing unit.
#[derive(Clone, Default)]
pub(super) struct Cursor {
    pub segment: Option<SegmentId>,
    pub edge: Option<EdgeId>,
    pub action: usize,
    /// Index of the next selected unit to begin in the current segment.
    pub phase: usize,
    pub active: Option<ActiveUnit>,
}

/// Ephemeral evidence about this execution's actual paired bounds, never a token capability.
struct ExactSourceInterval {
    start: u32,
    width: u32,
}

/// One verification run borrowing immutable claims and owning independent proof state.
/// Its liveness and scratch demand are recomputed from sealed source semantics.
pub(super) struct Verifier<'a> {
    pub plan: &'a ProcedurePlan,
    pub frame: &'a FinalFrameLayout,
    pub tokens: Tokens,
    pub live: Liveness,
    pub storage: super::storage::StorageDemand,
}

impl<'a> Verifier<'a> {
    /// Replay one internal control arm with a path-local call ordinal, then require complete
    /// registered call coverage on normal completion before restoring the enclosing path.
    pub(super) fn execute_path(
        &mut self,
        body: &AnnotatedMasmBody,
        state: State,
        cursor: &mut Cursor,
        path: CallPath,
    ) -> Result<Option<State>, VerificationError> {
        let previous = cursor.active.as_ref().map(|active| active.call);
        if let Some(active) = &mut cursor.active {
            active.path.push(path);
            active.call = 0;
        }
        let result = self.execute(body, state, cursor)?;
        if let Some(previous) = previous {
            let active = cursor
                .active
                .as_mut()
                .ok_or_else(|| error("internal control crossed unit completion"))?;
            let required = self
                .plan
                .selected
                .evidence(active.id)?
                .calls
                .iter()
                .filter(|site| site.path == active.path)
                .count();
            if result.is_some() && active.call != required {
                return Err(fail(
                    DiagnosticCategory::AbiMismatch,
                    "normal control arm omitted registered call site",
                ));
            }
            active.path.pop();
            active.call = previous;
        }
        Ok(result)
    }

    fn check_call_sites(&self, body: &AnnotatedMasmBody) -> Result<(), VerificationError> {
        type Counts = BTreeMap<(SelectedStepId, Vec<CallPath>), usize>;
        fn scan(
            verifier: &Verifier<'_>,
            body: &AnnotatedMasmBody,
            mut active: Option<SelectedStepId>,
            path: &mut Vec<CallPath>,
            counts: &mut Counts,
        ) -> Result<(), VerificationError> {
            for op in body.ops() {
                match op {
                    AnnotatedOp::Boundary(Boundary::UnitStart(id)) => {
                        active = Some(*id);
                        path.clear();
                    }
                    AnnotatedOp::Boundary(Boundary::UnitEnd(_)) => {
                        active = None;
                        path.clear();
                    }
                    AnnotatedOp::Instruction { instruction, unit }
                        if matches!(
                            instruction,
                            Instruction::Exec(_)
                                | Instruction::Call(_)
                                | Instruction::SysCall(_)
                                | Instruction::DynExec
                                | Instruction::DynCall
                        ) =>
                    {
                        (|| {
                            let id = active
                                .filter(|id| id == unit)
                                .ok_or_else(|| error("call outside its registered unit"))?;
                            let index = counts.entry((id, path.clone())).or_default();
                            let site = verifier
                                .plan
                                .selected
                                .evidence(id)?
                                .calls
                                .iter()
                                .filter(|site| site.path == *path)
                                .nth(*index)
                                .ok_or_else(|| {
                                    fail(
                                        DiagnosticCategory::AbiMismatch,
                                        "unregistered call occurrence/control path",
                                    )
                                })?;
                            if site.instruction != *instruction {
                                return Err(fail(
                                    DiagnosticCategory::AbiMismatch,
                                    "actual call site differs from registered target/family",
                                ));
                            }
                            *index += 1;
                            Ok(())
                        })()
                        .map_err(|error| verifier.context(error, active))?;
                    }
                    AnnotatedOp::If {
                        id,
                        then_body,
                        else_body,
                    } => {
                        for (part, body) in
                            [(CallPath::Then(*id), then_body), (CallPath::Else(*id), else_body)]
                        {
                            let mut child = path.clone();
                            if active.is_some() {
                                child.push(part);
                            }
                            scan(verifier, body, active, &mut child, counts)?;
                        }
                    }
                    AnnotatedOp::While { id, body } | AnnotatedOp::Repeat { id, body, .. } => {
                        let mut child = path.clone();
                        if active.is_some() {
                            child.push(CallPath::Loop(*id));
                        }
                        scan(verifier, body, active, &mut child, counts)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        let mut counts = Counts::new();
        scan(self, body, None, &mut Vec::new(), &mut counts)?;
        for segment in &self.plan.selected.segments {
            for unit in &segment.units {
                let mut expected = BTreeMap::<Vec<CallPath>, usize>::new();
                for site in &self.plan.selected.evidence(unit.id)?.calls {
                    *expected.entry(site.path.clone()).or_default() += 1;
                }
                for (path, count) in expected {
                    if counts.get(&(unit.id, path)).copied().unwrap_or(0) != count {
                        return Err(self.context(
                            fail(
                                DiagnosticCategory::AbiMismatch,
                                "actual body omits required registered call site",
                            ),
                            Some(unit.id),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate plan/frame domains, source layout, entry-contract references, and lowering
    /// bindings before replay can use them as hypotheses. This does not yet prove the body.
    pub fn new(
        plan: &'a ProcedurePlan,
        frame: &'a FinalFrameLayout,
    ) -> Result<Self, VerificationError> {
        if plan.selected.abi.convention() == CallingConvention::Root
            && (plan.selected.abi.arguments() != 16 || plan.selected.abi.results() != 16)
        {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "root entry and exit must model all sixteen VM lanes",
            ));
        }
        if frame.source() != plan.selected.source_locals() {
            return Err(fail(
                DiagnosticCategory::FrameCapacityExceeded,
                "final source-local layout disagrees with checked source",
            ));
        }
        let declared: BTreeSet<_> = plan.spill_slots.iter().copied().collect();
        if declared.len() != plan.spill_slots.len()
            || declared != frame.spills().keys().copied().collect()
        {
            return Err(error("spill domain disagrees with final frame"));
        }
        let scratch: BTreeSet<_> = plan
            .selected
            .segments
            .iter()
            .flat_map(|segment| &segment.units)
            .flat_map(|unit| &unit.scratch)
            .map(|object| object.id)
            .collect();
        if scratch != frame.scratch().keys().copied().collect() {
            return Err(error("scratch domain disagrees with final frame"));
        }
        if plan.contracts.len() != plan.selected.segments.len() {
            return Err(error("entry contract domain disagrees with selected segments"));
        }
        for contract in plan.contracts.values() {
            for object in contract
                .required_scratch
                .iter()
                .map(|fact| fact.object)
                .chain(contract.address_facts.iter().map(|fact| fact.object))
            {
                if frame
                    .scratch()
                    .get(&object)
                    .is_none_or(|allocation| allocation.lifetime != ScratchLifetime::Spanning)
                {
                    return Err(fail(
                        DiagnosticCategory::InvalidEdgeTransfer,
                        "unit-local scratch fact crosses a selected boundary",
                    ));
                }
            }

            for atom in contract
                .stack_top_first
                .iter()
                .chain(contract.required_spill_slots.iter().map(|(_, atom)| atom))
                .chain(contract.aliases.pairs().iter().flat_map(|(a, b)| [a, b]))
                .chain(contract.required_scratch.iter().filter_map(|fact| match &fact.content {
                    ContentRequirement::Atom(atom) => Some(atom),
                    _ => None,
                }))
            {
                selection::validate_atom(&plan.selected, *atom)?;
            }
            for address in &contract.address_facts {
                selection::validate_atom(&plan.selected, Atom::new(address.pointer, 0))?;
            }
        }
        for segment in &plan.selected.segments {
            if !plan.contracts.contains_key(&segment.id)
                || !plan.segment_actions.contains_key(&segment.id)
            {
                return Err(error("missing segment contract or actions"));
            }
            for unit in &segment.units {
                plan.lowering_bindings
                    .get(&unit.id)
                    .ok_or_else(|| error("missing lowering bindings"))?
                    .validate(unit, frame)?;
            }
        }
        Ok(Self {
            plan,
            frame,
            tokens: Tokens::new(frame.source().extent),
            live: Liveness::derive(&plan.selected),
            storage: super::storage::StorageDemand::derive(&plan.selected),
        })
    }

    /// Check control/call coverage and discharge physical graph obligations.
    pub fn verify(mut self, body: &AnnotatedMasmBody) -> Result<(), VerificationError> {
        control::check_projection(&self.plan.selected.structure, body)?;
        self.check_call_sites(body)?;
        self.verify_graph(body)
    }

    /// Resolve a selected step or reject an expanded reference to an unknown unit.
    pub fn unit(&self, id: SelectedStepId) -> Result<&SelectedLowering, VerificationError> {
        self.plan
            .selected
            .segments
            .iter()
            .flat_map(|segment| &segment.units)
            .find(|unit| unit.id == id)
            .ok_or_else(|| error("unknown lowering step"))
    }

    fn actions(&self, cursor: &Cursor) -> &[PlanAction] {
        if let Some(edge) = cursor.edge {
            self.plan.edge_actions.get(&edge).map_or(&[], Vec::as_slice)
        } else {
            cursor
                .segment
                .and_then(|segment| self.plan.segment_actions.get(&segment))
                .map_or(&[], Vec::as_slice)
        }
    }

    /// Require complete consumption of the current owner's planned action stream.
    pub(super) fn finish_actions(&self, cursor: &mut Cursor) -> Result<(), VerificationError> {
        if cursor.action != self.actions(cursor).len() {
            return Err(error("emitted body omits planned action or phase"));
        }
        Ok(())
    }

    /// Replay an annotated body from the supplied raw state. `None` denotes a path with
    /// no normal continuation; markers and instructions must match the pending action phases.
    pub fn execute(
        &mut self,
        body: &AnnotatedMasmBody,
        mut state: State,
        cursor: &mut Cursor,
    ) -> Result<Option<State>, VerificationError> {
        for op in body.ops() {
            // UnitEnd consumes the active unit, so retain the boundary identity first.
            let step = cursor.active.as_ref().map(|active| active.id).or(match op {
                AnnotatedOp::Boundary(Boundary::UnitStart(id) | Boundary::UnitEnd(id)) => Some(*id),
                _ => None,
            });
            // Preparation belongs to the next selected unit, but transfers and trailing
            // cleanup do not. Capture only the cursor here; resolve the unit on failure.
            let pending = (step.is_none()
                && cursor.edge.is_none()
                && matches!(op, AnnotatedOp::Instruction { .. }))
            .then_some((cursor.segment, cursor.phase));
            state = match (|| -> Result<Option<State>, VerificationError> {
                match op {
                    AnnotatedOp::Anchor(Anchor::Nop) => {}
                    AnnotatedOp::Anchor(Anchor::Debug { id, .. }) => {
                        if cursor.active.is_some()
                            || self.actions(cursor).get(cursor.action)
                                != Some(&PlanAction::DebugLocation(*id))
                        {
                            return Err(error(
                                "debug anchor differs from exact pending debug action",
                            ));
                        }
                        cursor.action += 1;
                    }
                    AnnotatedOp::Boundary(boundary) => {
                        self.boundary(*boundary, &mut state, cursor)?
                    }
                    AnnotatedOp::Instruction { instruction, unit } => {
                        if let Some(active) = &cursor.active {
                            if *unit != active.id {
                                return Err(error("instruction belongs to wrong lowering unit"));
                            }
                            if !self.instruction(instruction, &mut state, cursor)? {
                                return Ok(None);
                            }
                        } else {
                            self.action(instruction, &mut state, cursor)?;
                        }
                    }
                    AnnotatedOp::If {
                        id,
                        then_body,
                        else_body,
                    } => {
                        let predicate = state.pop()?;
                        if cursor.active.is_none() {
                            return Err(error(
                                "selected control must be checked through graph contracts",
                            ));
                        }
                        let known = self.tokens.constant(predicate);
                        if known.is_some_and(|value| value > 1) {
                            return Err(fail(
                                DiagnosticCategory::IllegalStackAccess,
                                "nonboolean branch predicate",
                            ));
                        }
                        let mut then_cursor = cursor.clone();
                        let mut else_cursor = cursor.clone();
                        let then_state = if known != Some(0) {
                            self.execute_path(
                                then_body,
                                state.clone(),
                                &mut then_cursor,
                                CallPath::Then(*id),
                            )?
                        } else {
                            None
                        };
                        let else_state = if known != Some(1) {
                            self.execute_path(
                                else_body,
                                state.clone(),
                                &mut else_cursor,
                                CallPath::Else(*id),
                            )?
                        } else {
                            None
                        };
                        match (then_state, else_state) {
                            (Some(a), Some(b)) => {
                                self.match_cursors(&then_cursor, &else_cursor)?;
                                state = a.merge(&b, *id, &mut self.tokens)?;
                                *cursor = then_cursor;
                            }
                            (Some(a), None) => {
                                state = a;
                                *cursor = then_cursor;
                            }
                            (None, Some(b)) => {
                                state = b;
                                *cursor = else_cursor;
                            }
                            (None, None) => return Ok(None),
                        }
                    }
                    AnnotatedOp::While { id, body } => {
                        let predicate = state.pop()?;
                        if self.tokens.constant(predicate) != Some(0) {
                            state = self.loop_body(*id, body, state, cursor, true)?;
                        }
                    }
                    AnnotatedOp::Repeat { id, count, body } => {
                        if *count != 0 {
                            state = self.loop_body(*id, body, state, cursor, false)?;
                        }
                    }
                }
                Ok(Some(state))
            })()
            .map_err(|error| {
                let step = step.or_else(|| {
                    let (segment, phase) = pending?;
                    self.plan
                        .selected
                        .segments
                        .iter()
                        .find(|candidate| Some(candidate.id) == segment)?
                        .units
                        .get(phase)
                        .map(|unit| unit.id)
                });
                self.context(error, step)
            })? {
                Some(state) => state,
                None => return Ok(None),
            };
        }
        Ok(Some(state))
    }

    /// Preserve inner diagnostic context while attaching the enclosing selected boundary.
    fn context(&self, error: VerificationError, step: Option<SelectedStepId>) -> VerificationError {
        let Some(step) = step else {
            return error;
        };
        let location = self
            .unit(step)
            .ok()
            .and_then(|unit| self.plan.selected.source_map.get(&unit.source))
            .map(|record| &record.location);
        error.at_step(step, location)
    }

    /// Tokens can have several names; choose the first established atom deterministically.
    fn actual_atom(state: &State, token: Token) -> Option<Atom> {
        state.names.iter().find_map(|(atom, known)| (*known == token).then_some(*atom))
    }

    fn match_cursors(&self, a: &Cursor, b: &Cursor) -> Result<(), VerificationError> {
        if a.segment != b.segment
            || a.edge != b.edge
            || a.action != b.action
            || a.phase != b.phase
            || a.active.as_ref().map(|unit| (unit.id, unit.call))
                != b.active.as_ref().map(|unit| (unit.id, unit.call))
        {
            return Err(error("normal arms disagree on selected phase boundary"));
        }
        Ok(())
    }

    fn boundary(
        &mut self,
        boundary: Boundary,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<(), VerificationError> {
        match boundary {
            Boundary::Segment(id) => {
                if cursor.active.is_some() {
                    return Err(error("segment inside uninterrupted lowering unit"));
                }
                self.finish_actions(cursor)?;
                if cursor.segment.is_some() || cursor.edge.is_some() {
                    return Err(error("segment replay crossed its contract boundary"));
                }
                self.check_contract(id, state)?;
                cursor.segment = Some(id);
                cursor.edge = None;
                cursor.action = 0;
                cursor.phase = 0;
                self.available(state, &self.live.entry[&id])?;
                self.storage_available(state, &self.storage.entry[&id])?;
            }
            Boundary::Edge(id) => {
                if cursor.active.is_some() {
                    return Err(error("edge inside lowering unit"));
                }
                self.finish_actions(cursor)?;
                let edge = self
                    .plan
                    .selected
                    .edges
                    .iter()
                    .find(|edge| edge.id == id)
                    .ok_or_else(|| error("unknown edge"))?;
                if cursor.segment != Some(edge.source) {
                    return Err(error("edge source disagrees with executed segment"));
                }
                let segment = self
                    .plan
                    .selected
                    .segments
                    .iter()
                    .find(|segment| Some(segment.id) == cursor.segment)
                    .unwrap();
                if cursor.phase != segment.units.len() {
                    return Err(error("selected segment phases omitted"));
                }
                cursor.edge = Some(id);
                cursor.action = 0;
            }
            Boundary::UnitStart(id) => self.start_unit(id, state, cursor)?,
            Boundary::UnitEnd(id) => self.end_unit(id, state, cursor)?,
            Boundary::Control(_) => return Err(error("unbound control marker")),
        }
        Ok(())
    }

    fn start_unit(
        &mut self,
        id: SelectedStepId,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<(), VerificationError> {
        if cursor.active.is_some() || cursor.edge.is_some() {
            return Err(error("nested or edge-local lowering publication"));
        }
        if self.actions(cursor).get(cursor.action) != Some(&PlanAction::RunLowering { step: id }) {
            return Err(error("unit boundary disagrees with planned phase order"));
        }
        let segment = self
            .plan
            .selected
            .segments
            .iter()
            .find(|segment| Some(segment.id) == cursor.segment)
            .ok_or_else(|| error("lowering before segment"))?;
        if segment.units.get(cursor.phase).map(|unit| unit.id) != Some(id) {
            return Err(error("duplicate or reordered executable phase"));
        }
        let unit = self.unit(id)?.clone();
        let bindings = &self.plan.lowering_bindings[&id];
        let ordered = self
            .plan
            .selected
            .input_symmetry(id)?
            .ordered_inputs(&unit.stack_inputs, bindings.input_order)?;
        let expected = ordered
            .iter()
            .map(|atom| state.token(*atom).map_err(|error| error.expecting(*atom)))
            .collect::<Result<Vec<_>, _>>()?;
        for (depth, (atom, expected)) in ordered.iter().zip(&expected).enumerate() {
            let actual = state.stack.get(depth).copied();
            if actual != Some(*expected) {
                return Err(fail(
                    DiagnosticCategory::UnavailableValue,
                    "actual lowering inputs differ from selected atoms",
                )
                .mismatch(
                    depth,
                    *atom,
                    actual.and_then(|token| Self::actual_atom(state, token)),
                ));
            }
        }
        let mut readonly = BTreeMap::new();
        for input in &unit.stored_inputs {
            let offset =
                self.frame.resolve(FrameRef::Spill(bindings.stored_inputs[&input.id]))?.into();
            let expected = memory::stored_token(state, input.value, &mut self.tokens)?;
            if memory::read(state, offset, self.frame)? != expected {
                return Err(fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "stored input contains wrong or stale token",
                ));
            }
            readonly.insert(offset, expected);
        }
        for requirement in &unit.scratch {
            if matches!(requirement.lifetime, ScratchLifetime::Unit(_)) {
                for offset in memory::object_range(requirement.id, 0, requirement.size, self.frame)?
                {
                    state.cells.remove(&offset);
                }
            }
        }
        for effect in &unit.scratch_effects {
            if let ScratchEffect::Read(fact) = effect {
                memory::check_fact(state, *fact, self.frame, &state.contents)?;
            }
            if matches!(effect, ScratchEffect::Borrow { retained: true, .. }) {
                return Err(error("scratch address retention is forbidden"));
            }
        }
        self.available(state, &self.live.before[&id])?;
        self.storage_available(state, &self.storage.before[&id])?;
        cursor.action += 1;
        state.written.clear();
        state.results.clear();
        cursor.active = Some(ActiveUnit {
            id,
            incoming: state.clone(),
            readonly,
            call: 0,
            path: Vec::new(),
        });
        Ok(())
    }

    fn end_unit(
        &mut self,
        id: SelectedStepId,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<(), VerificationError> {
        let active = cursor
            .active
            .take()
            .filter(|active| active.id == id)
            .ok_or_else(|| error("unmatched normal completion"))?;
        let unit = self.unit(id)?.clone();
        let evidence = self.plan.selected.evidence(id)?;
        if !unit.effects.normal_return {
            return Err(error("nonreturning lowering reaches publication"));
        }
        if active.call != evidence.calls.iter().filter(|site| site.path.is_empty()).count() {
            return Err(error("required registered invocation omitted"));
        }
        let outputs = match unit.publication {
            OutputPublication::Stack => {
                let suffix = &active.incoming.stack[unit.stack_inputs.len()..];
                if state.stack.get(unit.outputs.len()..) != Some(suffix) {
                    return Err(fail(
                        DiagnosticCategory::UnavailableValue,
                        "lowering changed preserved suffix or failed output cleanup",
                    ));
                }
                state.stack[..unit.outputs.len()].to_vec()
            }
            OutputPublication::Aliases => {
                let incoming = &active.incoming;
                if state.stack != incoming.stack
                    || state.cells != incoming.cells
                    || state.slots != incoming.slots
                    || state.padding != incoming.padding
                    || state.contents != incoming.contents
                    || state.written != incoming.written
                    || state.results != incoming.results
                {
                    return Err(fail(
                        DiagnosticCategory::UnavailableValue,
                        "alias publication changed physical state",
                    ));
                }
                super::reconstruction::alias_outputs(
                    &self.plan.selected,
                    &unit.outputs,
                    incoming,
                    &self.tokens,
                    self.frame,
                )?
            }
        };
        for (offset, token) in &active.readonly {
            if state.cells.get(offset) != Some(token) {
                return Err(fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "stored input overwritten before publication",
                ));
            }
        }
        // Alias publication names semantic results without placing them on the stack.
        // Report their resolved tokens and reserve raw depth for stack publications.
        let output_error = |category, message, index: usize| {
            let mut diagnostic = fail(category, message).expecting(unit.outputs[index]);
            diagnostic.actual = Self::actual_atom(state, outputs[index]);
            if unit.publication == OutputPublication::Stack {
                diagnostic.depth = Some(index);
            }
            diagnostic
        };
        for (index, atom) in unit.outputs.iter().enumerate() {
            if state.results.iter().any(|((output, _), required)| {
                output == atom && !required.satisfied(outputs[index], &self.tokens)
            }) {
                return Err(output_error(
                    DiagnosticCategory::AbiMismatch,
                    "cleanup destroyed signature result or published nonresult lane",
                    index,
                ));
            }
        }
        for (index, atom) in unit.outputs.iter().enumerate() {
            let token = outputs[index];
            if !super::reconstruction::matches(
                &self.plan.selected,
                *atom,
                token,
                &active.incoming,
                &self.tokens,
            )? {
                return Err(output_error(
                    DiagnosticCategory::UnavailableValue,
                    "output violates checked reconstruction identity",
                    index,
                ));
            }
            if let Some(source) = evidence.aliases.source(*atom)
                && active.incoming.token(source)? != token
            {
                return Err(output_error(
                    DiagnosticCategory::UnavailableValue,
                    "alias output does not equal registered input",
                    index,
                ));
            }
        }
        for (a, b) in &evidence.equal_outputs {
            let a = super::result_index(*a, unit.outputs.len())?;
            let b = super::result_index(*b, unit.outputs.len())?;
            if outputs[a] != outputs[b] {
                return Err(fail(
                    DiagnosticCategory::UnavailableValue,
                    "registered output equality was not established",
                ));
            }
        }
        for (reference, index) in &evidence.scratch_outputs {
            let offset = self.frame.resolve(*reference)?.into();
            let index = super::result_index(*index, unit.outputs.len())?;
            if memory::read(state, offset, self.frame)? != outputs[index] {
                return Err(fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "scratch/output equality was not established",
                ));
            }
        }
        for fact in &unit.address_results {
            let index = unit
                .outputs
                .iter()
                .position(|atom| *atom == Atom::new(fact.pointer, 0))
                .ok_or_else(|| error("address result is not a unit output"))?;
            let offset = u32::from(self.frame.resolve(FrameRef::Scratch {
                object: fact.object,
                offset: fact.offset,
            })?);
            let allocation = &self.frame.scratch()[&fact.object];
            if self.tokens.frame_address(outputs[index]) != Some(offset)
                || fact.offset.checked_add(fact.accessible).is_none_or(|end| end > allocation.size)
            {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "output does not establish declared scratch address",
                ));
            }
        }
        // Publication binds names to existing tokens and does not rename stack or memory.
        for (atom, token) in unit.outputs.iter().zip(&outputs) {
            state.names.insert(*atom, *token);
        }
        for effect in &unit.scratch_effects {
            if let ScratchEffect::Write(fact) = effect {
                let offset = u32::from(self.frame.resolve(FrameRef::Scratch {
                    object: fact.object,
                    offset: fact.offset,
                })?);
                if !state.written.contains(&offset) {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "scratch publication lacks a definite physical write",
                    ));
                }
                if let ContentRequirement::Content(content) = fact.content {
                    let offset = self
                        .frame
                        .resolve(FrameRef::Scratch {
                            object: fact.object,
                            offset: fact.offset,
                        })?
                        .into();
                    let token = memory::read(state, offset, self.frame)?;
                    state.contents.insert(content, token);
                }
                memory::check_fact(state, *fact, self.frame, &state.contents)?;
            }
        }
        self.available(state, &self.live.after[&id])?;
        self.storage_available(state, &self.storage.after[&id])?;
        cursor.phase += 1;
        Ok(())
    }

    fn action(
        &mut self,
        instruction: &Instruction,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<(), VerificationError> {
        let action = self
            .actions(cursor)
            .get(cursor.action)
            .cloned()
            .ok_or_else(|| error("unplanned executable instruction"))?;
        let expected = match action {
            PlanAction::Dup { depth } => Primitive::Dup(depth).instruction()?,
            PlanAction::Swap { depth } => Primitive::Swap(depth).instruction()?,
            PlanAction::SwapWord { word } => Primitive::SwapWord(word).instruction()?,
            PlanAction::MoveUp { depth } => Primitive::MoveUp(depth).instruction()?,
            PlanAction::MoveDown { depth } => Primitive::MoveDown(depth).instruction()?,
            PlanAction::Drop => Instruction::Drop,
            PlanAction::PushLiteral(value) => Primitive::Push(value).instruction()?,
            PlanAction::StoreLane(slot) => {
                let token = *state.prefix(1)?.first().unwrap();
                state.slots.insert(slot, token);
                Instruction::LocStore(self.frame.resolve(FrameRef::Spill(slot))?.into())
            }
            PlanAction::LoadLane(slot) => {
                let offset = self.frame.resolve(FrameRef::Spill(slot))?.into();
                let actual = memory::read(state, offset, self.frame)?;
                let expected = state.slots.get(&slot).copied().ok_or_else(|| {
                    fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "abstract slot never stored on this path",
                    )
                })?;
                if actual != expected {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "physical cell clobbered through another abstract slot",
                    ));
                }
                Instruction::LocLoad(self.frame.resolve(FrameRef::Spill(slot))?.into())
            }
            _ => return Err(error("expected lowering/debug boundary instead of instruction")),
        };
        if *instruction != expected {
            return Err(error("actual instruction differs from physical action"));
        }
        self.primitive(instruction, state, cursor)?;
        cursor.action += 1;
        let live = self.pending_live(cursor)?;
        self.available(state, &live)?;
        let demand = if let Some(id) = cursor.edge {
            let edge = self.plan.selected.edges.iter().find(|edge| edge.id == id).unwrap();
            edge.destination
                .map(|destination| {
                    self.storage.entry[&destination]
                        .iter()
                        .map(|fact| super::storage::source_fact(*fact, edge))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            let segment = self
                .plan
                .selected
                .segments
                .iter()
                .find(|segment| Some(segment.id) == cursor.segment)
                .unwrap();
            segment.units.get(cursor.phase).map_or_else(
                || self.storage.exit[&segment.id].clone(),
                |unit| self.storage.before[&unit.id].clone(),
            )
        };
        self.storage_available(state, &demand)
    }

    fn pending_live(&self, cursor: &Cursor) -> Result<Live, VerificationError> {
        if let Some(id) = cursor.edge {
            let edge = self
                .plan
                .selected
                .edges
                .iter()
                .find(|edge| edge.id == id)
                .ok_or_else(|| error("unknown edge"))?;
            let mut live = Live::new();
            if let Some(destination) = edge.destination {
                for atom in &self.live.entry[&destination] {
                    let source = edge
                        .bindings
                        .iter()
                        .find(|binding| binding.destination == atom.value)
                        .map_or(atom.value, |binding| binding.source);
                    live.insert(Atom::new(source, atom.lane));
                }
            } else if edge.kind == EdgeKind::Return {
                for binding in &edge.bindings {
                    for lane in 0..self.plan.selected.values[&binding.source].layout.lanes() {
                        live.insert(Atom::new(binding.source, lane));
                    }
                }
            }
            return Ok(live);
        }
        let segment = self
            .plan
            .selected
            .segments
            .iter()
            .find(|segment| Some(segment.id) == cursor.segment)
            .ok_or_else(|| error("action outside segment"))?;
        Ok(segment.units.get(cursor.phase).map_or_else(
            || self.live.exit[&segment.id].clone(),
            |unit| self.live.before[&unit.id].clone(),
        ))
    }

    fn storage_available(
        &self,
        state: &State,
        demand: &super::storage::Demand,
    ) -> Result<(), VerificationError> {
        for fact in demand {
            memory::check_fact(state, *fact, self.frame, &state.contents)?;
        }
        Ok(())
    }

    /// Require a physical representation or proven reconstruction for every live semantic lane.
    pub fn available(&self, state: &State, live: &Live) -> Result<(), VerificationError> {
        for atom in live {
            let token = state.token(*atom).map_err(|error| error.expecting(*atom))?;
            if !state.available(token, self.frame.spills().values().copied())
                && !super::reconstruction::recoverable(
                    &self.plan.selected,
                    *atom,
                    state,
                    &self.tokens,
                )
            {
                return Err(fail(
                    DiagnosticCategory::UnavailableValue,
                    "destructive action lost last representation of live atom",
                )
                .expecting(*atom));
            }
        }
        Ok(())
    }

    /// Discharge the destination entry hypothesis against actual incoming raw contents,
    /// including reconstruction, spill homes, scratch facts, aliases, and exact addresses.
    pub(super) fn check_contract(
        &self,
        id: SegmentId,
        state: &State,
    ) -> Result<(), VerificationError> {
        let contract =
            self.plan.contracts.get(&id).ok_or_else(|| error("missing entry contract"))?;
        let segment = self.plan.selected.segments.iter().find(|segment| segment.id == id).unwrap();
        for value in &segment.parameters {
            for lane in 0..self.plan.selected.values[value].layout.lanes() {
                let atom = Atom::new(*value, lane);
                if !super::reconstruction::matches(
                    &self.plan.selected,
                    atom,
                    state.token(atom)?,
                    state,
                    &self.tokens,
                )? {
                    return Err(fail(
                        DiagnosticCategory::InvalidEdgeTransfer,
                        "incoming parameter violates checked reconstruction identity",
                    ));
                }
            }
        }
        let expected = contract
            .stack_top_first
            .iter()
            .map(|atom| state.token(*atom).map_err(|error| error.expecting(*atom)))
            .collect::<Result<Vec<_>, _>>()?;
        let entry_context =
            self.plan.selected.segments.first().is_some_and(|segment| segment.id == id)
                && matches!(
                    self.plan.selected.abi.convention(),
                    CallingConvention::Context | CallingConvention::Kernel
                );
        let matches = if entry_context {
            contract.tail == TailPolicy::ContextWindow
                && state.stack.len() == 16
                && state.stack.starts_with(&expected)
        } else {
            state.stack == expected
        };
        if !matches {
            let mut failure = fail(
                DiagnosticCategory::InvalidEdgeTransfer,
                "incoming stack fails destination contract",
            );
            if let Some(depth) = (0..expected.len().max(state.stack.len()))
                .find(|depth| expected.get(*depth) != state.stack.get(*depth))
            {
                failure.depth = Some(depth);
                failure.expected = contract.stack_top_first.get(depth).copied();
                failure.actual =
                    state.stack.get(depth).and_then(|token| Self::actual_atom(state, *token));
            }
            return Err(failure);
        }
        for (slot, atom) in &contract.required_spill_slots {
            let offset = self.frame.resolve(FrameRef::Spill(*slot))?.into();
            let actual =
                memory::read(state, offset, self.frame).map_err(|error| error.expecting(*atom))?;
            if actual != state.token(*atom).map_err(|error| error.expecting(*atom))? {
                let mut failure = fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "incoming spill fails destination identity",
                )
                .expecting(*atom);
                failure.actual = Self::actual_atom(state, actual);
                return Err(failure);
            }
        }
        for fact in &contract.required_scratch {
            memory::check_fact(state, *fact, self.frame, &state.contents)?;
        }
        for (a, b) in contract.aliases.pairs() {
            if state.token(*a)? != state.token(*b)? {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "unproved contract equality",
                ));
            }
        }
        for fact in &contract.address_facts {
            let token = state.token(Atom::new(fact.pointer, 0))?;
            let offset = u32::from(self.frame.resolve(FrameRef::Scratch {
                object: fact.object,
                offset: fact.offset,
            })?);
            let allocation = &self.frame.scratch()[&fact.object];
            if self.tokens.frame_address(token) != Some(offset)
                || fact.offset.checked_add(fact.accessible).is_none_or(|end| end > allocation.size)
            {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "unproved frame address provenance",
                ));
            }
        }
        Ok(())
    }

    /// Read all source identities before applying simultaneous parameter/content renaming.
    /// Required destination scratch facts are proved in predecessor names before substitution.
    pub(super) fn transfer(&self, id: EdgeId, state: &mut State) -> Result<(), VerificationError> {
        let edge = self
            .plan
            .selected
            .edges
            .iter()
            .find(|edge| edge.id == id)
            .ok_or_else(|| error("unknown transfer"))?;
        let mut substitutions = Vec::new();
        for binding in &edge.bindings {
            for lane in 0..self.plan.selected.values[&binding.source].layout.lanes() {
                substitutions.push((
                    Atom::new(binding.destination, lane),
                    state.token(Atom::new(binding.source, lane))?,
                ));
            }
        }
        let contents = edge
            .content_bindings
            .iter()
            .map(|binding| {
                state
                    .contents
                    .get(&binding.source)
                    .copied()
                    .map(|token| (binding.destination, token))
                    .ok_or_else(|| {
                        fail(
                            DiagnosticCategory::InvalidEdgeTransfer,
                            "content binding source has no verified current definition",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(destination) = edge.destination {
            let demand = self.plan.contracts[&destination]
                .required_scratch
                .iter()
                .map(|fact| super::storage::source_fact(*fact, edge))
                .collect();
            self.storage_available(state, &demand)?;
        }
        for (atom, token) in substitutions {
            state.names.insert(atom, token);
        }
        for (content, token) in contents {
            state.contents.insert(content, token);
        }
        Ok(())
    }

    /// Prove the exact declared result prefix and permitted physical return height, rejecting
    /// compiler-frame address escape even if the returned semantic names otherwise match.
    pub(super) fn check_return(
        &self,
        state: &State,
        edge: &TransferEdge,
    ) -> Result<(), VerificationError> {
        let mut expected = Vec::new();
        for binding in &edge.bindings {
            for lane in 0..self.plan.selected.values[&binding.source].layout.lanes() {
                expected.push(state.token(Atom::new(binding.source, lane))?);
            }
        }
        if state.stack.iter().any(|token| self.tokens.origins(*token).compiler()) {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "compiler frame address escapes through procedure return",
            ));
        }
        if expected.len() != self.plan.selected.abi.results() as usize || state.stack != expected {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "return does not contain exactly declared result prefix",
            ));
        }
        if matches!(
            self.plan.selected.abi.convention(),
            CallingConvention::Context | CallingConvention::Kernel | CallingConvention::Root
        ) {
            let padding = state.padding.0[0];
            if (0..=16).any(|count| padding & (1 << count) != 0 && state.stack.len() + count != 16)
            {
                return Err(fail(
                    DiagnosticCategory::AbiMismatch,
                    "context return is not exactly sixteen physical lanes",
                ));
            }
        }
        Ok(())
    }

    fn target_effect(
        &self,
        instruction: &Instruction,
        state: &State,
        cursor: &Cursor,
    ) -> Result<StackEffect, VerificationError> {
        if let Some(active) = &cursor.active {
            match (self.plan.selected.evidence(active.id)?.generated_event, instruction) {
                (Some(GeneratedEvent::Dynamic(id)), Instruction::Emit) => {
                    let effect = TargetEffects::generated_event(false);
                    effect.validate()?;
                    if state.prefix(usize::from(effect.consumed))?[0] != state.token(id)? {
                        return Err(error("generic event ID differs from selected input binding"));
                    }
                    return Ok(effect);
                }
                (Some(GeneratedEvent::Immediate), Instruction::EmitImm(_)) => {
                    let effect = TargetEffects::generated_event(true);
                    effect.validate()?;
                    return Ok(effect);
                }
                _ => {}
            }
        }
        TargetEffects::instruction(instruction)
    }

    /// Apply a registered target or call rule, checking source effect authority and pointer
    /// provenance before mutating raw state. The boolean reports whether normal execution remains.
    pub fn instruction(
        &mut self,
        instruction: &Instruction,
        state: &mut State,
        cursor: &mut Cursor,
    ) -> Result<bool, VerificationError> {
        if matches!(
            instruction,
            Instruction::Exec(_)
                | Instruction::Call(_)
                | Instruction::SysCall(_)
                | Instruction::DynExec
                | Instruction::DynCall
        ) {
            self.call(instruction, state, cursor)?;
            return Ok(true);
        }
        let effect = self.target_effect(instruction, state, cursor)?;
        if effect.may_trap {
            let active = cursor.active.as_ref().ok_or_else(|| error("trap outside lowering"))?;
            if !self.unit(active.id)?.effects.may_trap {
                return Err(error("undeclared trapping instruction"));
            }
        }
        if matches!(instruction, Instruction::CDrop | Instruction::CDropW) {
            let predicate = state.prefix(effect.consumed.into())?[0];
            if self.tokens.constant(predicate).is_some_and(|value| value > 1) {
                return Ok(false);
            }
        }
        if matches!(instruction, Instruction::AssertEqw | Instruction::AssertEqwWithError(_)) {
            let inputs = state.prefix(8)?.to_vec();
            let equal = (0..4).all(|lane| {
                match (self.tokens.constant(inputs[lane]), self.tokens.constant(inputs[lane + 4])) {
                    (Some(left), Some(right)) => left == right,
                    _ => true,
                }
            });
            state.replace(8, vec![], &effect.floor)?;
            return Ok(equal);
        }
        if matches!(instruction, Instruction::AssertEq | Instruction::AssertEqWithError(_)) {
            // Eq followed by Assert consumes both real lanes, including both floor updates.
            // Unknown values may be equal; do not invent an identity from normal completion.
            let right = state.pop()?;
            let left = state.pop()?;
            return Ok(match (self.tokens.constant(left), self.tokens.constant(right)) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            });
        }
        if matches!(
            instruction,
            Instruction::Assert
                | Instruction::Assertz
                | Instruction::AssertWithError(_)
                | Instruction::AssertzWithError(_)
        ) {
            let token = state.pop()?;
            let desired = u64::from(matches!(
                instruction,
                Instruction::Assert | Instruction::AssertWithError(_)
            ));
            return Ok(self.tokens.constant(token).is_none_or(|value| value == desired));
        }
        self.primitive(instruction, state, cursor)?;
        Ok(true)
    }

    fn primitive(
        &mut self,
        instruction: &Instruction,
        state: &mut State,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        let effect = self.target_effect(instruction, state, cursor)?;
        effect.validate()?;
        let inputs = state.prefix(effect.consumed.into())?.to_vec();
        // Strict observations precede both paired proofs and every pre-read shortcut.
        for observation in &effect.observed_inputs {
            if let Observation::Value(index) = *observation
                && self.tokens.origins(inputs[usize::from(index)]).compiler()
            {
                return Err(fail(
                    DiagnosticCategory::IllegalStackAccess,
                    "observed input exposes compiler storage provenance",
                ));
            }
        }
        let interval = if let Some(MemoryRead::SourceRange { start, end }) = effect.memory.read {
            self.source_bounds(inputs[usize::from(start)], inputs[usize::from(end)])?
        } else {
            None
        };
        // Validate the entire footprint before constructing outputs or performing writes.
        // Only output-demanded fixed lanes need identities; repeated references share a token.
        let mut loaded = BTreeMap::new();
        if let Some(read) = effect.memory.read {
            let memory = match read {
                MemoryRead::Fixed(region) => self
                    .memory_region(region, &inputs, cursor, false)?
                    .map(|offset| (offset, u32::from(region.width()))),
                MemoryRead::Circuit {
                    address,
                    read_count,
                    eval_count,
                } => self.circuit_read(&inputs, address, read_count, eval_count, cursor)?,
                MemoryRead::PrintLn { address, length } => {
                    self.println_read(
                        inputs[usize::from(address)],
                        inputs[usize::from(length)],
                        cursor,
                    )?;
                    None
                }
                MemoryRead::SourceRange { start, end } => {
                    self.source_range_read(
                        inputs[usize::from(start)],
                        inputs[usize::from(end)],
                        interval,
                        cursor,
                    )?;
                    None
                }
                MemoryRead::DeferredRegisterData {
                    tag,
                    address,
                    chunks,
                } => {
                    self.deferred_data_read(&inputs, tag, address, chunks, cursor)?;
                    None
                }
            };
            if let Some((offset, width)) = memory {
                for cell in offset..offset + width {
                    memory::read(state, cell, self.frame)?;
                }
            }
            for lane in &effect.outputs {
                if let LaneEffect::Memory { lane } = *lane {
                    loaded.entry(lane).or_insert_with(|| {
                        if let Some((offset, _)) = memory {
                            // The full footprint has already been checked above.
                            state.cells[&(offset + u32::from(lane))]
                        } else {
                            self.tokens.fresh()
                        }
                    });
                }
            }
        }
        let origins =
            inputs.iter().fold(super::tokens::Origins::default(), |mut origins, token| {
                origins.union(self.tokens.origins(*token));
                origins
            });
        // Equality produces a boolean, never an address, in either native or immediate form.
        let is_equality = matches!(
            instruction,
            Instruction::Eq | Instruction::Neq | Instruction::EqImm(_) | Instruction::NeqImm(_)
        );
        let address = if effect.outputs.iter().any(|lane| matches!(lane, LaneEffect::Computed(_)))
            && origins.has_frame()
        {
            let value = match instruction {
                Instruction::Add => {
                    let (address, delta) = if self.tokens.frame_address(inputs[0]).is_some() {
                        (inputs[0], inputs[1])
                    } else {
                        (inputs[1], inputs[0])
                    };
                    self.tokens.frame_address(address).and_then(|offset| {
                        self.tokens
                            .constant(delta)
                            .and_then(|delta| u32::try_from(delta).ok())
                            .and_then(|delta| offset.checked_add(delta))
                    })
                }
                Instruction::Sub => self.tokens.frame_address(inputs[1]).and_then(|offset| {
                    self.tokens
                        .constant(inputs[0])
                        .and_then(|delta| u32::try_from(delta).ok())
                        .and_then(|delta| offset.checked_sub(delta))
                }),
                Instruction::AddImm(Immediate::Value(delta)) => {
                    self.tokens.frame_address(inputs[0]).and_then(|offset| {
                        u32::try_from(delta.as_canonical_u64())
                            .ok()
                            .and_then(|delta| offset.checked_add(delta))
                    })
                }
                _ if is_equality => None,
                _ if !origins.compiler() => None,
                _ => {
                    return Err(fail(
                        DiagnosticCategory::IllegalStackAccess,
                        "compiler address arithmetic loses provenance",
                    ));
                }
            };
            if origins.compiler() && !is_equality && value.is_none() {
                return Err(fail(
                    DiagnosticCategory::IllegalStackAccess,
                    "compiler address arithmetic is not exact",
                ));
            }
            value.map(|offset| self.tokens.intern(Identity::FrameAddress(offset)))
        } else {
            None
        };
        for lane in &effect.outputs {
            if let LaneEffect::FrameAddress(offset) = lane {
                memory::check_range(*offset, 1, self.frame)?;
            }
        }
        let outputs: Vec<_> = effect
            .outputs
            .iter()
            .map(|lane| match *lane {
                LaneEffect::Input(index) => inputs[usize::from(index)],
                LaneEffect::Literal(value) => self.tokens.literal(value),
                LaneEffect::Memory { lane } => loaded[&lane],
                LaneEffect::FrameAddress(offset) => {
                    self.tokens.intern(Identity::FrameAddress(offset))
                }
                LaneEffect::Computed(_) => address.unwrap_or_else(|| {
                    let token = self.tokens.fresh();
                    // Opaque source arithmetic retains may-origin, without inventing equality
                    // or an exact frame offset. Compiler/mixed arithmetic is rejected above.
                    if !is_equality {
                        self.tokens.add_origins(token, origins);
                    }
                    token
                }),
                LaneEffect::Conditional {
                    condition,
                    if_true,
                    if_false,
                } => {
                    let yes = inputs[usize::from(if_true)];
                    let no = inputs[usize::from(if_false)];
                    match self.tokens.constant(inputs[usize::from(condition)]) {
                        Some(1) => yes,
                        Some(0) => no,
                        _ if yes == no => yes,
                        _ => {
                            // Value equality is unknown, but either candidate may carry a
                            // compiler address. Keep its provenance even after abstraction.
                            let token = self.tokens.fresh();
                            self.tokens.add_origins(token, self.tokens.origins(yes));
                            self.tokens.add_origins(token, self.tokens.origins(no));
                            token
                        }
                    }
                }
            })
            .collect();
        if let Some(write) = &effect.memory.write {
            let destination = self.memory_region(write.region, &inputs, cursor, true)?;
            let values = write
                .values
                .iter()
                .map(|value| match *value {
                    WriteValue::Input(i) => inputs[usize::from(i)],
                    WriteValue::Output(i) => outputs[usize::from(i)],
                })
                .collect::<Vec<_>>();
            if destination.is_none()
                && values.iter().any(|token| self.tokens.origins(*token).compiler())
            {
                return Err(fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "compiler address escapes into source memory",
                ));
            }
            if let Some(offset) = destination {
                for (lane, token) in values.into_iter().enumerate() {
                    state.cells.insert(offset + lane as u32, token);
                    state.written.insert(offset + lane as u32);
                }
            }
        }
        state.replace(effect.consumed.into(), outputs, &effect.floor)
    }

    fn memory_region(
        &self,
        region: MemoryRegion,
        inputs: &[Token],
        cursor: &Cursor,
        write: bool,
    ) -> Result<Option<u32>, VerificationError> {
        let (offset, width) = match region {
            MemoryRegion::Frame { offset, width } => (Some(offset), u32::from(width)),
            MemoryRegion::Indirect { address, width } => {
                let token = inputs[usize::from(address)];
                let offset = self.tokens.frame_address(token);
                if offset.is_none() && self.tokens.origins(token).compiler() {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "compiler memory address is not precisely resolved",
                    ));
                }
                (offset, u32::from(width))
            }
            MemoryRegion::SourceUnknown { width } => (None, u32::from(width)),
        };
        self.memory_access(offset, Some(width), width == 4, cursor, write)
    }

    fn source_bounds(
        &self,
        start: Token,
        end: Token,
    ) -> Result<Option<ExactSourceInterval>, VerificationError> {
        if !self.tokens.origins(start).compiler() && !self.tokens.origins(end).compiler() {
            return Ok(None);
        }
        match (self.tokens.frame_address(start), self.tokens.frame_address(end)) {
            (Some(start), Some(end)) if start <= end && end <= self.frame.source().extent => {
                Ok(Some(ExactSourceInterval {
                    start,
                    width: end
                        .checked_sub(start)
                        .ok_or_else(|| error("source interval overflow"))?,
                }))
            }
            _ => Err(fail(
                DiagnosticCategory::IllegalStackAccess,
                "observed bounds lack an exact current-frame source interval proof",
            )),
        }
    }

    fn source_range_read(
        &self,
        start: Token,
        end: Token,
        interval: Option<ExactSourceInterval>,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        let start_literal = self.tokens.constant(start);
        let end_literal = self.tokens.constant(end);
        if start_literal.is_some_and(|n| n > u64::from(u32::MAX))
            || end_literal.is_some_and(|n| n > u64::from(u32::MAX))
            || matches!((start_literal, end_literal), (Some(s), Some(e)) if s > e)
            || start == end
        {
            return Ok(());
        }
        let (offset, width) = if let Some(interval) = interval {
            (Some(interval.start), Some(interval.width))
        } else {
            let offset = self.tokens.frame_address(start);
            let width = match (offset, self.tokens.frame_address(end)) {
                (Some(start), Some(end)) => end.checked_sub(start),
                _ => match (start_literal, end_literal) {
                    (Some(start), Some(end)) => {
                        end.checked_sub(start).and_then(|n| n.try_into().ok())
                    }
                    _ => None,
                },
            };
            (offset, width)
        };
        if width == Some(0) {
            return Ok(());
        }
        self.source_only_read(offset, width, cursor)
    }

    /// The closed framework guards are universal pre-read failures, not a registry interpreter.
    fn deferred_data_read(
        &self,
        inputs: &[Token],
        tag: [u8; 4],
        address: u8,
        chunks: u8,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        let n = self.tokens.constant(inputs[usize::from(chunks)]);
        let tag = tag.map(|i| self.tokens.constant(inputs[usize::from(i)]));
        let pointer = inputs[usize::from(address)];
        let ptr = self.tokens.constant(pointer);
        if n.is_some_and(|n| n == 0 || n > 131071)
            || tag[0] == Some(0)
            || matches!(tag[0], Some(1 | 2))
                && tag[1..].iter().any(|arg| arg.is_some_and(|n| n != 0))
            || tag[0] == Some(1) && n.is_some_and(|n| n != 1)
            || ptr.is_some_and(|p| p > u64::from(u32::MAX) || !p.is_multiple_of(4))
            || matches!((ptr, n), (Some(p), Some(n))
                if p.checked_add(8 * n).is_none_or(|end| end > u64::from(u32::MAX)))
        {
            return Ok(());
        }
        // The preceding count guard bounds the largest possible width to 1048568.
        let width = n.map(|n| (8 * n) as u32);
        self.source_only_read(self.tokens.frame_address(pointer), width, cursor)
    }

    /// Packed bytes have no frame-relative identity in the source-memory ABI.
    fn println_read(
        &self,
        pointer: Token,
        length: Token,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        let pointer = self.tokens.constant(pointer);
        let length = self.tokens.constant(length);
        if pointer.is_some_and(|p| p > u64::from(u32::MAX))
            || length.is_some_and(|n| n == 0 || n > 524288)
        {
            return Ok(());
        }
        // Absolute pointer/4 is not a frame offset. Native FrameAddress facts likewise
        // cannot be divided by four: floor((base+offset)/4) is not base+offset/4.
        // The guards bound this arithmetic to at most 131073 cells; unknown residues
        // leave the exact width unresolved, except when every possible residue gives the same width.
        let width = match (pointer, length) {
            (Some(p), Some(n)) => Some(((p % 4 + n).div_ceil(4)) as u32),
            (_, Some(n)) if n % 4 == 1 => Some((n.div_ceil(4)) as u32),
            _ => None,
        };
        self.source_only_read(None, width, cursor)
    }

    /// Check an implicit aligned word read restricted to source memory.
    /// Provenance comes first: an exact compiler address must never borrow source authority.
    pub(super) fn source_word_read(
        &self,
        token: Token,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        if self.tokens.origins(token).compiler() {
            return Err(fail(
                DiagnosticCategory::InvalidSpillSlotRead,
                "dynamic root pointer must refer to source memory",
            ));
        }
        let offset = self.tokens.frame_address(token);
        if self.tokens.constant(token).is_some_and(|address| {
            !address.is_multiple_of(4)
                || address.checked_add(4).is_none_or(|end| end > (1u64 << 32))
        }) {
            return Err(fail(
                DiagnosticCategory::IllegalStackAccess,
                "source root word address is unaligned or outside native memory",
            ));
        }
        self.memory_access(offset, Some(4), true, cursor, false)?;
        Ok(())
    }

    /// Footprint-only source reads never initialize, load tokens, or enter compiler storage.
    fn source_only_read(
        &self,
        offset: Option<u32>,
        width: Option<u32>,
        cursor: &Cursor,
    ) -> Result<(), VerificationError> {
        self.memory_access(offset, width, false, cursor, false)?;
        Ok(())
    }

    /// Circuit guards run before memory in the VM. Known invalid counts have no footprint.
    fn circuit_read(
        &self,
        inputs: &[Token],
        address: u8,
        read_count: u8,
        eval_count: u8,
        cursor: &Cursor,
    ) -> Result<Option<(u32, u32)>, VerificationError> {
        let width = match (
            self.tokens.constant(inputs[usize::from(read_count)]),
            self.tokens.constant(inputs[usize::from(eval_count)]),
        ) {
            (Some(read), Some(eval)) => {
                if read.saturating_add(eval) > 32768
                    || read == 0
                    || !read.is_multiple_of(2)
                    || eval == 0
                    || !eval.is_multiple_of(4)
                {
                    return Ok(None);
                }
                // Guarded canonical counts bound the successful footprint to 65532 cells.
                Some(
                    read.checked_mul(2)
                        .and_then(|n| n.checked_add(eval))
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or_else(|| error("circuit read extent overflow"))?,
                )
            }
            _ => None,
        };
        let token = inputs[usize::from(address)];
        let offset = self.tokens.frame_address(token);
        if offset.is_none() && self.tokens.origins(token).compiler() {
            return Err(fail(
                DiagnosticCategory::InvalidSpillSlotRead,
                "compiler memory address is not precisely resolved",
            ));
        }
        self.memory_access(offset, width, true, cursor, false)
            .map(|offset| offset.zip(width))
    }

    fn memory_access(
        &self,
        offset: Option<u32>,
        width: Option<u32>,
        word_aligned: bool,
        cursor: &Cursor,
        write: bool,
    ) -> Result<Option<u32>, VerificationError> {
        if let Some(offset) = offset {
            if word_aligned && offset % 4 != 0 {
                return Err(fail(
                    DiagnosticCategory::IllegalStackAccess,
                    "unaligned word memory access",
                ));
            }
            // An unresolved SourceLocal extent retains its exact base and origin. Checking
            // that base still prevents a known crossing from being erased by unknown counts.
            memory::check_range(offset, width.unwrap_or(1), self.frame)?;
            if offset < self.frame.source().extent {
                if width.is_some_and(|width| offset + width > self.frame.source().extent) {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "source access crosses into compiler storage",
                    ));
                }
                let active = cursor
                    .active
                    .as_ref()
                    .ok_or_else(|| error("source local outside registered lowering"))?;
                if !self.unit(active.id)?.effects.memory.permits(write) {
                    return Err(error("undeclared source-local memory effect"));
                }
                return Ok(None);
            }
            let width = width.ok_or_else(|| {
                fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "compiler memory read extent is not precisely resolved",
                )
            })?;
            if let Some(active) = &cursor.active {
                if (offset..offset + width)
                    .any(|offset| active.readonly.contains_key(&offset) && write)
                {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "write to read-only stored input",
                    ));
                }
                let unit = self.unit(active.id)?;
                let allowed = (offset..offset + width).all(|cell| {
                    active.readonly.contains_key(&cell) && !write
                        || unit.scratch.iter().any(|requirement| {
                            let allocation = &self.frame.scratch()[&requirement.id];
                            (allocation.offset..allocation.offset + allocation.size).contains(&cell)
                        })
                });
                if !allowed {
                    return Err(fail(
                        DiagnosticCategory::InvalidSpillSlotRead,
                        "undeclared compiler-storage access inside lowering",
                    ));
                }
            } else if !(offset..offset + width)
                .all(|cell| self.frame.spills().values().any(|offset| *offset == cell))
            {
                return Err(fail(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "allocation action accesses undeclared spill",
                ));
            }
        } else {
            let active = cursor
                .active
                .as_ref()
                .ok_or_else(|| error("source memory outside registered lowering"))?;
            let unit = self.unit(active.id)?;
            if !unit.effects.memory.permits(write) {
                return Err(error("unregistered source memory effect"));
            }
        }
        Ok(offset)
    }
}
