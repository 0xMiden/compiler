use super::*;

#[test]
fn context_and_kernel_exec_returns_have_registered_cleanup() {
    for (owner, convention) in [("ordinary", "component-model"), ("$kernel", "C")] {
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @"{owner}" {{
                builtin.module public @nested {{
                    builtin.function public extern("C") @callee(%x: felt) -> felt {{ builtin.ret %x : (felt); }};
                }};
                builtin.function public extern("{convention}") @dispatch(%x: felt) -> felt {{
                    %r=hir.exec @nested::@callee(%x) : extern("C") (felt) -> felt;
                    builtin.ret %r : (felt);
                }};
            }};"#
        ));
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        let procedure = verify::verify(&plan, &frame, body).unwrap();
        let source = procedure
            .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
            .unwrap()
            .to_pretty_string();
        assert!(source.contains("truncate_stack"), "{source}");
        let source = source
            .replace("::ordinary::nested::callee", "callee")
            .replace("::\"$kernel\"::nested::callee", "callee")
            .replace("::$kernel::nested::callee", "callee");
        execute_context_cleanup(&source, &[1]);
    }
}

#[test]
fn generated_root_rejects_context_entrypoints_before_allocation() {
    use crate::{plan::*, select::generated::GeneratedRoot};
    let source = GeneratedRoot {
        initializer: None,
        harness: None,
        entry: crate::lower::raw::target("::entry"),
        entry_abi: ProcedureAbi::new(CallingConvention::Context, 0, 0).unwrap(),
    };
    assert!(crate::select::select_generated(&source, &[]).is_err());
}

#[test]
fn context_return_cleanup_covers_empty_full_and_nested_results() {
    for results in 0..=16 {
        let args = (0..16).map(|i| alloc::format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
        let returned = (0..results).map(|i| alloc::format!("%a{i}")).collect::<Vec<_>>().join(", ");
        let types = vec!["felt"; results].join(", ");
        let result_signature = if results == 0 {
            alloc::string::String::new()
        } else {
            alloc::format!(" -> ({types})")
        };
        let ret = if results == 0 {
            "builtin.ret;".into()
        } else {
            alloc::format!("builtin.ret {returned} : ({types});")
        };
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @context {{
            builtin.module public @nested {{
                builtin.function public extern("C") @callee(%x: felt) -> felt {{ builtin.ret %x : (felt); }};
            }};
            builtin.function public extern("component-model") @dispatch({args}){result_signature} {{
                %r=hir.exec @nested::@callee(%a15) : extern("C") (felt) -> felt;
                {ret}
            }};
        }};"#
        ));
        let env = SelectEnv::default();
        let (plan, frame, body) = ready(&f, &env);
        let procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
            .unwrap()
            .to_pretty_string();
        execute_context_cleanup(
            &procedure.replace("::context::nested::callee", "callee"),
            &(1..=results).collect::<Vec<_>>(),
        );
        let function = f.borrow();
        let selected = crate::select::evidence(&function, &env).unwrap().source.program;
        let cleanup = selected
            .segments
            .iter()
            .find(|s| s.units.len() == 1 && plan.selected.return_cleanup(s.id))
            .unwrap();
        let mut moved = selected.clone();
        let cleanup_unit =
            moved.segments.iter_mut().find(|s| s.id == cleanup.id).unwrap().units.remove(0);
        moved.segments[0].units.insert(0, cleanup_unit);
        assert!(
            check_selection(
                moved,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err()
        );
        let mut omitted = selected.clone();
        omitted.segments.iter_mut().find(|s| s.id == cleanup.id).unwrap().units.clear();
        assert!(
            check_selection(
                omitted,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err()
        );
        if results > 0 {
            let mut rebound = selected.clone();
            rebound.edges.iter_mut().find(|e| e.kind == EdgeKind::Return).unwrap().bindings[0]
                .source = rebound.segments[0].parameters[15];
            assert!(
                check_selection(
                    rebound,
                    &SourceContext::Hir {
                        function: &function,
                        env: &env
                    }
                )
                .is_err()
            );
        }
    }
}

/// Execute a context window with the exact-depth assertion before returning to its caller.
fn execute_context_cleanup(procedure: &str, expected: &[usize]) {
    let core = midenc_session::LinkLibrary::core()
        .load(&crate::plan::tests::lowering_units::session().options)
        .unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    let code = alloc::format!(
        "proc callee nop end {procedure} proc probe exec.dispatch sdepth push.16 assert_eq {} end \
         begin {} call.probe exec.::miden::core::sys::truncate_stack end",
        crate::plan::tests::lowering_units::assert_top(expected),
        crate::plan::tests::lowering_units::push_top(&(1..=16).collect::<Vec<_>>())
    );
    let program = assembler
        .assemble_program("context_cleanup", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host().with_library(core).unwrap();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .unwrap();
}

#[test]
fn generated_immediate_stores_reuse_typed_memory_construction() {
    use midenc_hir::{Context, SourceSpan, Type};
    let context = Rc::new(Context::default());
    for (ty, address) in [
        (Type::U8, 65),
        (Type::U16, 66),
        (Type::U32, 68),
        (Type::U64, 72),
        (Type::U128, 80),
        (Type::Felt, 96),
    ] {
        let memory = crate::lower::units::memory::Memory::store_immediate(
            context.clone(),
            ty,
            address,
            SourceSpan::UNKNOWN,
        )
        .unwrap();
        assert!(memory.effects.memory.write);
    }
}

fn ready_initializer(
    source: &crate::select::generated::GeneratedInitializer<'_, '_>,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let evidence = source.evidence().unwrap();
    let selected =
        check_selection(evidence.source.program, &SourceContext::Generated(source)).unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &evidence.source.source_locals).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut registry = LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition).unwrap();
    }
    let mut env = ExpansionEnv::default();
    env.resolve_debug(&plan, &frame);
    let mut body = expand_procedure(&plan, &frame, &registry, &env).unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    (plan, frame, body)
}

