use miden_core::{Felt, FieldElement};
use miden_processor::AdviceInputs;
use midenc_compile::LinkOutput;
use midenc_debug::{ExecutionTrace, Executor, FromMidenRepr};
use midenc_session::Session;
use proptest::test_runner::TestCaseError;

use super::*;

/// Evaluates `package` using the debug executor, producing an output of type `T`
///
/// * `initializers` is an optional set of [Initializer] to run at program start by the compiler-
///   emitted test harness, to set up memory or other global state.
/// * `args` are the set of arguments that will be placed on the operand stack, in order of
///   appearance
/// * `verify_trace` is a callback which gets the [ExecutionTrace], and can be used to assert
///   things about the trace, such as the state of memory at program exit.
pub fn eval_package<'a, T, I, F>(
    package: &miden_mast_package::Package,
    initializers: I,
    args: &[Felt],
    session: &Session,
    verify_trace: F,
) -> Result<T, TestCaseError>
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
    I: IntoIterator<Item = Initializer<'a>>,
    F: Fn(&ExecutionTrace) -> Result<(), TestCaseError>,
{
    // Provide input bytes/felts/words via the advice stack
    //
    // NOTE: This relies on MasmComponent to emit a test harness via `emit_test_harness` during
    // assembly of the package.
    //
    // First, convert the input to words, zero-padding as needed; and push on to the
    // advice stack in reverse.
    let mut advice_stack = Vec::<Felt>::with_capacity(64);
    for initializer in initializers {
        let num_words = match &initializer {
            Initializer::Value { value, .. } => {
                value.push_words_to_advice_stack(&mut advice_stack) as u32
            }
            Initializer::MemoryBytes { bytes, .. } => {
                let words = midenc_debug::bytes_to_words(bytes);
                let num_words = words.len() as u32;
                for word in words.into_iter().rev() {
                    for felt in word.into_iter() {
                        advice_stack.push(felt);
                    }
                }
                num_words
            }
            Initializer::MemoryFelts { felts, .. } => {
                let num_felts = felts.len().next_multiple_of(4);
                let num_words = num_felts / 4;
                let mut buf = Vec::with_capacity(num_words);
                let mut words = felts.iter().copied().array_chunks::<4>();
                for mut word in words.by_ref() {
                    word.reverse();
                    buf.push(word);
                }
                if let Some(remainder) = words.into_remainder().filter(|r| r.len() > 0) {
                    let mut word = [Felt::ZERO; 4];
                    for (i, felt) in remainder.enumerate() {
                        word[i] = felt;
                    }
                    word.reverse();
                    buf.push(word);
                }
                for word in buf.into_iter().rev() {
                    for felt in word.into_iter() {
                        advice_stack.push(felt);
                    }
                }
                num_words as u32
            }
            Initializer::MemoryWords { words, .. } => {
                for word in words.iter().rev() {
                    for elem in word.iter() {
                        advice_stack.push(*elem);
                    }
                }
                words.len() as u32
            }
        };

        // The test harness invokes std::mem::pipe_words_to_memory, which expects the operand stack
        // to look like: `[num_words, write_ptr]`.
        //
        // Since we're feeding this data in via the advice stack, the test harness code will expect
        // these values on the advice stack in the opposite order, as the `adv_push` instruction
        // will pop each element off the advice stack, and push on to the operand stack, after which
        // these two values will be in the expected order.
        advice_stack.push(Felt::new(num_words as u64)); // num_words
        advice_stack.push(Felt::new(initializer.element_addr() as u64)); // dest_ptr
    }

    let mut exec = Executor::for_package(package, args.to_vec(), session)
        .map_err(|err| TestCaseError::fail(format_report(err)))?;

    // Reverse the stack contents, so that the correct order is preserved after MemAdviceProvider
    // does its own reverse
    advice_stack.reverse();

    exec.with_advice_inputs(AdviceInputs::default().with_stack(advice_stack));

    let trace = exec.execute(&package.unwrap_program(), session);
    verify_trace(&trace)?;

    dbg!(trace.outputs());

    let output = trace.parse_result::<T>().expect("expected output was not returned");
    dbg!(&output);

    Ok(output)
}

/// This helper exists to handle the boilerplate of compiling/assembling the output of the link
/// stage of the compiler to a package, and then evaluating that package with [eval_package].
///
/// Evaluates the package assembled from `link_output` using the debug executor, producing an output
/// of type `T`
///
/// * `initializers` is an optional set of [Initializer] to run at program start by the compiler-
///   emitted test harness, to set up memory or other global state.
/// * `args` are the set of arguments that will be placed on the operand stack, in order of
///   appearance
/// * `verify_trace` is a callback which gets the [ExecutionTrace], and can be used to assert
///   things about the trace, such as the state of memory at program exit.
pub fn eval_link_output<'a, T, I, F>(
    link_output: LinkOutput,
    initializers: I,
    args: &[Felt],
    session: &Session,
    verify_trace: F,
) -> Result<T, TestCaseError>
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
    I: IntoIterator<Item = Initializer<'a>>,
    F: Fn(&ExecutionTrace) -> Result<(), TestCaseError>,
{
    use midenc_compile::{compile_link_output_to_masm_with_pre_assembly_stage, CodegenOutput};

    // Compile to Package
    let mut pre_assembly_stage = |output: CodegenOutput, _context| {
        println!("# Assembled\n{}", &output.component);
        Ok(output)
    };
    let artifact =
        compile_link_output_to_masm_with_pre_assembly_stage(link_output, &mut pre_assembly_stage)
            .map_err(|err| TestCaseError::fail(format_report(err)))?;
    let package = artifact.unwrap_mast();

    eval_package::<T, _, _>(&package, initializers, args, session, verify_trace)
}
