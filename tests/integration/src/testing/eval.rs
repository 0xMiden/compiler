use std::sync::Arc;

use miden_core::Felt;
use miden_core_lib::CoreLibrary;
use miden_debug::{ExecutionTrace, Executor, FromMidenRepr};
use miden_processor::advice::AdviceInputs;
use miden_protocol::ProtocolLib;
use miden_standards::StandardsLib;
use midenc_compile::LinkOutput;
use midenc_hir::{Type, dialects::builtin::attributes::Signature};
use midenc_session::{STDLIB, Session};
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
    eval_package_with_advice_stack(
        package,
        initializers,
        core::iter::empty::<Felt>(),
        args,
        session,
        verify_trace,
    )
}

/// Evaluates `package` using the debug executor, producing an output of type `T`
///
/// * `initializers` is an optional set of [Initializer] to run at program start by the compiler-
///   emitted test harness, to set up memory or other global state.
/// * `advice_stack` contains additional values to place on the advice stack before program start.
///   The first element is treated as the top of the stack. Initializer-related values are pushed
///   on top of these (i.e. they are consumed before user-supplied advice inputs).
/// * `args` are the set of arguments that will be placed on the operand stack, in order of
///   appearance
/// * `verify_trace` is a callback which gets the [ExecutionTrace], and can be used to assert
///   things about the trace, such as the state of memory at program exit.
pub fn eval_package_with_advice_stack<'a, T, I, A, F>(
    package: &miden_mast_package::Package,
    initializers: I,
    advice_stack: A,
    args: &[Felt],
    session: &Session,
    verify_trace: F,
) -> Result<T, TestCaseError>
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
    I: IntoIterator<Item = Initializer<'a>>,
    A: IntoIterator<Item = Felt>,
    F: Fn(&ExecutionTrace) -> Result<(), TestCaseError>,
{
    // Provide initializer data and any user-supplied advice inputs via the advice stack.
    //
    // NOTE: This relies on MasmComponent emitting a test harness via `emit_test_harness` during
    // assembly of the package. The test harness consumes initializer inputs in FIFO order from the
    // advice stack (top = index 0).
    let user_advice_stack: Vec<Felt> = advice_stack.into_iter().collect();
    let mut advice_stack = Vec::new();
    let mut num_initializers = 0u64;

    for initializer in initializers {
        num_initializers += 1;

        // The harness uses `adv_push.2` to place `[num_words, dest_ptr]` on the operand stack, so
        // we provide `[dest_ptr, num_words]` on the advice stack.
        let dest_ptr = initializer.element_addr();

        let reverse_word_elements =
            matches!(&initializer, Initializer::Value { .. } | Initializer::MemoryBytes { .. });

        let words: Vec<miden_core::Word> = match initializer {
            Initializer::Value { value, .. } => {
                miden_debug::bytes_to_words(value.to_bytes().as_slice())
                    .into_iter()
                    .map(miden_core::Word::from)
                    .collect()
            }
            Initializer::MemoryBytes { bytes, .. } => miden_debug::bytes_to_words(bytes)
                .into_iter()
                .map(miden_core::Word::from)
                .collect(),
            Initializer::MemoryFelts { felts, .. } => {
                let padded = felts.len().next_multiple_of(4);
                let mut felts = felts.into_owned();
                felts.resize(padded, Felt::ZERO);
                felts
                    .chunks_exact(4)
                    .map(|chunk| miden_core::Word::new([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect()
            }
            Initializer::MemoryWords { words, .. } => words.into_owned(),
        };

        advice_stack.push(Felt::new(dest_ptr as u64));
        advice_stack.push(Felt::new(words.len() as u64));

        for word in words {
            if reverse_word_elements {
                for felt in word.iter().rev() {
                    advice_stack.push(*felt);
                }
            } else {
                for felt in word.iter() {
                    advice_stack.push(*felt);
                }
            }
        }
    }

    advice_stack.insert(0, Felt::new(num_initializers));
    advice_stack.extend(user_advice_stack);

    let mut exec = Executor::new(args.to_vec());
    let core_library = CoreLibrary::default();
    // The debug executor path does not automatically install core-library event handlers, but
    // integration tests execute core helpers such as `u64::div` through the VM.
    for (event, handler) in core_library.handlers() {
        exec.register_event_handler(event, handler)
            .expect("failed to register core library event handler");
    }

    // Register the standard library so dependencies can be resolved at runtime.
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut().insert(*std_library.digest(), std_library);
    let protocol_library = Arc::new(ProtocolLib::default().as_ref().clone());
    exec.dependency_resolver_mut().insert(*protocol_library.digest(), protocol_library);
    let standards_library = Arc::new(StandardsLib::default().as_ref().clone());
    exec.dependency_resolver_mut().insert(*standards_library.digest(), standards_library);

    exec.with_dependencies(package.manifest.dependencies())
        .map_err(|err| TestCaseError::fail(format_report(err)))?;

    exec.with_advice_inputs(AdviceInputs::default().with_stack(advice_stack));

    let trace = exec.execute(&package.unwrap_program(), session.source_manager.clone());
    verify_trace(&trace)?;
    Ok(trace.parse_result::<T>().expect("expected output was not returned"))
}

/// Helper function to compile a test module with the given signature and build function
pub fn compile_test_module(
    params: impl IntoIterator<Item = Type>,
    results: impl IntoIterator<Item = Type>,
    build_fn: impl Fn(&mut midenc_hir::dialects::builtin::FunctionBuilder<'_, midenc_hir::OpBuilder>),
) -> (miden_mast_package::Package, std::rc::Rc<midenc_hir::Context>) {
    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let signature = Signature::new(&context, params, results);
    let link_output = setup::build_empty_component_for_test(context.clone());
    setup::build_entrypoint(link_output.component, &signature, build_fn);
    let package = compile_link_output_to_package(link_output).unwrap();
    (package, context)
}

/// Compiles a LinkOutput to a Package, suitable for execution
pub fn compile_link_output_to_package(
    link_output: LinkOutput,
) -> Result<miden_mast_package::Package, TestCaseError> {
    use midenc_compile::{CodegenOutput, compile_link_output_to_masm_with_pre_assembly_stage};

    // Compile to Package
    let mut pre_assembly_stage = |output: CodegenOutput, _context| {
        println!("# Assembled\n{}", &output.component);
        Ok(output)
    };
    let artifact =
        compile_link_output_to_masm_with_pre_assembly_stage(link_output, &mut pre_assembly_stage)
            .map_err(|err| TestCaseError::fail(format_report(err)))?;
    Ok(artifact.unwrap_mast())
}

/// This helper exists to handle the boilerplate of compiling/assembling the output of the link
/// stage of the compiler to a package, and then evaluating that package with [eval_package].
///
/// Evaluates the package assembled from `link_output` using the debug executor, producing an output
/// of type `T`
///
/// * `initializers` is an optional set of [Initializer] to run at program start by the compiler-
///   emitted test harness, to set up memory or other global state.
/// * `advice_stack` contains additional values to place on the advice stack before program start.
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
    eval_link_output_with_advice_stack(
        link_output,
        initializers,
        core::iter::empty::<Felt>(),
        args,
        session,
        verify_trace,
    )
}

/// Evaluates the package assembled from `link_output` using the debug executor, producing an
/// output of type `T`
///
/// * `initializers` is an optional set of [Initializer] to run at program start by the compiler-
///   emitted test harness, to set up memory or other global state.
/// * `advice_stack` contains additional values to place on the advice stack before program start.
/// * `args` are the set of arguments that will be placed on the operand stack, in order of
///   appearance
/// * `verify_trace` is a callback which gets the [ExecutionTrace], and can be used to assert
///   things about the trace, such as the state of memory at program exit.
pub fn eval_link_output_with_advice_stack<'a, T, I, A, F>(
    link_output: LinkOutput,
    initializers: I,
    advice_stack: A,
    args: &[Felt],
    session: &Session,
    verify_trace: F,
) -> Result<T, TestCaseError>
where
    T: Clone + FromMidenRepr + PartialEq + core::fmt::Debug,
    I: IntoIterator<Item = Initializer<'a>>,
    A: IntoIterator<Item = Felt>,
    F: Fn(&ExecutionTrace) -> Result<(), TestCaseError>,
{
    let package = compile_link_output_to_package(link_output)?;
    eval_package_with_advice_stack(
        &package,
        initializers,
        advice_stack,
        args,
        session,
        verify_trace,
    )
}
