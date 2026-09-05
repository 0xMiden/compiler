use miden_debug::{FromMidenRepr, ToMidenRepr};
use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};

use crate::{CompilerTestBuilder, end_to_end::support::default_host_with_core_lib};

/// Preserve the wide-product and local-use ordering of a producer artifact independently
/// of Rust optimization choices. Wasm semantics, including zero-initialized locals, are the oracle.
#[test]
fn wide_product_local_order_matches_wasm() {
    let wasm = wat::parse_str(
        r#"(module
  (func $entrypoint (export "entrypoint") (param i32 i32) (result i32)
    (local i64 i64 i64)
    local.get 1
    i32.extend8_s
    local.get 0
    i32.extend16_s
    i32.xor
    local.get 3
    local.get 0
    i64.extend_i32_s
    local.tee 2
    i64.const -1311768467463790321
    i64.mul_wide_s
    local.set 3
    local.tee 4
    i64.extend8_s
    i64.xor
    i64.extend16_s
    local.get 1
    i64.extend_i32_s
    local.get 2
    i64.mul_wide_s
    local.set 2
    i64.extend32_s
    i64.xor
    local.get 3
    local.get 4
    local.get 2
    i64.xor
    i64.xor
    i64.xor
    local.tee 3
    i64.const 32
    i64.shr_u
    local.get 3
    i64.xor
    i32.wrap_i64
    i32.xor
  )
)"#,
    )
    .unwrap();
    let mut config = wasmi::Config::default();
    config.wasm_wide_arithmetic(true);
    let engine = wasmi::Engine::new(&config);
    let module = wasmi::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmi::Store::new(&engine, ());
    let instance = wasmi::Linker::<()>::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    let oracle = instance.get_typed_func::<(u32, u32), u32>(&store, "entrypoint").unwrap();

    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let program = builder.build().compile_package().unwrap_program();
    let pinned = (3022925119, 3340151117);
    assert_eq!(oracle.call(&mut store, pinned).unwrap(), 3550391763);
    for inputs in [pinned, (0, 0), (1, u32::MAX), (u32::MAX, 1), (1 << 31, 1 << 31)] {
        let expected = oracle.call(&mut store, inputs).unwrap();
        let mut stack = Vec::new();
        inputs.0.push_to_operand_stack(&mut stack);
        inputs.1.push_to_operand_stack(&mut stack);
        let trace = execute_sync(
            &program,
            StackInputs::new(&stack).unwrap(),
            AdviceInputs::default(),
            &mut default_host_with_core_lib(),
            ExecutionOptions::default(),
        )
        .unwrap();
        assert_eq!(
            u32::from_felts(trace.stack.get_num_elements(1)),
            expected,
            "inputs: {inputs:?}"
        );
    }
}
