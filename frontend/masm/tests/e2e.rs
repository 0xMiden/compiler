use std::{rc::Rc, sync::Arc};

use miden_assembly::Assembler;
use miden_assembly_syntax::{
    Parse, ParseOptions,
    ast::ModuleKind,
    debuginfo::{SourceLanguage, Uri},
};
use miden_core_lib::CoreLibrary;
use miden_processor::{
    DefaultHost, ExecutionOptions, Felt, Program, StackInputs, advice::AdviceInputs, execute_sync,
};
use midenc_codegen_masm::{
    ToMasmComponent,
    intrinsics::{self, MEM_INTRINSICS_MODULE_NAME},
};
use midenc_frontend_masm::{DisassemblerConfig, disassemble_source};
use midenc_hir::{Context, pass::AnalysisManager};
use midenc_session::{Options, Session, diagnostics::DefaultSourceManager};

#[test]
fn e2e_roundtrip_straight_line_felt_arithmetic() {
    assert_roundtrip_outputs(
        r#"
pub proc entry(a: felt, b: felt) -> felt
    add
    mul.3
    add.5
end
"#,
        &[7, 11],
        1,
    );
}

#[test]
fn e2e_roundtrip_stack_reordering() {
    assert_roundtrip_outputs(
        r#"
pub proc entry(a: felt, b: felt) -> (felt, felt)
    swap
end
"#,
        &[3, 5],
        2,
    );
}

#[test]
fn e2e_roundtrip_u32_arithmetic() {
    assert_roundtrip_outputs(
        r#"
pub proc entry(a: u32, b: u32) -> u32
    u32wrapping_add
    u32wrapping_mul.3
end
"#,
        &[13, 29],
        1,
    );
}

#[test]
fn e2e_roundtrip_u32cast_truncates_felt() {
    assert_roundtrip_outputs(
        r#"
pub proc entry() -> u32
    push.4294967297
    u32cast
end
"#,
        &[],
        1,
    );
}

#[test]
fn e2e_roundtrip_word_immediate_order() {
    assert_roundtrip_outputs(
        r#"
pub proc entry() -> (felt, felt, felt, felt)
    push.[1,2,3,4]
end
"#,
        &[],
        4,
    );
}

#[test]
fn e2e_roundtrip_word_slice_order() {
    assert_roundtrip_outputs(
        r#"
pub proc entry() -> (felt, felt)
    push.[1,2,3,4][1..3]
end
"#,
        &[],
        2,
    );
}

#[test]
fn e2e_roundtrip_local_exec() {
    assert_roundtrip_outputs(
        r#"
proc helper(a: felt) -> felt
    add.2
    mul.5
end

pub proc entry(a: felt) -> felt
    exec.helper
    add.1
end
"#,
        &[9],
        1,
    );
}

#[test]
fn e2e_roundtrip_structured_if() {
    let source = r#"
pub proc entry(flag: i1) -> felt
    if.true
        push.17
    else
        push.23
    end
end
"#;
    assert_roundtrip_outputs(source, &[1], 1);
    assert_roundtrip_outputs(source, &[0], 1);
}

#[test]
fn e2e_roundtrip_locals() {
    assert_roundtrip_outputs(
        r#"
@locals(1)
pub proc entry(a: felt) -> felt
    loc_store.0
    loc_load.0
    add.7
end
"#,
        &[35],
        1,
    );
}

#[test]
fn e2e_roundtrip_memory_load_store() {
    assert_roundtrip_outputs(
        r#"
pub proc entry(a: felt) -> felt
    mem_store.0
    mem_load.0
    add.3
end
"#,
        &[41],
        1,
    );
}

#[test]
fn e2e_roundtrip_advice_push() {
    assert_roundtrip_outputs_with_advice(
        r#"
pub proc entry(a: felt) -> felt
    adv_push
    add
end
"#,
        &[37],
        &[5],
        1,
    );
}

fn assert_roundtrip_outputs(source: &str, inputs: &[u64], num_outputs: usize) {
    assert_roundtrip_outputs_with_advice(source, inputs, &[], num_outputs);
}

