//! Contract-local graph induction. Hypotheses are checked on every actual incoming edge;
//! no particular forward-entry token is mistaken for an arbitrary loop iteration.
use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    replay::{Cursor, Verifier},
    stack::{Padding, State},
    tokens::{Identity, Origins, Token, Tokens},
    *,
};
use crate::lower::expand::{AnnotatedMasmBody, AnnotatedOp, Boundary};

#[derive(Clone, Copy)]
enum Owner {
    Segment(SegmentId),
    Edge(EdgeId),
}
#[derive(Default)]
struct Blocks {
    segments: BTreeMap<SegmentId, AnnotatedMasmBody>,
    edges: BTreeMap<EdgeId, AnnotatedMasmBody>,
}

/// These finite may-facts cross contract abstraction independently of allocator facts.
/// Replaying all transfers after any union prevents a later backedge from hiding taint.
#[derive(Clone, Default, PartialEq, Eq)]
struct OriginFacts {
    names: BTreeMap<Atom, Origins>,
    cells: BTreeMap<u32, Origins>,
}
impl OriginFacts {
    fn include(&mut self, state: &State, tokens: &Tokens) {
        for (atom, token) in &state.names {
            let origin = tokens.origins(*token);
            if origin.has_frame() {
                self.names.entry(*atom).or_default().union(origin);
            }
        }
        for (offset, token) in &state.cells {
            let origin = tokens.origins(*token);
            if origin.has_frame() {
                self.cells.entry(*offset).or_default().union(origin);
            }
        }
    }

    fn apply(&self, state: &State, tokens: &mut Tokens) {
        for (atom, origin) in &self.names {
            if let Some(token) = state.names.get(atom) {
                tokens.add_origins(*token, *origin);
            }
        }
        for (offset, origin) in &self.cells {
            if let Some(token) = state.cells.get(offset) {
                tokens.add_origins(*token, *origin);
            }
        }
    }
}
impl Blocks {
    fn collect(body: &AnnotatedMasmBody) -> Result<Self, VerificationError> {
        fn walk(
            body: &AnnotatedMasmBody,
            blocks: &mut Blocks,
            mut owner: Option<Owner>,
        ) -> Result<(), VerificationError> {
            let mut active = false;
            let mut iterator = body.ops().iter();
            while let Some(op) = iterator.next() {
                match op {
                    AnnotatedOp::Boundary(Boundary::Control(_)) if !active => {
                        iterator.next().ok_or_else(|| error("missing physical synthetic flag"))?;
                        continue;
                    }
                    AnnotatedOp::Boundary(Boundary::Segment(id)) => {
                        if blocks.segments.insert(*id, AnnotatedMasmBody::new()).is_some() {
                            return Err(error("duplicate physical segment"));
                        }
                        owner = Some(Owner::Segment(*id));
                    }
                    AnnotatedOp::Boundary(Boundary::Edge(id)) => {
                        if blocks.edges.insert(*id, AnnotatedMasmBody::new()).is_some() {
                            return Err(error("duplicate physical edge"));
                        }
                        owner = Some(Owner::Edge(*id));
                    }
                    AnnotatedOp::If {
                        then_body,
                        else_body,
                        ..
                    } if !active => {
                        walk(then_body, blocks, owner)?;
                        walk(else_body, blocks, owner)?;
                        owner = None;
                        continue;
                    }
                    AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. }
                        if !active =>
                    {
                        walk(body, blocks, owner)?;
                        owner = None;
                        continue;
                    }
                    AnnotatedOp::Boundary(Boundary::UnitStart(_)) => active = true,
                    AnnotatedOp::Boundary(Boundary::UnitEnd(_)) => active = false,
                    _ => {}
                }
                match owner
                    .ok_or_else(|| error("executable instruction lacks selected segment/edge"))?
                {
                    Owner::Segment(id) => blocks.segments.get_mut(&id).unwrap().push(op.clone()),
                    Owner::Edge(id) => blocks.edges.get_mut(&id).unwrap().push(op.clone()),
                }
            }
            Ok(())
        }
        let mut blocks = Self::default();
        walk(body, &mut blocks, None)?;
        Ok(blocks)
    }
}

