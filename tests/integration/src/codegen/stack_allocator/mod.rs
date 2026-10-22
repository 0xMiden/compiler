//! Production allocation regressions and opt-in, comparable workload measurements.

use std::time::Instant;

use miden_debug::DebugQuery;
use midenc_hir::Felt;

use crate::{
    CompilerTest,
    testing::{eval_package, stripped_mast_size_str},
};

/// Keep measurements descriptive: semantic assertions are the acceptance oracle.
fn execute_workload(name: &'static str, source: &str, inputs: &[u32], expected: u32) {
    let wasm = wat::parse_str(source).unwrap();
    let mut test = CompilerTest::from_wasm(
        name,
        wasm,
        ["--test-harness".into(), "--entrypoint".into(), format!("{name}::entrypoint")],
    );
    let started = Instant::now();
    let masm = test.masm_src();
    let package = test.compile_package();
    let compile_us = started.elapsed().as_micros();
    if let Some(directory) = std::env::var_os("MIDENC_STACK_ALLOCATOR_MASM_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::write(directory.join(format!("{name}.masm")), &masm).unwrap();
        std::fs::write(directory.join(format!("{name}.wat")), source).unwrap();
    }
    let args = inputs.iter().copied().map(Felt::from).collect::<Vec<_>>();
    let measured = std::env::var_os("MIDENC_STACK_ALLOCATOR_METRICS").is_some();
    let output = eval_package::<u32, _, _>(package.clone(), None, &args, &test.session, |trace| {
        if measured {
            eprintln!(
                "allocator-workload {name} cycles={} mast_bytes={} compile_us={compile_us} \
                 local_reads={} local_writes={} frames={:?}",
                trace.current_clock().as_usize(),
                stripped_mast_size_str(&package),
                masm.matches("loc_load").count(),
                masm.matches("loc_store").count(),
                masm.lines()
                    .filter(|line| line.trim().starts_with("@locals("))
                    .collect::<Vec<_>>(),
            );
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(output, expected, "{name}, inputs={inputs:?}");
}

#[test]
fn baseline_pressure() {
    let parameters = "i32 ".repeat(16);
    let sum = (1..16).map(|index| format!("local.get {index} i32.add ")).collect::<String>();
    let arguments = (0..16).map(|index| format!("local.get {index} ")).collect::<String>();
    let source = format!(
        r#"(module
      (func $callee (param {parameters}) (result i32) local.get 0 {sum})
      (func (export "entrypoint") (param {parameters}) (result i32)
        local.get 0 i32.const 17 i32.mul {arguments} call $callee i32.add))"#
    );
    execute_workload("allocator_pressure", &source, &(1..=16).collect::<Vec<_>>(), 153);
}

#[test]
fn baseline_join() {
    for (condition, expected) in [(0, 40), (1, 20)] {
        execute_workload(
            "allocator_join",
            r#"(module
          (func (export "entrypoint") (param i32 i32 i32) (result i32)
            local.get 0 if (result i32) local.get 1 i32.const 3 i32.mul
            else local.get 2 i32.const 5 i32.mul end local.get 1 i32.add))"#,
            &[condition, 5, 7],
            expected,
        );
    }
}

#[test]
fn baseline_loop() {
    execute_workload(
        "allocator_loop",
        r#"(module
      (func (export "entrypoint") (param i32 i32) (result i32) (local i32)
        block loop local.get 0 i32.eqz br_if 1
          local.get 2 local.get 1 i32.add local.set 2
          local.get 1 i32.const 3 i32.add local.set 1
          local.get 0 i32.const 1 i32.sub local.set 0 br 0
        end end local.get 2 local.get 1 i32.add))"#,
        &[4, 7],
        65,
    );
}

#[test]
fn baseline_call() {
    execute_workload(
        "allocator_call",
        r#"(module
      (func $callee (param i32 i32) (result i32)
        local.get 0 i32.const 11 i32.mul local.get 1 i32.add)
      (func (export "entrypoint") (param i32 i32) (result i32)
        local.get 1 local.get 0 call $callee local.get 0 i32.add))"#,
        &[5, 7],
        87,
    );
}

#[test]
fn baseline_generated_data_globals() {
    execute_workload(
        "allocator_data_globals",
        r#"(module
      (memory 1) (data (i32.const 32) "\11\00\00\00")
      (global $g (mut i32) (i32.const 7))
      (func (export "entrypoint") (param i32) (result i32)
        i32.const 32 i32.load global.get $g i32.add local.get 0 i32.add))"#,
        &[5],
        29,
    );
}

/// Select one exact migrated module from the CLI publication without duplicating its source.
fn compile_shape(
    shape: &str,
    entrypoint: &str,
) -> (std::sync::Arc<miden_mast_package::Package>, std::rc::Rc<midenc_hir::Context>) {
    let fixtures = include_str!("../../../../lit/midenc/stack-allocator-selection.hir");
    let begin = format!("// BEGIN {shape}\n");
    let end = format!("// END {shape}\n");
    let module = fixtures.split_once(&begin).unwrap().1.split_once(&end).unwrap().0;
    compile_hir_module(shape, entrypoint, module, |_| {})
}

fn compile_hir_module(
    shape: &str,
    entrypoint: &str,
    module: &str,
    prepare: impl FnOnce(midenc_hir::OperationRef),
) -> (std::sync::Arc<miden_mast_package::Package>, std::rc::Rc<midenc_hir::Context>) {
    let (_, package, context) = compile_hir_module_with_masm(shape, entrypoint, module, prepare);
    (package, context)
}

fn compile_hir_module_with_masm(
    shape: &str,
    entrypoint: &str,
    module: &str,
    prepare: impl FnOnce(midenc_hir::OperationRef),
) -> (
    String,
    std::sync::Arc<miden_mast_package::Package>,
    std::rc::Rc<midenc_hir::Context>,
) {
    use midenc_hir::{diagnostics::Uri, dialects::builtin, pass::registry::RegistryEntry};

    let entry = format!("{}::{entrypoint}", shape.replace('-', "_"));
    let context = crate::testing::setup::dummy_context(&["--test-harness", "--entrypoint", &entry]);
    let source = format!("builtin.world {{\n{module}\n}};\n");
    let world = midenc_hir::parse::parse_any(
        midenc_hir::parse::ParserConfig {
            context: context.clone(),
            verify: true,
        },
        Uri::new(format!("{shape}.hir")),
        &source,
    )
    .unwrap()
    .try_downcast_op::<builtin::World>()
    .unwrap();
    prepare(world.as_operation_ref());
    // The package helper accepts final HIR, so lift the preserved CFG before handing it over.
    let mut passes = midenc_hir::pass::PassManager::on::<builtin::World>(
        context.clone(),
        midenc_hir::pass::Nesting::Implicit,
    );
    {
        let mut module_passes = passes.nest::<builtin::Module>();
        let mut function_passes = module_passes.nest::<builtin::Function>();
        let canonicalizer = midenc_hir::pass::registry::PassInfo::lookup("canonicalizer").unwrap();
        canonicalizer
            .add_to_pipeline(&mut function_passes, "", context.diagnostics())
            .unwrap();
        function_passes.add_pass(Box::new(midenc_dialect_scf::transforms::LiftControlFlowToSCF));
        canonicalizer
            .add_to_pipeline(&mut function_passes, "", context.diagnostics())
            .unwrap();
    }
    passes.run(world.as_operation_ref()).unwrap();
    let mut component = crate::testing::setup::build_empty_component_for_test(context.clone());
    component.world = world;
    component.component = None;
    let (masm, package) =
        crate::testing::compile_miden_component_to_package_with_masm(component).unwrap();
    (masm, package, context)
}

