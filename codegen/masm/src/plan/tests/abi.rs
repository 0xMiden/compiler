use alloc::{format, vec::Vec};

use miden_core::Felt;
use midenc_hir::Report;

use super::effects::run_program;
use crate::plan::*;

fn padding_text(width: u8) -> String {
    crate::lower::raw::pad_inputs(width as usize, Default::default())
        .iter()
        .map(|op| match op {
            crate::masm::Op::Inst(instruction) => format!("{}", instruction.inner()),
            _ => panic!("padding must be straight-line stack instructions"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn cleanup_text(width: u8) -> String {
    context_result_cleanup(width)
        .unwrap()
        .iter()
        .map(|i| format!("{i}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn context_window_construction_uses_fewer_vm_operations() -> Result<(), Report> {
    use crate::opt::cost::tests::assembled_cost;

    for width in 0..=16 {
        // The old construction inserts/removes one lane at a time. Deep moves expand to four
        // operations, so a shorter textual alternative is not necessarily an improvement.
        let baseline = (16 - width) as usize
            * match width {
                0 => 1,
                1..=8 => 2,
                _ => 5,
            };
        for body in [padding_text(width), cleanup_text(width)] {
            let cost = assembled_cost(if body.is_empty() { "nop" } else { &body })?;
            assert!(cost <= baseline, "width={width}: {cost} > {baseline}");
            if let Some(expected) = match width {
                1 => Some(19),
                4 => Some(15),
                8 => Some(12),
                _ => None,
            } {
                assert_eq!(cost, expected, "width={width}: {body}");
            }
        }
    }
    Ok(())
}

#[test]
fn context_window_construction_preserves_floor_and_deep_suffix() -> Result<(), Report> {
    let initial = (101..117).map(Felt::new_unchecked).collect::<Vec<_>>();
    for width in 0..=16 {
        let mut expected = initial[..width as usize].to_vec();
        expected.resize(16, Felt::ZERO);
        assert_eq!(
            run_program(&format!("begin {} nop end", cleanup_text(width)), &initial)?,
            expected
        );

        for suffix in [0, 7, 40] {
            let lanes = (0..width as usize + suffix).map(|i| 1001 + i).collect::<Vec<_>>();
            let pushes =
                lanes.iter().rev().map(|v| format!("push.{v}")).collect::<Vec<_>>().join(" ");
            let checks = lanes[..width as usize]
                .iter()
                .copied()
                .chain(core::iter::repeat_n(0, 16 - width as usize))
                .chain(lanes[width as usize..].iter().copied())
                .map(|v| format!("push.{v} assert_eq"))
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(
                run_program(&format!("begin {pushes} {} {checks} end", padding_text(width)), &[])?,
                [Felt::ZERO; 16],
                "width={width} suffix={suffix}"
            );
        }
    }
    Ok(())
}

#[test]
fn abi_families_fail_closed_and_syscall_keeps_kernel_root_memory() {
    for invocation in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        for width in 0..=16 {
            let abi = ProcedureAbi::new(
                if invocation == Invocation::SysCall {
                    CallingConvention::Kernel
                } else {
                    CallingConvention::Context
                },
                width,
                width,
            )
            .unwrap();
            let effect = abi.invocation(invocation).unwrap();
            assert_eq!(effect.window, Some(16));
            assert_eq!(u32::from(effect.zero_padding), 16 - width);
            assert_eq!(u32::from(effect.clobbered), 16 - width);
            assert!(effect.preserves_overflow);
            assert_eq!(
                effect.memory,
                if invocation == Invocation::SysCall {
                    CallMemory::Root
                } else {
                    CallMemory::Fresh
                }
            );
        }
    }
    let same = ProcedureAbi::new(CallingConvention::SameContext, 22, 16).unwrap();
    assert!(same.invocation(Invocation::Exec).unwrap().preserves_suffix);
    assert!(same.invocation(Invocation::DynExec).unwrap().preserves_suffix);
    assert!(same.invocation(Invocation::Call).is_err());
    let context = ProcedureAbi::new(CallingConvention::Context, 16, 0).unwrap();
    assert!(context.invocation(Invocation::Exec).is_err());
    assert!(context.invocation(Invocation::SysCall).is_err());
    assert!(ProcedureAbi::new(CallingConvention::Context, 17, 0).is_err());
    assert!(ProcedureAbi::new(CallingConvention::Kernel, 0, 17).is_err());
    assert!(ReturnBoundary::SameContext.validate_truncation().is_err());
    assert!(ReturnBoundary::Root.validate_truncation().is_ok());
    assert!(ReturnBoundary::Context.validate_truncation().is_ok());
}

#[test]
fn context_cleanup_all_result_widths_matches_vm() -> Result<(), Report> {
    // Assert/consume every result before testing sentinels, including those below depth 15.
    for width in 0..=16 {
        let cleanup = context_result_cleanup(width)?;
        let cleanup = cleanup.iter().map(|i| format!("{i}")).collect::<Vec<_>>().join(" ");
        let exposed = (0..16).map(|i| format!("push.{}", 100 + i)).collect::<Vec<_>>().join(" ");
        let replacement = (100..116)
            .rev()
            .map(|value| format!("push.{value} add movdn.15"))
            .collect::<Vec<_>>()
            .join(" ");
        let results = (0..width)
            .map(|i| format!("push.{} assert_eq", 115 - i))
            .collect::<Vec<_>>()
            .join(" ");
        for invocation in ["call.callee", "procref.callee mem_storew_le.100 dropw push.100 dyncall"]
        {
            let source = format!(
                "proc callee repeat.16 drop end {replacement} end begin push.9001.9002.9003 \
                 {exposed} {invocation} {cleanup} {results} push.9003 assert_eq push.9002 \
                 assert_eq push.9001 assert_eq end"
            );
            assert_eq!(
                run_program(&source, &[])?,
                [Felt::ZERO; 16],
                "results={width} {invocation}"
            );
        }
    }
    assert!(context_result_cleanup(17).is_err());
    Ok(())
}

#[test]
fn exec_preserves_addressable_suffix_and_overflow_in_vm() -> Result<(), Report> {
    let input = (1..=16).map(Felt::new_unchecked).collect::<Vec<_>>();
    let assertions = [777, 102, 101, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        .iter()
        .map(|value| format!("push.{value} assert_eq"))
        .collect::<Vec<_>>()
        .join(" ");
    for invocation in ["exec.callee", "procref.callee mem_storew_le.100 dropw push.100 dynexec"] {
        let source = format!(
            "proc callee drop push.777 end begin push.101.102.103 {invocation} {assertions} end"
        );
        assert_eq!(run_program(&source, &input)?, [Felt::ZERO; 16], "{invocation}");
    }
    Ok(())
}

#[test]
fn syscall_uses_kernel_root_memory_and_restores_caller_overflow_in_vm() -> Result<(), Report> {
    use alloc::sync::Arc;

    use miden_assembly::{
        Assembler, DefaultSourceManager, Path,
        ast::{Module, ModuleKind},
    };
    use miden_processor::{DefaultHost, FastProcessor, StackInputs};

    let manager = Arc::new(DefaultSourceManager::default());
    // Kernel observes root memory even when called from a fresh call context.
    let kernel_source = "pub proc probe mem_load.100 push.4242 assert_eq push.9191 mem_store.100 \
                         repeat.16 drop end push.888 add end";
    let module = Module::parser(Some(ModuleKind::Kernel)).parse_str(
        Some(Path::KERNEL),
        kernel_source,
        manager.clone(),
    )?;
    let kernel: Arc<miden_assembly::package::Package> =
        Assembler::new(manager.clone()).assemble_kernel("kernel", module, None)?.into();
    let assembler = Assembler::with_kernel(manager.clone(), kernel.clone())?;
    // Normal context call has fresh memory; syscall sees and mutates the root's memory.
    let source = "proc callee mem_load.100 assertz push.1234 mem_store.100 push.7001.7002.7003 \
                  repeat.16 push.0 end syscall.probe repeat.15 swap.1 drop end push.888 assert_eq \
                  push.7003 assert_eq push.7002 assert_eq push.7001 assert_eq mem_load.100 \
                  push.1234 assert_eq end begin push.4242 mem_store.100 call.callee mem_load.100 \
                  push.9191 assert_eq end";
    let program = assembler.assemble_program("syscall_target", source)?.try_into_program()?;
    let mut host = DefaultHost::default()
        .with_library(kernel)
        .map_err(|error| Report::msg(error.to_string()))?;
    let output = FastProcessor::new(StackInputs::default())
        .execute_sync(&program, &mut host)
        .map_err(|error| Report::msg(error.to_string()))?;
    assert_eq!(output.stack.get_element(0), Some(Felt::ZERO));
    // A nonkernel local target must not be accepted as a syscall target.
    assert!(
        miden_assembly::Assembler::default()
            .assemble_program(
                "bad_syscall",
                "proc local_target nop end begin syscall.local_target end"
            )
            .is_err()
    );
    Ok(())
}
