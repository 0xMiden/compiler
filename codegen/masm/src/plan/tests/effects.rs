use alloc::{format, vec::Vec};

use miden_core::Felt;
use miden_processor::{DefaultHost, FastProcessor, StackInputs};
use midenc_hir::Report;

use crate::{masm::Instruction, plan::*};

#[test]
fn ordinary_arithmetic_target_rules_cover_actual_raw_shapes() {
    use Instruction::*;
    for (instruction, consumed, outputs) in [
        (Lt, 2, 1),
        (U32Split, 1, 2),
        (U32AssertW, 4, 4),
        (CDrop, 3, 1),
        (CDropW, 9, 4),
        (Eqw, 8, 9),
        (Ext2Add, 4, 2),
        (Ext2Div, 4, 2),
        (Ext2Inv, 2, 2),
        (U32WideningMul, 2, 2),
        (U32Clz, 1, 1),
    ] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert_eq!(effect.consumed, consumed, "{instruction:?}");
        assert_eq!(effect.outputs.len(), outputs, "{instruction:?}");
    }
}

/// Apply the declarative lane expressions, then compare them with processor execution.
/// The test contains no instruction-specific stack simulator.
#[test]
fn target_effects_match_vm() -> Result<(), Report> {
    let values = (0..16)
        .map(|i| Felt::new_unchecked(0xffff_fffe_0000_0100 + i))
        .collect::<Vec<_>>();
    let top = values.clone();
    let primitives = [
        Primitive::Dup(0),
        Primitive::Dup(1),
        Primitive::Dup(15),
        Primitive::Swap(1),
        Primitive::Swap(15),
        Primitive::MoveUp(1),
        Primitive::MoveUp(2),
        Primitive::MoveUp(15),
        Primitive::MoveDown(1),
        Primitive::MoveDown(2),
        Primitive::MoveDown(15),
        Primitive::DupWord(0),
        Primitive::DupWord(3),
        Primitive::SwapWord(1),
        Primitive::SwapWord(3),
        Primitive::Drop,
        Primitive::Push(Felt::new_unchecked(0xffff_ffff_0000_0000)),
    ];
    let indexed = (0..16)
        .map(Primitive::Dup)
        .chain((1..16).map(Primitive::Swap))
        .chain((1..16).map(Primitive::MoveUp))
        .chain((1..16).map(Primitive::MoveDown))
        .chain((0..4).map(Primitive::DupWord))
        .chain((1..4).map(Primitive::SwapWord));
    let instructions = primitives
        .into_iter()
        .chain(indexed)
        .map(|primitive| primitive.instruction().unwrap())
        .chain([Instruction::Nop, Instruction::DropW, Instruction::PadW]);
    for instruction in instructions {
        let effect = TargetEffects::instruction(&instruction)?;
        let mut expected = effect
            .outputs
            .iter()
            .map(|lane| match lane {
                LaneEffect::Input(index) => top[usize::from(*index)],
                LaneEffect::Literal(value) => *value,
                other => panic!("unexpected effect {other:?}"),
            })
            .chain(top.iter().skip(usize::from(effect.consumed)).copied())
            .collect::<Vec<_>>();
        expected.resize(expected.len().max(16), Felt::ZERO);
        let assertions = expected
            .iter()
            .map(|value| format!("push.{value} assert_eq"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            run(&format!("{instruction} {assertions}"), &values)?,
            [Felt::ZERO; 16],
            "{instruction}"
        );
    }
    for primitive in [
        Primitive::Dup(16),
        Primitive::Swap(0),
        Primitive::Swap(16),
        Primitive::MoveUp(0),
        Primitive::MoveDown(16),
        Primitive::DupWord(4),
        Primitive::SwapWord(0),
        Primitive::SwapWord(4),
    ] {
        assert!(primitive.instruction().is_err(), "{primitive:?}");
    }
    for illegal in ["dup.16", "swap.0", "swap.16", "movup.16", "movdn.16", "dupw.4", "swapw.4"] {
        assert!(
            miden_assembly::Assembler::default()
                .assemble_program("illegal", format!("begin {illegal} end"))
                .is_err(),
            "{illegal}"
        );
    }
    assert!(TargetEffects::instruction(&Instruction::AdvPush).unwrap().may_trap);
    Ok(())
}

#[test]
fn raw_slots_address_generation_and_word_boundaries_match_vm() -> Result<(), Report> {
    let high = 0xffff_ffff_0000_0000u64;
    let body = format!(
        "@locals(4) proc test push.{high} loc_store.3 loc_load.3 locaddr.3 locaddr.0 sub push.3 \
         assert_eq swap.1 drop end begin exec.test end"
    );
    let output = run_program(&body, &[])?;
    assert_eq!(output[0], Felt::new_unchecked(high));
    let store = TargetEffects::instruction(&Instruction::LocStore(3.into()))?;
    assert_eq!(store.consumed, 1);
    assert!(store.outputs.is_empty());
    assert_eq!(
        store.memory,
        MemoryEffect::Write(MemoryRegion::Frame {
            offset: 3,
            width: 1
        })
    );
    let load = TargetEffects::instruction(&Instruction::LocLoad(3.into()))?;
    assert_eq!(load.outputs, [LaneEffect::Memory { lane: 0 }]);
    let address = TargetEffects::instruction(&Instruction::Locaddr(3.into()))?;
    assert_eq!(address.outputs, [LaneEffect::FrameAddress(3)]);
    assert_eq!(
        TargetEffects::instruction(&Instruction::MemLoad)?.memory,
        MemoryEffect::Read(MemoryRegion::Indirect {
            address: 0,
            width: 1
        })
    );
    for offset in [0, 4] {
        let body = format!(
            "@locals(8) proc test push.101.202.303.{high} loc_storew_le.{offset} dropw padw \
             loc_loadw_le.{offset} push.{high} assert_eq push.303 assert_eq push.202 assert_eq \
             push.101 assert_eq end begin exec.test end"
        );
        assert_eq!(run_program(&body, &[])?, [Felt::ZERO; 16]);
        let effect = TargetEffects::instruction(&Instruction::LocLoadWLe(offset.into()))?;
        assert_eq!(
            effect.memory,
            MemoryEffect::Read(MemoryRegion::Frame {
                offset: offset.into(),
                width: 4
            })
        );
    }
    for offset in [1, 2, 3, 5] {
        let body =
            format!("@locals(8) proc test padw loc_loadw_le.{offset} end begin exec.test end");
        assert!(run_program(&body, &[]).is_err(), "unaligned word offset {offset}");
    }
    assert!(
        miden_assembly::Assembler::default()
            .assemble_program(
                "frame_max",
                "@locals(65532) proc callee nop end begin exec.callee end"
            )
            .is_ok()
    );
    assert!(
        miden_assembly::Assembler::default()
            .assemble_program(
                "frame_over",
                "@locals(65533) proc callee nop end begin exec.callee end"
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn floor_pops_and_pushes_match_vm() -> Result<(), Report> {
    let effect = FloorState {
        owned: 2,
        protected: 14,
        padding: 0,
    }
    .pop(2)
    .unwrap();
    assert_eq!(
        effect,
        FloorState {
            owned: 0,
            protected: 14,
            padding: 2
        }
    );
    assert_eq!(
        effect.push(2).unwrap(),
        FloorState {
            owned: 2,
            protected: 14,
            padding: 2
        }
    );
    assert!(effect.pop(1).is_err());
    let assertions = [8, 9, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 0, 0]
        .iter()
        .map(|value| format!("push.{value} assert_eq"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        run(
            &format!("drop drop push.9 push.8 sdepth push.18 assert_eq {assertions}"),
            &(1..=16).map(Felt::new_unchecked).collect::<Vec<_>>()
        )?,
        [Felt::ZERO; 16]
    );
    Ok(())
}

pub(super) fn run(body: &str, inputs: &[Felt]) -> Result<Vec<Felt>, Report> {
    run_program(&format!("begin {body} end"), inputs)
}

pub(super) fn run_program(source: &str, inputs: &[Felt]) -> Result<Vec<Felt>, Report> {
    let package = miden_assembly::Assembler::default().assemble_program("plan_target", source)?;
    let program = package.try_into_program()?;
    let output = FastProcessor::new(StackInputs::new(inputs).unwrap())
        .execute_sync(&program, &mut DefaultHost::default())
        .map_err(|error| Report::msg(error.to_string()))?;
    Ok((0..16).map(|i| output.stack.get_element(i).unwrap()).collect())
}

#[test]
fn word_effects_preserve_actual_cell_order() -> Result<(), Report> {
    let top = [0xffff_ffff_0000_0000, 0xffff_fffe_1234_5678, 303, 101].map(Felt::new_unchecked);
    for (load, store) in [
        (Instruction::LocLoadWLe(0.into()), Instruction::LocStoreWLe(0.into())),
        (Instruction::LocLoadWBe(0.into()), Instruction::LocStoreWBe(0.into())),
    ] {
        let write = TargetEffects::instruction(&store)?;
        let cell_checks = write
            .memory_inputs
            .iter()
            .enumerate()
            .map(|(cell, input)| {
                format!("loc_load.{cell} push.{} assert_eq", top[usize::from(*input)])
            })
            .collect::<Vec<_>>()
            .join(" ");
        let push = top
            .iter()
            .rev()
            .map(|value| format!("push.{value}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "@locals(4) proc test {push} {store} dropw {cell_checks} end begin exec.test end"
        );
        assert_eq!(run_program(&source, &[])?, [Felt::ZERO; 16]);
        let read = TargetEffects::instruction(&load)?;
        let cell_writes = top
            .iter()
            .enumerate()
            .map(|(cell, value)| format!("push.{value} loc_store.{cell}"))
            .collect::<Vec<_>>()
            .join(" ");
        let checks = read
            .outputs
            .iter()
            .map(|lane| match lane {
                LaneEffect::Memory { lane } => {
                    format!("push.{} assert_eq", top[usize::from(*lane)])
                }
                _ => panic!("expected memory lane"),
            })
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "@locals(4) proc test {cell_writes} padw {load} {checks} end begin exec.test end"
        );
        assert_eq!(run_program(&source, &[])?, [Felt::ZERO; 16]);
    }
    Ok(())
}

#[test]
fn unknown_rules_and_unresolved_immediates_fail_closed() {
    for instruction in [
        Instruction::AdvLoadW,
        Instruction::CryptoStream,
        Instruction::MTreeGet,
        Instruction::Emit,
        Instruction::EmitImm(crate::masm::Immediate::from(Felt::new_unchecked(999)).into()),
    ] {
        assert!(TargetEffects::instruction(&instruction).is_err(), "{instruction}");
    }
    for instruction in [
        Instruction::DivImm(Felt::ZERO.into()),
        Instruction::U32DivImm(0.into()),
        Instruction::U32DivModImm(0.into()),
        Instruction::U32ModImm(0.into()),
        Instruction::U32ShlImm(32.into()),
        Instruction::U32ShrImm(32.into()),
        Instruction::U32RotlImm(32.into()),
        Instruction::U32RotrImm(32.into()),
        Instruction::ExpBitLength(64),
        Instruction::EqImm(crate::masm::Immediate::Constant(
            crate::masm::Ident::new("field_constant").unwrap(),
        )),
    ] {
        assert!(TargetEffects::instruction(&instruction).is_err(), "{instruction}");
    }
    assert!(
        TargetEffects::instruction(&Instruction::LocLoad(crate::masm::Immediate::Constant(
            crate::masm::Ident::new("offset").unwrap()
        )))
        .is_err()
    );
    for offset in [1, 2, 3] {
        assert!(TargetEffects::instruction(&Instruction::LocLoadWLe(offset.into())).is_err());
        assert!(TargetEffects::instruction(&Instruction::LocStoreWBe(offset.into())).is_err());
    }
}

#[test]
fn opaque_binary_effects_match_vm() -> Result<(), Report> {
    for (instruction, expected) in [
        (Instruction::Add, 12),
        (Instruction::Sub, 2),
        (Instruction::Mul, 35),
        (Instruction::Eq, 0),
        (Instruction::Neq, 1),
    ] {
        let effect = TargetEffects::instruction(&instruction)?;
        assert_eq!(effect.consumed, 2);
        assert_eq!(effect.outputs, [LaneEffect::Computed(0)]);
        assert_eq!(effect.floor, [FloorStep::Pop(1)]);
        let input = [Felt::new_unchecked(5), Felt::new_unchecked(7), Felt::new_unchecked(91)];
        run(&format!("{instruction} push.{expected} assert_eq push.91 assert_eq"), &input)?;
    }
    Ok(())
}

#[test]
fn indirect_memory_effects_expose_the_address_for_provenance_checking() -> Result<(), Report> {
    let load = TargetEffects::instruction(&Instruction::MemLoad)?;
    let store = TargetEffects::instruction(&Instruction::MemStore)?;
    assert_eq!(
        load.memory,
        MemoryEffect::Read(MemoryRegion::Indirect {
            address: 0,
            width: 1
        })
    );
    assert_eq!(
        store.memory,
        MemoryEffect::Write(MemoryRegion::Indirect {
            address: 0,
            width: 1
        })
    );
    assert_eq!(store.memory_inputs, [1]);
    let high = 0xffff_ffff_0000_0000u64;
    let output = run(
        &format!(
            "push.991 push.{high} push.100 mem_store push.100 mem_load push.{high} assert_eq \
             push.991 assert_eq"
        ),
        &[],
    )?;
    assert_eq!(output, [Felt::ZERO; 16]);
    // The same dynamic primitives can target compiler-frame storage. Their address must be
    // resolved using token provenance; it cannot be classified as unknown source memory.
    let source = format!(
        "@locals(4) proc test push.991 push.{high} locaddr.3 mem_store locaddr.3 mem_load \
         push.{high} assert_eq push.991 assert_eq end begin exec.test end"
    );
    assert_eq!(run_program(&source, &[])?, [Felt::ZERO; 16]);
    Ok(())
}

#[test]
fn real_lowering_effects_match_vm() -> Result<(), Report> {
    use LaneEffect::Computed;
    let cases = [
        (Instruction::U32DivModImm(4.into()), vec![9], vec![1, 2]),
        (Instruction::U32OverflowingAdd, vec![u32::MAX as u64, 1], vec![1, 0]),
        (Instruction::U32WrappingAdd, vec![u32::MAX as u64, 2], vec![1]),
        (Instruction::U32WrappingSubImm(1.into()), vec![0], vec![u32::MAX as u64]),
        (Instruction::U32WrappingMul, vec![8, 3], vec![24]),
        (Instruction::U32Gt, vec![2, 9], vec![1]),
        (Instruction::U32Lt, vec![9, 2], vec![1]),
        (Instruction::AddImm(Felt::new_unchecked(7).into()), vec![5], vec![12]),
        (Instruction::MulImm(Felt::new_unchecked(8).into()), vec![5], vec![40]),
        (Instruction::EqImm(Felt::ZERO.into()), vec![0], vec![1]),
        (Instruction::And, vec![1, 0], vec![0]),
        (Instruction::U32Shr, vec![8, 0x1234], vec![0x12]),
        (Instruction::U32Shl, vec![8, 0x12], vec![0x1200]),
        (Instruction::U32And, vec![0xff, 0x1234], vec![0x34]),
        (Instruction::U32Or, vec![0xff, 0x1200], vec![0x12ff]),
        (Instruction::U32Not, vec![0], vec![u32::MAX as u64]),
    ];
    for (instruction, inputs, outputs) in cases {
        let effect = TargetEffects::instruction(&instruction)?;
        assert_eq!(effect.consumed as usize, inputs.len(), "{instruction}");
        assert_eq!(
            effect.outputs,
            (0..outputs.len()).map(|i| Computed(i as u8)).collect::<Vec<_>>(),
            "{instruction}"
        );
        let top = inputs
            .iter()
            .copied()
            .chain([101, 103, 107])
            .map(Felt::new_unchecked)
            .collect::<Vec<_>>();
        let assertions = outputs
            .into_iter()
            .chain([101, 103, 107])
            .map(|value| format!("push.{value} assert_eq "))
            .collect::<String>();
        assert_eq!(
            run(&format!("{instruction} {assertions}"), &top)?,
            [Felt::ZERO; 16],
            "{instruction}"
        );
        let state = effect.floor.iter().try_fold(
            FloorState {
                owned: top.len(),
                protected: 0,
                padding: 16 - top.len(),
            },
            |s, step| match step {
                FloorStep::Push(n) => s.push(*n as usize),
                FloorStep::Pop(n) => s.pop(*n as usize),
            },
        )?;
        assert_eq!(state.owned, top.len() - inputs.len() + effect.outputs.len());
    }
    Ok(())
}

#[test]
fn root_word_store_and_frame_events_match_vm() -> Result<(), Report> {
    let effect = TargetEffects::instruction(&Instruction::MemStoreWLe)?;
    assert_eq!(effect.consumed, 5);
    assert_eq!(effect.outputs, (1..5).map(LaneEffect::Input).collect::<Vec<_>>());
    assert_eq!(
        effect.memory,
        MemoryEffect::Write(MemoryRegion::Indirect {
            address: 0,
            width: 4
        })
    );
    assert_eq!(effect.memory_inputs, [1, 2, 3, 4]);
    assert_eq!(effect.floor, [FloorStep::Pop(1)]);
    let roots = [0xffff_ffff_0000_0000u64, 13, 17, 19];
    let inputs = core::iter::once(128)
        .chain(roots)
        .chain([101, 103, 107])
        .map(Felt::new_unchecked)
        .collect::<Vec<_>>();
    let memory = roots
        .iter()
        .enumerate()
        .map(|(i, v)| format!("mem_load.{} push.{v} assert_eq ", 128 + i))
        .collect::<String>();
    let values = roots
        .into_iter()
        .chain([101, 103, 107])
        .map(|v| format!("push.{v} assert_eq "))
        .collect::<String>();
    let events = [
        Instruction::EmitImm(crate::Event::FrameStart.into()),
        Instruction::EmitImm(crate::Event::FrameEnd.into()),
    ];
    for instruction in &events {
        let effect = TargetEffects::instruction(instruction)?;
        assert_eq!(effect.consumed, 0);
        assert!(effect.outputs.is_empty());
    }
    let program = miden_assembly::Assembler::default()
        .assemble_program(
            "root_word_effect",
            format!(
                "begin mem_storew_le sdepth push.16 assert_eq {memory} {} {} {values} end",
                events[0], events[1]
            ),
        )?
        .try_into_program()?;
    let output = FastProcessor::new(StackInputs::new(&inputs).unwrap())
        .execute_sync(&program, &mut super::lowering_units::host())
        .map_err(|e| Report::msg(e.to_string()))?;
    assert_eq!(
        (0..16).map(|i| output.stack.get_element(i).unwrap()).collect::<Vec<_>>(),
        [Felt::ZERO; 16]
    );
    assert!(
        TargetEffects::instruction(&Instruction::EmitImm(crate::masm::EventImmediate::Immediate(
            Felt::ONE.into()
        )))
        .is_err()
    );
    Ok(())
}

#[test]
fn checked_table_lookup_effects_match_vm() -> Result<(), Report> {
    let values = (5..21).map(Felt::new_unchecked).collect::<Vec<_>>();
    for (instruction, answer) in [
        (Instruction::AddImm(Felt::ZERO.into()), 5),
        (Instruction::AddImm(Felt::ONE.into()), 6),
        (Instruction::AddImm(Felt::new_unchecked(2).into()), 7),
        (Instruction::AddImm(Felt::new_unchecked(9).into()), 14),
        (Instruction::MulImm(Felt::ZERO.into()), 0),
        (Instruction::MulImm(Felt::ONE.into()), 5),
        (Instruction::MulImm(Felt::new_unchecked(8).into()), 40),
    ] {
        let effect = TargetEffects::instruction(&instruction)?;
        let mut floor = FloorState {
            owned: 1,
            protected: 15,
            padding: 0,
        };
        for step in &effect.floor {
            floor = match step {
                FloorStep::Push(n) => floor.push(*n as usize),
                FloorStep::Pop(n) => floor.pop(*n as usize),
            }?;
        }
        assert_eq!(effect.consumed, 1);
        let expected_lane = match &instruction {
            Instruction::AddImm(value) if value.expect_value() == Felt::ZERO => {
                LaneEffect::Input(0)
            }
            Instruction::MulImm(value) if value.expect_value() == Felt::ONE => LaneEffect::Input(0),
            Instruction::MulImm(value) if value.expect_value() == Felt::ZERO => {
                LaneEffect::Literal(Felt::ZERO)
            }
            _ => LaneEffect::Computed(0),
        };
        assert_eq!(effect.outputs, [expected_lane]);
        let output = run(
            &alloc::format!(
                "{instruction} sdepth push.{} assert_eq push.{answer} assert_eq",
                floor.owned + floor.protected + floor.padding
            ),
            &values,
        )?;
        assert_eq!(&output[..15], &values[1..]);
    }
    Ok(())
}

#[test]
fn equality_assertion_floor_and_failure_match_vm() -> Result<(), Report> {
    let mut values = (7..23).map(Felt::new_unchecked).collect::<Vec<_>>();
    values[1] = values[0];
    let output = run("assert_eq sdepth push.16 assert_eq", &values)?;
    assert_eq!(&output[..14], &values[2..]);
    assert_eq!(&output[14..], &[Felt::ZERO; 2]);
    values[1] = Felt::new_unchecked(99);
    assert!(run("assert_eq", &values).is_err());
    Ok(())
}

fn arithmetic_cases() -> Vec<(Instruction, Vec<u64>, Vec<u64>)> {
    use Instruction::*;
    let mut cases = Vec::new();
    macro_rules! case {
        ($op:expr, [$($input:expr),*], [$($output:expr),*]) => {
            cases.push(($op, alloc::vec![$($input),*], alloc::vec![$($output),*]));
        };
    }
    case!(Div, [2, 14], [7]);
    case!(Neg, [0], [0]);
    case!(Incr, [7], [8]);
    case!(Inv, [1], [1]);
    case!(Not, [0], [1]);
    case!(And, [1, 0], [0]);
    case!(Or, [1, 0], [1]);
    case!(Xor, [1, 0], [1]);
    for op in [Lt, Lte, Gt, Gte] {
        let expected = u64::from(matches!(op, Lt | Lte));
        case!(op, [0xffff_ffff_0000_0000, 0xffff_fffe_ffff_ffff], [expected]);
    }
    case!(IsOdd, [0xffff_ffff_0000_0000], [0]);
    case!(Pow2, [7], [128]);
    case!(Exp, [3, 5], [125]);
    case!(ExpBitLength(3), [3, 5], [125]);
    case!(ILog2, [0xffff_ffff_0000_0000], [63]);
    case!(Eqw, [2, 3, 5, 7, 2, 3, 5, 7], [1, 2, 3, 5, 7, 2, 3, 5, 7]);
    case!(Assert, [1], []);
    case!(Assertz, [0], []);
    case!(AssertEq, [13, 13], []);
    case!(AssertEqw, [2, 3, 5, 7, 2, 3, 5, 7], []);
    case!(U32Assert, [31], [31]);
    case!(U32Assert2, [31, 32], [31, 32]);
    case!(U32AssertW, [31, 32, 33, 34], [31, 32, 33, 34]);
    case!(U32Split, [0x1234_5678_9abc_def0], [0x9abc_def0, 0x1234_5678]);
    case!(U32Cast, [0x1234_5678_9abc_def0], [0x9abc_def0]);
    for (op, expected) in [
        (U32And, 1),
        (U32Or, 7),
        (U32Xor, 6),
        (U32WrappingAdd, 8),
        (U32WrappingSub, 2),
        (U32WrappingMul, 15),
        (U32Lt, 0),
        (U32Lte, 0),
        (U32Gt, 1),
        (U32Gte, 1),
        (U32Div, 1),
        (U32Mod, 2),
        (U32Min, 3),
        (U32Max, 5),
    ] {
        case!(op, [3, 5], [expected]);
    }
    case!(U32Not, [3], [u32::MAX as u64 - 3]);
    case!(U32OverflowingAdd, [1, u32::MAX as u64], [1, 0]);
    case!(U32OverflowingSub, [1, 0], [1, u32::MAX as u64]);
    case!(U32WideningMul, [2, u32::MAX as u64], [0xffff_fffe, 1]);
    case!(U32DivMod, [3, 14], [2, 4]);
    case!(U32WrappingAdd3, [1, 2, 3], [6]);
    case!(U32WrappingMadd, [3, 7, 11], [32]);
    case!(U32WrappingMadd, [0xffff_ffff, 0xffff_ffff, 0xffff_ffff], [0]);
    for (op, expected) in [(U32Shl, 20), (U32Shr, 1), (U32Rotl, 20), (U32Rotr, 0x4000_0001)] {
        case!(op, [2, 5], [expected]);
    }
    for (op, expected) in [(U32Popcnt, 4), (U32Clz, 24), (U32Clo, 0), (U32Ctz, 4), (U32Cto, 0)] {
        case!(op, [0xf0], [expected]);
    }
    case!(Ext2Add, [2, 3, 5, 7], [7, 10]);
    case!(Ext2Sub, [2, 3, 5, 7], [3, 4]);
    case!(Ext2Mul, [2, 3, 5, 7], [157, 29]);
    case!(Ext2Div, [1, 0, 5, 7], [5, 7]);
    case!(Ext2Inv, [1, 0], [1, 0]);
    case!(Ext2Neg, [0, 0], [0, 0]);
    for predicate in [0, 1] {
        case!(CDrop, [predicate, 19, 23], [if predicate == 1 { 19 } else { 23 }]);
        let expected = if predicate == 1 {
            [2, 3, 5, 7]
        } else {
            [11, 13, 17, 19]
        };
        cases.push((CDropW, alloc::vec![predicate, 2, 3, 5, 7, 11, 13, 17, 19], expected.to_vec()));
    }
    for value in [0, 1, 2, 7, 8] {
        let felt = Felt::new_unchecked(value);
        case!(AddImm(felt.into()), [5], [5 + value]);
        case!(SubImm(felt.into()), [19], [19 - value]);
        case!(MulImm(felt.into()), [5], [5 * value]);
        case!(ExpImm(felt.into()), [2], [1 << value]);
        case!(EqImm(felt.into()), [5], [0]);
        case!(NeqImm(felt.into()), [5], [1]);
        case!(U32WrappingAddImm((value as u32).into()), [5], [5 + value]);
        case!(U32WrappingSubImm((value as u32).into()), [19], [19 - value]);
        case!(U32WrappingMulImm((value as u32).into()), [5], [5 * value]);
        case!(U32OverflowingAddImm((value as u32).into()), [5], [0, 5 + value]);
        case!(U32OverflowingSubImm((value as u32).into()), [19], [0, 19 - value]);
        case!(U32WideningMulImm((value as u32).into()), [5], [5 * value, 0]);
        if value != 0 {
            case!(DivImm(felt.into()), [value * 5], [5]);
            case!(U32DivImm((value as u32).into()), [19], [19 / value]);
            case!(U32ModImm((value as u32).into()), [19], [19 % value]);
            case!(U32DivModImm((value as u32).into()), [19], [19 % value, 19 / value]);
        }
    }
    for shift in [0u8, 1, 31] {
        case!(U32ShlImm(shift.into()), [5], [5u32.wrapping_shl(shift.into()) as u64]);
        case!(U32ShrImm(shift.into()), [5], [5u32.wrapping_shr(shift.into()) as u64]);
        case!(U32RotlImm(shift.into()), [5], [5u32.rotate_left(shift.into()) as u64]);
        case!(U32RotrImm(shift.into()), [5], [5u32.rotate_right(shift.into()) as u64]);
    }
    let word = miden_assembly_syntax::parser::WordValue([2, 3, 5, 7].map(Felt::new_unchecked));
    case!(Push(crate::masm::PushValue::from(word).into()), [], [2, 3, 5, 7]);
    cases
}

#[test]
fn arithmetic_and_fixed_target_rules_match_pinned_assembler_microsteps() -> Result<(), Report> {
    use miden_core::operations::Operation as O;
    for instruction in arithmetic_cases().into_iter().map(|(instruction, ..)| instruction).chain([
        Instruction::Hash,
        Instruction::HMerge,
        Instruction::HPerm,
        Instruction::AdvPush,
        Instruction::Caller,
        Instruction::Clk,
    ]) {
        // A library procedure has no program-entry FMP initialization prologue.
        // Inspect its exact exported root rather than subtracting an assumed prefix.
        let manager = alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
        let module =
            miden_assembly::ast::Module::parser(Some(miden_assembly::ast::ModuleKind::Library))
                .parse_str(
                    Some(miden_assembly::Path::new("::arithmetic_microsteps")),
                    format!("pub proc test {instruction} end"),
                    manager.clone(),
                )?;
        let package = miden_assembly::Assembler::new(manager).assemble_library(
            "arithmetic_microsteps",
            module,
            core::iter::empty::<&str>(),
        )?;
        let export = package.manifest.exports().next().unwrap().as_procedure().unwrap();
        let block = package.mast_forest()[export.node.unwrap()].unwrap_basic_block();
        let actual = block
            .operations()
            .filter_map(|op| match op {
                O::Push(_)
                | O::Clk
                | O::Pad
                | O::AdvPop
                | O::U32split
                | O::Dup0
                | O::Dup1
                | O::Dup2
                | O::Dup3
                | O::Dup4
                | O::Dup5
                | O::Dup6
                | O::Dup7
                | O::Dup9
                | O::Dup11
                | O::Dup13
                | O::Dup15 => Some(FloorStep::Push(1)),
                O::Drop
                | O::Assert(_)
                | O::Add
                | O::Mul
                | O::And
                | O::Or
                | O::Eq
                | O::U32and
                | O::U32xor
                | O::U32add3
                | O::U32madd
                | O::CSwap
                | O::CSwapW => Some(FloorStep::Pop(1)),
                O::Noop
                | O::HPerm
                | O::Caller
                | O::Neg
                | O::Inv
                | O::Incr
                | O::Not
                | O::Eqz
                | O::Expacc
                | O::Ext2Mul
                | O::U32assert2(_)
                | O::U32add
                | O::U32sub
                | O::U32mul
                | O::U32div
                | O::Swap
                | O::SwapW
                | O::SwapW2
                | O::SwapW3
                | O::SwapDW
                | O::MovUp2
                | O::MovUp3
                | O::MovUp4
                | O::MovUp5
                | O::MovUp6
                | O::MovUp7
                | O::MovUp8
                | O::MovDn2
                | O::MovDn3
                | O::MovDn4
                | O::MovDn5
                | O::MovDn6
                | O::MovDn7
                | O::MovDn8
                | O::Emit => None,
                other => panic!("unregistered arithmetic micro-operation {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(TargetEffects::instruction(&instruction)?.floor, actual, "{instruction}");
    }
    Ok(())
}

#[test]
fn arithmetic_outputs_suffix_and_physical_floor_match_vm() -> Result<(), Report> {
    for (instruction, inputs, outputs) in arithmetic_cases() {
        let effect = TargetEffects::instruction(&instruction)?;
        assert_eq!(effect.consumed as usize, inputs.len(), "{instruction}");
        assert_eq!(effect.outputs.len(), outputs.len(), "{instruction}");
        for (lane, expected) in effect.outputs.iter().zip(&outputs) {
            let preserved = match *lane {
                LaneEffect::Input(index) => Some(inputs[index as usize]),
                LaneEffect::Literal(value) => Some(value.as_canonical_u64()),
                LaneEffect::Conditional {
                    condition,
                    if_true,
                    if_false,
                } => Some(
                    inputs[if inputs[condition as usize] == 1 {
                        if_true
                    } else {
                        if_false
                    } as usize],
                ),
                LaneEffect::Computed(_) => None,
                other => panic!("unexpected arithmetic lane {other:?}"),
            };
            if let Some(value) = preserved {
                assert_eq!(value, *expected, "{instruction}");
            }
        }
        for suffix_len in [0, 24] {
            let suffix = (0..suffix_len).map(|i| 1001 + i as u64).collect::<Vec<_>>();
            let mut state = FloorState {
                owned: inputs.len() + suffix.len(),
                protected: 0,
                // Inputs are pushed by the program above the VM's original sixteen zeroes.
                padding: if suffix_len == 0 {
                    16 - inputs.len()
                } else {
                    16
                },
            };
            for step in &effect.floor {
                state = match *step {
                    FloorStep::Push(n) => state.push(n.into())?,
                    FloorStep::Pop(n) => state.pop(n.into())?,
                };
            }
            let push = inputs
                .iter()
                .chain(&suffix)
                .rev()
                .map(|value| format!("push.{value}"))
                .collect::<Vec<_>>()
                .join(" ");
            let assertions = outputs
                .iter()
                .chain(&suffix)
                .map(|value| format!("push.{value} assert_eq"))
                .collect::<Vec<_>>()
                .join(" ");
            let push = if suffix_len == 0 {
                alloc::string::String::new()
            } else {
                push
            };
            let vm_inputs = if suffix_len == 0 {
                inputs.iter().copied().map(Felt::new_unchecked).collect::<Vec<_>>()
            } else {
                vec![]
            };
            let source = format!(
                "{push} {instruction} sdepth push.{} assert_eq {assertions}",
                state.owned + state.padding
            );
            assert_eq!(
                run(&source, &vm_inputs)?,
                [Felt::ZERO; 16],
                "{instruction}, suffix={suffix_len}"
            );
        }
    }
    Ok(())
}

#[test]
fn memory_wrapping_madd_has_exact_target_rule() {
    let effect = TargetEffects::instruction(&Instruction::U32WrappingMadd).unwrap();
    assert_eq!(effect.consumed, 3);
    assert_eq!(effect.outputs.len(), 1);
    assert!(effect.may_trap);
    assert_eq!(effect.floor, [FloorStep::Pop(1), FloorStep::Pop(1)]);
}

#[test]
fn memory_wrapping_madd_rejects_each_invalid_u32_limb() {
    for lane in 0..3 {
        let mut inputs = [Felt::ONE; 3];
        inputs[lane] = Felt::new_unchecked(1u64 << 32);
        assert!(run("u32wrapping_madd", &inputs).is_err(), "invalid lane {lane}");
    }
}

#[test]
fn fixed_primitive_target_rules_describe_actual_owned_windows() {
    for (i, n, m) in [
        (Instruction::Hash, 4, 4),
        (Instruction::HMerge, 8, 4),
        (Instruction::HPerm, 12, 12),
        (Instruction::AdvPush, 0, 1),
        (Instruction::Caller, 4, 4),
        (Instruction::Clk, 0, 1),
    ] {
        let effect = TargetEffects::instruction(&i).unwrap();
        assert_eq!(effect.consumed, n);
        assert_eq!(effect.outputs.len(), m);
    }
}

#[test]
fn fixed_primitives_preserve_physical_floor_and_raw_caller_suffix() -> Result<(), Report> {
    use miden_core::crypto::hash::Poseidon2;
    for (instruction, n) in
        [(Instruction::Hash, 4), (Instruction::HMerge, 8), (Instruction::HPerm, 12)]
    {
        let input = (0..n).map(|i| Felt::new_unchecked(101 + i as u64 * 37)).collect::<Vec<_>>();
        let expected = if n == 12 {
            let mut state: [Felt; 12] = input.clone().try_into().unwrap();
            Poseidon2::apply_permutation(&mut state);
            state.to_vec()
        } else {
            Poseidon2::hash_elements(&input).as_elements().to_vec()
        };
        let actual = run(&instruction.to_string(), &input)?;
        assert_eq!(&actual[..expected.len()], expected);
        assert!(actual[expected.len()..].iter().all(|f| *f == Felt::ZERO));
        let effect = TargetEffects::instruction(&instruction)?;
        let state = effect.floor.iter().try_fold(
            FloorState {
                owned: n,
                protected: 0,
                padding: 16 - n,
            },
            |s, step| match step {
                FloorStep::Push(n) => s.push(*n as usize),
                FloorStep::Pop(n) => s.pop(*n as usize),
            },
        )?;
        assert_eq!(state.owned, expected.len());
        assert_eq!(state.padding, 16 - expected.len());
    }
    let input = (1..=16).map(Felt::new_unchecked).collect::<Vec<_>>();
    let actual = run("caller", &input)?;
    assert_eq!(&actual[..4], &[Felt::ZERO; 4]);
    assert_eq!(&actual[4..], &input[4..]);
    assert_eq!(TargetEffects::instruction(&Instruction::Caller)?.floor, []);
    Ok(())
}
