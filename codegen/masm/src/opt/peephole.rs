//! Local identities over emitted Miden Assembly, including redundant movement and immediate fusion.
//!
//! These rules preserve raw stack behavior, including VM floor padding. They inspect native
//! instructions rather than inferring algebraic properties of HIR operations. Production callers
//! use [`simplify_annotated`] to keep allocation actions and proof boundaries intact, then verify
//! the resulting body before serialization.

use alloc::vec::Vec;

use miden_core::Felt;
use smallvec::SmallVec;

use crate::masm::{self, Immediate, Instruction, PushValue, Span, Spanned};

/// One identity: `matches` inspects a window of `len` adjacent instructions and returns what they
/// are to be replaced with when they fit. Immediates that must agree are compared inside
/// `matches`; that is the whole binding mechanism, and it keeps the table readable.
///
/// A replacement inherits the span of the first matched instruction. Empty replacements delete
/// the complete window.
struct Pattern {
    name: &'static str,
    len: usize,
    matches: fn(&[&Instruction]) -> Option<Vec<Instruction>>,
}

/// The identities, each of two adjacent instructions:
///
/// * `swap.n swap.n`: a swap undone
/// * `movup.n movdn.n`: a move undone
/// * `movdn.n movup.n`: a move undone
/// * `push.x drop`: an element pushed and dropped, when `x` is one element
/// * `dup.n drop`: an element copied and dropped
/// * `push.0 eq` and `push.1 add`: native zero-test and increment forms
/// * `swap.1` before field add/mul or equality: operand order is irrelevant
/// * `swapw.n swapw.n`: a complete word swap undone
const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "native immediate fusion",
        len: 2,
        matches: |window| {
            let Instruction::Push(Immediate::Value(value)) = window[0] else {
                return None;
            };
            let PushValue::Int(value) = value.inner() else {
                return None;
            };
            match (value.as_int(), window[1]) {
                (0, Instruction::Eq) => Some(vec![Instruction::EqImm(Felt::ZERO.into())]),
                (1, Instruction::Add) => Some(vec![Instruction::AddImm(Felt::ONE.into())]),
                _ => None,
            }
        },
    },
    Pattern {
        name: "swap before symmetric field operation",
        len: 2,
        matches: |window| {
            (matches!(window[0], Instruction::Swap1)
                && matches!(
                    window[1],
                    Instruction::Add | Instruction::Mul | Instruction::Eq | Instruction::Neq
                ))
            .then(|| vec![window[1].clone()])
        },
    },
    Pattern {
        name: "word swap undone",
        len: 2,
        matches: |window| {
            matches!(
                (window[0], window[1]),
                (Instruction::SwapW1, Instruction::SwapW1)
                    | (Instruction::SwapW2, Instruction::SwapW2)
                    | (Instruction::SwapW3, Instruction::SwapW3)
            )
            .then(Vec::new)
        },
    },
    Pattern {
        name: "swap.n swap.n",
        len: 2,
        matches: |window| (swap_depth(window[0])? == swap_depth(window[1])?).then(Vec::new),
    },
    Pattern {
        name: "movup.n movdn.n",
        len: 2,
        matches: |window| (movup_depth(window[0])? == movdn_depth(window[1])?).then(Vec::new),
    },
    Pattern {
        name: "movdn.n movup.n",
        len: 2,
        matches: |window| (movdn_depth(window[0])? == movup_depth(window[1])?).then(Vec::new),
    },
    Pattern {
        name: "push.x drop",
        len: 2,
        matches: |window| {
            let pushes_one_element = matches!(
                window[0],
                Instruction::Push(Immediate::Value(value)) if matches!(**value, PushValue::Int(_))
            );
            (pushes_one_element && matches!(window[1], Instruction::Drop)).then(Vec::new)
        },
    },
    Pattern {
        name: "dup.n drop",
        len: 2,
        matches: |window| {
            (dup_depth(window[0]).is_some() && matches!(window[1], Instruction::Drop))
                .then(Vec::new)
        },
    },
];