fn execute_shape(shape: &str, entrypoint: &str, inputs: &[u32], expected: u32) {
    let (package, context) = compile_shape(shape, entrypoint);
    let args = inputs.iter().copied().map(Felt::from).collect::<Vec<_>>();
    let output =
        eval_package::<u32, _, _>(package, None, &args, context.session(), |_| Ok(())).unwrap();
    assert_eq!(output, expected, "{shape}, inputs={inputs:?}");
}

#[test]
fn reloaded_region_capture() {
    for predecessor in [0, 1] {
        for captured in [0, 1] {
            execute_shape(
                "reloaded-region-capture",
                "reloaded_capture",
                &[predecessor, captured],
                captured,
            );
        }
    }
}

#[test]
fn unreachable_predecessor() {
    for base in [0, 4] {
        execute_shape("unreachable-predecessor", "unreachable_predecessor", &[base], 79 + 2 * base);
    }
}

#[test]
fn region_live_through() {
    for condition in [0, 1] {
        for live in [17, 31337] {
            execute_shape("region-live-through", "live_through", &[condition, live], live);
            for inner in [0, 1] {
                execute_shape(
                    "region-live-through",
                    "nested_live_through",
                    &[condition, live, inner],
                    live,
                );
            }
        }
    }
}

#[test]
fn entry_backedge() {
    let (package, context) = compile_shape("entry-backedge", "entry_backedge");
    let mut executor = crate::testing::executor_with_std(vec![]);
    executor.with_advice_inputs(
        miden_processor::advice::AdviceInputs::default().with_stack(vec![Felt::ZERO].into()),
    );
    let mut vm = executor.into_debug(package, context.session().source_manager.clone());
    // The original regression has two edges to the parameterless entry and never returns.
    // Bounded stepping exercises repeated backedges without replacing its control flow.
    for _ in 0..4096 {
        assert!(!vm.stopped, "the original entry-backedge fixture cannot terminate");
        vm.step().unwrap();
    }
}

#[test]
fn dead_successor() {
    for base in [0, 4] {
        execute_shape("dead-successor", "dead_successor", &[base], 79 + 2 * base);
    }
}

#[test]
fn join_covered_reload() {
    for base in [0, 4] {
        execute_shape("join-covered-reload", "join_covered_reload", &[base], 79 + 2 * base);
    }
}

#[test]
fn entry_loop_preserves_mixed_argument_order_and_evolution() {
    let source = r#"builtin.module public @typed_entry {
        builtin.function public extern("C") @entrypoint() -> u64 {
            %initial_a = arith.constant 1 : u32;
            %initial_b = arith.constant 4294967303 : u64;
            %result = hir.exec ::@typed_entry::@cycle(%initial_a, %initial_b) : extern("C") (u32, u64) -> u64;
            builtin.ret %result : u64;
        };
        builtin.function private extern("C") @cycle(%a: u32, %b: u64) -> u64 {

            %one = arith.constant 1 : u32;
            %seven = arith.constant 7 : u64;
            %next_a = arith.add %a, %one <{ overflow = #builtin.overflow<unchecked> }>;
            %next_b = arith.add %b, %seven <{ overflow = #builtin.overflow<unchecked> }>;
            %limit = arith.constant 4 : u32;
            %done = arith.eq %next_a, %limit;
            cf.cond_br %done ^exit, ^again : (i1);
        ^again: cf.br ^back(%next_a, %next_b : u32, u64);
        ^exit:
            %wide_a = arith.zext %next_a <{ ty = #builtin.type<u64> }>;
            %result = arith.add %next_b, %wide_a <{ overflow = #builtin.overflow<unchecked> }>;
            builtin.ret %result : u64;
        ^back(%x: u32, %y: u64): ub.unreachable;
        };
    };"#;
    let (package, context) = compile_hir_module("typed-entry", "entrypoint", source, |world| {
        use midenc_hir::{Forward, OperationRef, RawWalk, dialects::builtin::Function};

        // Textual HIR cannot label an entry with named signature arguments. Construct the
        // actual typed entry back edge before running any canonicalization or lifting.
        let mut cycle = None;
        world.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
            let op = op.borrow();
            if let Some(function) = op.downcast_ref::<Function>() {
                let body = function.body();
                let entry = body.entry_block_ref().unwrap();
                if entry.borrow().num_arguments() == 2 {
                    cycle = Some((entry, body.body().back().as_pointer().unwrap()));
                }
            }
        });
        let (entry, mut placeholder) = cycle.expect("typed loop function");
        world.raw_prewalk_all::<Forward, _>(|mut op: OperationRef| {
            let mut op = op.borrow_mut();
            for index in 0..op.num_successors() {
                if op.successor(index).successor() == placeholder {
                    op.successor_mut(index).set(entry);
                }
            }
        });
        placeholder.borrow_mut().erase();
        assert!(entry.borrow().has_predecessors());
        world.borrow().recursively_verify().unwrap();
    });
    let output =
        eval_package::<u64, _, _>(package, None, &[], context.session(), |_| Ok(())).unwrap();
    assert_eq!(output, 4_294_967_303 + 3 * 7 + 4);
}

#[test]
fn wide_join_257_predecessors() {
    let (package, context) = compile_shape("wide-join", "wide_join");
    // Visit all 256 explicit CFG predecessors and the distinct fallback predecessor.
    for selector in 0u32..=256 {
        let live = 1009 + selector * 17;
        let output = eval_package::<u32, _, _>(
            package.clone(),
            None,
            &[Felt::from(selector), Felt::from(live)],
            context.session(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(output, selector + live, "predecessor {selector}");
    }
}

fn wide_memory_oracle(name: &str, source: &str, inputs: &[u32], expected: &[u32]) {
    let wasm = wat::parse_str(source).unwrap();
    let mut test = CompilerTest::from_wasm(
        name.to_owned(),
        wasm,
        ["--test-harness".into(), "--entrypoint".into(), format!("{name}::entrypoint")],
    );
    let package = test.compile_package();
    let args = inputs.iter().copied().map(Felt::from).collect::<Vec<_>>();
    eval_package::<u32, _, _>(package, None, &args, &test.session, |trace| {
        for (lane, value) in expected.iter().enumerate() {
            assert_eq!(
                trace.read_from_rust_memory::<u32>(256 + lane as u32 * 4).unwrap(),
                *value,
                "{name}: lane {lane}, inputs={inputs:?}"
            );
        }
        Ok(())
    })
    .unwrap();
}

/// All columns remain observable individually after a twenty-lane control-flow interface.
#[test]
fn wide_if_distinct_lanes() {
    let results = "i32 ".repeat(20);
    let arm = |offset| {
        (0..20)
            .map(|lane| format!("local.get 1 i32.const {} i32.add ", offset + lane))
            .collect::<String>()
    };
    let assign = (2..22).rev().map(|local| format!("local.set {local} ")).collect::<String>();
    let stores = (0..20)
        .map(|lane| format!("i32.const {} local.get {} i32.store ", 256 + lane * 4, lane + 2))
        .collect::<String>();
    let source = format!(
        r#"(module (memory 1)
      (func (export "entrypoint") (param i32 i32) (result i32) (local {results})
        local.get 0 if (result {results}) {} else {} end {assign} {stores} local.get 0))"#,
        arm(100),
        arm(200)
    );
    for condition in [0, 1] {
        let offset = if condition == 1 { 100 } else { 200 };
        let expected = (0..20).map(|lane| 31 + offset + lane).collect::<Vec<_>>();
        wide_memory_oracle("allocator_wide_if", &source, &[condition, 31], &expected);
    }
}

#[test]
fn wide_while_distinct_lanes() {
    let locals = "i32 ".repeat(20);
    let initialize = (0..20)
        .map(|lane| format!("local.get 1 i32.const {} i32.add local.set {} ", lane * 7, lane + 2))
        .collect::<String>();
    let update = (0..20)
        .map(|lane| {
            format!(
                "local.get {} i32.const {} i32.add local.set {} ",
                lane + 2,
                lane * 3 + 1,
                lane + 2
            )
        })
        .collect::<String>();
    let stores = (0..20)
        .map(|lane| format!("i32.const {} local.get {} i32.store ", 256 + lane * 4, lane + 2))
        .collect::<String>();
    let source = format!(
        r#"(module (memory 1)
      (func (export "entrypoint") (param i32 i32) (result i32) (local {locals})
        {initialize} block loop local.get 0 i32.eqz br_if 1 {update}
          local.get 0 i32.const 1 i32.sub local.set 0 br 0
        end end {stores} local.get 0))"#
    );
    for iterations in [0, 1, 3] {
        let expected = (0..20)
            .map(|lane| 31 + lane * 7 + iterations * (lane * 3 + 1))
            .collect::<Vec<_>>();
        wide_memory_oracle("allocator_wide_while", &source, &[iterations, 31], &expected);
    }
}

#[test]
fn wide_switch_distinct_lanes() {
    let results = "i32 ".repeat(20);
    let arm = |offset| {
        (0..20)
            .map(|lane| format!("local.get 1 i32.const {} i32.add ", offset + lane * 7))
            .collect::<String>()
    };
    let assign = (2..22).rev().map(|local| format!("local.set {local} ")).collect::<String>();
    let stores = (0..20)
        .map(|lane| format!("i32.const {} local.get {} i32.store ", 256 + lane * 4, lane + 2))
        .collect::<String>();
    let source = format!(
        r#"(module (memory 1)
      (func (export "entrypoint") (param i32 i32) (result i32) (local {results})
        block $merge (result {results})
          block $default block $two block $one block $zero
            local.get 0 br_table $zero $one $two $default
          end {} br $merge
          end {} br $merge
          end {} br $merge
          end {}
        end {assign} {stores} local.get 0))"#,
        arm(100),
        arm(200),
        arm(300),
        arm(400)
    );
    for (selector, offset) in [(0, 100), (1, 200), (2, 300), (77, 400)] {
        let expected = (0..20).map(|lane| 31 + offset + lane * 7).collect::<Vec<_>>();
        wide_memory_oracle("allocator_wide_switch", &source, &[selector, 31], &expected);
    }
}