#[test]
fn combined_real_global_initializers_spill_reuse_cells_and_execute_in_source_order() {
    use crate::select::generated::{GeneratedInitializer, Initialization};
    let fragment = |name: &str| {
        let values = (0..20)
            .map(|i| alloc::format!("%a{i}=hir.advice_pop;"))
            .collect::<alloc::string::String>();
        let mut sum =
            "%s1=arith.add %a0,%a1 <{ overflow = #builtin.overflow<wrapping> }>;".to_string();
        for i in 2..20 {
            sum.push_str(&alloc::format!(
                "%s{i}=arith.add %s{},%a{i} <{{ overflow = #builtin.overflow<wrapping> }}>;",
                i - 1
            ));
        }
        alloc::format!(
            "builtin.global_variable private @{name}: felt {{ {values} {sum} builtin.ret %s19 : \
             (felt); }};"
        )
    };
    let f = parse_module_function(&alloc::format!(
        "builtin.module public @globals {{ {} {} builtin.function public extern(\"C\") @f() {{ \
         builtin.ret; }}; }};",
        fragment("first"),
        fragment("second")
    ));
    let owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let globals = owner
        .borrow()
        .region(0)
        .entry()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<builtin::GlobalVariable>()
                .map(|global| global.as_global_var_ref())
        })
        .collect::<Vec<_>>();
    {
        use midenc_hir::{
            Builder, OpBuilder, SourceSpan,
            dialects::debuginfo::{DIBuilder, attributes::Variable},
        };
        let block = globals[0].borrow().initializer().entry().as_block_ref();
        let operations =
            block.borrow().body().iter().map(|op| op.as_operation_ref()).collect::<Vec<_>>();
        let value = operations[19].borrow().results().all()[0].borrow().as_value_ref();
        let mut builder = OpBuilder::new(f.context.clone());
        builder.set_insertion_point_before(operations[21]);
        builder
            .debug_value(
                value,
                Variable {
                    name: "initializer_spill".into(),
                    ..Default::default()
                },
                SourceSpan::UNKNOWN,
            )
            .unwrap();
    }
    let link = linked(&f);
    let env = SelectEnv {
        link_info: Some(&link),
        ..Default::default()
    };
    let source = GeneratedInitializer {
        stages: globals.iter().copied().map(Initialization::Global).collect(),
        env: &env,
    };
    let (plan, frame, body) = ready_initializer(&source);
    assert!(frame.aligned_size() > 0);
    assert!(
        plan.spill_slots.len() > frame.resources().scalar_colors as usize,
        "disjoint fragments should share final cells"
    );
    let record = plan
        .debug_locations
        .values()
        .find(|record| !record.locations.is_empty())
        .unwrap();
    let crate::lower::debug::Location::Cell(slot) = record.locations[0] else {
        panic!("initializer must observe an actual spill");
    };
    let offset = frame.resolve(FrameRef::Spill(slot)).unwrap();
    let mut expanded = ExpansionEnv::default();
    expanded.resolve_debug(&plan, &frame);
    let variable = expanded
        .debug
        .values()
        .find_map(|decorator| match decorator {
            DebugDecorator::Variable(info) => Some(info),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        *variable.value_location(),
        crate::masm::DebugVarLocation::Local(
            i16::try_from(i32::from(offset) - frame.aligned_size() as i32).unwrap()
        )
    );
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("initialize").unwrap())
        .unwrap();
    assert_eq!(u32::from(procedure.num_locals()), frame.aligned_size());
    let addresses = globals
        .iter()
        .map(|global| {
            crate::NativePtr::from_ptr(link.globals_layout().get_computed_addr(*global).unwrap())
                .addr
        })
        .collect::<Vec<_>>();
    let suffix = (901..917).collect::<Vec<_>>();
    assert!(procedure.to_pretty_string().contains(&alloc::format!("loc_store.{offset}")));
    let code = alloc::format!(
        "{} begin {} exec.initialize sdepth push.32 assert_eq mem_load.{} push.210 assert_eq \
         mem_load.{} push.610 assert_eq {} end",
        procedure.to_pretty_string(),
        crate::plan::tests::lowering_units::push_top(&suffix),
        addresses[0],
        addresses[1],
        crate::plan::tests::lowering_units::assert_top(&suffix)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("initializers", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let advice = (1..=40).collect::<Vec<_>>();
    execute_program(&program, &mut crate::plan::tests::lowering_units::host(), &advice).unwrap();
    let mut missing = source.evidence().unwrap().source.program;
    missing
        .segments
        .iter_mut()
        .find(|s| !s.units.is_empty())
        .unwrap()
        .units
        .remove(0);
    assert!(check_selection(missing, &SourceContext::Generated(&source)).is_err());
}

#[test]
fn generated_harness_and_initializer_share_root_memory_and_preserve_original_window() {
    use crate::{
        lower::units::LoweringDefinition,
        select::generated::{GeneratedHarness, GeneratedInitializer, Initialization},
        verify::GeneratedDefinition,
    };
    let f = parse_module_function(
        "builtin.module public @root_data { builtin.global_variable private @g: felt { \
         builtin.ret_imm 77 : felt; }; builtin.function public extern(\"C\") @f() { builtin.ret; \
         }; };",
    );
    let owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let global = owner
        .borrow()
        .region(0)
        .entry()
        .body()
        .iter()
        .find_map(|op| op.downcast_ref::<builtin::GlobalVariable>().map(|g| g.as_global_var_ref()))
        .unwrap();
    let link = linked(&f);
    let env = SelectEnv {
        link_info: Some(&link),
        ..Default::default()
    };
    let init = GeneratedInitializer {
        stages: vec![Initialization::Global(global)],
        env: &env,
    };
    let (plan, frame, body) = ready_initializer(&init);
    let initialize = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("initialize").unwrap())
        .unwrap()
        .to_pretty_string();
    let harness = GeneratedHarness;
    let selected = check_selection(
        harness.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&harness),
    )
    .unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut registry = LoweringRegistry::default();
    registry.register(&harness).unwrap();
    let body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    let harness_proc = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("harness").unwrap())
        .unwrap()
        .to_pretty_string();
    assert_eq!(harness.kind(), LoweringKind(1300));
    let root = crate::plan::tests::pipeline::root_source(16, 16);
    let (plan, frame, body) = crate::plan::tests::pipeline::ready_root(&root);
    let root = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("root").unwrap())
        .unwrap()
        .to_pretty_string();
    let address =
        crate::NativePtr::from_ptr(link.globals_layout().get_computed_addr(global).unwrap()).addr;
    let checks = crate::plan::tests::lowering_units::assert_top(&(1..=16).collect::<Vec<_>>());
    let code = alloc::format!(
        "{initialize} {harness_proc} proc entry mem_load.{address} push.77 assert_eq mem_load.100 \
         push.555 assert_eq mem_load.103 push.558 assert_eq {checks} {} end {root} begin \
         exec.root sdepth push.16 assert_eq {checks} end",
        crate::plan::tests::lowering_units::push_top(&(1..=16).collect::<Vec<_>>())
    );
    let core = midenc_session::LinkLibrary::core()
        .load(&crate::plan::tests::lowering_units::session().options)
        .unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    let program = assembler
        .assemble_program("real_generated_root", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host().with_library(core).unwrap();
    let inputs = (1..=16).map(miden_core::Felt::new_unchecked).collect::<Vec<_>>();
    let advice = miden_processor::advice::AdviceInputs::default().with_stack(
        [1, 100, 1, 555, 556, 557, 558]
            .map(miden_core::Felt::new_unchecked)
            .to_vec()
            .into(),
    );
    miden_processor::FastProcessor::new(miden_processor::StackInputs::new(&inputs).unwrap())
        .with_advice(advice)
        .unwrap()
        .execute_sync(&program, &mut host)
        .unwrap();
}

