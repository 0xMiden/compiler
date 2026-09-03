use miden_assembly::{Assembler, DefaultSourceManager, Linkage};
use miden_core::Felt;
use miden_core_lib::CoreLibrary;
use miden_debug::DebugQuery;
use miden_mast_package::{Package, QualifiedProcedureName};
use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};

use crate::{
    CompilerTestBuilder, end_to_end::support::default_host_with_core_lib,
    testing::executor_with_std,
};

/// This is the component shape emitted by current `wasm-component-ld` for a WASI reactor: a
/// state-free adapter imports an initialization function from an already-instantiated core module
/// and names that import as its start function. The deliberately renamed import module and aliased
/// function export ensure the frontend resolves the target structurally rather than matching
/// `"main"` or `"_initialize"`.
const START_WITH_OBSERVABLE_EFFECT: &str = r#"
(component
  (core module $main
    (type $ctor-result (func (result i32)))
    (memory (export "memory") 1)
    (table 1 funcref)
    (func $read-seed (type $ctor-result) (result i32)
      i32.const 16
      i32.load)
    (elem (i32.const 0) func $read-seed)
    (data (i32.const 16) "\29\00\00\00\00\00\00\00")
    (func $initialize (export "initialize-alias")
      i32.const 4
      i32.const 0
      call_indirect (type $ctor-result)
      i32.const 1
      i32.add
      i32.store)
    (func $entrypoint (export "entrypoint") (result i32)
      i32.const 4
      i32.load))
  (core instance $main-instance (instantiate $main))
  (core module $startup-adapter
    (type $startup (func))
    (import "renamed-main" "initialize-alias" (func $start (type $startup)))
    (start $start))
  (core instance $startup-instance
    (instantiate $startup-adapter
      (with "renamed-main" (instance $main-instance))))
  (type $entrypoint-type (func (result u32)))
  (alias core export $main-instance "entrypoint" (core func $entrypoint-core))
  (func $entrypoint-lifted (type $entrypoint-type)
    (canon lift (core func $entrypoint-core)))
  (component $export-component
    (type $entrypoint-type (func (result u32)))
    (import "import-func-entrypoint" (func $entrypoint (type $entrypoint-type)))
    (export "entrypoint" (func $entrypoint) (func (type $entrypoint-type))))
  (instance $exports
    (instantiate $export-component
      (with "import-func-entrypoint" (func $entrypoint-lifted))))
  (export "miden:test/component-start@1.0.0" (instance $exports))
)
"#;

const TRAPPING_START: &str = r#"
(component
  (core module $main
    (memory 1)
    (func $initialize (export "start-alias") unreachable)
    (func $entrypoint (export "entrypoint") (result i32)
      i32.const 4
      i32.const 99
      i32.store
      unreachable))
  (core instance $main-instance (instantiate $main))
  (core module $startup-adapter
    (import "target" "start-alias" (func $start))
    (start $start))
  (core instance $startup-instance
    (instantiate $startup-adapter (with "target" (instance $main-instance))))
  (type $entrypoint-type (func (result u32)))
  (alias core export $main-instance "entrypoint" (core func $entrypoint-core))
  (func $entrypoint-lifted (type $entrypoint-type)
    (canon lift (core func $entrypoint-core)))
  (component $export-component
    (type $entrypoint-type (func (result u32)))
    (import "import-func-entrypoint" (func $entrypoint (type $entrypoint-type)))
    (export "entrypoint" (func $entrypoint) (func (type $entrypoint-type))))
  (instance $exports
    (instantiate $export-component
      (with "import-func-entrypoint" (func $entrypoint-lifted))))
  (export "miden:test/component-start@1.0.0" (instance $exports))
)
"#;

/// Start increments context-local memory. The lifted entrypoint reaches the read through an
/// internal core-Wasm call, so one boundary invocation covers both lifecycle edges: `call`
/// creates a fresh context and the internal call stays in that context without reinitializing it.
const COUNTING_START: &str = r#"
(component
  (core module $main
    (memory (export "memory") 1)
    (func $initialize (export "start-alias")
      i32.const 4
      i32.const 4
      i32.load
      i32.const 1
      i32.add
      i32.store)
    (func $read-count (result i32)
      i32.const 4
      i32.load)
    (func $entrypoint (export "entrypoint") (result i32)
      call $read-count))
  (core instance $main-instance (instantiate $main))
  (core module $startup-adapter
    (import "target" "start-alias" (func $start))
    (start $start))
  (core instance $startup-instance
    (instantiate $startup-adapter (with "target" (instance $main-instance))))
  (type $entrypoint-type (func (result u32)))
  (alias core export $main-instance "entrypoint" (core func $entrypoint-core))
  (func $entrypoint-lifted (type $entrypoint-type)
    (canon lift (core func $entrypoint-core)))
  (component $export-component
    (type $entrypoint-type (func (result u32)))
    (import "import-func-entrypoint" (func $entrypoint (type $entrypoint-type)))
    (export "entrypoint" (func $entrypoint) (func (type $entrypoint-type))))
  (instance $exports
    (instantiate $export-component
      (with "import-func-entrypoint" (func $entrypoint-lifted))))
  (export "miden:test/component-start@1.0.0" (instance $exports))
)
"#;