#[test]
fn direct_call_widths_preserve_each_argument_and_result_lane() {
    for width in [0usize, 1, 4, 8, 12, 16] {
        eprintln!("direct-call ABI width {width}");
        let parameters = "i32 ".repeat(width);
        let returned = (0..width)
            .rev()
            .map(|lane| format!("local.get {lane} i32.const {} i32.add ", 100 + lane * 11))
            .collect::<String>();
        let arguments = (0..width).map(|lane| format!("local.get {lane} ")).collect::<String>();
        let assigned = (width..width * 2)
            .rev()
            .map(|local| format!("local.set {local} "))
            .collect::<String>();
        let stores = (0..width)
            .map(|lane| {
                format!("i32.const {} local.get {} i32.store ", 256 + lane * 4, width + lane)
            })
            .collect::<String>();
        let source = format!(
            r#"(module (memory 1)
          (func $callee (param {parameters}) (result {parameters}) {returned})
          (func (export "entrypoint") (param {parameters}) (result i32) (local {parameters})
            {arguments} call $callee {assigned} {stores} i32.const 1))"#
        );
        let inputs = (0..width as u32).map(|lane| 31 + lane * 3).collect::<Vec<_>>();
        let expected = (0..width as u32).rev().map(|lane| 131 + lane * 14).collect::<Vec<_>>();
        wide_memory_oracle(&format!("allocator_call_width_{width}"), &source, &inputs, &expected);
    }
}

#[test]
fn nonliteral_memory_values_survive_a_mutating_call() {
    let locals = "i32 ".repeat(20);
    let bytes = (0..20u32)
        .flat_map(|lane| (7 + lane * 11).to_le_bytes())
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let load = (0..20)
        .map(|lane| format!("i32.const {} i32.load local.set {lane} ", 64 + lane * 4))
        .collect::<String>();
    let mutate = (0..20)
        .map(|lane| format!("i32.const {} i32.const 0 i32.store ", 64 + lane * 4))
        .collect::<String>();
    let stores = (0..20)
        .map(|lane| format!("i32.const {} local.get {lane} i32.store ", 256 + lane * 4))
        .collect::<String>();
    // Publish both the captured values and the post-call memory contents. Every original
    // source address must be zero, while each captured lane must retain its distinct value.
    let observed = (0..20)
        .map(|lane| {
            format!("i32.const {} i32.const {} i32.load i32.store ", 336 + lane * 4, 64 + lane * 4)
        })
        .collect::<String>();
    let source = format!(
        r#"(module (memory 1) (data (i32.const 64) "{bytes}")
      (func $mutate {mutate})
      (func (export "entrypoint") (result i32) (local {locals})
        {load} call $mutate {stores} {observed} i32.const 1))"#
    );
    let expected = (0..20).map(|lane| 7 + lane * 11).chain([0; 20]).collect::<Vec<_>>();
    wide_memory_oracle("allocator_memory_live_call", &source, &[], &expected);
}

#[test]
fn production_debug_on_off_resolves_live_source_stack_memory() {
    let source = r#"
        (seed: u32) -> u32 {
            let mut frame_debug_local = [0u32; 20];
            for lane in 0..20 {
                frame_debug_local[lane] = seed.wrapping_add(lane as u32 * 7);
            }
            mutate_source_stack(&mut frame_debug_local);
            let mut fingerprint = 0u32;
            for value in frame_debug_local {
                fingerprint = fingerprint.rotate_left(3) ^ value;
            }
            fingerprint
        }

        #[inline(never)]
        fn mutate_source_stack(values: &mut [u32; 20]) {
            for lane in 0..20 {
                values[lane] = values[lane].wrapping_add(lane as u32 * 3 + 1);
            }
        }
    "#;
    let seed = 31u32;
    let expected_values = (0..20).map(|lane| Felt::from(seed + lane * 10 + 1)).collect::<Vec<_>>();
    let expected = expected_values.iter().fold(0u32, |fingerprint, value| {
        fingerprint.rotate_left(3) ^ value.as_canonical_u64() as u32
    });
    for debug in ["none", "full"] {
        let mut builder =
            crate::CompilerTestBuilder::rust_fn_body(source, ["--debug".into(), debug.into()]);
        builder.with_rustflags(
            ["-C", "debuginfo=2", "-C", "opt-level=0", "-C", "overflow-checks=off"]
                .map(std::borrow::Cow::Borrowed),
        );
        let mut test = builder.build();
        let package = test.compile_package();
        let args = [Felt::from(seed)];
        if debug == "none" {
            let output =
                eval_package::<u32, _, _>(package, None, &args, &test.session, |_| Ok(())).unwrap();
            assert_eq!(output, expected);
            continue;
        }
        let info = package.debug_info().unwrap().expect("full production debug metadata");
        assert!(info.nodes().iter().flat_map(|node| node.debug_vars.iter()).any(|var| {
            if info[var.name_idx].as_ref() != "frame_debug_local" || var.arg_idx.is_some() {
                return false;
            }
            eprintln!("frame_debug_local production metadata: {var:?}");
            true
        }));
        let mut executor = crate::testing::executor_with_std(args.to_vec());
        executor.with_advice_inputs(
            miden_processor::advice::AdviceInputs::default().with_stack(vec![Felt::ZERO].into()),
        );
        let mut vm = executor.into_debug(package, test.session.source_manager.clone());
        // The fixture declares this type; scheduled DWARF locals do not yet carry source types.
        let fixture_type = miden_assembly::ast::types::Type::Array(std::sync::Arc::new(
            miden_assembly::ast::types::ArrayType::new(miden_assembly::ast::types::Type::U32, 20),
        ));
        let mut observed_live_array = false;
        while !vm.stopped {
            vm.step().unwrap();
            if let Some(snapshot) = vm.debug_vars.get_variable("frame_debug_local") {
                if let Some(ty) = snapshot.info.ty() {
                    assert_eq!(ty, &fixture_type, "source type disagrees with fixture declaration");
                }
                if let Some(values) = vm.debug_vars.captured_values("frame_debug_local") {
                    observed_live_array |= values == expected_values;
                    continue;
                }
                let read_memory = |address| vm.read_memory_element(address);
                let read_local = |offset: i16| {
                    let fmp = read_memory(miden_core::FMP_ADDR.as_canonical_u64() as u32)?;
                    let fmp = u32::try_from(fmp.as_canonical_u64()).ok()?;
                    read_memory(fmp.checked_add_signed(i32::from(offset))?)
                };
                let values = miden_debug::resolve_typed_variable_values(
                    snapshot.info.value_location(),
                    &fixture_type,
                    20,
                    &vm.current_stack,
                    read_memory,
                    read_local,
                );
                observed_live_array |= values.as_ref() == Some(&expected_values);
            }
        }
        assert!(
            observed_live_array,
            "the named live source array must resolve all twenty values after the call"
        );
        assert_eq!(vm.into_execution_trace().parse_result::<u32>().unwrap(), expected);
    }
}