#[test]
fn generated_table_uses_live_last_entry_and_preserves_dependency_metadata() {
    use crate::select::generated::{GeneratedInitializer, Initialization, TableFragment};
    let f = parse_module_function(
        r#"builtin.module public @tables {
        builtin.function private extern("C") @dead() { builtin.ret; };
        builtin.function private extern("C") @winner() { builtin.ret; };
        builtin.function_alias private @winning_alias -> @winner;
        builtin.function_table private @table: 1 { builtin.function_table_entry 0 @dead tag 3; builtin.function_table_entry 0 @winning_alias tag 9; };
        builtin.function public extern("C") @f() { builtin.ret; };
    };"#,
    );
    let owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let table = owner
        .borrow()
        .region(0)
        .entry()
        .body()
        .iter()
        .find_map(|op| {
            op.downcast_ref::<builtin::FunctionTable>().map(|t| t.as_function_table_ref())
        })
        .unwrap();
    let env = SelectEnv::default();
    let source = GeneratedInitializer {
        stages: vec![Initialization::Table(TableFragment {
            owner,
            tables: vec![(table, 100)],
            children: vec![crate::lower::raw::target("::tables::child_init")],
        })],
        env: &env,
    };
    let (plan, frame, body) = ready_initializer(&source);
    let mut procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("table_init").unwrap())
        .unwrap();
    assert_eq!(procedure.invoked().count(), 2);
    procedure.set_visibility(crate::masm::Visibility::Public);
    let text = procedure.to_pretty_string();
    assert!(text.contains("winning_alias") && !text.contains("dead"), "{text}");
    let manager = alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
    let module =
        miden_assembly::ast::Module::parser(Some(miden_assembly::ast::ModuleKind::Library))
            .parse_str(
                Some(miden_assembly::Path::new("::tables")),
                alloc::format!(
                    "use {{winner as winning_alias}} from self proc dead push.999 mem_store.200 \
                     end proc winner push.555 mem_store.200 end proc child_init mem_load.104 \
                     push.9 assert_eq push.123 mem_store.201 end {text}"
                ),
                manager.clone(),
            )
            .unwrap();
    let library: alloc::sync::Arc<miden_assembly::package::Package> =
        miden_assembly::Assembler::new(manager)
            .assemble_library("tables", module, core::iter::empty::<&str>())
            .unwrap()
            .into();
    let mut assembler = miden_assembly::Assembler::default();
    assembler
        .link_package(library.clone(), miden_assembly::Linkage::Dynamic)
        .unwrap();
    let program = assembler
        .assemble_program(
            "table_last_entry",
            "begin exec.::tables::table_init mem_load.104 push.9 assert_eq push.100 dynexec \
             mem_load.200 push.555 assert_eq mem_load.201 push.123 assert_eq end",
        )
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host().with_library(library).unwrap();
    execute_program(&program, &mut host, &[]).unwrap();
    let mut altered_effects = source.evidence().unwrap().source.program;
    altered_effects.segments[0].units[0].effects.memory = SourceMemoryEffects::default();
    assert!(check_selection(altered_effects, &SourceContext::Generated(&source)).is_err());
}