impl Verifier<'_> {
    /// Replay each selected segment under its entry hypothesis and discharge that hypothesis
    /// on every actual incoming transfer. Iterate finite padding and may-origin facts to a
    /// fixed point without reusing one concrete forward-entry token for arbitrary iterations.
    pub(super) fn verify_graph(
        &mut self,
        body: &AnnotatedMasmBody,
    ) -> Result<(), VerificationError> {
        let blocks = Blocks::collect(body)?;
        let entry = self
            .plan
            .selected
            .segments
            .first()
            .ok_or_else(|| error("procedure has no entry"))?
            .id;
        let dominators = super::reconstruction::dominators(&self.plan.selected);
        let mut incoming = BTreeMap::<SegmentId, Padding>::new();
        let mut origins = BTreeMap::<SegmentId, OriginFacts>::new();
        let entry_state = self.abi_entry(entry)?;
        incoming.insert(entry, entry_state.padding.clone());
        // Token induction is local to each pass; only the finite padding possibilities flow.
        loop {
            let previous = incoming.clone();
            let previous_origins = origins.clone();
            for segment in &self.plan.selected.segments {
                let mut state = if segment.id == entry {
                    self.abi_entry(entry)?
                } else {
                    self.hypothesis(segment.id, &dominators)?
                };
                if let Some(padding) = incoming.get(&segment.id) {
                    state.padding = padding.clone();
                }
                if let Some(facts) = origins.get(&segment.id) {
                    facts.apply(&state, &mut self.tokens);
                }
                if is_header(&self.plan.selected.structure, segment.id) {
                    self.flag_transition(&state, true)?;
                }
                let mut cursor = Cursor::default();
                let code = blocks
                    .segments
                    .get(&segment.id)
                    .ok_or_else(|| error("missing physical selected segment"))?;
                let Some(state) = self.execute(code, state, &mut cursor)? else {
                    continue;
                };
                self.finish_actions(&mut cursor)?;
                if cursor.active.is_some() || cursor.phase != segment.units.len() {
                    return Err(error("segment omitted checked normal completion"));
                }
                for edge in self.plan.selected.edges.iter().filter(|edge| edge.source == segment.id)
                {
                    let mut edge_state = state.clone();
                    let mut edge_cursor = cursor.clone();
                    if let Some(predicate) = self.edge_predicate(edge.id)? {
                        let token = edge_state.pop()?;
                        if state.token(predicate)? != token {
                            return Err(fail(
                                DiagnosticCategory::UnavailableValue,
                                "selected control consumed wrong predicate identity",
                            ));
                        }
                        if self.tokens.constant(token).is_some_and(|value| value > 1) {
                            return Err(fail(
                                DiagnosticCategory::IllegalStackAccess,
                                "selected predicate is not boolean",
                            ));
                        }
                        if matches!(
                            (edge.kind, self.tokens.constant(token)),
                            (EdgeKind::True, Some(0)) | (EdgeKind::False, Some(1))
                        ) {
                            continue;
                        }
                    }
                    let code = blocks
                        .edges
                        .get(&edge.id)
                        .ok_or_else(|| error("missing physical selected edge"))?;
                    let Some(mut edge_state) = self.execute(code, edge_state, &mut edge_cursor)?
                    else {
                        continue;
                    };
                    self.finish_actions(&mut edge_cursor)?;
                    if is_exit(&self.plan.selected.structure, edge.id) {
                        self.flag_transition(&edge_state, false)?;
                    }
                    if let Some(destination) = edge.destination {
                        self.transfer(edge.id, &mut edge_state)?;
                        self.check_contract(destination, &edge_state)?;
                        origins.entry(destination).or_default().include(&edge_state, &self.tokens);
                        if let Some(padding) = incoming.get_mut(&destination) {
                            padding.union(&edge_state.padding);
                        } else {
                            incoming.insert(destination, edge_state.padding);
                        }
                    } else if edge.kind == EdgeKind::Return {
                        self.check_return(&edge_state, edge)?;
                    } else {
                        return Err(error("normal execution reaches selected trap edge"));
                    }
                }
            }
            if incoming == previous && origins == previous_origins {
                break;
            }
        }
        Ok(())
    }

    fn flag_transition(&mut self, state: &State, value: bool) -> Result<(), VerificationError> {
        let mut temporary = state.clone();
        let literal = if value {
            miden_core::Felt::ONE
        } else {
            miden_core::Felt::ZERO
        };
        let instruction = Primitive::Push(literal).instruction()?;
        let effect = TargetEffects::instruction(&instruction)?;
        temporary.replace(0, alloc::vec![self.tokens.literal(literal)], &effect.floor)?;
        temporary.pop()?;
        if temporary.stack != state.stack || temporary.padding != state.padding {
            return Err(fail(
                DiagnosticCategory::IllegalStackAccess,
                "synthetic flag consumption changed semantic state or padding",
            ));
        }
        Ok(())
    }

    fn abi_entry(&mut self, id: SegmentId) -> Result<State, VerificationError> {
        let segment = self.plan.selected.segments.iter().find(|segment| segment.id == id).unwrap();
        let mut names = BTreeMap::new();
        let mut stack = Vec::new();
        for value in &segment.parameters {
            for lane in 0..self.plan.selected.values[value].layout.lanes() {
                let token = self.tokens.intern(Identity::Input(stack.len() as u32));
                names.insert(Atom::new(*value, lane), token);
                stack.push(token);
            }
        }
        if stack.len() != self.plan.selected.abi.arguments() as usize {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "entry parameters disagree with ABI",
            ));
        }
        if matches!(
            self.plan.selected.abi.convention(),
            CallingConvention::Context | CallingConvention::Kernel
        ) {
            while stack.len() < 16 {
                stack.push(self.tokens.fresh());
            }
        }
        Ok(State::new(stack, names))
    }

    fn hypothesis(
        &mut self,
        id: SegmentId,
        dominators: &super::reconstruction::Dominators,
    ) -> Result<State, VerificationError> {
        let contract = &self.plan.contracts[&id];
        let mut names = BTreeMap::new();
        let mut atoms = self.live.entry[&id].clone();
        let segment = self.plan.selected.segments.iter().find(|segment| segment.id == id).unwrap();
        // Parameters and outgoing bindings are checked even when semantically dead.
        for value in segment.parameters.iter().copied().chain(
            self.plan
                .selected
                .edges
                .iter()
                .filter(|edge| edge.source == id)
                .flat_map(|edge| edge.bindings.iter().map(|binding| binding.source)),
        ) {
            atoms.extend(
                (0..self.plan.selected.values[&value].layout.lanes())
                    .map(|lane| Atom::new(value, lane)),
            );
        }
        atoms.extend(contract.stack_top_first.iter().copied());
        atoms.extend(contract.required_spill_slots.iter().map(|(_, atom)| *atom));
        atoms.extend(contract.aliases.pairs().iter().flat_map(|(a, b)| [*a, *b]));
        atoms.extend(contract.required_scratch.iter().filter_map(|fact| match fact.content {
            ContentRequirement::Atom(atom) => Some(atom),
            _ => None,
        }));
        atoms.extend(contract.address_facts.iter().map(|fact| Atom::new(fact.pointer, 0)));
        // Recovery follows source aliases recursively, including dependencies without a copy.
        let mut pending: Vec<_> = atoms.iter().copied().collect();
        while let Some(atom) = pending.pop() {
            if let Some(Reconstruction::Alias(sources)) =
                &self.plan.selected.values[&atom.value].reconstruction
            {
                let source = sources[usize::from(atom.lane)];
                if atoms.insert(source) {
                    pending.push(source);
                }
            }
        }
        for atom in atoms {
            names.insert(atom, self.tokens.fresh());
        }
        for fact in &contract.address_facts {
            let offset = u32::from(self.frame.resolve(FrameRef::Scratch {
                object: fact.object,
                offset: fact.offset,
            })?);
            names.insert(
                Atom::new(fact.pointer, 0),
                self.tokens.intern(Identity::FrameAddress(offset)),
            );
        }
        super::reconstruction::seed(
            &self.plan.selected,
            id,
            dominators,
            &mut names,
            &mut self.tokens,
        )?;
        let stack = contract
            .stack_top_first
            .iter()
            .map(|atom| {
                names.get(atom).copied().ok_or_else(|| error("undefined contract stack atom"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut state = State::new(stack, names);
        for (a, b) in contract.aliases.pairs() {
            let a = state.token(*a)?;
            let b = state.token(*b)?;
            unify_hypothesis(&mut state, a, b, &self.tokens)?;
        }
        for (slot, atom) in &contract.required_spill_slots {
            let token = state.names[atom];
            seed_cell(
                &mut state,
                self.frame.resolve(FrameRef::Spill(*slot))?.into(),
                token,
                &self.tokens,
            )?;
            state.slots.insert(*slot, state.names[atom]);
        }
        for fact in &contract.required_scratch {
            let token = match fact.content {
                ContentRequirement::Initialized => self.tokens.fresh(),
                ContentRequirement::Atom(atom) => state.names[&atom],
                ContentRequirement::Content(id) => {
                    *state.contents.entry(id).or_insert_with(|| self.tokens.fresh())
                }
            };
            seed_cell(
                &mut state,
                self.frame
                    .resolve(FrameRef::Scratch {
                        object: fact.object,
                        offset: fact.offset,
                    })?
                    .into(),
                token,
                &self.tokens,
            )?;
        }
        Ok(state)
    }

    fn edge_predicate(&self, id: EdgeId) -> Result<Option<Atom>, VerificationError> {
        fn find(tree: &ControlTree, edge: &TransferEdge) -> Option<Atom> {
            match tree {
                ControlTree::If {
                    predicate,
                    on_true,
                    on_false,
                    then_body,
                    else_body,
                    ..
                } => {
                    if *on_true == edge.id || *on_false == edge.id {
                        Some(*predicate)
                    } else {
                        find(then_body, edge).or_else(|| find(else_body, edge))
                    }
                }
                ControlTree::Loop {
                    predicate,
                    on_true,
                    on_false,
                    before,
                    body,
                    ..
                } => {
                    if *on_true == edge.id || *on_false == edge.id {
                        Some(*predicate)
                    } else {
                        find(before, edge).or_else(|| find(body, edge))
                    }
                }
                ControlTree::Sequence(children) => {
                    children.iter().find_map(|tree| find(tree, edge))
                }
                _ => None,
            }
        }
        let edge = self
            .plan
            .selected
            .edges
            .iter()
            .find(|edge| edge.id == id)
            .ok_or_else(|| error("unknown control edge"))?;
        Ok(find(&self.plan.selected.structure, edge))
    }
}

fn is_header(tree: &ControlTree, id: SegmentId) -> bool {
    match tree {
        ControlTree::Loop {
            header,
            before,
            body,
            ..
        } => *header == id || is_header(before, id) || is_header(body, id),
        ControlTree::Sequence(children) => children.iter().any(|tree| is_header(tree, id)),
        ControlTree::If {
            then_body,
            else_body,
            ..
        } => is_header(then_body, id) || is_header(else_body, id),
        _ => false,
    }
}
fn is_exit(tree: &ControlTree, id: EdgeId) -> bool {
    match tree {
        ControlTree::Loop {
            on_false,
            before,
            body,
            ..
        } => *on_false == id || is_exit(before, id) || is_exit(body, id),
        ControlTree::Sequence(children) => children.iter().any(|tree| is_exit(tree, id)),
        ControlTree::If {
            then_body,
            else_body,
            ..
        } => is_exit(then_body, id) || is_exit(else_body, id),
        _ => false,
    }
}

/// Equalities in a hypothesis are assumptions discharged at every actual incoming edge.
/// This substitution is deliberately local to hypothesis construction; publication never does it.
fn seed_cell(
    state: &mut State,
    offset: u32,
    token: Token,
    tokens: &super::tokens::Tokens,
) -> Result<(), VerificationError> {
    let token = if let Some(old) = state.cells.get(&offset).copied() {
        unify_hypothesis(state, old, token, tokens)?
    } else {
        token
    };
    state.cells.insert(offset, token);
    Ok(())
}
fn unify_hypothesis(
    state: &mut State,
    a: Token,
    b: Token,
    tokens: &super::tokens::Tokens,
) -> Result<Token, VerificationError> {
    if a == b {
        return Ok(a);
    }
    let precise =
        |token| matches!(tokens.identity(token), Identity::Literal(_) | Identity::FrameAddress(_));
    if precise(a) && precise(b) {
        return Err(fail(
            DiagnosticCategory::InvalidEdgeTransfer,
            "inconsistent precise identities in entry hypothesis",
        ));
    }
    let (from, to) = if precise(b) { (a, b) } else { (b, a) };
    for value in state
        .stack
        .iter_mut()
        .chain(state.names.values_mut())
        .chain(state.cells.values_mut())
        .chain(state.slots.values_mut())
        .chain(state.contents.values_mut())
    {
        if *value == from {
            *value = to;
        }
    }
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_data_does_not_create_origin_facts() {
        let mut tokens = Tokens::new(4);
        let atom = Atom::new(ValueId(0), 0);
        let token = tokens.fresh();
        let mut state = State::new(alloc::vec![token], BTreeMap::from([(atom, token)]));
        state.cells.insert(0, token);
        let mut facts = OriginFacts::default();
        facts.include(&state, &tokens);
        assert!(facts.names.is_empty());
        assert!(facts.cells.is_empty());

        let pointer = tokens.intern(Identity::FrameAddress(4));
        state.names.insert(atom, pointer);
        state.cells.insert(0, pointer);
        facts.include(&state, &tokens);
        assert!(facts.names[&atom].compiler());
        assert!(facts.cells[&0].compiler());
        facts.include(&State::new(alloc::vec![token], BTreeMap::from([(atom, token)])), &tokens);
        assert!(facts.names[&atom].compiler());
    }
}

#[cfg(test)]
mod hypothesis_tests {
    use super::*;

    #[test]
    fn hypothesis_omits_unrelated_segment_values() {
        let (plan, frame, _) = super::super::tests::loops::loop_fixture(true);
        let mut verifier = Verifier::new(&plan, &frame).unwrap();
        let state = verifier.hypothesis(SegmentId(1)).unwrap();
        assert!(state.names.contains_key(&Atom::new(ValueId(10), 0)));
        assert!(state.names.contains_key(&Atom::new(ValueId(11), 0)));
        assert!(!state.names.contains_key(&Atom::new(ValueId(40), 0)));
        assert!(!state.names.contains_key(&Atom::new(ValueId(41), 0)));
    }
}
