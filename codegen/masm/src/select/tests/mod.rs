use alloc::rc::Rc;

use midenc_hir::{
    Context,
    diagnostics::Uri,
    dialects::{builtin, builtin::FunctionRef},
};

use crate::{
    select::{SelectEnv, select_function},
    verify::{SourceContext, check_selection},
};

pub(crate) struct Parsed {
    pub context: Rc<Context>,
    pub function: FunctionRef,
}

impl core::ops::Deref for Parsed {
    type Target = FunctionRef;

    fn deref(&self) -> &FunctionRef {
        &self.function
    }
}

pub(crate) fn parse(text: &str) -> Parsed {
    parse_with_verification(text, true)
}

fn parse_unverified(text: &str) -> Parsed {
    parse_with_verification(text, false)
}

fn parse_with_verification(text: &str, verify: bool) -> Parsed {
    let context = Rc::new(Context::default());
    let function = midenc_hir::parse::parse_any(
        midenc_hir::parse::ParserConfig {
            context: context.clone(),
            verify,
        },
        Uri::new("selection.hir"),
        text,
    )
    .expect("fixture parses")
    .try_downcast_op::<builtin::Function>()
    .expect("function fixture");
    Parsed { context, function }
}

#[test]
fn real_hir_api_returns_checked_registered_source_evidence() {
    let f = parse(
        "builtin.function public extern(\"C\") @simple(%0: felt) -> felt { builtin.ret %0 : \
         (felt); };",
    );
    let function = f.borrow();
    let env = SelectEnv::default();
    let selected = select_function(&function, &env).unwrap();
    let checked = check_selection(
        selected,
        &SourceContext::Hir {
            function: &function,
            env: &env,
        },
    )
    .unwrap();
    assert_eq!(checked.abi.arguments(), 1);
    assert_eq!(checked.edges.len(), 1);
}

mod aliases;
mod arithmetic;
mod control;
mod coverage;
mod indirect;
mod intrinsics;
mod memory;

use alloc::{vec, vec::Vec};

use midenc_hir::{Op, formatter::PrettyPrint};

use crate::{
    allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
    lower::{expand::*, units::LoweringRegistry},
    plan::*,
    verify,
};

fn checked(f: &Parsed, env: &SelectEnv<'_>) -> verify::CheckedSelectedProgram {
    let function = f.borrow();
    let program = select_function(&function, env).unwrap();
    check_selection(
        program,
        &SourceContext::Hir {
            function: &function,
            env,
        },
    )
    .unwrap()
}
pub(crate) fn ready(
    f: &Parsed,
    env: &SelectEnv<'_>,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let function = f.borrow();
    let evidence = crate::select::evidence(&function, env).unwrap();
    let locals = evidence.source.source_locals.clone();
    let selected = check_selection(
        evidence.source.program,
        &SourceContext::Hir {
            function: &function,
            env,
        },
    )
    .unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &locals).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut registry = LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition).unwrap();
    }
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    (plan, frame, body)
}
fn selected_procedure(f: &Parsed, env: &SelectEnv<'_>) -> alloc::string::String {
    let (plan, frame, body) = ready(f, env);
    verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string()
}

fn execute(f: &Parsed, env: &SelectEnv<'_>, inputs: &[usize], expected: &[usize]) {
    let procedure = selected_procedure(f, env);
    let mut result = expected.to_vec();
    result.extend([901, 902]);
    let code = alloc::format!(
        "{procedure} begin push.902 push.901 {} exec.selected {} end",
        crate::plan::tests::lowering_units::push_top(inputs),
        crate::plan::tests::lowering_units::assert_top(&result)
    );
    execute_code(&code).unwrap();
}
fn execute_linked(f: &Parsed, inputs: &[usize], expected: &[usize]) {
    execute_linked_memory(f, inputs, expected, "", "").unwrap();
}

fn execute_linked_memory(
    f: &Parsed,
    inputs: &[usize],
    expected: &[usize],
    initialize: &str,
    observe: &str,
) -> Result<(), midenc_hir::Report> {
    execute_linked_memory_mutate(f, inputs, expected, initialize, observe, |_| {})
}