#[test]
fn generated_heap_data_and_start_are_ordered_and_use_real_commitment_advice() {
    use crate::select::generated::{GeneratedInitializer, Initialization};
    let env = SelectEnv::default();
    let data = [111, 222, 333, 444].map(miden_core::Felt::new_unchecked).to_vec();
    let digest = miden_core::crypto::hash::Poseidon2::hash_elements(&data);
    let source = GeneratedInitializer {
        stages: vec![
            Initialization::Heap { base: 2048 },
            Initialization::Data {
                commitment: digest.as_elements().try_into().unwrap(),
                destination: 100,
                words: 1,
            },
            Initialization::Invoke(crate::lower::raw::target("start")),
        ],
        env: &env,
    };
    let (plan, frame, body) = ready_initializer(&source);
    let mut retargeted = body.clone();
    let instruction = retargeted
        .ops_mut()
        .iter_mut()
        .find_map(|op| match op {
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Exec(target),
                ..
            } => Some(target),
            _ => None,
        })
        .unwrap();
    *instruction = crate::lower::raw::target("wrong_stage");
    assert!(verify::verify(&plan, &frame, retargeted).is_err());
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("initialize").unwrap())
        .unwrap()
        .to_pretty_string();
    let core = midenc_session::LinkLibrary::core()
        .load(&crate::plan::tests::lowering_units::session().options)
        .unwrap();
    let intrinsics =
        crate::intrinsics::load(&crate::plan::tests::lowering_units::session()).unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    assembler
        .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
        .unwrap();
    let code = alloc::format!(
        "proc start mem_load.100 push.111 assert_eq push.99 mem_store.200 end {procedure} begin \
         exec.initialize mem_load.103 push.444 assert_eq mem_load.200 push.99 assert_eq \
         exec.::miden::core::sys::truncate_stack end"
    );
    let program = assembler
        .assemble_program("setup_order", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host()
        .with_library(core)
        .unwrap()
        .with_library(intrinsics)
        .unwrap();
    let advice = miden_processor::advice::AdviceInputs::default().with_map([(digest, data)]);
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .with_advice(advice)
        .unwrap()
        .execute_sync(&program, &mut host)
        .unwrap();
    let mut reordered = source.evidence().unwrap().source.program;
    reordered.segments[0].units.swap(0, 1);
    assert!(check_selection(reordered, &SourceContext::Generated(&source)).is_err());
}

