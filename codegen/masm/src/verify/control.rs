//! Check executable control projection and inductive loops internal to a selected unit.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

use super::{
    replay::{Cursor, Verifier},
    stack::State,
    tokens::{Identity, Token},
    *,
};
use crate::lower::expand::{AnnotatedMasmBody, AnnotatedOp, Boundary};

#[derive(Debug, PartialEq, Eq)]
enum Projection {
    Segment(SegmentId),
    Edge(EdgeId),
    If(ControlId, Vec<Self>, Vec<Self>),
    While(ControlId, Vec<Self>),
    Repeat(ControlId, u32, Vec<Self>),
    Flag(ControlId, bool),
}

/// Compare expanded control and boundary markers with the selected structured skeleton.
/// Unit-internal control is left for registered schemas; external synthetic loop flags must
/// match the selected edges rather than merely having well-formed annotations.
pub(super) fn check_projection(
    tree: &ControlTree,
    body: &AnnotatedMasmBody,
) -> Result<(), VerificationError> {
    fn selected(
        tree: &ControlTree,
        out: &mut Vec<Projection>,
        loops: &mut Vec<(ControlId, Vec<EdgeId>)>,
    ) {
        match tree {
            ControlTree::Segment(id) => out.push(Projection::Segment(*id)),
            ControlTree::Edge(id) => {
                out.push(Projection::Edge(*id));
                if let Some((control, _)) = loops.iter().rev().find(|(_, edges)| edges.contains(id))
                {
                    out.push(Projection::Flag(*control, true));
                }
            }
            ControlTree::Sequence(children) => {
                for child in children {
                    selected(child, out, loops);
                }
            }
            ControlTree::If {
                id,
                on_true,
                then_body,
                on_false,
                else_body,
                ..
            } => {
                let mut a = vec![Projection::Edge(*on_true)];
                selected(then_body, &mut a, loops);
                let mut b = vec![Projection::Edge(*on_false)];
                selected(else_body, &mut b, loops);
                out.push(Projection::If(*id, a, b));
            }
            ControlTree::Loop {
                id,
                condition,
                header,
                on_true,
                on_false,
                body,
                backedges,
                ..
            } => {
                loops.push((*id, backedges.clone()));
                let mut taken = vec![Projection::Edge(*on_true)];
                selected(body, &mut taken, loops);
                loops.pop();
                out.push(Projection::Flag(*id, true));
                out.push(Projection::While(
                    *id,
                    vec![
                        Projection::Segment(*header),
                        Projection::If(
                            *condition,
                            taken,
                            vec![Projection::Edge(*on_false), Projection::Flag(*id, false)],
                        ),
                    ],
                ));
            }
        }
    }
    fn actual(
        body: &AnnotatedMasmBody,
        out: &mut Vec<Projection>,
        mut active: bool,
    ) -> Result<(), VerificationError> {
        let mut index = 0;
        while let Some(op) = body.ops().get(index) {
            match op {
                AnnotatedOp::Boundary(Boundary::UnitStart(_)) => {
                    if active {
                        return Err(error("nested unit annotation"));
                    }
                    active = true;
                }
                AnnotatedOp::Boundary(Boundary::UnitEnd(_)) => {
                    if !active {
                        return Err(error("orphan unit completion annotation"));
                    }
                    active = false;
                }
                AnnotatedOp::Boundary(Boundary::Segment(id)) => {
                    if active {
                        return Err(error("segment marker inside unit"));
                    }
                    out.push(Projection::Segment(*id));
                }
                AnnotatedOp::Boundary(Boundary::Edge(id)) => {
                    if active {
                        return Err(error("edge marker inside unit"));
                    }
                    out.push(Projection::Edge(*id));
                }
                AnnotatedOp::Boundary(Boundary::Control(id)) if !active => {
                    let Some(AnnotatedOp::Instruction { instruction, unit }) =
                        body.ops().get(index + 1)
                    else {
                        return Err(error("control flag marker lacks actual push"));
                    };
                    if *unit != SelectedStepId(u32::MAX) {
                        return Err(error("synthetic flag cannot be published by a lowering"));
                    }
                    let flag = if *instruction
                        == Primitive::Push(miden_core::Felt::ONE).instruction()?
                    {
                        true
                    } else if *instruction
                        == Primitive::Push(miden_core::Felt::ZERO).instruction()?
                    {
                        false
                    } else {
                        return Err(error("synthetic control flag is not the required literal"));
                    };
                    let entry = matches!(body.ops().get(index+2),Some(AnnotatedOp::While{id:next,..})if next==id)
                        && flag;
                    if !entry && index + 2 != body.ops().len() {
                        return Err(error(
                            "synthetic flag is not adjacent to its while consumption",
                        ));
                    }
                    out.push(Projection::Flag(*id, flag));
                    index += 1;
                }
                AnnotatedOp::If {
                    id,
                    then_body,
                    else_body,
                } if !active => {
                    let mut a = Vec::new();
                    let mut b = Vec::new();
                    actual(then_body, &mut a, false)?;
                    actual(else_body, &mut b, false)?;
                    out.push(Projection::If(*id, a, b));
                }
                AnnotatedOp::While { id, body } if !active => {
                    let mut inner = Vec::new();
                    actual(body, &mut inner, false)?;
                    out.push(Projection::While(*id, inner));
                }
                AnnotatedOp::Repeat { id, count, body } if !active => {
                    let mut inner = Vec::new();
                    actual(body, &mut inner, false)?;
                    out.push(Projection::Repeat(*id, *count, inner));
                }
                _ => {}
            }
            index += 1;
        }
        if active {
            return Err(error("unterminated unit annotation"));
        }
        Ok(())
    }
    let mut expected = Vec::new();
    selected(tree, &mut expected, &mut Vec::new());
    let mut observed = Vec::new();
    actual(body, &mut observed, false)?;
    if expected != observed {
        return Err(error("expanded executable control disagrees with selected skeleton"));
    }
    Ok(())
}