fn execute_linked_memory_mutate(
    f: &Parsed,
    inputs: &[usize],
    expected: &[usize],
    initialize: &str,
    observe: &str,
    mutate: impl FnOnce(&mut AnnotatedMasmBody),
) -> Result<(), midenc_hir::Report> {
    let (plan, frame, mut body) = ready(f, &SelectEnv::default());
    mutate(&mut body);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
    let intrinsics = crate::intrinsics::load(f.context.session()).unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    assembler
        .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
        .unwrap();
    let suffix = (0..20).map(|i| 900 + i).collect::<Vec<_>>();
    let mut result = expected.to_vec();
    result.extend(&suffix);
    let code = alloc::format!(
        "{procedure} begin {initialize} {} {} exec.selected {} {observe} \
         exec.::miden::core::sys::truncate_stack end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::push_top(inputs),
        crate::plan::tests::lowering_units::assert_top(&result)
    );
    let program = assembler
        .assemble_program("selected_arithmetic", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
    host.load_library(miden_processor::HostLibrary::from(intrinsics)).unwrap();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .map_err(|e| midenc_hir::Report::msg(e.to_string()))?;
    Ok(())
}
fn execute_code(code: &str) -> Result<(), midenc_hir::Report> {
    let program = miden_assembly::Assembler::default()
        .assemble_program("selected_hir", code)?
        .try_into_program()?;
    execute_program(&program, &mut crate::plan::tests::lowering_units::host(), &[])
}

fn execute_program(
    program: &miden_core::program::Program,
    host: &mut miden_processor::DefaultHost,
    advice: &[u64],
) -> Result<(), midenc_hir::Report> {
    processor_with_advice(advice)
        .execute_sync(program, host)
        .map_err(|e| midenc_hir::Report::msg(e.to_string()))?;
    Ok(())
}
fn processor_with_advice(advice: &[u64]) -> miden_processor::FastProcessor {
    let advice = miden_processor::advice::AdviceInputs::default()
        .with_stack(advice.iter().copied().map(miden_core::Felt::new_unchecked).collect());
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .with_advice(advice)
        .unwrap()
}

fn execute_failure_state(
    program: &miden_core::program::Program,
    advice: &[u64],
) -> (miden_processor::FastProcessor, miden_processor::ExecutionError) {
    let mut processor = processor_with_advice(advice);
    let mut host = crate::plan::tests::lowering_units::host();
    let mut context = processor.get_initial_resume_context(program).unwrap();
    loop {
        match processor.step_sync(&mut host, context) {
            Ok(Some(next)) => context = next,
            Ok(None) => panic!("expected VM failure"),
            Err(error) => return (processor, error),
        }
    }
}

fn execute_kernel_probe(f: &Parsed, probe: &str, source: &str) -> Result<(), midenc_hir::Report> {
    use alloc::sync::Arc;

    use miden_assembly::{
        Assembler, DefaultSourceManager, Path,
        ast::{Module, ModuleKind},
    };
    let manager = Arc::new(DefaultSourceManager::default());
    let procedure = selected_procedure(f, &SelectEnv::default());
    let module = Module::parser(Some(ModuleKind::Kernel)).parse_str(
        Some(Path::KERNEL),
        format!("{procedure} pub proc probe {probe} end"),
        manager.clone(),
    )?;
    let kernel: Arc<miden_assembly::package::Package> =
        Assembler::new(manager.clone()).assemble_kernel("kernel", module, None)?.into();
    let program = Assembler::with_kernel(manager, kernel.clone())?
        .assemble_program("selected_kernel", source)?
        .try_into_program()?;
    let mut host = crate::plan::tests::lowering_units::host();
    host.load_library(miden_processor::HostLibrary::from(kernel)).unwrap();
    execute_program(&program, &mut host, &[])
}

fn parse_module_function(text: &str) -> Parsed {
    let context = Rc::new(Context::default());
    let root = midenc_hir::parse::parse_any(
        midenc_hir::parse::ParserConfig {
            context: context.clone(),
            verify: true,
        },
        Uri::new("table.hir"),
        text,
    )
    .expect("module fixture parses");
    let root = root.borrow();
    let module = root.downcast_ref::<builtin::Module>().expect("module");
    let function = module
        .body()
        .entry()
        .body()
        .iter()
        .filter_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
        .next_back()
        .unwrap();
    Parsed { context, function }
}

fn linked(f: &Parsed) -> crate::linker::LinkInfo {
    let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let root = module.borrow().nearest_symbol_table().unwrap();
    crate::linker::Linker::new(0, 0).link(None, &root.borrow()).unwrap()
}

fn assert_malformed_source_rejected(f: &Parsed, env: &SelectEnv<'_>) {
    let function = f.borrow();
    function.as_operation().recursively_verify().unwrap();
    let before = function.as_operation().to_pretty_string();
    assert!(
        select_function(&function, env).is_err(),
        "malformed source was selected: {before}"
    );
    assert_eq!(before, function.as_operation().to_pretty_string());
}

#[derive(Debug, PartialEq, Eq)]
struct PrintLnRecord(log::Level, alloc::string::String);

std::thread_local! {
    static PRINTLN_CAPTURE: core::cell::RefCell<Option<Vec<PrintLnRecord>>> = const { core::cell::RefCell::new(None) };
}

struct PrintLnLogger;

impl log::Log for PrintLnLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        matches!(
            (metadata.target(), metadata.level()),
            ("stdout", log::Level::Info) | ("executor", log::Level::Warn)
        )
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            PRINTLN_CAPTURE.with(|capture| {
                if let Some(records) = capture.borrow_mut().as_mut() {
                    records
                        .push(PrintLnRecord(record.level(), alloc::format!("{}", record.args())));
                }
            });
        }
    }

    fn flush(&self) {}
}