#[test]
fn generated_export_initialization_and_empty_body_are_verified_together() {
    use crate::select::generated::GeneratedExport;
    let f = parse("builtin.function public extern(\"C\") @empty() { builtin.ret; };");
    let env = SelectEnv::default();
    let source = GeneratedExport {
        function: f.function,
        initializer: crate::lower::raw::target("initialize"),
        env: &env,
    };
    let evidence = source.evidence().unwrap();
    let selected =
        check_selection(evidence.source.program, &SourceContext::Generated(&source)).unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &evidence.source.source_locals).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut registry = LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition).unwrap();
    }
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    assert_eq!(
        body.top_level_instructions()
            .filter(|i| matches!(i, crate::masm::Instruction::EmitImm(_)))
            .count(),
        2
    );
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("exported").unwrap())
        .unwrap()
        .to_pretty_string();
    execute_code(&alloc::format!(
        "proc initialize push.777 mem_store.100 end {procedure} begin exec.exported mem_load.100 \
         push.777 assert_eq end"
    ))
    .unwrap();
    assert!(procedure.contains("initialize"));
}

#[test]
fn typed_global_initializers_execute_packed_and_wide_stores_with_a_protected_suffix() {
    use crate::select::generated::{GeneratedInitializer, Initialization};
    let values = [
        ("u8", 17u128),
        ("u16", 0x2222),
        ("u32", 0x33445566),
        ("u64", 0x1122334455667788),
        ("u128", 1 | (2 << 32) | (3 << 64) | (4 << 96)),
    ];
    let declarations = values
        .iter()
        .enumerate()
        .map(|(i, (ty, value))| {
            alloc::format!(
                "builtin.global_variable private @g{i}: {ty} {{ builtin.ret_imm {value} : {ty}; \
                 }};"
            )
        })
        .collect::<alloc::string::String>();
    let f = parse_module_function(&alloc::format!(
        "builtin.module public @typed {{ {declarations} builtin.function public extern(\"C\") \
         @f() {{ builtin.ret; }}; }};"
    ));
    let owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let globals = owner
        .borrow()
        .region(0)
        .entry()
        .body()
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<builtin::GlobalVariable>().map(|g| g.as_global_var_ref())
        })
        .collect::<Vec<_>>();
    let link = linked(&f);
    let env = SelectEnv {
        link_info: Some(&link),
        ..Default::default()
    };
    let source = GeneratedInitializer {
        stages: globals.iter().copied().map(Initialization::Global).collect(),
        env: &env,
    };
    let (plan, frame, body) = ready_initializer(&source);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("initialize").unwrap())
        .unwrap()
        .to_pretty_string();
    let mut expected = alloc::collections::BTreeMap::<u32, u32>::new();
    for (global, (_, value)) in globals.iter().zip(values) {
        let pointer =
            crate::NativePtr::from_ptr(link.globals_layout().get_computed_addr(*global).unwrap());
        for byte in 0..global.borrow().get_ty().size_in_bytes() {
            let absolute = usize::from(pointer.offset) + byte;
            *expected.entry(pointer.addr + (absolute / 4) as u32).or_default() |=
                (((value >> (byte * 8)) & 255) as u32) << ((absolute % 4) * 8);
        }
    }
    let checks = expected
        .iter()
        .map(|(address, value)| alloc::format!("mem_load.{address} push.{value} assert_eq "))
        .collect::<alloc::string::String>();
    let suffix = (901..917).collect::<Vec<_>>();
    let code = alloc::format!(
        "{procedure} begin {} exec.initialize {checks} {} end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::assert_top(&suffix)
    );
    let intrinsics =
        crate::intrinsics::load(&crate::plan::tests::lowering_units::session()).unwrap();
    let core = midenc_session::LinkLibrary::core()
        .load(&crate::plan::tests::lowering_units::session().options)
        .unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler
        .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
        .unwrap();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    let program = assembler
        .assemble_program("typed_globals", code)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host()
        .with_library(intrinsics)
        .unwrap()
        .with_library(core)
        .unwrap();
    execute_program(&program, &mut host, &[]).unwrap();
}

