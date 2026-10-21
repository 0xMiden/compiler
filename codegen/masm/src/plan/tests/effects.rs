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
        store.memory.write.as_ref().unwrap().region,
        MemoryRegion::Frame {
            offset: 3,
            width: 1
        }
    );
    let load = TargetEffects::instruction(&Instruction::LocLoad(3.into()))?;
    assert_eq!(load.outputs, [LaneEffect::Memory { lane: 0 }]);
    let address = TargetEffects::instruction(&Instruction::Locaddr(3.into()))?;
    assert_eq!(address.outputs, [LaneEffect::FrameAddress(3)]);
    assert_eq!(
        TargetEffects::instruction(&Instruction::MemLoad)?.memory.read,
        Some(MemoryRead::Fixed(MemoryRegion::Indirect {
            address: 0,
            width: 1
        }))
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
            effect.memory.read,
            Some(MemoryRead::Fixed(MemoryRegion::Frame {
                offset: offset.into(),
                width: 4
            }))
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
            .memory
            .write
            .as_ref()
            .unwrap()
            .values
            .iter()
            .enumerate()
            .map(|(cell, input)| {
                format!(
                    "loc_load.{cell} push.{} assert_eq",
                    top[usize::from(match *input {
                        WriteValue::Input(i) | WriteValue::Output(i) => i,
                    })]
                )
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
        Instruction::MTreeVerifyWithError(crate::masm::Immediate::Constant(
            crate::masm::Ident::new("diagnostic").unwrap(),
        )),
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
        load.memory.read,
        Some(MemoryRead::Fixed(MemoryRegion::Indirect {
            address: 0,
            width: 1
        }))
    );
    assert_eq!(
        store.memory.write.as_ref().unwrap().region,
        MemoryRegion::Indirect {
            address: 0,
            width: 1
        }
    );
    assert_eq!(store.memory.write.unwrap().values, [WriteValue::Input(1)]);
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
        effect.memory.write.as_ref().unwrap().region,
        MemoryRegion::Indirect {
            address: 0,
            width: 4
        }
    );
    assert_eq!(
        effect.memory.write.unwrap().values,
        (0..4).map(WriteValue::Output).collect::<Vec<_>>()
    );
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
        Instruction::HornerBase,
        Instruction::HornerExt,
        Instruction::LogDeferred,
        Instruction::FriExt2Fold4,
        Instruction::MemStream,
        Instruction::AdvPipe,
        Instruction::CryptoStream,
        Instruction::MTreeGet,
        Instruction::MTreeSet,
        Instruction::MTreeMerge,
        Instruction::MTreeVerify,
        Instruction::MTreeVerifyWithError(crate::masm::Immediate::Value(crate::masm::Span::new(
            midenc_hir::SourceSpan::UNKNOWN,
            alloc::sync::Arc::<str>::from("bad path"),
        ))),
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
                | O::CSwapW
                | O::FriE2F4 => Some(FloorStep::Pop(1)),
                O::Noop
                | O::HPerm
                | O::HornerBase
                | O::HornerExt
                | O::LogDeferred
                | O::Pipe
                | O::CryptoStream
                | O::MStream
                | O::AdvPopW
                | O::MpVerify(_)
                | O::MrUpdate
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

#[test]
fn advice_word_target_rule_is_trapping_replacement() {
    let effect = TargetEffects::instruction(&Instruction::AdvLoadW).unwrap();
    assert_eq!(effect.consumed, 4);
    assert_eq!(effect.outputs.len(), 4);
    assert!(effect.may_trap);
    assert!(effect.floor.is_empty());
    assert_eq!(effect.memory, MemoryEffects::default());
    assert!(effect.memory.write.is_none());
    assert_eq!(effect.outputs, (0..4).map(LaneEffect::Computed).collect::<Vec<_>>());
}

#[test]
fn advice_word_raw_vm_order_consumption_suffix_floor_and_exhaustion() {
    use miden_core::advice::AdviceStack;
    use miden_processor::advice::AdviceInputs;
    for suffix_len in [12, 20] {
        let suffix = (0..suffix_len).map(|i| 0x1_0000_0000usize + i).collect::<Vec<_>>();
        let mut input = vec![101, 202, 303, 404];
        input.extend(&suffix);
        let mut expected = vec![3, 4, 5, 6];
        expected.extend(&suffix);
        let source = format!(
            "begin {} adv_loadw sdepth push.{} assert_eq adv_push push.97 assert_eq {} end",
            super::lowering_units::push_top(&input[..input.len() - 16]),
            input.len(),
            super::lowering_units::assert_top(&expected)
        );
        let program = miden_assembly::Assembler::default()
            .assemble_program("raw_advice_word", source)
            .unwrap()
            .try_into_program()
            .unwrap();
        let mut advice = AdviceStack::new();
        advice
            .append_word([3, 4, 5, 6].map(Felt::new_unchecked).into())
            .append_element(Felt::new_unchecked(97));
        let base = input[input.len() - 16..]
            .iter()
            .map(|n| Felt::new_unchecked(*n as u64))
            .collect::<Vec<_>>();
        let processor = |advice: AdviceStack| {
            FastProcessor::new(StackInputs::new(&base).unwrap())
                .with_advice(AdviceInputs::default().with_stack(advice))
                .unwrap()
        };
        processor(advice).execute_sync(&program, &mut DefaultHost::default()).unwrap();
        for count in 0..4 {
            let advice = AdviceStack::from(vec![Felt::ONE; count]);
            let mut processor = processor(advice);
            let mut context = processor.get_initial_resume_context(&program).unwrap();
            loop {
                match processor.step_sync(&mut DefaultHost::default(), context) {
                    Ok(Some(next)) => context = next,
                    Ok(None) => panic!("partial word must trap"),
                    Err(error) => {
                        assert!(
                            matches!(
                                error,
                                miden_processor::ExecutionError::AdviceError {
                                    err: miden_processor::advice::AdviceError::StackReadFailed,
                                    ..
                                }
                            ),
                            "{error:?}"
                        );
                        let actual = processor
                            .stack()
                            .iter()
                            .rev()
                            .map(|f| f.as_canonical_u64() as usize)
                            .collect::<Vec<_>>();
                        assert_eq!(actual, input, "exhaustion with {count} elements");
                        break;
                    }
                }
            }
        }
    }
}

#[test]
fn merkle_target_contracts_preserve_exact_identities_and_traps() {
    for (instruction, consumed, produced, peak) in [
        (Instruction::MTreeGet, 6, 8, 4),
        (Instruction::MTreeSet, 10, 8, 8),
        (Instruction::MTreeMerge, 8, 4, 4),
        (Instruction::MTreeVerify, 10, 10, 0),
    ] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert_eq!(effect.consumed, consumed);
        assert_eq!(effect.outputs.len(), produced);
        assert!(effect.may_trap);
        let (mut height, mut maximum) = (0i32, 0);
        for step in &effect.floor {
            height += match step {
                FloorStep::Push(n) => i32::from(*n),
                FloorStep::Pop(n) => -i32::from(*n),
            };
            maximum = maximum.max(height);
        }
        assert_eq!(maximum, peak);
        assert_eq!(height, produced as i32 - i32::from(consumed));
        for (i, lane) in effect.outputs.iter().enumerate() {
            match instruction {
                Instruction::MTreeGet if i >= 4 => {
                    assert_eq!(*lane, LaneEffect::Input((i - 2) as u8))
                }
                Instruction::MTreeVerify => assert_eq!(*lane, LaneEffect::Input(i as u8)),
                _ => assert_eq!(*lane, LaneEffect::Computed(i as u8)),
            }
        }
    }
}

