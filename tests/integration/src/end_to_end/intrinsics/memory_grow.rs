use miden_core::Felt;
use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};

use crate::end_to_end::support::{assemble_test_program, default_host_with_core_lib};

#[test]
fn memory_grow_updates_only_on_success() {
    const PAGE: u32 = 65536;
    const LIMIT: u32 = 0xffff_fffc;
    const FAILED: u32 = u32::MAX;
    let program = assemble_test_program(
        r#"
        # Inputs: [heap_base, first_growth, second_growth]
        exec.::intrinsics::mem::heap_init
        exec.::intrinsics::mem::memory_grow
        exec.::intrinsics::mem::heap_top
        exec.::intrinsics::mem::memory_size
        # [first_size, first_top, first_result, second_growth]
        movup.3
        exec.::intrinsics::mem::memory_grow
        exec.::intrinsics::mem::heap_top
        exec.::intrinsics::mem::memory_size
        exec.::intrinsics::mem::heap_base
        # [base, size, top, result, first_size, first_top, first_result]
        "#,
    )
    .unwrap_program();

    let cases = [
        // A non-binary heap address exposed the old low/high limb reversal even on grow(0).
        ([1179648, 0, 0], [1179648, 0, 1179648, 0, 0, 1179648, 0]),
        ([4, 1, 2], [4, 3, 4 + 3 * PAGE, 1, 1, 4 + PAGE, 0]),
        // The heap endpoint is inclusive; crossing it must leave all metadata untouched.
        ([LIMIT - PAGE, 1, 0], [LIMIT - PAGE, 1, LIMIT, 1, 1, LIMIT, 0]),
        (
            [LIMIT - PAGE + 1, 1, 0],
            [LIMIT - PAGE + 1, 0, LIMIT - PAGE + 1, 0, 0, LIMIT - PAGE + 1, FAILED],
        ),
        // Page-count addition overflow after a successful allocation.
        ([4, 1, u32::MAX], [4, 1, 4 + PAGE, FAILED, 1, 4 + PAGE, 0]),
        // Multiplication yields high limbs 1, 2, and 65535 respectively. Each is failure,
        // including non-binary high limbs which cannot be consumed directly by if.true.
        ([4, 1, 65535], [4, 1, 4 + PAGE, FAILED, 1, 4 + PAGE, 0]),
        ([4, 1, 131071], [4, 1, 4 + PAGE, FAILED, 1, 4 + PAGE, 0]),
        ([4, 0, u32::MAX], [4, 0, 4, FAILED, 0, 4, 0]),
    ];
    for (inputs, expected) in cases {
        let inputs = inputs.map(Felt::from);
        let trace = execute_sync(
            &program,
            StackInputs::new(&inputs).unwrap(),
            AdviceInputs::default(),
            &mut default_host_with_core_lib(),
            ExecutionOptions::default(),
        )
        .unwrap_or_else(|err| panic!("memory.grow trapped for {inputs:?}: {err}"));
        let actual: Vec<u64> = trace
            .stack
            .get_num_elements(expected.len())
            .iter()
            .map(|value| value.as_canonical_u64())
            .collect();
        assert_eq!(actual, expected.map(u64::from), "inputs: {inputs:?}");
    }
}