fn assert_roundtrip_outputs_with_advice(
    source: &str,
    inputs: &[u64],
    advice: &[u64],
    num_outputs: usize,
) {
    let context = e2e_context();
    let original = assemble_original_program(source, &context);
    let roundtripped = assemble_roundtripped_program(source, context.clone());
    let inputs = inputs.iter().copied().map(Felt::new_unchecked).collect::<Vec<_>>();

    let original_outputs = execute_program(&original, &inputs, advice, num_outputs);
    let roundtripped_outputs = execute_program(&roundtripped, &inputs, advice, num_outputs);

    assert_eq!(
        roundtripped_outputs, original_outputs,
        "round-tripped MASM changed VM-visible stack outputs"
    );
}

fn e2e_context() -> Rc<Context> {
    let options = Box::new(Options {
        entrypoint: Some("test::entry".to_owned()),
        ..Options::default()
    })
    .with_output_types(Default::default(), None);
    let source_manager = Arc::new(DefaultSourceManager::default());
    let session = Rc::new(
        Session::new(midenc_session::InputFile::empty(), options, None, source_manager)
            .expect("valid session configuration"),
    );
    Rc::new(Context::new(session))
}

fn assemble_original_program(source: &str, context: &Context) -> Program {
    let source_manager = context.session().source_manager.clone();
    let core_library = CoreLibrary::default();
    let source_file = source_manager.load(
        SourceLanguage::Masm,
        Uri::from("test.masm".to_owned()),
        source.to_owned(),
    );
    let module = source_file
        .parse_with_options(source_manager.clone(), ParseOptions::new(ModuleKind::Library, "test"))
        .expect("original MASM library should parse");
    let library = Assembler::new(source_manager.clone())
        .assemble_library([module])
        .expect("original MASM library should assemble");
    Assembler::new(source_manager)
        .with_static_library(library)
        .expect("original MASM library should link")
        .with_static_library(core_library.library())
        .expect("Miden core library should link")
        .assemble_program(
            r#"
use miden::core::sys

begin
    exec.::test::entry
    exec.sys::truncate_stack
end
"#,
        )
        .expect("original MASM program should assemble")
}

fn assemble_roundtripped_program(source: &str, context: Rc<Context>) -> Program {
    let disassembled =
        disassemble_source(source, "test", &DisassemblerConfig::default(), context.clone())
            .expect("MASM should disassemble to HIR");

    let analysis_manager = AnalysisManager::new(disassembled.world.as_operation_ref(), None);
    let world = disassembled.world.borrow();
    let masm_component = world
        .to_masm_component(analysis_manager)
        .expect("HIR should lower back to MASM");
    let source_manager = context.session().source_manager.clone();
    let core_library = CoreLibrary::default();
    let mut assembler = Assembler::new(source_manager.clone());
    let mem_intrinsics = intrinsics::load(MEM_INTRINSICS_MODULE_NAME, source_manager.clone())
        .expect("memory intrinsics should load");
    assembler
        .compile_and_statically_link(mem_intrinsics)
        .expect("memory intrinsics should statically link");
    for module in masm_component.modules.iter() {
        std::dbg!(module.path());
    }
    let library = assembler
        .assemble_library(masm_component.modules.iter().cloned())
        .unwrap_or_else(|err| {
            panic!(
                "round-tripped MASM should assemble:\nerror: {err}\n\n# Emitted \
                 MASM\n{masm_component}"
            )
        });
    Assembler::new(source_manager)
        .with_static_library(library)
        .expect("round-tripped MASM library should link")
        .with_static_library(core_library.library())
        .expect("Miden core library should link")
        .assemble_program(
            r#"
use miden::core::sys

begin
    exec.::test::entry
    exec.sys::truncate_stack
end
"#,
        )
        .expect("round-tripped MASM program should assemble")
}

fn execute_program(
    program: &Program,
    inputs: &[Felt],
    advice: &[u64],
    num_outputs: usize,
) -> Vec<Felt> {
    let stack_inputs = StackInputs::new(inputs).expect("test inputs should fit on VM stack");
    let advice_inputs = AdviceInputs::default()
        .with_stack_values(advice.iter().copied())
        .expect("test advice inputs should fit on VM advice stack");
    let mut host = DefaultHost::default();
    let trace =
        execute_sync(program, stack_inputs, advice_inputs, &mut host, ExecutionOptions::default())
            .expect("program should execute");
    trace.stack.get_num_elements(num_outputs).to_vec()
}
