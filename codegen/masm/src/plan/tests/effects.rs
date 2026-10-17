use alloc::{format, vec::Vec};

use miden_core::Felt;
use miden_processor::{DefaultHost, FastProcessor, StackInputs};
use midenc_hir::Report;

use crate::{masm::Instruction, plan::*};

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
    assert!(TargetEffects::instruction(&Instruction::U32Div).is_err());
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
    assert!(TargetEffects::instruction(&Instruction::U32Div).is_err());
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