#[test]
fn immediate_source_writes_reject_compiler_address_values_during_replay() {
    use crate::{
        lower::units::{LoweringBindings, LoweringDefinition},
        verify::{DefinitionEvidence, GeneratedDefinition, UnitEvidence, VerificationError},
    };
    struct Escape(bool);
    impl GeneratedDefinition for Escape {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(1004)
        }

        fn instantiate(
            &self,
            _: &[miden_core::Felt],
        ) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = crate::select::generated::GeneratedHarness.instantiate(&[])?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.source_map.get_mut(&SourceStepId(0)).unwrap().origin =
                SourceOrigin::Generated {
                    definition: self.id(),
                    stage: 0,
                };
            let unit = &mut evidence.program.segments[0].units[0];
            unit.kind = self.kind();
            unit.scratch.push(ScratchRequirement {
                id: ScratchId(0),
                size: 4,
                alignment: 4,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Unit(unit.id),
                uses: vec![unit.id],
            });
            evidence.units.insert(unit.id, UnitEvidence::default());
            Ok(evidence)
        }
    }
    impl LoweringDefinition for Escape {
        fn kind(&self) -> LoweringKind {
            LoweringKind(1400)
        }

        fn expand(
            &self,
            unit: &SelectedLowering,
            bindings: &LoweringBindings,
            frame: &FinalFrameLayout,
            _: &ExpansionEnv,
        ) -> Result<AnnotatedMasmBody, midenc_hir::Report> {
            bindings.validate(unit, frame)?;
            let offset = frame.resolve(FrameRef::Scratch {
                object: ScratchId(0),
                offset: 0,
            })?;
            let mut body = AnnotatedMasmBody::new();
            for _ in 0..if self.0 { 4 } else { 1 } {
                body.push_instruction(crate::masm::Instruction::Locaddr(offset.into()), unit.id);
            }
            body.push_instruction(
                if self.0 {
                    crate::masm::Instruction::MemStoreWLeImm(100u32.into())
                } else {
                    crate::masm::Instruction::MemStoreImm(100u32.into())
                },
                unit.id,
            );
            if self.0 {
                body.push_instruction(crate::masm::Instruction::DropW, unit.id);
            }
            Ok(body)
        }
    }
    for word in [false, true] {
        let source = Escape(word);
        let selected = check_selection(
            source.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(&source),
        )
        .unwrap();
        let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
        let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
        resolve_frame_bindings(&mut plan, &frame).unwrap();
        let mut registry = LoweringRegistry::default();
        registry.register(&source).unwrap();
        let body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
        let error = verify::verify(&plan, &frame, body).unwrap_err();
        assert!(
            error.message.contains("compiler") && error.message.contains("escape"),
            "{error}"
        );
    }
}

#[test]
fn nested_context_and_actual_syscall_returns_keep_results_and_exact_window() {
    use alloc::sync::Arc;

    use miden_assembly::{
        Assembler, DefaultSourceManager, Path,
        ast::{Module, ModuleKind},
    };
    for (kernel, looping) in [(false, false), (true, false), (false, true), (true, true)] {
        let owner = if kernel { "$kernel" } else { "contexts" };
        let convention = if kernel { "C" } else { "component-model" };
        let body = if looping {
            r#"%r = scf.while %x before { ^before(%b: felt): scf.condition %p,%b : (i1,felt); } after { ^after(%c: felt): scf.yield %c : (felt); } : (felt) -> felt; %v=hir.exec @nested::@callee(%r) : extern("C") (felt) -> felt; builtin.ret %v : (felt);"#
        } else {
            r#"scf.if %p then { scf.if %q then { %r=hir.exec @nested::@callee(%x) : extern("C") (felt) -> felt; builtin.ret %r : (felt); } else { builtin.ret %x : (felt); } : (i1) -> (); builtin.ret %x : (felt); } else { builtin.ret %x : (felt); } : (i1) -> (); builtin.ret %x : (felt);"#
        };
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @"{owner}" {{
            builtin.module public @nested {{ builtin.function public extern("C") @callee(%x: felt) -> felt {{ builtin.ret %x : (felt); }}; }};
            builtin.function public extern("{convention}") @dispatch(%p: i1,%q: i1,%x: felt) -> felt {{
                {body}
            }};
        }};"#
        ));
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert_eq!(
            plan.selected.edges.iter().filter(|edge| edge.kind == EdgeKind::Return).count(),
            if looping { 1 } else { 3 }
        );
        let mut procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
            .unwrap();
        procedure.set_visibility(crate::masm::Visibility::Public);
        let core = midenc_session::LinkLibrary::core()
            .load(&crate::plan::tests::lowering_units::session().options)
            .unwrap();
        let manager = Arc::new(DefaultSourceManager::default());
        let nested_path = if kernel {
            "::$kernel::nested"
        } else {
            "::contexts::nested"
        };
        let nested = Module::parser(Some(ModuleKind::Library))
            .parse_str(Some(Path::new(nested_path)), "pub proc callee nop end", manager.clone())
            .unwrap();
        let nested: Arc<miden_assembly::package::Package> = Assembler::new(manager.clone())
            .assemble_library("nested", nested, core::iter::empty::<&str>())
            .unwrap()
            .into();
        let module = Module::parser(Some(if kernel {
            ModuleKind::Kernel
        } else {
            ModuleKind::Library
        }))
        .parse_str(
            Some(if kernel {
                Path::KERNEL
            } else {
                Path::new("::contexts")
            }),
            procedure.to_pretty_string(),
            manager.clone(),
        )
        .unwrap();
        let mut callee_assembler = Assembler::new(manager.clone());
        callee_assembler
            .link_package(core.clone(), miden_assembly::Linkage::Dynamic)
            .unwrap();
        callee_assembler
            .link_package(nested.clone(), miden_assembly::Linkage::Dynamic)
            .unwrap();
        let package: Arc<miden_assembly::package::Package> = if kernel {
            callee_assembler
                .assemble_kernel("kernel", module, None::<alloc::boxed::Box<Module>>)
                .unwrap()
                .into()
        } else {
            callee_assembler
                .assemble_library("contexts", module, core::iter::empty::<&str>())
                .unwrap()
                .into()
        };
        for &(p, q) in if looping {
            &[(0, 0)][..]
        } else {
            &[(0, 0), (1, 0), (1, 1)][..]
        } {
            let mut assembler = if kernel {
                Assembler::with_kernel(manager.clone(), package.clone()).unwrap()
            } else {
                Assembler::new(manager.clone())
            };
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            if !kernel {
                assembler
                    .link_package(package.clone(), miden_assembly::Linkage::Dynamic)
                    .unwrap();
            }
            let invocation = if kernel {
                "syscall.dispatch"
            } else {
                "call.::contexts::dispatch"
            };
            let code = alloc::format!(
                "begin push.42.{q}.{p} {invocation} push.42 assert_eq \
                 exec.::miden::core::sys::truncate_stack sdepth push.16 assert_eq end"
            );
            let program = assembler
                .assemble_program("nested_returns", code)
                .unwrap()
                .try_into_program()
                .unwrap();
            let mut host = crate::plan::tests::lowering_units::host()
                .with_library(core.clone())
                .unwrap()
                .with_library(nested.clone())
                .unwrap()
                .with_library(package.clone())
                .unwrap();
            execute_program(&program, &mut host, &[]).unwrap();
        }
    }
}