fn compile_library(wat: &str) -> std::sync::Arc<Package> {
    let wasm = wat::parse_str(wat).expect("component fixture must be valid WebAssembly text");
    let builder = CompilerTestBuilder::from_wasm(
        "component_start",
        wasm,
        ["--target=miden:test/component-start@1.0.0".to_string()],
    );
    let package = builder.build().compile_package();
    assert!(package.is_library(), "component fixture should compile as a library");
    package
}

fn entrypoint(package: &Package) -> QualifiedProcedureName {
    let entrypoint = package
        .manifest
        .exports()
        .filter_map(|export| export.as_procedure())
        .find(|export| export.path.as_ref().as_str().ends_with("::entrypoint"))
        .expect("component fixture should export entrypoint");
    QualifiedProcedureName::from(entrypoint.path.clone())
}

fn compile(wat: &str) -> std::sync::Arc<Package> {
    let package = compile_library(wat);
    package
        .make_executable(&entrypoint(&package))
        .map(std::sync::Arc::new)
        .expect("component entrypoint should be executable")
}

fn boundary_caller(library: std::sync::Arc<Package>, calls: usize) -> std::sync::Arc<Package> {
    let source_manager = std::sync::Arc::new(DefaultSourceManager::default());
    let mut assembler = Assembler::new(source_manager);
    for package in CoreLibrary::default().packages() {
        assembler
            .link_package(package, Linkage::Dynamic)
            .expect("core library package should link");
    }
    let target = entrypoint(&library);
    let calls = (0..calls).map(|_| format!("    call.{target}\n")).collect::<String>();
    let source = format!("begin\n{calls}end\n");
    assembler
        .with_package(library, Linkage::Static)
        .expect("component library should link into caller")
        .assemble_program("component-start-caller", &source)
        .map(std::sync::Arc::from)
        .expect("component boundary caller should assemble")
}

#[test]
fn component_start_runs_after_static_initialization_and_before_entrypoint() {
    let package = compile(START_WITH_OBSERVABLE_EFFECT);
    let trace = execute_sync(
        &package.unwrap_program(),
        StackInputs::default(),
        AdviceInputs::default(),
        &mut default_host_with_core_lib(),
        ExecutionOptions::default(),
    )
    .expect("component start and entrypoint should execute");

    assert_eq!(trace.stack.get_num_elements(1), &[Felt::new_unchecked(42)]);
}

#[test]
fn a_trapping_component_start_prevents_entrypoint_execution() {
    let package = compile(TRAPPING_START);
    execute_sync(
        &package.unwrap_program(),
        StackInputs::default(),
        AdviceInputs::default(),
        &mut default_host_with_core_lib(),
        ExecutionOptions::default(),
    )
    .expect_err("a trapping component start must prevent entrypoint execution");

    let trace = executor_with_std(vec![])
        .capture_trace(package, std::sync::Arc::new(DefaultSourceManager::default()));
    assert_eq!(
        trace.read_from_rust_memory::<u32>(4).unwrap_or_default(),
        0,
        "the trapping start must stop execution before the entrypoint's memory write"
    );
}

#[test]
fn each_library_boundary_call_initializes_one_fresh_non_reentrant_context() {
    let library = compile_library(COUNTING_START);
    let package = boundary_caller(library, 2);
    let trace = execute_sync(
        &package.unwrap_program(),
        StackInputs::default(),
        AdviceInputs::default(),
        &mut default_host_with_core_lib(),
        ExecutionOptions::default(),
    )
    .expect("both component boundary calls should execute");

    assert_eq!(
        trace.stack.get_num_elements(2),
        &[Felt::ONE, Felt::ONE],
        "each fresh context must run start exactly once; the entrypoint's internal call must not"
    );
}