fn capture_println<T>(run: impl FnOnce() -> T) -> (T, Vec<PrintLnRecord>) {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            PRINTLN_CAPTURE.with(|capture| {
                capture.borrow_mut().take();
            });
        }
    }
    PRINTLN_CAPTURE.with(|capture| assert!(capture.borrow_mut().replace(vec![]).is_none()));
    let clear = Clear;
    let result = run();
    let records = PRINTLN_CAPTURE.with(|capture| capture.borrow_mut().take().unwrap());
    drop(clear);
    (result, records)
}

fn debug_println_code(code: &str) -> Vec<PrintLnRecord> {
    debug_println_packages(code, &[], 1, 0)
}

fn debug_println_packages(
    code: &str,
    packages: &[alloc::sync::Arc<miden_assembly::package::Package>],
    println_events: usize,
    frame_events: usize,
) -> Vec<PrintLnRecord> {
    debug_println_inputs(code, packages, println_events, frame_events, &[])
}

fn debug_println_inputs(
    code: &str,
    packages: &[alloc::sync::Arc<miden_assembly::package::Package>],
    println_events: usize,
    frame_events: usize,
    inputs: &[usize],
) -> Vec<PrintLnRecord> {
    use alloc::sync::Arc;
    let source = Arc::new(miden_assembly::DefaultSourceManager::default());
    let mut assembler = miden_assembly::Assembler::new(source.clone());
    let mut executor = miden_debug::Executor::new(
        inputs.iter().map(|v| miden_core::Felt::new_unchecked(*v as u64)).collect(),
    );
    for package in packages {
        assembler
            .link_package(package.clone(), miden_assembly::Linkage::Dynamic)
            .unwrap();
        executor.with_package(package.clone()).unwrap();
    }
    let program = assembler.assemble_program("println", code).map(Arc::from).unwrap();
    executor.with_advice_inputs(
        miden_processor::advice::AdviceInputs::default()
            .with_stack(vec![miden_core::Felt::from(79u32), miden_core::Felt::from(83u32)].into()),
    );
    executor.with_event_advice_mutations_recording();
    let mut debug = executor.into_debug(program, source);
    assert_eq!(debug.processor.stack_depth() as usize, inputs.len().max(16));
    let mut printed = 0;
    let mut frames = 0;
    let (_, records) = capture_println(|| {
        while !debug.stopped {
            // Push(ID) has already run. Snapshot only this narrow event boundary;
            // procedure setup, spills and the following cleanup have independent effects.
            let boundary = if debug.processor.stack_get(0).as_canonical_u64()
                == crate::Event::PrintLn.as_event_id().as_u64()
            {
                let state = debug.processor.state();
                Some((
                    state.get_stack_state(),
                    state.advice_provider().clone(),
                    debug.processor.memory().get_memory_state(state.ctx()),
                ))
            } else {
                None
            };
            debug.step().unwrap();
            let batches = debug.host.take_recorded_event_mutations();
            if batches.is_empty() {
                continue;
            }
            assert_eq!(batches.len(), 1);
            assert!(batches[0].is_empty());
            if let Some((stack, advice, memory)) = boundary {
                printed += 1;
                let state = debug.processor.state();
                assert_eq!(state.get_stack_state(), stack, "handler changed operand stack");
                assert_eq!(*state.advice_provider(), advice, "handler changed advice");
                assert_eq!(
                    debug.processor.memory().get_memory_state(state.ctx()),
                    memory,
                    "handler wrote operand memory"
                );
            } else {
                frames += 1;
            }
        }
    });
    assert_eq!(printed, println_events, "PrintLn event occurrences");
    assert_eq!(frames, frame_events, "frame event occurrences");
    assert_eq!(
        records.len(),
        println_events,
        "one actual output/warning per PrintLn invocation"
    );
    records
}