/// Rewrites `ops` to a fixed point with the identities in [`PATTERNS`], recursing into the blocks
/// of control flow ops. A window never spans one of those: it is a run of adjacent instructions.
pub fn simplify(ops: &mut Vec<masm::Op>) {
    for op in ops.iter_mut() {
        match op {
            masm::Op::If {
                then_blk, else_blk, ..
            } => {
                simplify_block(then_blk);
                simplify_block(else_blk);
            }
            masm::Op::While { body, .. } | masm::Op::Repeat { body, .. } => simplify_block(body),
            masm::Op::DoWhile {
                body, condition, ..
            } => {
                simplify_block(body);
                simplify_block(condition);
            }
            masm::Op::Inst(_) => (),
        }
    }

    // A window that newly matches after a rewrite takes in the instructions on both sides of it,
    // or one of its replacement, so it starts at most one less than the longest pattern before it
    let step_back = PATTERNS.iter().map(|pattern| pattern.len).max().unwrap_or(1) - 1;
    let mut cursor = 0;
    'scan: while cursor < ops.len() {
        for pattern in PATTERNS {
            let Some(window) = ops.get(cursor..cursor + pattern.len) else {
                continue;
            };
            let Some(window) = window
                .iter()
                .map(|op| match op {
                    masm::Op::Inst(inst) => Some(&**inst),
                    _ => None,
                })
                .collect::<Option<SmallVec<[&Instruction; 4]>>>()
            else {
                continue;
            };
            let replacement = (pattern.matches)(&window);
            drop(window);
            let Some(replacement) = replacement else {
                continue;
            };
            log::trace!(target: "peephole", "rewriting '{}' at {cursor}", pattern.name);
            let span = ops[cursor].span();
            ops.splice(
                cursor..cursor + pattern.len,
                replacement.into_iter().map(|inst| masm::Op::Inst(Span::new(span, inst))),
            );
            cursor = cursor.saturating_sub(step_back);
            continue 'scan;
        }
        cursor += 1;
    }
}

/// [`simplify`] the ops of `block`.
fn simplify_block(block: &mut masm::Block) {
    let span = block.span();
    let mut ops = block
        .iter_mut()
        .map(|op| core::mem::replace(op, masm::Op::Inst(Span::unknown(Instruction::Nop))))
        .collect();
    simplify(&mut ops);
    *block = masm::Block::new(span, ops);
}

fn swap_depth(inst: &Instruction) -> Option<u8> {
    Some(match inst {
        Instruction::Swap1 => 1,
        Instruction::Swap2 => 2,
        Instruction::Swap3 => 3,
        Instruction::Swap4 => 4,
        Instruction::Swap5 => 5,
        Instruction::Swap6 => 6,
        Instruction::Swap7 => 7,
        Instruction::Swap8 => 8,
        Instruction::Swap9 => 9,
        Instruction::Swap10 => 10,
        Instruction::Swap11 => 11,
        Instruction::Swap12 => 12,
        Instruction::Swap13 => 13,
        Instruction::Swap14 => 14,
        Instruction::Swap15 => 15,
        _ => return None,
    })
}

fn movup_depth(inst: &Instruction) -> Option<u8> {
    Some(match inst {
        Instruction::MovUp2 => 2,
        Instruction::MovUp3 => 3,
        Instruction::MovUp4 => 4,
        Instruction::MovUp5 => 5,
        Instruction::MovUp6 => 6,
        Instruction::MovUp7 => 7,
        Instruction::MovUp8 => 8,
        Instruction::MovUp9 => 9,
        Instruction::MovUp10 => 10,
        Instruction::MovUp11 => 11,
        Instruction::MovUp12 => 12,
        Instruction::MovUp13 => 13,
        Instruction::MovUp14 => 14,
        Instruction::MovUp15 => 15,
        _ => return None,
    })
}