// Each path must preserve the declared tuple, independently of how control reaches function exit.
const RETURN_PATHS: [(&str, &str, &str); 5] = [
    ("implicit", "", ""),
    ("explicit", "", "return"),
    ("outer_branch", "", "br 0"),
    (
        "explicit_lower_prefix",
        "i64.const 0x1122334455667788 i32.const 3735928559",
        "return",
    ),
    ("unreachable_end", "", "return unreachable"),
];

#[test]
fn mixed_result_return_paths_preserve_each_ordered_lane() {
    for (path, lower_prefix, ending) in RETURN_PATHS {
        eprintln!("mixed-result return path {path}");
        let source = format!(
            r#"(module (memory 1)
          (func $callee (param i32) (result i32 i64 i32 i64)
            {lower_prefix}
            local.get 0 i32.const 3 i32.add
            local.get 0 i64.extend_i32_u i64.const 0x1234567800000000 i64.add
            local.get 0 i32.const 11 i32.mul
            local.get 0 i64.extend_i32_u i64.const 0x7123456700001000 i64.add
            {ending})
          (func (export "entrypoint") (param i32) (result i32) (local i32 i64 i32 i64)
            local.get 0 call $callee local.set 4 local.set 3 local.set 2 local.set 1
            i32.const 256 local.get 1 i32.store
            i32.const 264 local.get 2 i64.store
            i32.const 272 local.get 3 i32.store
            i32.const 280 local.get 4 i64.store
            i32.const 1))"#
        );
        // Check both halves of each i64 and each intervening i32 in source result order.
        wide_memory_oracle(
            &format!("allocator_mixed_return_{path}"),
            &source,
            &[31],
            &[34, 0, 31, 0x12345678, 341, 0, 0x101f, 0x71234567],
        );
    }
}

#[test]
fn zero_and_one_result_return_paths_preserve_arity_and_effects() {
    for width in [0, 1] {
        for (path, lower_prefix, ending) in RETURN_PATHS {
            eprintln!("return width {width}, path {path}");
            let (results, values, publish) = if width == 0 {
                ("", "", "i32.const 256 i32.const 512 i32.load i32.store")
            } else {
                (
                    "i32",
                    "local.get 0 i32.const 3 i32.mul i32.const 1 i32.add",
                    "local.set 1 i32.const 256 local.get 1 i32.store i32.const 260 i32.const 512 \
                     i32.load i32.store",
                )
            };
            let source = format!(
                r#"(module (memory 1)
              (func $callee (param i32) (result {results})
                i32.const 512 local.get 0 i32.const 17 i32.add i32.store
                {lower_prefix} {values} {ending})
              (func (export "entrypoint") (param i32) (result i32) (local i32)
                local.get 0 call $callee {publish} i32.const 1))"#
            );
            let expected: &[u32] = if width == 0 { &[48] } else { &[94, 48] };
            wide_memory_oracle(
                &format!("allocator_return_{width}_{path}"),
                &source,
                &[31],
                expected,
            );
        }
    }
}

#[test]
fn unreachable_function_endings_preserve_traps_for_all_result_arities() {
    for results in ["", "i32", "i32 i64 i32 i64"] {
        let drops = "drop ".repeat(results.split_whitespace().count());
        let source = format!(
            r#"(module
          (func $callee (result {results}) unreachable)
          (func (export "entrypoint") (result i32) call $callee {drops} i32.const 1))"#
        );
        let mut test = CompilerTest::from_wasm(
            "allocator_unreachable_return",
            wat::parse_str(&source).unwrap(),
            ["--entrypoint".into(), "allocator_unreachable_return::entrypoint".into()],
        );
        let package = test.compile_package();
        let executor = crate::testing::executor_with_std(vec![]);
        let mut vm = executor.into_debug(package, test.session.source_manager.clone());
        loop {
            match vm.step() {
                Err(error) => {
                    assert!(
                        crate::trap_helpers::trap_matches(
                            &error.to_string(),
                            "entered unreachable code"
                        ),
                        "result types {results:?}: {error}"
                    );
                    break;
                }
                Ok(_) => assert!(!vm.stopped, "the unreachable callee must trap: {results:?}"),
            }
        }
    }
}

