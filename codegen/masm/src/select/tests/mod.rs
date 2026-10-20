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
fn ready(
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
fn execute(f: &Parsed, env: &SelectEnv<'_>, inputs: &[usize], expected: &[usize]) {
    let (plan, frame, body) = ready(f, env);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
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
    let (plan, frame, body) = ready(f, &SelectEnv::default());
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
        "{procedure} begin {} {} exec.selected {} exec.::miden::core::sys::truncate_stack end",
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
        .unwrap();
}
fn execute_code(code: &str) -> Result<(), midenc_hir::Report> {
    let program = miden_assembly::Assembler::default()
        .assemble_program("selected_hir", code)?
        .try_into_program()?;
    let mut host = crate::plan::tests::lowering_units::host();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .map_err(|e| midenc_hir::Report::msg(e.to_string()))?;
    Ok(())
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