#[test]
fn horner_closed_read_and_retained_rules() {
    for instruction in [Instruction::HornerBase, Instruction::HornerExt] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert_eq!(effect.consumed, 16);
        assert_eq!(
            effect.outputs,
            (0..14)
                .map(LaneEffect::Input)
                .chain((0..2).map(LaneEffect::Computed))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            effect.memory.read,
            Some(MemoryRead::Fixed(MemoryRegion::Indirect {
                address: 13,
                width: 4
            }))
        );
        assert!(effect.may_trap);
        assert!(effect.floor.is_empty());
    }
}

#[test]
fn log_deferred_closed_state_rule() {
    let effect = TargetEffects::instruction(&Instruction::LogDeferred).unwrap();
    assert_eq!(effect.consumed, 12);
    assert_eq!(effect.outputs, (0..12).map(LaneEffect::Computed).collect::<Vec<_>>());
    assert_eq!(effect.memory, MemoryEffects::default());
    assert!(effect.may_trap);
    assert!(effect.floor.is_empty());
}

/// Independent extension-field oracle for raw VM cases.
fn horner_expected(input: &[usize], alpha: [u64; 2], extension: bool) -> Vec<usize> {
    use miden_core::field::{BasedVectorSpace, QuadFelt};
    let alpha = QuadFelt::new(alpha.map(Felt::new_unchecked));
    let mut accumulator =
        QuadFelt::new([input[14], input[15]].map(|n| Felt::new_unchecked(n as u64)));
    if extension {
        for pair in input[..8].as_chunks::<2>().0 {
            accumulator = accumulator * alpha
                + QuadFelt::new([pair[0], pair[1]].map(|n| Felt::new_unchecked(n as u64)));
        }
    } else {
        for coefficient in &input[..8] {
            accumulator =
                accumulator * alpha + QuadFelt::from(Felt::new_unchecked(*coefficient as u64));
        }
    }
    let mut expected = input.to_vec();
    let coefficients: &[Felt] = accumulator.as_basis_coefficients_slice();
    for (lane, value) in expected[14..].iter_mut().zip(coefficients) {
        *lane = value.as_canonical_u64() as usize;
    }
    expected
}

