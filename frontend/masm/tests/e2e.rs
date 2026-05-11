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
use midenc_hir::{
    Builder, BuilderExt, Context, Ident, OpBuilder, SourceSpan,
    dialects::builtin::{self, ComponentBuilder, WorldBuilder},
    pass::AnalysisManager,
    version::Version,
};
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

fn assert_roundtrip_outputs(source: &str, inputs: &[u64], num_outputs: usize) {
    let context = e2e_context();
    let original = assemble_original_program(source, &context);
    let roundtripped = assemble_roundtripped_program(source, context.clone());
    let inputs = inputs.iter().copied().map(Felt::new).collect::<Vec<_>>();

    let original_outputs = execute_program(&original, &inputs, num_outputs);
    let roundtripped_outputs = execute_program(&roundtripped, &inputs, num_outputs);

    assert_eq!(
        roundtripped_outputs, original_outputs,
        "round-tripped MASM changed VM-visible stack outputs"
    );
}

fn e2e_context() -> Rc<Context> {
    let mut options = Options::default();
    options.entrypoint = Some("test::entry".to_owned());
    let source_manager = Arc::new(DefaultSourceManager::default());
    let session = Rc::new(Session::new(
        [],
        None,
        None,
        std::env::current_dir().expect("current directory must be available"),
        options,
        None,
        source_manager,
    ));
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

    let component = wrap_module_in_component(context.clone(), disassembled.module);
    let analysis_manager = AnalysisManager::new(component.as_operation_ref(), None);
    let masm_component = component
        .borrow()
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
    let library = assembler
        .assemble_library(masm_component.modules.iter().cloned())
        .unwrap_or_else(|err| {
            panic!("round-tripped MASM should assemble:\n{err}\n\n# Emitted MASM\n{masm_component}")
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
    exec.::"root_ns:root@1.0.0"::test::entry
    exec.sys::truncate_stack
end
"#,
        )
        .expect("round-tripped MASM program should assemble")
}

fn wrap_module_in_component(
    context: Rc<Context>,
    module: builtin::ModuleRef,
) -> builtin::ComponentRef {
    let mut builder = OpBuilder::new(context.clone());
    let world = {
        let builder = builder.create::<builtin::World, ()>(SourceSpan::default());
        builder().expect("failed to create test world")
    };
    let mut world_builder = WorldBuilder::new(world);
    let component = world_builder
        .define_component(
            Ident::with_empty_span("root_ns".into()),
            Ident::with_empty_span("root".into()),
            Version::new(1, 0, 0),
        )
        .expect("failed to create test component");
    {
        let _ = ComponentBuilder::new(component);
    }
    drop(world_builder);
    let body_block = component
        .borrow()
        .body()
        .entry_block_ref()
        .expect("component builder should create an entry block");
    let mut inserter = OpBuilder::new(context);
    inserter.set_insertion_point_to_end(body_block);
    inserter.insert(module.as_operation_ref());
    component
}

fn execute_program(program: &Program, inputs: &[Felt], num_outputs: usize) -> Vec<Felt> {
    let stack_inputs = StackInputs::new(inputs).expect("test inputs should fit on VM stack");
    let mut host = DefaultHost::default();
    let trace = execute_sync(
        program,
        stack_inputs,
        AdviceInputs::default(),
        &mut host,
        ExecutionOptions::default(),
    )
    .expect("program should execute");
    trace.stack_outputs().get_num_elements(num_outputs).to_vec()
}
