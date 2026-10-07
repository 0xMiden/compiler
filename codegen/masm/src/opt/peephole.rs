//! A peephole simplifier over emitted Miden Assembly: a table of instruction sequences that leave
//! the operand stack as it was, and that therefore can go.
//!
//! The emitter schedules each instruction's operands on its own, so the stack movement one
//! instruction ends with can be undone by the next one's. The identities here remove what is left
//! of that once the instructions are emitted; they look at nothing but the instructions.

use alloc::vec::Vec;

use smallvec::SmallVec;

use crate::masm::{self, Immediate, Instruction, PushValue, Span, Spanned};

/// One identity: `matches` inspects a window of `len` adjacent instructions and returns what they
/// are to be replaced with when they fit. Immediates that must agree are compared inside
/// `matches`; that is the whole binding mechanism, and it keeps the table readable.
///
/// Every replacement in the table is empty. A pattern that replaces rather than deletes gives the
/// instructions it inserts the span of the first instruction it matched.
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
const PATTERNS: &[Pattern] = &[
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

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String, sync::Arc};

    use miden_assembly::{ModuleParser, ast::ModuleKind};
    use midenc_hir::formatter::PrettyPrint;
    use midenc_session::diagnostics::{DefaultSourceManager, SourceLanguage, SourceManager};

    use super::*;

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