#[test]
fn horner_raw_vm_oracle_floor_suffix_and_invalid_reads() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    for (instruction, extension) in [("horner_eval_base", false), ("horner_eval_ext", true)] {
        for alpha in [[0, 0], [3, 5]] {
            for initial in [[0, 0], [17, 29]] {
                let mut input =
                    (0..16).map(|i| 0xffff_fffe_0000_0100usize + i * 37).collect::<Vec<_>>();
                input[13] = 400;
                input[14..].copy_from_slice(&initial);
                let expected = horner_expected(&input, alpha, extension);
                let floor_input =
                    input.iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>();
                let actual = run_program(
                    &format!(
                        "begin push.{} mem_store.400 push.{} mem_store.401 {instruction} end",
                        alpha[0], alpha[1]
                    ),
                    &floor_input,
                )
                .unwrap();
                assert_eq!(
                    actual,
                    expected.iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>()
                );
                for suffix_len in [0, 20] {
                    let suffix = (0..suffix_len).map(|i| 900 + i).collect::<Vec<_>>();
                    let all = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
                    let code = format!(
                        "begin push.{} mem_store.400 push.{} mem_store.401 {} {} {instruction} {} \
                         end",
                        alpha[0],
                        alpha[1],
                        push_top(&suffix),
                        push_top(&input),
                        assert_top(&all)
                    );
                    run_program(&code, &[]).unwrap();
                }
                for (address, padding) in [(401, 0), (1usize << 32, 0), (400, 1)] {
                    let mut bad = input.clone();
                    bad[13] = address;
                    let code = format!(
                        "begin push.{padding} mem_store.402 {} {instruction} end",
                        push_top(&bad)
                    );
                    assert!(run_program(&code, &[]).is_err());
                }
            }
        }
    }
}

fn deferred_expected(root: miden_core::Word, statement: miden_core::Word) -> [Felt; 12] {
    use miden_core::{crypto::hash::Poseidon2, deferred::Tag};
    let mut state = [Felt::ZERO; 12];
    state[..4].copy_from_slice(root.as_elements());
    state[4..8].copy_from_slice(statement.as_elements());
    state[8..].copy_from_slice(&Tag::AND.as_word());
    Poseidon2::apply_permutation(&mut state);
    state
}