fn movdn_depth(inst: &Instruction) -> Option<u8> {
    Some(match inst {
        Instruction::MovDn2 => 2,
        Instruction::MovDn3 => 3,
        Instruction::MovDn4 => 4,
        Instruction::MovDn5 => 5,
        Instruction::MovDn6 => 6,
        Instruction::MovDn7 => 7,
        Instruction::MovDn8 => 8,
        Instruction::MovDn9 => 9,
        Instruction::MovDn10 => 10,
        Instruction::MovDn11 => 11,
        Instruction::MovDn12 => 12,
        Instruction::MovDn13 => 13,
        Instruction::MovDn14 => 14,
        Instruction::MovDn15 => 15,
        _ => return None,
    })
}

fn dup_depth(inst: &Instruction) -> Option<u8> {
    Some(match inst {
        Instruction::Dup0 => 0,
        Instruction::Dup1 => 1,
        Instruction::Dup2 => 2,
        Instruction::Dup3 => 3,
        Instruction::Dup4 => 4,
        Instruction::Dup5 => 5,
        Instruction::Dup6 => 6,
        Instruction::Dup7 => 7,
        Instruction::Dup8 => 8,
        Instruction::Dup9 => 9,
        Instruction::Dup10 => 10,
        Instruction::Dup11 => 11,
        Instruction::Dup12 => 12,
        Instruction::Dup13 => 13,
        Instruction::Dup14 => 14,
        Instruction::Dup15 => 15,
        _ => return None,
    })
}