#[test]
fn production_full_width_table_preserves_each_lane_and_live_argument() {
    let params = (0..16).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let args = (0..16).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(", ");
    let result_names = (0..16).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(", ");
    let callee = (0..16)
        .map(|i| {
            format!(
                "%k{i}_bits=arith.constant {} : u32; %k{i}=hir.bitcast %k{i}_bits <{{ ty = \
                 #builtin.type<felt> }}>; %r{i}=arith.add %a{i},%k{i} <{{ overflow = \
                 #builtin.overflow<wrapping> }}>;",
                100 + 11 * i
            )
        })
        .collect::<String>();
    let source = format!(
        r#"builtin.module public @production_table {{
        builtin.function private extern("C") @callee({params}) -> ({types}) {{
            {callee} builtin.ret {result_names} : ({types});
        }};
        builtin.function_table private @table : 2 {{ builtin.function_table_entry 1 @callee tag 7; }};
        builtin.function public extern("C") @entrypoint({params}) -> ({types}) {{
            %index=arith.constant 1 : u32;
            {result_names}=hir.exec_indirect @table[%index]({args}) : extern("C") ({types}) -> ({types}) tag 7;
            %delta_bits=arith.constant 111 : u32; %delta=hir.bitcast %delta_bits <{{ ty = #builtin.type<felt> }}>;
            %expected=arith.add %a1,%delta <{{ overflow = #builtin.overflow<wrapping> }}>;
            hir.assert_eq %r1,%expected <{{ code = #builtin.u32<0>, message = "live argument" }}> : (felt,felt);
            builtin.ret {result_names} : ({types});
        }};
    }};"#
    );
    let (package, context) = compile_hir_module("production-table", "entrypoint", &source, |_| {});
    let inputs = (0..16).map(|i| Felt::from(31u32 + 3 * i)).collect::<Vec<_>>();
    eval_package::<Felt, _, _>(package, None, &inputs, context.session(), |trace| {
        let expected = (0..16).map(|i| Felt::from(131u32 + 14 * i)).collect::<Vec<_>>();
        assert_eq!(trace.outputs().get_num_elements(16), expected);
        Ok(())
    })
    .unwrap();
}

#[test]
fn production_full_width_dyncall_preserves_each_lane_and_live_root_argument() {
    let params = (0..16).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let results = (0..16).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(", ");
    let callee = (0..16)
        .map(|i| {
            format!(
                "%k{i}_bits=arith.constant {} : u32; %k{i}=hir.bitcast %k{i}_bits <{{ ty = \
                 #builtin.type<felt> }}>; %r{i}=arith.add %a{i},%k{i} <{{ overflow = \
                 #builtin.overflow<wrapping> }}>;",
                100 + 11 * i
            )
        })
        .collect::<String>();
    let payload = (1..16)
        .map(|i| {
            format!(
                "%a{i}_bits=arith.constant {} : u32; %a{i}=hir.bitcast %a{i}_bits <{{ ty = \
                 #builtin.type<felt> }}>;",
                31 + 3 * i
            )
        })
        .collect::<String>();
    let args = std::iter::once("%root0".to_owned())
        .chain((1..16).map(|i| format!("%a{i}")))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"builtin.module public @production_dynamic {{
        builtin.function public extern("component-model") @callee({params}) -> ({types}) {{
            {callee} builtin.ret {results} : ({types});
        }};
        builtin.function public extern("C") @entrypoint(%root0: felt,%root1: felt,%root2: felt,%root3: felt) -> ({types}) {{
            %target0,%target1,%target2,%target3=hir.procedure_root @callee;
            hir.assert_eq %root0,%target0 <{{ code = #builtin.u32<0>, message = "target root lane 0" }}> : (felt,felt);
            hir.assert_eq %root1,%target1 <{{ code = #builtin.u32<0>, message = "target root lane 1" }}> : (felt,felt);
            hir.assert_eq %root2,%target2 <{{ code = #builtin.u32<0>, message = "target root lane 2" }}> : (felt,felt);
            hir.assert_eq %root3,%target3 <{{ code = #builtin.u32<0>, message = "target root lane 3" }}> : (felt,felt);
            {payload}
            {results}=hir.dyncall [%root0,%root1,%root2,%root3]({args}) : extern("component-model") ({types}) -> ({types});
            %delta_bits=arith.constant 100 : u32; %delta=hir.bitcast %delta_bits <{{ ty = #builtin.type<felt> }}>;
            %expected=arith.add %root0,%delta <{{ overflow = #builtin.overflow<wrapping> }}>;
            hir.assert_eq %r0,%expected <{{ code = #builtin.u32<0>, message = "live root argument" }}> : (felt,felt);
            builtin.ret {results} : ({types});
        }};
    }};"#
    );
    let (package, context) =
        compile_hir_module("production-dynamic", "entrypoint", &source, |_| {});
    // Executable manifests expose only the generated main. Resolve the actual dynamic target
    // from this package's validated procedure metadata and require its exact MAST root.
    let info = package.debug_info().unwrap().expect("production procedure metadata");
    let targets = info
        .functions()
        .iter()
        .filter(|function| {
            let name = function.linkage_name_idx.into_option().unwrap_or(function.name_idx);
            info[name].trim_start_matches("::") == "production_dynamic::callee"
        })
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 1, "one exact production_dynamic::callee target: {info:?}");
    let target = targets[0];
    let source = target.source_node.into_option().expect("callee source occurrence");
    let node = info.nodes()[source].exec_node;
    assert!(
        package
            .mast_forest()
            .is_procedure_root_with_exact_digest(node, target.mast_root),
        "callee metadata must identify its actual compiled procedure root"
    );
    let digest = target.mast_root;
    let inputs = digest.as_elements();
    eval_package::<Felt, _, _>(package, None, inputs, context.session(), |trace| {
        let expected = std::iter::once(inputs[0] + Felt::from(100u32))
            .chain((1..16).map(|i| Felt::from(131u32 + 14 * i)))
            .collect::<Vec<_>>();
        assert_eq!(trace.outputs().get_num_elements(16), expected);
        Ok(())
    })
    .unwrap();
}

#[test]
fn production_spilling_initializer_and_harness_preserve_original_root_window() {
    let fragment = |name: &str, first: u32| {
        let values = (0..20).map(|i| format!("%a{i}=hir.advice_pop;")).collect::<String>();
        // Keep every advice value live until the entire fragment has been read, then check
        // its position independently before the sum consumes the values.
        let positions = (0..20)
            .map(|i| {
                format!(
                    "%expected{i}_bits=arith.constant {} : u32; %expected{i}=hir.bitcast \
                     %expected{i}_bits <{{ ty = #builtin.type<felt> }}>; hir.assert_eq \
                     %a{i},%expected{i} <{{ code = #builtin.u32<0>, message = \"{name} advice \
                     lane {i}\" }}> : (felt,felt);",
                    first + i
                )
            })
            .collect::<String>();
        let mut sum =
            "%s1=arith.add %a0,%a1 <{ overflow = #builtin.overflow<wrapping> }>;".to_owned();
        for i in 2..20 {
            sum.push_str(&format!(
                "%s{i}=arith.add %s{},%a{i} <{{ overflow = #builtin.overflow<wrapping> }}>;",
                i - 1
            ));
        }
        format!(
            "builtin.global_variable private @{name}: felt {{ {values} {positions} {sum} \
             builtin.ret %s19 : (felt); }};"
        )
    };
    let params = (0..16).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let args = (0..16).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(", ");
    let check = |name: &str, expression: &str, value: u32| {
        format!(
            "%{name}={expression}; %expected_{name}_bits=arith.constant {value} : u32; \
             %expected_{name}=hir.bitcast %expected_{name}_bits <{{ ty = #builtin.type<felt> }}>; \
             hir.assert_eq %{name},%expected_{name} <{{ code = #builtin.u32<0>, message = \
             \"{name}\" }}> : (felt,felt);"
        )
    };
    let mut checks = check("sentinel", "hir.advice_pop", 424242);
    for (name, value) in [("first", 210), ("second", 610)] {
        checks.push_str(&format!(
            "%raw_{name}=builtin.global_symbol @{name} : ptr<u8, byte>; %ptr_{name}=hir.bitcast \
             %raw_{name} <{{ ty = #builtin.type<ptr<felt, byte>> }}>;"
        ));
        checks.push_str(&check(name, &format!("hir.load %ptr_{name}"), value));
    }
    for i in 0..4 {
        checks.push_str(&format!(
            "%address{i}=arith.constant {} : u32; %ptr{i}=hir.int_to_ptr %address{i} <{{ ty = \
             #builtin.type<ptr<felt, element>> }}>;",
            100 + i
        ));
        checks.push_str(&check(&format!("harness{i}"), &format!("hir.load %ptr{i}"), 555 + i));
    }
    let source = format!(
        r#"builtin.module public @production_initializer {{ {} {}
        builtin.function public extern("C") @entrypoint({params}) -> ({types}) {{
            {checks} builtin.ret {args} : ({types});
        }};
    }};"#,
        fragment("first", 1),
        fragment("second", 21)
    );
    let (masm, package, context) =
        compile_hir_module_with_masm("production-initializer", "entrypoint", &source, |_| {});
    // Inspect the assembler input from the same production compilation as the executed package.
    let lines = masm.lines().collect::<Vec<_>>();
    let initializers = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("pub proc init("))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(initializers.len(), 1, "one combined production initializer");
    let start = initializers[0];
    let size = lines[..start]
        .iter()
        .rev()
        .take_while(|line| !line.starts_with("namespace ") && **line != "end")
        .find_map(|line| line.trim().strip_prefix("@locals(").and_then(|s| s.strip_suffix(')')))
        .expect("actual initializer must declare a spill frame")
        .parse::<u32>()
        .unwrap();
    assert!(size > 0 && size.is_multiple_of(4), "actual initializer spill frame: {size}");
    let end = lines[start..]
        .iter()
        .position(|line| *line == "end")
        .expect("initializer procedure terminator")
        + start;
    let body = lines[start..end].join("\n");
    for opcode in ["loc_store", "loc_load"] {
        let accesses = body
            .split_whitespace()
            .filter_map(|token| token.strip_prefix(opcode))
            .map(|suffix| {
                let (kind, offset) = suffix.split_once('.').expect("literal local access");
                let width = match kind {
                    "" => 1,
                    "w" | "w_le" | "w_be" => 4,
                    _ => panic!("unexpected local access opcode: {opcode}{suffix}"),
                };
                (offset, width)
            })
            .map(|(offset, width)| {
                (offset.parse::<u32>().expect("literal final local offset"), width)
            })
            .collect::<Vec<_>>();
        assert!(!accesses.is_empty(), "initializer must contain actual {opcode} spill accesses");
        assert!(
            accesses
                .iter()
                .all(|(offset, width)| offset.checked_add(*width).is_some_and(|end| end <= size)),
            "initializer spill access outside final frame"
        );
    }
    let advice_lanes = body
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("adv_push"))
        .map(|suffix| suffix.strip_prefix('.').map_or(1, |count| count.parse::<usize>().unwrap()))
        .sum::<usize>();
    assert_eq!(advice_lanes, 40, "both twenty-value fragments must survive production lowering");
    let inputs = (0..16).map(|i| Felt::from(901u32 + 17 * i)).collect::<Vec<_>>();
    // GeneratedRoot executes component initialization before harness setup, then entry.
    let advice = (1u32..=40)
        .chain([1, 100, 1, 555, 556, 557, 558, 424242])
        .map(Felt::from)
        .collect::<Vec<_>>();
    let mut executor = crate::testing::executor_with_std(inputs.clone());
    executor.with_advice_inputs(
        miden_processor::advice::AdviceInputs::default().with_stack(advice.into()),
    );
    let mut vm = executor.into_debug(package, context.session().source_manager.clone());
    while !vm.stopped {
        vm.step().unwrap();
    }
    assert_eq!(vm.current_stack.len(), 16, "physical final root stack depth");
    assert_eq!(
        vm.current_stack, inputs,
        "every original runtime root lane survives both stages"
    );
}

#[test]
fn production_terminal_switch_preserves_selected_traps_and_untaken_paths() {
    for selector in [0, 1] {
        for trap_in_case in [false, true] {
            let case = if trap_in_case {
                "ub.unreachable;"
            } else {
                "scf.yield : ();"
            };
            let default = if trap_in_case {
                "scf.yield : ();"
            } else {
                "ub.unreachable;"
            };
            let source = format!(
                r#"builtin.module public @terminal_switch {{
                builtin.function public extern("C") @entrypoint(%value: u32) -> u32 {{
                    %selector = arith.constant {selector} : u32;
                    scf.index_switch %selector case 1 {{ {case} }} default {{ {default} }} : ();
                    builtin.ret %value : (u32);
                }};
            }};"#
            );
            let (package, context) =
                compile_hir_module("terminal-switch", "entrypoint", &source, |_| {});
            let selected_trap = (selector == 1) == trap_in_case;
            if selected_trap {
                let mut executor = crate::testing::executor_with_std(vec![Felt::from(37u32)]);
                executor.with_advice_inputs(
                    miden_processor::advice::AdviceInputs::default()
                        .with_stack(vec![Felt::ZERO].into()),
                );
                let mut vm = executor.into_debug(package, context.session().source_manager.clone());
                loop {
                    match vm.step() {
                        Err(error) => {
                            assert!(
                                crate::trap_helpers::trap_matches(
                                    &error.to_string(),
                                    "entered unreachable code"
                                ),
                                "{error}"
                            );
                            break;
                        }
                        Ok(_) => assert!(!vm.stopped, "selected terminal branch returned normally"),
                    }
                }
            } else {
                let actual = eval_package::<u32, _, _>(
                    package,
                    None,
                    &[Felt::from(37u32)],
                    context.session(),
                    |_| Ok(()),
                )
                .unwrap();
                assert_eq!(actual, 37);
            }
        }
    }
}

#[test]
fn production_terminal_if_preserves_taken_and_untaken_traps() {
    for trap_in_then in [false, true] {
        let then_body = if trap_in_then {
            "ub.unreachable;"
        } else {
            "scf.yield %value : (u32);"
        };
        let else_body = if trap_in_then {
            "scf.yield %value : (u32);"
        } else {
            "ub.unreachable;"
        };
        let source = format!(
            r#"builtin.module public @terminal_if {{
            builtin.function public extern("C") @entrypoint(%condition: i1, %value: u32) -> u32 {{
                %result = scf.if %condition then {{ {then_body} }} else {{ {else_body} }} : (i1) -> u32;
                builtin.ret %result : (u32);
            }};
        }};"#
        );
        let (package, context) = compile_hir_module("terminal-if", "entrypoint", &source, |_| {});
        for condition in [false, true] {
            let args = [Felt::from(u32::from(condition)), Felt::from(37u32)];
            if condition == trap_in_then {
                let mut executor = crate::testing::executor_with_std(args.to_vec());
                executor.with_advice_inputs(
                    miden_processor::advice::AdviceInputs::default()
                        .with_stack(vec![Felt::ZERO].into()),
                );
                let mut vm =
                    executor.into_debug(package.clone(), context.session().source_manager.clone());
                loop {
                    match vm.step() {
                        Err(error) => {
                            assert!(
                                crate::trap_helpers::trap_matches(
                                    &error.to_string(),
                                    "entered unreachable code"
                                ),
                                "{error}"
                            );
                            break;
                        }
                        Ok(_) => assert!(!vm.stopped, "taken terminal If arm returned normally"),
                    }
                }
            } else {
                let actual = eval_package::<u32, _, _>(
                    package.clone(),
                    None,
                    &args,
                    context.session(),
                    |_| Ok(()),
                )
                .unwrap();
                assert_eq!(actual, 37);
            }
        }
    }
}

// The current compile_hir_module helper deliberately unwraps backend diagnostics.
// This checks the existing diagnostic at that boundary without changing the helper API.
#[test]
#[should_panic(expected = "logical return would fall through into a normal continuation")]
fn production_mixed_return_yield_keeps_existing_diagnostic() {
    let source = r#"builtin.module public @terminal_return {
        builtin.function public extern("C") @entrypoint(%index: u32, %value: u32) -> u32 {
            %unused = scf.index_switch %index
                case 1 { scf.yield %value : (u32); }
                default { builtin.ret %value : (u32); } : u32;
            builtin.ret %value : (u32);
        };
    };"#;
    let _ = compile_hir_module("terminal-return", "entrypoint", source, |_| {});
}

/// Uses the same shim/fixup shape as a standard component linker so canon lower can name the
/// caller's memory after instantiation. All imports still pass through the frontend Validator.
#[allow(clippy::too_many_arguments)]
fn canonical_component_source(
    name: &str,
    declarations: &str,
    parameters: &str,
    result: &str,
    core_parameters: &str,
    core_results: &str,
    entry_parameters: &str,
    entry_body: &str,
) -> String {
    let signature = format!("(param {core_parameters}) {core_results}");
    let core_entry_parameters = if entry_parameters.is_empty() {
        String::new()
    } else {
        format!("(param {entry_parameters})")
    };
    let component_entry_parameters = entry_parameters
        .split_whitespace()
        .enumerate()
        .map(|(index, ty)| {
            format!(
                "(param \"p{index}\" {})",
                match ty {
                    "f32" => "float32",
                    "i32" => "u32",
                    _ => panic!("unsupported fixture entry type"),
                }
            )
        })
        .collect::<String>();
    let external_name = name.replace('-', "_");
    let forwards = (0..core_parameters.split_whitespace().count())
        .map(|index| format!("local.get {index} "))
        .collect::<String>();
    format!(
        r#"(component
          (import "miden:canonical/protocol@1.0.0" (instance $protocol
            {declarations}
            (export "{name}" (external-id "miden::canonical::protocol::{external_name}")
              (func {parameters} {result}))))
          (alias export $protocol "{name}" (func $import))
          (core module $shim
            (type $call (func {signature}))
            (table (export "$imports") 1 1 funcref)
            (func (export "0") (type $call)
              {forwards} i32.const 0 call_indirect (type $call)))
          (core instance $shim-instance (instantiate $shim))
          (alias core export $shim-instance "0" (core func $shim-call))
          (core instance $args (export "{name}" (func $shim-call)))
          (core module $main
            (import "miden:canonical/protocol@1.0.0" "{name}" (func $call {signature}))
            (memory (export "memory") 1)
            (func (export "entrypoint") {core_entry_parameters} (result i32)
              {entry_body}))
          (core instance $main-instance (instantiate $main
            (with "miden:canonical/protocol@1.0.0" (instance $args))))
          (alias core export $main-instance "memory" (core memory $memory))
          (core func $lower (canon lower (func $import) (memory $memory)))
          (core module $fixup
            (type $call (func {signature}))
            (import "" "0" (func (type $call)))
            (import "" "$imports" (table 1 1 funcref))
            (elem (table 0) (i32.const 0) func 0))
          (alias core export $shim-instance "$imports" (core table $imports))
          (core instance $fixup-args
            (export "0" (func $lower)) (export "$imports" (table $imports)))
          (core instance (instantiate $fixup (with "" (instance $fixup-args))))
          (func $entrypoint-lifted {component_entry_parameters} (result u32)
            (canon lift (core func $main-instance "entrypoint")))
          (component $exports
            (import "entrypoint" (func $entry {component_entry_parameters} (result u32)))
            (export "entrypoint" (external-id "miden::canonical::fixture::entrypoint") (func $entry)))
          (instance $exports-instance (instantiate $exports
            (with "entrypoint" (func $entrypoint-lifted))))
          (export "miden:canonical/fixture@1.0.0" (instance $exports-instance))
        )"#
    )
}

fn compile_canonical_component(
    source: &str,
    helper: Option<String>,
) -> std::sync::Arc<miden_mast_package::Package> {
    let wasm = wat::parse_str(source).expect("canonical fixture must parse as component WAT");
    assert_eq!(&wasm[..8], b"\0asm\x0d\0\x01\0", "fixture must be a component binary");
    let mut builder = crate::CompilerTestBuilder::from_wasm("canonical_component", wasm, []);
    if let Some(helper) = helper {
        builder.link_with_masm_module("miden::protocol::tx", helper);
    }
    builder.build().compile_package()
}

fn canonical_executable(
    package: &miden_mast_package::Package,
) -> std::sync::Arc<miden_mast_package::Package> {
    let entrypoint = package
        .manifest
        .exports()
        .filter_map(|e| e.as_procedure())
        .find(|e| e.path.as_ref().as_str().ends_with("::entrypoint"))
        .unwrap();
    std::sync::Arc::new(
        package
            .make_executable(&miden_mast_package::QualifiedProcedureName::from(
                entrypoint.path.clone(),
            ))
            .unwrap(),
    )
}

/// Check every physical payload lane, then replace it without growing the sixteen-lane floor.
fn canonical_target_payload(expected: &[u32]) -> String {
    assert_eq!(expected.len(), 16);
    let checks = expected
        .iter()
        .enumerate()
        .map(|(index, value)| format!("dup.{index} push.{value} assert_eq\n"))
        .collect::<String>();
    let results = expected
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let change = format!("push.{} add", 777 + index as u32 - value);
            match index {
                0 => change + "\n",
                1 => format!("swap.1 {change} swap.1\n"),
                _ => format!("movup.{index} {change} movdn.{index}\n"),
            }
        })
        .collect::<String>();
    checks + &results
}

fn canonical_fpi_fixture(tuple: bool, output_pointer: bool) -> (String, String) {
    let payloads = if tuple {
        vec!["float32"; 11]
    } else {
        vec!["u64"; 6].into_iter().chain(["float32"]).collect()
    };
    let parameters = vec!["float32"; 6]
        .into_iter()
        .chain(payloads)
        .enumerate()
        .map(|(i, ty)| format!("(param \"a{i}\" {ty})"))
        .collect::<String>();
    let core_parameters = if tuple {
        "i32".to_owned()
    } else {
        vec!["f32"; 6]
            .into_iter()
            .chain(vec!["i64"; 6])
            .chain(["f32"])
            .collect::<Vec<_>>()
            .join(" ")
    } + if output_pointer { " i32" } else { "" };
    let mut values = (0..6)
        .map(|i| (4 * i, format!("i32.const {} f32.reinterpret_i32", 11 + i)))
        .collect::<Vec<_>>();
    if tuple {
        values.extend(
            (0..11).map(|i| (24 + 4 * i, format!("i32.const {} f32.reinterpret_i32", 31 + i))),
        );
    } else {
        values.extend((0..6).map(|i| {
            (
                24 + 8 * i,
                format!("i64.const {}", ((2 * i + 1) as u64) << 32 | (2 * i + 2) as u64),
            )
        }));
        values.push((72, "i32.const 91 f32.reinterpret_i32".into()));
    }
    let args = if tuple {
        values
            .iter()
            .map(|(offset, value)| format!("i32.const {} {value} f32.store ", 256 + offset))
            .collect::<String>()
            + "i32.const 256 "
    } else {
        values.iter().map(|(_, v)| format!("{v} ")).collect::<String>()
    };
    let body = format!(
        "i32.const 700 i32.const 313 i32.store i32.const 700 i32.load {args} {} call $call {} \
         i32.add",
        if output_pointer { "i32.const 512" } else { "" },
        if output_pointer {
            "i32.const 512 f32.load i32.reinterpret_f32 i32.const 777 i32.ne if unreachable end \
             i32.const 516 f32.load i32.reinterpret_f32 i32.const 778 i32.ne if unreachable end \
             i32.const 1"
        } else {
            "i32.reinterpret_f32 i32.const 777 i32.ne if unreachable end i32.const 1"
        }
    );
    let source = canonical_component_source(
        "fpi-check",
        "(type $record (record (field \"first\" float32) (field \"second\" float32))) (export \
         \"record\" (type $public-record (eq $record)))",
        &parameters,
        if output_pointer {
            "(result $public-record)"
        } else {
            "(result float32)"
        },
        &core_parameters,
        if output_pointer { "" } else { "(result f32)" },
        "",
        &body,
    );
    let mut expected = vec![12, 11, 13, 14, 15, 16];
    if tuple {
        expected.extend(31..42);
    } else {
        expected.extend([2, 1, 4, 3, 6, 5, 8, 7, 10, 9, 12, 11, 91]);
    }
    expected.resize(22, 0);
    let prefix_checks = expected[..6]
        .iter()
        .map(|v| format!("push.{v} assert_eq\n"))
        .collect::<String>();
    let helper = format!(
        "@locals(1) pub proc execute_foreign_procedure\nsdepth \
         loc_store.0\n{prefix_checks}{}\nsdepth loc_load.0 push.6 sub assert_eq.err=\"FPI helper \
         return depth\"\nend\n",
        canonical_target_payload(&expected[6..])
    );
    (source, helper)
}

fn execute_canonical_fpi(tuple: bool, output_pointer: bool) {
    use miden_processor::{FastProcessor, StackInputs};
    let (source, helper) = canonical_fpi_fixture(tuple, output_pointer);
    let library = compile_canonical_component(&source, Some(helper));
    let package = canonical_executable(&library);
    let result = FastProcessor::new(StackInputs::default())
        .execute_sync(
            &package.unwrap_program(),
            &mut crate::end_to_end::support::default_host_with_core_package(),
        )
        .unwrap();
    assert_eq!(result.stack.get_num_elements(1), &[Felt::from(314u32)]);
}

#[test]
fn canonical_component_fpi_thirteen_flat_nineteen_felt_executes() {
    execute_canonical_fpi(false, false);
}

#[test]
fn canonical_component_fpi_thirteen_flat_with_result_pointer_executes() {
    execute_canonical_fpi(false, true);
}

#[test]
fn canonical_component_fpi_genuine_tuple_executes() {
    execute_canonical_fpi(true, true);
}

fn canonical_dyncall_fixture(output_pointer: bool, typed_variant: Option<u8>) -> (String, String) {
    let root = r#"(type $root (record (field "a" float32) (field "b" float32) (field "c" float32) (field "d" float32)))
        (export "root" (type $public-root (eq $root)))"#;
    let result_record = format!(
        "(type $result (record {})) (export \"result\" (type $public-result (eq $result)))",
        (0..16).map(|i| format!("(field \"r{i}\" float32)")).collect::<String>()
    );
    let mut declarations = format!("{root} {result_record}");
    let mut parameters = "(param \"root\" $public-root)".to_owned();
    let mut stores = (0..4)
        .map(|i| format!("i32.const {} local.get {i} f32.store ", 256 + 4 * i))
        .collect::<String>();
    let mut expected = vec![];
    if let Some(tag) = typed_variant {
        declarations.push_str(
            r#"
            (type $nested (record (field "small" u8) (field "wide" u64) (field "felt" float32)))
            (export "nested" (type $public-nested (eq $nested)))
            (type $variant (variant (case "empty") (case "wide" u64)))
            (export "variant" (type $public-variant (eq $variant)))"#,
        );
        parameters
            .push_str("(param \"nested\" $public-nested) (param \"variant\" $public-variant)");
        parameters
            .push_str(&(0..9).map(|i| format!("(param \"p{i}\" float32)")).collect::<String>());
        stores.push_str(&format!(
            "i32.const 272 i32.const 7 i32.store8 i32.const 280 i64.const {} i64.store i32.const \
             288 i32.const 91 f32.reinterpret_i32 f32.store i32.const 296 i32.const {tag} \
             i32.store8 i32.const 304 i64.const {} i64.store ",
            (0x34u64 << 32) | 0x12,
            (0x78u64 << 32) | 0x56
        ));
        expected.extend([7, 0x12, 0x34, 91, tag as u32]);
        expected.extend(if tag == 0 { [0, 0] } else { [0x56, 0x78] });
        for i in 0..9 {
            stores.push_str(&format!(
                "i32.const {} i32.const {} f32.reinterpret_i32 f32.store ",
                312 + 4 * i,
                31 + i
            ));
            expected.push(31 + i);
        }
    } else {
        parameters
            .push_str(&(0..16).map(|i| format!("(param \"p{i}\" float32)")).collect::<String>());
        for i in 0..16 {
            stores.push_str(&format!(
                "i32.const {} i32.const {} f32.reinterpret_i32 f32.store ",
                272 + 4 * i,
                31 + 3 * i
            ));
            expected.push(31 + 3 * i);
        }
    }
    assert_eq!(expected.len(), 16);
    let result_check = if output_pointer {
        (0..16)
            .map(|i| {
                format!(
                    "i32.const {} f32.load i32.reinterpret_f32 i32.const {} i32.ne if unreachable \
                     end ",
                    512 + 4 * i,
                    777 + i
                )
            })
            .collect::<String>()
            + "i32.const 1"
    } else {
        "i32.reinterpret_f32 i32.const 777 i32.ne if unreachable end i32.const 1".to_owned()
    };
    let body = format!(
        "i32.const 700 i32.const 313 i32.store {stores} i32.const 700 i32.load i32.const 256 {} \
         call $call {result_check} i32.add",
        if output_pointer { "i32.const 512" } else { "" }
    );
    let source = canonical_component_source(
        "dyncall-check",
        &declarations,
        &parameters,
        if output_pointer {
            "(result $public-result)"
        } else {
            "(result float32)"
        },
        if output_pointer { "i32 i32" } else { "i32" },
        if output_pointer { "" } else { "(result f32)" },
        "f32 f32 f32 f32",
        &body,
    );
    let helper = if typed_variant == Some(2) {
        "pub proc callee push.0 assert.err=\"canonical target must not run\" end".to_owned()
    } else {
        format!(
            "pub proc callee\nsdepth push.16 assert_eq.err=\"dyncall target entry \
             depth\"\n{}\nsdepth push.16 assert_eq.err=\"dyncall target return depth\"\nend\n",
            canonical_target_payload(&expected)
        )
    };
    (source, helper)
}

fn execute_canonical_dyncall(output_pointer: bool, typed_variant: Option<u8>) {
    use miden_processor::{FastProcessor, StackInputs};
    let (source, helper) = canonical_dyncall_fixture(output_pointer, typed_variant);
    let manager = std::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
    let module =
        miden_assembly::ast::Module::parser(Some(miden_assembly::ast::ModuleKind::Library))
            .parse_str(
                Some(miden_assembly::Path::new("::miden::canonical::target")),
                &helper,
                manager.clone(),
            )
            .unwrap();
    let target: std::sync::Arc<miden_mast_package::Package> =
        miden_assembly::Assembler::new(manager)
            .assemble_library("canonical_dynamic_target", module, core::iter::empty::<&str>())
            .unwrap()
            .into();
    let export = target
        .manifest
        .exports()
        .filter_map(|e| e.as_procedure())
        .find(|e| {
            e.path.as_ref().as_str().trim_start_matches("::") == "miden::canonical::target::callee"
        })
        .unwrap();
    let digest = export.digest;
    assert!(
        digest.as_elements().iter().all(|f| f.as_canonical_u64() > u32::MAX as u64),
        "actual root must cover all four full field lanes"
    );
    assert!(
        target
            .mast_forest()
            .is_procedure_root_with_exact_digest(export.node.unwrap(), digest)
    );
    let wasm = wat::parse_str(&source).unwrap();
    assert_eq!(&wasm[..8], b"\0asm\x0d\0\x01\0");
    let mut builder = crate::CompilerTestBuilder::from_wasm("canonical_component", wasm, []);
    builder.link_with_masm_module("miden::canonical::target", helper);
    let library = builder.build().compile_package();
    let package = canonical_executable(&library);
    let mut host = crate::end_to_end::support::default_host_with_core_package()
        .with_library(target)
        .unwrap();
    let result = FastProcessor::new(StackInputs::new(digest.as_elements()).unwrap())
        .execute_sync(&package.unwrap_program(), &mut host);
    if typed_variant == Some(2) {
        let error = result
            .expect_err("invalid variant must trap before target execution")
            .to_string();
        assert!(crate::trap_helpers::trap_matches(&error, "entered unreachable code"), "{error}");
        assert!(
            !crate::trap_helpers::trap_matches(&error, "canonical target must not run"),
            "{error}"
        );
    } else {
        assert_eq!(result.unwrap().stack.get_num_elements(1), &[Felt::from(314u32)]);
    }
}

#[test]
fn canonical_component_dyncall_full_tuple_executes() {
    execute_canonical_dyncall(false, None);
}

#[test]
fn canonical_component_dyncall_full_tuple_result_pointer_executes() {
    execute_canonical_dyncall(true, None);
}

#[test]
fn canonical_component_dyncall_tuple_typed_layout_and_variant_padding_execute() {
    execute_canonical_dyncall(false, Some(0));
    execute_canonical_dyncall(true, Some(1));
}

#[test]
fn canonical_component_dyncall_tuple_variant_traps() {
    execute_canonical_dyncall(false, Some(2));
}