#[test]
fn log_deferred_raw_two_folds_all_lanes_state_and_unknown_statement() {
    use miden_core::{
        Word,
        deferred::{Node, TRUE_DIGEST},
    };

    use crate::plan::tests::lowering_units::{assert_top, push_top};
    let first = deferred_expected(TRUE_DIGEST, TRUE_DIGEST);
    let root = Word::new(first[..4].try_into().unwrap());
    let second = deferred_expected(root, root);
    let first_node = Node::and(TRUE_DIGEST, TRUE_DIGEST);
    let second_node = Node::and(root, root);
    assert_eq!(root, first_node.digest());
    assert_eq!(&second[..4], second_node.digest().as_elements());
    for suffix_len in [4, 20] {
        let suffix = (0..suffix_len).map(|i| 900 + i).collect::<Vec<_>>();
        let input = [13, 17, 19, 23, 0, 0, 0, 0, 29, 31, 37, 41];
        let first_check = first
            .iter()
            .map(|f| f.as_canonical_u64() as usize)
            .chain(suffix.iter().copied())
            .collect::<Vec<_>>();
        let second_check = second
            .iter()
            .map(|f| f.as_canonical_u64() as usize)
            .chain(suffix.iter().copied())
            .collect::<Vec<_>>();
        let mut next = input;
        for (lane, value) in next[4..8].iter_mut().zip(root.as_elements()) {
            *lane = value.as_canonical_u64() as usize;
        }
        let source = format!(
            "begin {} {} log_deferred {} {} {} log_deferred {} end",
            push_top(&suffix),
            push_top(&input),
            assert_top(&first_check),
            push_top(&suffix),
            push_top(&next),
            assert_top(&second_check)
        );
        let program = miden_assembly::Assembler::default()
            .assemble_program("deferred", source)
            .unwrap()
            .try_into_program()
            .unwrap();
        let mut processor = FastProcessor::new(StackInputs::default());
        let mut host = DefaultHost::default();
        let mut context = processor.get_initial_resume_context(&program).unwrap();
        while let Some(next) = processor.step_sync(&mut host, context).unwrap() {
            context = next;
        }
        assert_eq!(processor.deferred_state().root(), second_node.digest());
        assert_eq!(processor.deferred_state().get_node(&root), Some(&first_node));
        assert_eq!(processor.deferred_state().get_node(&second_node.digest()), Some(&second_node));
    }
    let source = "begin push.41.37.31.29.1.2.3.4.23.19.17.13 log_deferred end";
    assert!(run_program(source, &[]).is_err());
}

#[test]
fn memstream_closed_read_and_lane_rule() {
    let effect = TargetEffects::instruction(&Instruction::MemStream).unwrap();
    assert_eq!(effect.consumed, 13);
    assert_eq!(
        effect.outputs,
        (0..8)
            .map(|lane| LaneEffect::Memory { lane })
            .chain((8..12).map(LaneEffect::Input))
            .chain([LaneEffect::Computed(0)])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        effect.memory.read,
        Some(MemoryRead::Fixed(MemoryRegion::Indirect {
            address: 12,
            width: 8
        }))
    );
    assert!(effect.may_trap);
    assert!(effect.floor.is_empty());
}