impl Verifier<'_> {
    /// Prove a unit-internal loop from its source-registered schema, checking initialization
    /// and every backedge before generalizing identities. `predicate` distinguishes while
    /// loops, which consume a continuing condition, from fixed-count repeat bodies.
    pub(super) fn loop_body(
        &mut self,
        id: ControlId,
        body: &AnnotatedMasmBody,
        initial: State,
        cursor: &mut Cursor,
        predicate: bool,
    ) -> Result<State, VerificationError> {
        let active = cursor
            .active
            .as_ref()
            .ok_or_else(|| error("selected loop lacks inductive entry contract"))?;
        let schema = self
            .plan
            .selected
            .evidence(active.id)?
            .loops
            .get(&id)
            .cloned()
            .ok_or_else(|| error("internal loop lacks registered invariant schema"))?;
        if schema.stack.len() > initial.stack.len() {
            return Err(fail(
                DiagnosticCategory::InvalidEdgeTransfer,
                "loop stack shape disagrees with schema",
            ));
        }
        let mut positions = Vec::new();
        for (index, kind) in schema.stack.iter().enumerate() {
            positions.push((None, index as u32, *kind, initial.stack[index]));
        }
        for (index, token) in initial.stack.iter().enumerate().skip(schema.stack.len()) {
            positions.push((None, index as u32, LoopPosition::Invariant, *token));
        }
        let mut cell_offsets = BTreeSet::new();
        for (reference, kind) in &schema.cells {
            let offset = u32::from(self.frame.resolve(*reference)?);
            if !cell_offsets.insert(offset) {
                return Err(error("duplicate physical loop-cell contract"));
            }
            positions.push((Some(offset), 0, *kind, memory::read(&initial, offset, self.frame)?));
        }
        let mut forward = BTreeMap::new();
        for (_, _, kind, token) in &positions {
            if let LoopPosition::Carry(parameter) = kind
                && forward.insert(*parameter, *token).is_some_and(|old| old != *token)
            {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "loop initialization fails simultaneous carried equality",
                ));
            }
        }
        // Carrying a temporary cannot weaken external semantic/suffix/read-only obligations.
        let unit = self.unit(active.id)?;
        let external = active.incoming.stack[unit.stack_inputs.len()..]
            .iter()
            .copied()
            .chain(active.readonly.values().copied())
            .chain(
                self.live.after[&active.id]
                    .iter()
                    .filter_map(|atom| initial.names.get(atom).copied()),
            )
            .collect::<BTreeSet<_>>();
        for token in external {
            let established = positions
                .iter()
                .any(|(_, _, kind, value)| *kind == LoopPosition::Invariant && *value == token)
                || self.frame.spills().values().any(|offset| {
                    initial.cells.get(offset) == Some(&token) && !cell_offsets.contains(offset)
                });
            if !established {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "loop generalized a required external identity",
                ));
            }
        }
        let scope = self.tokens.scope();
        let mut parameters = BTreeMap::new();
        for (parameter, incoming) in &forward {
            let token = self.tokens.intern(Identity::LoopParam(scope, *parameter));
            self.tokens.add_origins(token, self.tokens.origins(*incoming));
            parameters.insert(*parameter, token);
        }
        let mut header = initial.clone();
        header.cells.retain(|offset, _| cell_offsets.contains(offset));
        header.slots.retain(|slot, _| {
            self.frame
                .spills()
                .get(slot)
                .is_some_and(|offset| cell_offsets.contains(offset))
        });
        // Unspecified facts do not survive a possibly zero-trip loop.
        header
            .contents
            .retain(|_, token| header.cells.values().any(|cell| cell == token));
        for (offset, index, kind, incoming) in &positions {
            let token = match kind {
                LoopPosition::Invariant => *incoming,
                LoopPosition::Carry(parameter) => parameters[parameter],
            };
            if let Some(offset) = offset {
                header.cells.insert(*offset, token);
            } else {
                header.stack[*index as usize] = token;
            }
        }
        // Establish the same result relation on the forward edge that we require on
        // backedges. Retaining an outer iteration's token here would both reject valid
        // replacement by a nested site and lose the zero-trip/header binding proof.
        let forward_results = positions
            .iter()
            .map(|(offset, index, _, incoming)| {
                (
                    *incoming,
                    if let Some(offset) = offset {
                        header.cells[offset]
                    } else {
                        header.stack[*index as usize]
                    },
                )
            })
            .collect::<Vec<_>>();
        header.results = super::results::transport(
            &super::results::Results::new(),
            &initial.results,
            &forward_results,
            &self.tokens,
        )?;
        // The finite padding set reaches a fixed point independently of tokens/iteration count.
        loop {
            let mut iteration_cursor = cursor.clone();
            let Some(mut backedge) =
                self.execute_path(body, header.clone(), &mut iteration_cursor, CallPath::Loop(id))?
            else {
                // A zero-iteration path remains feasible unless the entry predicate was proved true.
                return Ok(header);
            };
            if predicate {
                backedge.pop()?;
            }
            self.match_loop_cursor(cursor, &iteration_cursor)?;
            if backedge.stack.len() != header.stack.len() {
                return Err(fail(
                    DiagnosticCategory::InvalidEdgeTransfer,
                    "loop backedge changes owned stack shape",
                ));
            }
            let mut next = BTreeMap::<u32, Token>::new();
            for (offset, index, kind, incoming) in &positions {
                let actual = if let Some(offset) = offset {
                    memory::read(&backedge, *offset, self.frame)?
                } else {
                    backedge.stack[*index as usize]
                };
                match kind {
                    LoopPosition::Invariant => {
                        if actual != *incoming {
                            return Err(fail(
                                DiagnosticCategory::InvalidEdgeTransfer,
                                "loop backedge changed invariant identity",
                            ));
                        }
                    }
                    LoopPosition::Carry(parameter) => {
                        if next.insert(*parameter, actual).is_some_and(|old| old != actual) {
                            return Err(fail(
                                DiagnosticCategory::InvalidEdgeTransfer,
                                "loop backedge fails simultaneous carried equality",
                            ));
                        }
                    }
                }
            }
            let retained = positions
                .iter()
                .map(|(offset, index, ..)| {
                    if let Some(offset) = offset {
                        Ok((memory::read(&backedge, *offset, self.frame)?, header.cells[offset]))
                    } else {
                        Ok((backedge.stack[*index as usize], header.stack[*index as usize]))
                    }
                })
                .collect::<Result<Vec<_>, VerificationError>>()?;
            let results = super::results::transport(
                &header.results,
                &backedge.results,
                &retained,
                &self.tokens,
            )?;
            let results_changed = header.results != results;
            header.results = results;
            let previous = header.padding.clone();
            header.padding.union(&backedge.padding);
            let mut origins_changed = false;
            for (parameter, token) in next {
                origins_changed |=
                    self.tokens.add_origins(parameters[&parameter], self.tokens.origins(token));
            }
            if header.padding == previous && !origins_changed && !results_changed {
                break;
            }
        }
        Ok(header)
    }

    fn match_loop_cursor(&self, before: &Cursor, after: &Cursor) -> Result<(), VerificationError> {
        if before.segment != after.segment
            || before.edge != after.edge
            || before.action != after.action
            || before.phase != after.phase
            || before.active.as_ref().map(|a| (a.id, a.call))
                != after.active.as_ref().map(|a| (a.id, a.call))
        {
            return Err(error("loop body crosses selected publication/phase boundary"));
        }
        Ok(())
    }
}