#[test]
fn returns_inside_physical_loops_remain_rejected() {
    for convention in ["C", "component-model"] {
        let f = parse(&alloc::format!(
            r#"builtin.function public extern("{convention}") @unsupported(%p: i1,%q: i1,%x: felt) -> felt {{
            %r = scf.while %x before {{ ^before(%b: felt): scf.condition %p,%b : (i1,felt); }} after {{ ^after(%c: felt):
                scf.if %q then {{ builtin.ret %c : (felt); }} else {{ scf.yield : (); }} : (i1) -> (); scf.yield %c : (felt);
            }} : (felt) -> felt; builtin.ret %r : (felt);
        }};"#
        ));
        let function = f.borrow();
        let env = SelectEnv::default();
        let selected = select_function(&function, &env).unwrap();
        let error = check_selection(
            selected,
            &SourceContext::Hir {
                function: &function,
                env: &env,
            },
        )
        .unwrap_err();
        assert!(alloc::format!("{error:?}").contains("logical return would fall through"));
    }
}

/// Exercise the real source observation, finalization, serialization and package validation seams.
/// Omitting the group anchor loses decorator-only blocks or leaves endpoint rows outside a block.
#[test]
fn variable_debug_groups_survive_package_boundaries() {
    use alloc::sync::Arc;

    use miden_assembly::{Assembler, DefaultSourceManager};
    use midenc_hir::{
        Builder, OpBuilder, Operation, SourceSpan,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };

    fn operations(operation: &Operation, result: &mut Vec<midenc_hir::OperationRef>) {
        for region in operation.regions() {
            for block in region.body() {
                for op in block.body() {
                    result.push(op.as_operation_ref());
                    operations(&op, result);
                }
            }
        }
    }
    let core = midenc_session::LinkLibrary::core()
        .load(&crate::plan::tests::lowering_units::session().options)
        .unwrap();
    let cases = [
        ("empty", "builtin.function public extern(\"C\") @subject() { builtin.ret; };"),
        (
            "terminal",
            "builtin.function public extern(\"C\") @subject(%x: felt) -> felt { %v=arith.add \
             %x,%x <{ overflow = #builtin.overflow<wrapping> }>; builtin.ret %v : (felt); };",
        ),
        (
            "siblings_and_loop",
            r#"builtin.function public extern("C") @subject(%p: i1,%x: felt) -> felt {
            %v=scf.if %p then { scf.yield %x : (felt); } else { scf.yield %x : (felt); } : (i1) -> felt;
            %r=scf.while %v before { ^before(%b: felt): scf.condition %p,%b : (i1,felt); } after { ^after(%c: felt): scf.yield %c : (felt); } : (felt) -> felt;
            builtin.ret %r : (felt);
        };"#,
        ),
        (
            "exec",
            r#"builtin.module public @calls {
            builtin.function public extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
            builtin.function public extern("C") @subject(%x: felt) -> felt { %v=hir.exec @callee(%x) : extern("C") (felt) -> felt; builtin.ret %v : (felt); };
        };"#,
        ),
        ("call", ""),
        (
            "dyncall",
            r#"builtin.function public extern("C") @subject(%r0: felt,%r1: felt,%r2: felt,%r3: felt) -> felt {
            %v=hir.dyncall [%r0,%r1,%r2,%r3](%r0) : extern("component-model") (felt) -> felt; builtin.ret %v : (felt);
        };"#,
        ),
        (
            "dynexec",
            r#"builtin.module public @calls {
            builtin.function private extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
            builtin.function_table private @table : 2 { builtin.function_table_entry 1 @callee tag 7; };
            builtin.function public extern("C") @subject(%i: u32,%x: felt) -> felt {
                %v=hir.exec_indirect @table[%i](%x) : extern("C") (felt) -> felt tag 7; builtin.ret %v : (felt);
            };
        };"#,
        ),
        ("syscall", ""),
        (
            "trapping",
            r#"builtin.function public extern("C") @subject() { ub.unreachable; };"#,
        ),
    ];
    for (case, text) in cases {
        let f = if case == "syscall" {
            let f = super::calls::direct("syscall", "C");
            let mut owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
            owner
                .borrow_mut()
                .downcast_mut::<builtin::Module>()
                .unwrap()
                .set_name(midenc_hir::Ident::from("$kernel"));
            f
        } else if case == "call" {
            super::calls::direct("call", "component-model")
        } else if text.starts_with("builtin.module") {
            parse_module_function(text)
        } else {
            parse(text)
        };
        let mut points = Vec::new();
        operations(f.borrow().as_operation(), &mut points);
        let mut builder = OpBuilder::new(f.context.clone());
        for (index, point) in points.iter().enumerate() {
            builder.set_insertion_point_before(*point);
            for suffix in ["first", "second"] {
                let variable = Variable {
                    name: midenc_hir::interner::Symbol::intern(alloc::format!(
                        "{case}_{index}_{suffix}"
                    )),
                    ..Default::default()
                };
                if suffix == "first" {
                    builder.debug_kill(variable, SourceSpan::UNKNOWN).unwrap();
                } else {
                    builder
                        .debug_declare(variable, Default::default(), SourceSpan::UNKNOWN)
                        .unwrap();
                }
            }
        }
        let link = (case == "dynexec").then(|| linked(&f));
        let env = SelectEnv {
            link_info: link.as_ref(),
            ..Default::default()
        };
        let evidence = crate::select::evidence(&f.borrow(), &env).unwrap();
        let selected = check_selection(
            evidence.source.program,
            &SourceContext::Hir {
                function: &f.borrow(),
                env: &env,
            },
        )
        .unwrap();
        let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
        let frame = assign_frame(&plan, &evidence.source.source_locals).unwrap();
        resolve_frame_bindings(&mut plan, &frame).unwrap();
        let mut registry = LoweringRegistry::default();
        for definition in &evidence.definitions {
            registry.register(definition).unwrap();
        }
        let mut expansion = ExpansionEnv::default();
        expansion.resolve_debug(&plan, &frame);
        assert_eq!(expansion.debug.len(), points.len() * 2, "{case}");
        let name = crate::masm::ProcedureName::new("subject").unwrap();
        let mut procedure =
            crate::lower::expand::finalize(&plan, &frame, &registry, &expansion, &name)
                .unwrap()
                .into_masm(name)
                .unwrap();
        procedure.set_visibility(crate::masm::Visibility::Public);
        for callee in ["proc callee nop end", "proc callee push.1 if.true nop else nop end end"] {
            let manager = Arc::new(DefaultSourceManager::default());
            let mut module = crate::masm::Module::parser(Some(crate::masm::ModuleKind::Library))
                .parse_str(
                    Some(miden_assembly::Path::new(if case == "call" {
                        "::direct_test"
                    } else {
                        "::calls"
                    })),
                    callee,
                    manager.clone(),
                )
                .unwrap();
            module.define_procedure(procedure.clone(), manager.clone()).unwrap();
            let mut assembler = if case == "syscall" {
                let kernel = crate::masm::Module::parser(Some(crate::masm::ModuleKind::Kernel))
                    .parse_str(
                        Some(miden_assembly::Path::KERNEL),
                        "pub proc callee nop end",
                        manager.clone(),
                    )
                    .unwrap();
                let kernel = Assembler::new(manager.clone())
                    .assemble_kernel(
                        "kernel",
                        kernel,
                        None::<alloc::boxed::Box<crate::masm::Module>>,
                    )
                    .unwrap();
                Assembler::with_kernel(manager.clone(), Arc::from(kernel)).unwrap()
            } else {
                Assembler::new(manager)
            };
            assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
            let package =
                assembler.assemble_library(case, module, core::iter::empty::<&str>()).unwrap();
            let debug =
                package.debug_info().unwrap_or_else(|error| panic!("{case}: {error}")).unwrap();
            for index in 0..points.len() {
                for suffix in ["first", "second"] {
                    let name = alloc::format!("{case}_{index}_{suffix}");
                    assert!(
                        debug.nodes().iter().flat_map(|node| &node.debug_vars).any(|row| debug
                            .get_string(row.name_idx)
                            .is_some_and(|actual| actual.as_ref() == name)),
                        "missing variable {name}: {procedure:?}"
                    );
                }
            }
        }
    }
}