#[test]
fn memstream_raw_vm_values_bounds_floor_and_no_publication() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    for address in [400usize, 0xffff_fff8] {
        for written in [0, 4, 8] {
            let memory = (0..8)
                .map(|i| {
                    if address == 0xffff_fff8 && i == 6 {
                        miden_core::FMP_INIT_VALUE.as_canonical_u64() as usize
                    } else if i < written {
                        0xffff_fffe_0000_0100usize + i * 37
                    } else {
                        0
                    }
                })
                .collect::<Vec<_>>();
            let initialize = memory
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    address == 0xffff_fff8 && *i != 6
                        || *i < written && !(address == 0xffff_fff8 && *i == 6)
                })
                .map(|(i, v)| format!("push.{v} mem_store.{}", address + i))
                .collect::<Vec<_>>()
                .join(" ");
            let mut input = (0..13).map(|i| 101 + i * 37).collect::<Vec<_>>();
            input[12] = address;
            let expected = memory
                .iter()
                .copied()
                .chain(input[8..12].iter().copied())
                .chain([address + 8])
                .collect::<Vec<_>>();
            let probe_input = input
                .iter()
                .copied()
                .chain([900, 901, 902])
                .map(|n| Felt::new_unchecked(n as u64))
                .collect::<Vec<_>>();
            let probe =
                run_program(&format!("begin {initialize} mem_stream end"), &probe_input).unwrap();
            let wanted = expected
                .iter()
                .copied()
                .chain([900, 901, 902])
                .map(|n| Felt::new_unchecked(n as u64))
                .collect::<Vec<_>>();
            assert_eq!(probe, wanted, "address={address} written={written}");
            for suffix_len in [3, 20] {
                let suffix = (0..suffix_len).map(|i| 900 + i).collect::<Vec<_>>();
                let all = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
                run_program(
                    &format!(
                        "begin {initialize} {} {} mem_stream {} end",
                        push_top(&suffix),
                        push_top(&input),
                        assert_top(&all)
                    ),
                    &[],
                )
                .unwrap();
            }
        }
    }
    for address in [401usize, 1usize << 32, 0xffff_fffc] {
        let mut input = (0..16).map(|i| 101 + i * 37).collect::<Vec<_>>();
        input[12] = address;
        let source = format!("begin {} mem_stream end", push_top(&input));
        let program = miden_assembly::Assembler::default()
            .assemble_program("bad_stream", source)
            .unwrap()
            .try_into_program()
            .unwrap();
        let mut processor = FastProcessor::new(StackInputs::default());
        let mut host = DefaultHost::default();
        let mut context = processor.get_initial_resume_context(&program).unwrap();
        let error = loop {
            match processor.step_sync(&mut host, context) {
                Ok(Some(next)) => context = next,
                Ok(None) => panic!("expected trap"),
                Err(error) => break error,
            }
        };
        assert!(matches!(error, miden_processor::ExecutionError::MemoryError { .. }));
        assert_eq!(
            processor
                .stack_top()
                .iter()
                .rev()
                .map(|f| f.as_canonical_u64() as usize)
                .collect::<Vec<_>>(),
            input
        );
    }
}

#[test]
fn fri_closed_tuple_floor_and_retained_rule() {
    let effect = TargetEffects::instruction(&Instruction::FriExt2Fold4).unwrap();
    assert_eq!(effect.consumed, 17);
    assert_eq!(effect.outputs.len(), 16);
    assert_eq!(effect.outputs[11], LaneEffect::Input(8));
    assert_eq!(effect.outputs[15], LaneEffect::Input(16));
    assert_eq!(effect.floor, vec![FloorStep::Pop(1)]);
    assert!(effect.may_trap);
    assert_eq!(effect.memory, MemoryEffects::default());
}

/// Build samples from a polynomial, independently of the processor's folding helpers.
fn fri_case(coset: usize, constant: bool) -> (Vec<usize>, Vec<usize>) {
    use miden_core::{
        Felt,
        field::{BasedVectorSpace, Field, QuadFelt, TwoAdicField},
    };
    let q = |a, b| QuadFelt::new([Felt::new_unchecked(a), Felt::new_unchecked(b)]);
    let coefficients = if constant {
        [q(19, 23), q(0, 0), q(0, 0), q(0, 0)]
    } else {
        [q(0xffff_fffe_0000_0100, 23), q(29, 31), q(37, 41), q(43, 47)]
    };
    let evaluate =
        |point: QuadFelt| coefficients.iter().rev().fold(q(0, 0), |acc, c| acc * point + *c);
    let alpha = q(53, 59);
    let poe = Felt::new_unchecked(7);
    let tau = Felt::two_adic_generator(2);
    let x = poe * tau.exp_u64(coset as u64).inverse();
    let samples = [
        evaluate(QuadFelt::from(x)),
        evaluate(QuadFelt::from(-x)),
        evaluate(QuadFelt::from(x * tau)),
        evaluate(QuadFelt::from(-x * tau)),
    ];
    let flatten = |value: QuadFelt| {
        let limbs: &[Felt] = value.as_basis_coefficients_slice();
        limbs.iter().map(|f| f.as_canonical_u64() as usize).collect::<Vec<_>>()
    };
    let mut input = samples.into_iter().flat_map(flatten).collect::<Vec<_>>();
    input.extend([701, coset, poe.as_canonical_u64() as usize]);
    input.extend(flatten(evaluate(QuadFelt::from(poe))));
    input.extend(flatten(alpha));
    let cptr = Felt::new_unchecked(0xffff_ffff_0000_0000);
    input.extend([cptr.as_canonical_u64() as usize, 809]);
    let x2 = x * x;
    let tmp0 =
        coefficients[0] + coefficients[2] * x2 + alpha * (coefficients[1] + coefficients[3] * x2);
    let tmp1 =
        coefficients[0] - coefficients[2] * x2 + alpha * (coefficients[1] - coefficients[3] * x2);
    let pointer = (cptr + Felt::new_unchecked(8)).as_canonical_u64() as usize;
    let mut expected = flatten(tmp0);
    expected.extend(flatten(tmp1));
    expected.extend((1..4).map(|i| usize::from(i == coset)));
    expected.extend([
        (poe * poe).as_canonical_u64() as usize,
        pointer,
        pointer,
        poe.exp_u64(4).as_canonical_u64() as usize,
        701,
    ]);
    expected.extend(flatten(evaluate(alpha)));
    expected.extend([pointer, 809]);
    (input, expected)
}