/// Simplify emitted instructions without crossing allocation or proof boundaries.
///
/// Run after expansion and before physical verification. Only adjacent instructions owned by the
/// same lowering unit may be combined; allocation actions, control structure and debug locations
/// remain explicit. Each maximal variable-decorator group receives one Nop anchor, as does an
/// otherwise empty executable body. Proof boundaries are transparent; inline decorators and real
/// instructions end groups. Canonical anchors are reconstructed so they cannot split a group.
/// Serialization adds no instructions. The caller must verify the modified body.
pub(crate) fn simplify_annotated(body: &mut crate::lower::expand::AnnotatedMasmBody) {
    use crate::{
        lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp, Boundary, DebugDecorator},
        plan::SelectedStepId,
    };
    fn walk(body: &mut AnnotatedMasmBody, mut active: Option<SelectedStepId>) {
        let ops = core::mem::take(body.ops_mut());
        let mut input = ops
            .into_iter()
            .filter(|op| !matches!(op, AnnotatedOp::Anchor(Anchor::Nop)))
            .peekable();
        while let Some(mut op) = input.next() {
            match &mut op {
                AnnotatedOp::Boundary(Boundary::UnitStart(id)) => active = Some(*id),
                AnnotatedOp::Boundary(Boundary::UnitEnd(_)) => active = None,
                AnnotatedOp::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, active);
                    walk(else_body, active);
                }
                AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. } => {
                    walk(body, active)
                }
                AnnotatedOp::Instruction { instruction, unit } if active == Some(*unit) => {
                    let id = *unit;
                    let mut run = vec![masm::Op::Inst(Span::unknown(instruction.clone()))];
                    while matches!(
                        input.peek(),
                        Some(AnnotatedOp::Instruction { unit, .. }) if *unit == id
                    ) {
                        if let AnnotatedOp::Instruction { instruction, .. } = input.next().unwrap()
                        {
                            run.push(masm::Op::Inst(Span::unknown(instruction)));
                        }
                    }
                    simplify(&mut run);
                    for op in run {
                        let masm::Op::Inst(inst) = op else {
                            unreachable!()
                        };
                        body.push_instruction(inst.into_parts().1, id);
                    }
                    continue;
                }
                _ => {}
            }
            body.push(op);
        }
        // Group the final simplified projection: a removed instruction run must not leave
        // an artificial boundary between observations. Insert before trailing proof markers.
        let ops = core::mem::take(body.ops_mut());
        let mut input = ops.into_iter();
        while let Some(op) = input.next() {
            let end_of_variable_group = matches!(
                &op,
                AnnotatedOp::Anchor(Anchor::Debug {
                    decorator: DebugDecorator::Variable(_),
                    ..
                })
            ) && !matches!(
                input.as_slice().iter().find(|next| !matches!(next, AnnotatedOp::Boundary(_))),
                Some(AnnotatedOp::Anchor(Anchor::Debug {
                    decorator: DebugDecorator::Variable(_),
                    ..
                }))
            );
            body.push(op);
            if end_of_variable_group {
                body.push(AnnotatedOp::Anchor(Anchor::Nop));
            }
        }
        if !body.ops().iter().any(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction { .. }
                    | AnnotatedOp::If { .. }
                    | AnnotatedOp::While { .. }
                    | AnnotatedOp::Repeat { .. }
                    | AnnotatedOp::Anchor(Anchor::Nop)
            )
        }) {
            body.push(AnnotatedOp::Anchor(Anchor::Nop));
        }
    }
    walk(body, None);
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String, sync::Arc};

    use miden_assembly::{ModuleParser, ast::ModuleKind};
    use midenc_hir::formatter::PrettyPrint;
    use midenc_session::diagnostics::{DefaultSourceManager, SourceLanguage, SourceManager};

    use super::*;

    #[test]
    fn variable_groups_receive_one_executable_anchor_before_each_barrier() {
        use crate::{
            lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp, Boundary, DebugDecorator},
            plan::{DebugLocationId, SegmentId, SelectedStepId},
        };
        let variable = |id| {
            AnnotatedOp::Anchor(Anchor::Debug {
                id: DebugLocationId(id),
                decorator: DebugDecorator::Variable(masm::DebugVarInfo::new(
                    "visible",
                    masm::DebugVarLocation::Unavailable,
                )),
            })
        };
        let mut body = AnnotatedMasmBody::new();
        body.push_instruction(Instruction::Nop, SelectedStepId(0));
        body.push(variable(0));
        body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        body.push(variable(1));
        body.push(AnnotatedOp::Anchor(Anchor::Debug {
            id: DebugLocationId(2),
            decorator: DebugDecorator::InlineCallClear,
        }));
        body.push(variable(3));
        body.push_instruction(Instruction::Nop, SelectedStepId(0));
        body.push(variable(4));
        simplify_annotated(&mut body);
        let shape = |body: &AnnotatedMasmBody| {
            body.ops()
                .iter()
                .map(|op| match op {
                    AnnotatedOp::Anchor(Anchor::Nop) => "nop",
                    AnnotatedOp::Anchor(Anchor::Debug {
                        decorator: DebugDecorator::Variable(_),
                        ..
                    }) => "var",
                    AnnotatedOp::Anchor(Anchor::Debug { .. }) => "inline",
                    AnnotatedOp::Boundary(_) => "boundary",
                    AnnotatedOp::Instruction { .. } => "instruction",
                    _ => "control",
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shape(&body),
            [
                "instruction",
                "var",
                "boundary",
                "var",
                "nop",
                "inline",
                "var",
                "nop",
                "instruction",
                "var",
                "nop"
            ]
        );
        simplify_annotated(&mut body);
        assert_eq!(
            shape(&body),
            [
                "instruction",
                "var",
                "boundary",
                "var",
                "nop",
                "inline",
                "var",
                "nop",
                "instruction",
                "var",
                "nop"
            ]
        );
    }

    #[test]
    fn removed_runs_merge_variable_groups_and_nested_bodies_anchor_independently() {
        use crate::{
            lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp, Boundary, DebugDecorator},
            plan::*,
        };
        let variable = |id| {
            AnnotatedOp::Anchor(Anchor::Debug {
                id: DebugLocationId(id),
                decorator: DebugDecorator::Variable(masm::DebugVarInfo::new(
                    "v",
                    masm::DebugVarLocation::Unavailable,
                )),
            })
        };
        let mut leaf = AnnotatedMasmBody::new();
        leaf.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
        leaf.push(variable(0));
        leaf.push_instruction(Instruction::Push(PushValue::from(1u32).into()), SelectedStepId(0));
        leaf.push_instruction(Instruction::Drop, SelectedStepId(0));
        leaf.push(variable(1));
        leaf.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        let mut body = AnnotatedMasmBody::new();
        body.push(AnnotatedOp::If {
            id: ControlId(0),
            then_body: leaf.clone(),
            else_body: leaf.clone(),
        });
        body.push(AnnotatedOp::While {
            id: ControlId(1),
            body: leaf.clone(),
        });
        body.push(AnnotatedOp::Repeat {
            id: ControlId(2),
            count: 2,
            body: leaf,
        });
        simplify_annotated(&mut body);
        for op in body.ops() {
            let leaves = match op {
                AnnotatedOp::If {
                    then_body,
                    else_body,
                    ..
                } => vec![then_body, else_body],
                AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. } => vec![body],
                _ => panic!("unexpected root op"),
            };
            for leaf in leaves {
                assert!(leaf.top_level_instructions().next().is_none());
                assert_eq!(
                    leaf.ops()
                        .iter()
                        .filter(|op| matches!(op, AnnotatedOp::Anchor(Anchor::Nop)))
                        .count(),
                    1
                );
                assert!(matches!(leaf.ops()[3], AnnotatedOp::Anchor(Anchor::Nop)));
                assert!(matches!(leaf.ops()[4], AnnotatedOp::Boundary(Boundary::UnitEnd(_))));
            }
        }
        let once = alloc::format!("{body:?}");
        simplify_annotated(&mut body);
        assert_eq!(alloc::format!("{body:?}"), once);
    }

    /// The ops of a procedure whose body is `insts`, one per line.
    fn parse(insts: &[&str]) -> Vec<masm::Op> {
        if insts.is_empty() {
            return Vec::new();
        }
        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let source = alloc::format!("proc f\n{}\nend\n", insts.join("\n"));
        let file = source_manager.load(SourceLanguage::Masm, "test.masm".into(), source);
        let module: Box<masm::Module> = ModuleParser::new(Some(ModuleKind::Library))
            .parse(Some(masm::LibraryPathRef::new("test")), file, source_manager)
            .unwrap_or_else(|err| panic!("{err:?}"));
        let procedure = module.procedures().next().unwrap();
        procedure.body().iter().cloned().collect()
    }

    fn render(ops: Vec<masm::Op>) -> String {
        masm::Block::new(Default::default(), ops).to_pretty_string()
    }

    #[track_caller]
    fn assert_simplifies(from: &[&str], to: &[&str]) {
        let mut ops = parse(from);
        simplify(&mut ops);
        assert_eq!(render(ops), render(parse(to)));
    }

    #[test]
    fn local_native_identities_remove_expanded_operations() -> Result<(), midenc_hir::Report> {
        use crate::opt::cost::tests::assembled_cost;

        for (before, after) in [
            (vec!["push.0", "eq"], vec!["eq.0"]),
            (vec!["push.1", "add"], vec!["add.1"]),
            (vec!["swap.1", "add"], vec!["add"]),
            (vec!["swap.1", "mul"], vec!["mul"]),
            (vec!["swap.1", "eq"], vec!["eq"]),
            (vec!["swap.1", "neq"], vec!["neq"]),
            (vec!["swapw.1", "swapw.1"], vec![]),
            (vec!["swapw.2", "swapw.2"], vec![]),
            (vec!["swapw.3", "swapw.3"], vec![]),
        ] {
            assert_simplifies(&before, &after);
            assert!(
                assembled_cost(&before.join(" "))?
                    > assembled_cost(&if after.is_empty() {
                        "nop".into()
                    } else {
                        after.join(" ")
                    })?
            );
        }
        for unchanged in [
            vec!["push.2", "add"],
            vec!["push.1", "eq"],
            vec!["swap.1", "sub"],
            vec!["swap.1", "div"],
            vec!["swap.1", "lt"],
            vec!["swap.1", "u32overflowing_add"],
            vec!["swapw.1", "swapw.2"],
        ] {
            assert_simplifies(&unchanged, &unchanged);
        }
        Ok(())
    }

    #[test]
    fn native_identities_preserve_vm_floor_and_overflow() -> Result<(), midenc_hir::Report> {
        use miden_processor::{DefaultHost, FastProcessor, StackInputs};
        use midenc_hir::Report;

        let cases = [
            ["push.0", "eq"],
            ["push.1", "add"],
            ["swap.1", "add"],
            ["swap.1", "mul"],
            ["swap.1", "eq"],
            ["swap.1", "neq"],
            ["swapw.1", "swapw.1"],
            ["swapw.2", "swapw.2"],
            ["swapw.3", "swapw.3"],
        ];
        for width in [16, 40] {
            for first in [0, 1, 7, 0xffff_ffff_0000_0000] {
                let mut input =
                    (0..width).map(|i| Felt::new_unchecked(101 + i as u64)).collect::<Vec<_>>();
                input[0] = Felt::new_unchecked(first);
                input[1] = Felt::new_unchecked(7);
                for (case, before) in cases.iter().enumerate() {
                    let mut expected = input.clone();
                    let a = expected[0];
                    let b = expected[1];
                    expected[0] = match case {
                        0 => Felt::new_unchecked(u64::from(a == Felt::ZERO)),
                        1 => a + Felt::ONE,
                        2 => a + b,
                        3 => a * b,
                        4 => Felt::new_unchecked(u64::from(a == b)),
                        5 => Felt::new_unchecked(u64::from(a != b)),
                        _ => a,
                    };
                    if (2..=5).contains(&case) {
                        expected.remove(1);
                    }
                    let checks = expected
                        .iter()
                        .map(|v| format!("push.{v} assert_eq"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let prefix = input[..width - 16]
                        .iter()
                        .rev()
                        .map(|v| format!("push.{v}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let mut optimized = parse(before);
                    simplify(&mut optimized);
                    for body in [before.join(" "), render(optimized)] {
                        let source = format!("begin {prefix} {body} {checks} end");
                        let program = miden_assembly::Assembler::default()
                            .assemble_program("peephole", source)?
                            .try_into_program()?;
                        let output =
                            FastProcessor::new(StackInputs::new(&input[width - 16..]).unwrap())
                                .execute_sync(&program, &mut DefaultHost::default())
                                .map_err(|error| {
                                    Report::msg(format!("case={case} width={width}: {error}"))
                                })?;
                        for i in 0..16 {
                            assert_eq!(output.stack.get_element(i), Some(Felt::ZERO));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn native_identities_respect_annotated_boundaries() {
        use crate::{
            lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp, Boundary, DebugDecorator},
            plan::{ControlId, DebugLocationId, SelectedStepId},
        };
        let id = SelectedStepId(0);
        for barrier in 0..4 {
            for pair in [
                [Instruction::Push(PushValue::from(0u32).into()), Instruction::Eq],
                [Instruction::Push(PushValue::from(1u32).into()), Instruction::Add],
                [Instruction::Swap1, Instruction::Mul],
                [Instruction::SwapW2, Instruction::SwapW2],
            ] {
                let mut body = AnnotatedMasmBody::new();
                body.push(AnnotatedOp::Boundary(Boundary::UnitStart(id)));
                body.push_instruction(pair[0].clone(), id);
                let second = match barrier {
                    0 => {
                        body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(id)));
                        body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(1))));
                        SelectedStepId(1)
                    }
                    1 => SelectedStepId(u32::MAX),
                    2 => {
                        body.push(AnnotatedOp::Anchor(Anchor::Debug {
                            id: DebugLocationId(0),
                            decorator: DebugDecorator::Variable(masm::DebugVarInfo::new(
                                "v",
                                masm::DebugVarLocation::Unavailable,
                            )),
                        }));
                        id
                    }
                    _ => {
                        body.push(AnnotatedOp::Repeat {
                            id: ControlId(0),
                            count: 1,
                            body: AnnotatedMasmBody::new(),
                        });
                        id
                    }
                };
                body.push_instruction(pair[1].clone(), second);
                simplify_annotated(&mut body);
                assert_eq!(body.top_level_instructions().cloned().collect::<Vec<_>>(), pair);
            }
        }
    }

    #[test]
    fn swap_swap_cancels() {
        assert_simplifies(&["swap.1", "swap.1"], &[]);
        assert_simplifies(&["swap.3", "swap.3"], &[]);
    }

    #[test]
    fn swap_of_different_depths_stays() {
        assert_simplifies(&["swap.1", "swap.2"], &["swap.1", "swap.2"]);
    }

    #[test]
    fn movup_movdn_cancels() {
        assert_simplifies(&["movup.3", "movdn.3"], &[]);
        assert_simplifies(&["movup.3", "movdn.2"], &["movup.3", "movdn.2"]);
    }

    #[test]
    fn movdn_movup_cancels() {
        assert_simplifies(&["movdn.3", "movup.3"], &[]);
        assert_simplifies(&["movdn.4", "movup.3"], &["movdn.4", "movup.3"]);
    }

    #[test]
    fn push_drop_cancels() {
        assert_simplifies(&["push.5", "drop"], &[]);
        // A word is four elements, of which `drop` drops one
        assert_simplifies(&["push.[1,2,3,4]", "drop"], &["push.[1,2,3,4]", "drop"]);
    }

    #[test]
    fn dup_drop_cancels() {
        assert_simplifies(&["dup.2", "drop"], &[]);
        assert_simplifies(&["dup.0", "dropw"], &["dup.0", "dropw"]);
    }

    #[test]
    fn cancellation_cascades() {
        assert_simplifies(&["swap.1", "push.0", "drop", "swap.1"], &[]);
        assert_simplifies(&["add", "movup.2", "dup.1", "drop", "movdn.2", "mul"], &["add", "mul"]);
    }

    #[test]
    fn nested_blocks_are_simplified() {
        let mut ops = parse(&[
            "if.true",
            "swap.1 swap.1",
            "else",
            "push.1 drop add",
            "end",
            "while.true",
            "dup.1 drop",
            "end",
            "repeat.2",
            "movup.2 movdn.2 mul",
            "end",
        ]);
        simplify(&mut ops);
        let [
            masm::Op::If {
                then_blk, else_blk, ..
            },
            masm::Op::While {
                body: loop_body, ..
            },
            masm::Op::Repeat {
                body: repeat_body, ..
            },
        ] = &ops[..]
        else {
            panic!("expected an if, a while and a repeat: {}", render(ops));
        };
        assert!(then_blk.is_empty(), "{}", then_blk.to_pretty_string());
        assert_eq!(else_blk.to_pretty_string(), render(parse(&["add"])));
        assert!(loop_body.is_empty(), "{}", loop_body.to_pretty_string());
        assert_eq!(repeat_body.to_pretty_string(), render(parse(&["mul"])));
    }

    #[test]
    fn a_window_never_spans_a_block() {
        let from = ["swap.1", "if.true", "add", "else", "mul", "end", "swap.1"];
        assert_simplifies(&from, &from);
    }
}