#[test]
fn fri_raw_polynomial_oracle_all_lanes_floor_suffix_and_traps() {
    use crate::plan::tests::lowering_units::{assert_top, push_top};
    for coset in 0..4 {
        for constant in [false, true] {
            let (input, expected) = fri_case(coset, constant);
            let initial =
                input[1..].iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>();
            let actual =
                run_program(&format!("begin push.{} fri_ext2fold4 end", input[0]), &initial)
                    .unwrap();
            assert_eq!(
                actual,
                expected.iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>()
            );
            let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
            let all = input.iter().chain(&suffix).copied().collect::<Vec<_>>();
            let expected = expected.iter().chain(&suffix).copied().collect::<Vec<_>>();
            let base = all[all.len() - 16..]
                .iter()
                .map(|n| Felt::new_unchecked(*n as u64))
                .collect::<Vec<_>>();
            run_program(
                &format!(
                    "begin {} fri_ext2fold4 {} end",
                    push_top(&all[..all.len() - 16]),
                    assert_top(&expected)
                ),
                &base,
            )
            .unwrap();
        }
    }
    let (input, _) = fri_case(1, false);
    for (index, value) in [
        (9, 4),
        (9, 0xffff_fffe_0000_0100),
        (10, 0),
        (11, input[11] + 1),
        (12, input[12] + 1),
    ] {
        let mut bad = input.clone();
        bad[index] = value;
        let initial = bad[1..].iter().map(|n| Felt::new_unchecked(*n as u64)).collect::<Vec<_>>();
        assert!(
            run_program(&format!("begin push.{} fri_ext2fold4 end", bad[0]), &initial).is_err()
        );
    }
}

#[test]
fn log_deferred_raw_actual_minimum_floor() {
    use miden_core::deferred::TRUE_DIGEST;
    let input =
        [13, 17, 19, 23, 0, 0, 0, 0, 29, 31, 37, 41, 901, 902, 903, 904].map(Felt::new_unchecked);
    let expected = deferred_expected(TRUE_DIGEST, TRUE_DIGEST)
        .into_iter()
        .chain(input[12..].iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(run_program("begin log_deferred end", &input).unwrap(), expected);
}

#[test]
fn stream_write_closed_rules_have_exact_windows() {
    for (instruction, width) in [(Instruction::AdvPipe, 13), (Instruction::CryptoStream, 14)] {
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert_eq!(effect.consumed, width);
        assert_eq!(effect.outputs.len(), usize::from(width));
        assert!(effect.may_trap);
        assert!(effect.floor.is_empty());
        assert_eq!(&effect.outputs[8..12], &(8..12).map(LaneEffect::Input).collect::<Vec<_>>());
    }
}

#[test]
fn stream_write_metadata_rejects_invalid_dependencies_before_indexing() {
    let base = TargetEffects::instruction(&Instruction::CryptoStream).unwrap();
    for mutation in 0..12 {
        let mut bad = base.clone();
        match mutation {
            0 => {
                bad.memory.read = Some(MemoryRead::Fixed(MemoryRegion::Indirect {
                    address: 14,
                    width: 8,
                }))
            }
            1 => {
                bad.memory.read = Some(MemoryRead::Fixed(MemoryRegion::SourceUnknown { width: 0 }))
            }
            2 => {
                bad.memory.write.as_mut().unwrap().region = MemoryRegion::Indirect {
                    address: 14,
                    width: 8,
                }
            }
            3 => bad.memory.write.as_mut().unwrap().values[0] = WriteValue::Input(14),
            4 => bad.memory.write.as_mut().unwrap().values[0] = WriteValue::Output(14),
            5 => {
                bad.memory.write.as_mut().unwrap().values.pop();
            }
            6 => {
                bad.memory.write.as_mut().unwrap().region = MemoryRegion::SourceUnknown { width: 0 }
            }
            7 => {
                bad.memory.read = None;
                bad.outputs[0] = LaneEffect::Memory { lane: 0 };
            }
            8 => bad.outputs[0] = LaneEffect::Memory { lane: 8 },
            9 => bad.outputs[0] = LaneEffect::Input(14),
            10 => {
                bad.outputs[0] = LaneEffect::Conditional {
                    condition: 14,
                    if_true: 0,
                    if_false: 1,
                }
            }
            _ => {
                bad.outputs[0] = LaneEffect::Conditional {
                    condition: 0,
                    if_true: 14,
                    if_false: 1,
                }
            }
        }
        assert!(bad.validate().is_err(), "mutation {mutation}");
    }
    assert!(base.validate().is_ok());
    let pipe = TargetEffects::instruction(&Instruction::AdvPipe).unwrap();
    assert!(pipe.memory.read.is_none());
    for effect in [base, pipe] {
        let write = effect.memory.write.unwrap();
        assert_eq!(
            write.region,
            MemoryRegion::Indirect {
                address: effect.consumed - 1,
                width: 8
            }
        );
        assert_eq!(write.values, (0..8).map(WriteValue::Output).collect::<Vec<_>>());
    }
}

#[test]
fn eval_circuit_closed_rule_retains_exact_inputs_without_floor_changes() {
    let effect = TargetEffects::instruction(&Instruction::EvalCircuit).unwrap();
    assert_eq!(effect.consumed, 3);
    assert_eq!(effect.outputs, (0..3).map(LaneEffect::Input).collect::<Vec<_>>());
    assert!(effect.may_trap);
    assert!(effect.floor.is_empty());
    assert!(effect.memory.read.is_some());
    assert!(effect.memory.write.is_none());
    effect.validate().unwrap();
}

#[test]
fn eval_circuit_metadata_rejects_invalid_raw_references_and_memory_outputs() {
    let base = TargetEffects::instruction(&Instruction::EvalCircuit).unwrap();
    assert_eq!(
        base.memory.read,
        Some(MemoryRead::Circuit {
            address: 0,
            read_count: 1,
            eval_count: 2,
        })
    );
    for (address, read_count, eval_count) in [(3, 1, 2), (0, 3, 2), (0, 1, 3)] {
        let mut bad = base.clone();
        bad.memory.read = Some(MemoryRead::Circuit {
            address,
            read_count,
            eval_count,
        });
        assert!(bad.validate().is_err());
    }
    let mut bad = base;
    bad.outputs[0] = LaneEffect::Memory { lane: 0 };
    assert!(bad.validate().is_err());
}

#[test]
fn eval_circuit_raw_actual_minimum_floor_retains_all_lanes() {
    let gate = |left: u64, right: u64, op: u64| left + (right << 30) + (op << 60);
    let gates = [gate(5u64, 5u64, 0u64), gate(5, 4, 2), gate(2, 2, 1), gate(1, 1, 0)];
    let cells = [3, 7, 11, 13].into_iter().chain(gates).collect::<Vec<_>>();
    let init = cells
        .iter()
        .enumerate()
        .map(|(i, v)| format!("push.{v} mem_store.{}", 400 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let input = [400, 2, 4]
        .into_iter()
        .chain(0..13)
        .map(Felt::new_unchecked)
        .collect::<Vec<_>>();
    assert_eq!(run_program(&format!("begin {init} eval_circuit end"), &input).unwrap(), input);
}

#[test]
fn system_event_closed_rules_retain_complete_windows_and_actual_floor() {
    use miden_core::events::SystemEvent;
    for event in SystemEvent::all() {
        if matches!(
            event,
            SystemEvent::MemToMap | SystemEvent::DeferredRegisterData | SystemEvent::TraceEvent
        ) {
            continue;
        }
        let instruction = Instruction::EmitImm(
            crate::masm::Immediate::Value(crate::masm::Span::unknown(event.event_id().as_felt()))
                .into(),
        );
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert!(effect.consumed > 0, "{event:?}");
        assert_eq!(effect.outputs, (0..effect.consumed).map(LaneEffect::Input).collect::<Vec<_>>());
        assert_eq!(effect.floor, vec![FloorStep::Push(1), FloorStep::Pop(1)]);
        assert!(effect.may_trap);
        assert_eq!(effect.memory, MemoryEffects::default());
    }
}

#[test]
fn system_event_metadata_rejects_observation_outside_consumed_inputs() {
    let mut effect = TargetEffects::instruction(&Instruction::Nop).unwrap();
    assert!(effect.observed_inputs.is_empty());
    effect.observed_inputs.push(0);
    assert!(effect.validate().is_err());
}

#[test]
fn system_event_observation_metadata_is_complete_bounded_and_closed() {
    use miden_core::events::SystemEvent::*;
    let cases = [
        (MapValueToStack, 4),
        (MapValueCountToStack, 4),
        (MapValueToStackN0, 4),
        (MapValueToStackN4, 4),
        (MapValueToStackN8, 4),
        (HasMapKey, 4),
        (MerkleNodeToStack, 6),
        (HdwordToMap, 8),
        (MerkleNodeMerge, 8),
        (HdwordToMapWithDomain, 9),
        (HqwordToMap, 16),
        (HpermToMap, 12),
        (DeferredRegister, 12),
        (DeferredEvaluate, 4),
        (DeferredEvaluateTag, 4),
        (DeferredEvaluatePayload, 4),
        (Ext2Inv, 2),
        (U32Clz, 1),
        (U32Ctz, 1),
        (U32Clo, 1),
        (U32Cto, 1),
        (ILog2, 1),
    ];
    assert_eq!(cases.len(), 22);
    for (event, n) in cases {
        let instruction = Instruction::EmitImm(
            crate::masm::Immediate::Value(crate::masm::Span::unknown(event.event_id().as_felt()))
                .into(),
        );
        let effect = TargetEffects::instruction(&instruction).unwrap();
        assert_eq!(effect.consumed, n, "{event:?}");
        assert_eq!(effect.observed_inputs, (0..n).collect::<Vec<_>>(), "{event:?}");
        effect.validate().unwrap();
        for invalid in [n, u8::MAX] {
            let mut bad = effect.clone();
            bad.observed_inputs.push(invalid);
            assert!(bad.validate().is_err(), "{event:?} index{invalid}");
        }
    }
    for event in [MemToMap, DeferredRegisterData, TraceEvent] {
        let instruction = Instruction::EmitImm(
            crate::masm::Immediate::Value(crate::masm::Span::unknown(event.event_id().as_felt()))
                .into(),
        );
        assert!(TargetEffects::instruction(&instruction).is_err());
    }
    for event in [crate::Event::FrameStart, crate::Event::FrameEnd] {
        let effect = TargetEffects::instruction(&Instruction::EmitImm(event.into())).unwrap();
        assert!(effect.observed_inputs.is_empty());
        assert_eq!(effect.consumed, 0);
    }
    for instruction in [Instruction::Nop, Instruction::Add, Instruction::Locaddr(0.into())] {
        assert!(TargetEffects::instruction(&instruction).unwrap().observed_inputs.is_empty());
    }
    assert!(TargetEffects::instruction(&Instruction::Emit).is_err());
    assert!(
        TargetEffects::instruction(&Instruction::EmitImm(
            crate::masm::Immediate::Value(crate::masm::Span::unknown(Felt::from(7u32))).into()
        ))
        .is_err()
    );
}
