use alloc::collections::BTreeMap;

use midenc_hir::formatter::PrettyPrint;

use super::*;

#[test]
fn final_cells_obey_signed_debug_displacement_boundaries() {
    for (size, offset, expected) in [
        (32768, 0, masm::DebugVarLocation::Local(-32768)),
        (32772, 3, masm::DebugVarLocation::Unavailable),
        (MAX_FRAME_ELEMENTS, 0, masm::DebugVarLocation::Unavailable),
        (MAX_FRAME_ELEMENTS, MAX_FRAME_ELEMENTS - 1, masm::DebugVarLocation::Local(-1)),
    ] {
        let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
        let frame = FinalFrameLayout::new(
            SourceLocalLayout::default(),
            BTreeMap::from([(slot, offset)]),
            BTreeMap::new(),
            size,
        )
        .unwrap();
        assert_eq!(resolve_lanes(&[Location::Cell(slot)], &frame), expected);
    }
}

#[test]
fn split_and_inaccessible_debug_values_are_unavailable() {
    let frame =
        FinalFrameLayout::new(SourceLocalLayout::default(), BTreeMap::new(), BTreeMap::new(), 0)
            .unwrap();
    assert_eq!(
        resolve_lanes(&[Location::Stack(15), Location::Stack(16)], &frame),
        masm::DebugVarLocation::Unavailable
    );
    assert_eq!(
        resolve_lanes(&[Location::Stack(0), Location::Unavailable], &frame),
        masm::DebugVarLocation::Unavailable
    );
    assert_eq!(
        resolve_lanes(&[Location::Stack(2), Location::Stack(3)], &frame),
        masm::DebugVarLocation::Stack(2)
    );
    assert_eq!(resolve_lanes(&[], &frame), masm::DebugVarLocation::Unavailable);
}

fn observed_function() -> crate::select::tests::Parsed {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, ValueRef,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };
    let args = (0..20)
        .map(|i| alloc::format!("%a{i}: felt"))
        .collect::<alloc::vec::Vec<_>>()
        .join(", ");
    let mut body = alloc::string::String::new();
    body.push_str("%s1 = arith.add %a0, %a1 <{ overflow = #builtin.overflow<wrapping> }>; ");
    for i in 2..20 {
        body.push_str(&alloc::format!(
            "%s{i} = arith.add %s{}, %a{i} <{{ overflow = #builtin.overflow<wrapping> }}>; ",
            i - 1
        ));
    }
    let f = crate::select::tests::parse(&alloc::format!(
        "builtin.function public extern(\"C\") @observed({args}) -> felt {{ {body} builtin.ret \
         %s19 : (felt); }};"
    ));
    let entry = f.borrow().entry_block();
    let ops = entry
        .borrow()
        .body()
        .iter()
        .map(|op| op.as_operation_ref())
        .collect::<alloc::vec::Vec<_>>();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(ops[1]);
    let arg = entry.borrow().arguments()[19] as ValueRef;
    let var = Variable {
        name: midenc_hir::interner::Symbol::intern("retained"),
        ..Default::default()
    };
    builder.debug_value(arg, var.clone(), SourceSpan::UNKNOWN).unwrap();
    let sum = ops[0].borrow().results().all()[0].borrow().as_value_ref();
    builder.debug_value(sum, var.clone(), SourceSpan::UNKNOWN).unwrap();
    builder.debug_kill(var, SourceSpan::UNKNOWN).unwrap();
    f
}

#[test]
fn source_observations_use_real_spills_without_affecting_allocation() {
    use crate::{
        allocate::{
            allocate,
            analysis::analyze,
            schedule::{AllocationOptions, SchedulingTier},
            slots::assign_frame,
        },
        lower::{
            expand::{ExpansionEnv, expand_procedure, resolve_frame_bindings},
            units::LoweringRegistry,
        },
        select::{SelectEnv, evidence},
        verify::{SourceContext, check_selection},
    };
    let f = observed_function();
    let env = SelectEnv::default();
    let options = AllocationOptions {
        force_tier: Some(SchedulingTier::Normalization),
        ..Default::default()
    };
    let (observed, mut plan) = {
        let function = f.borrow();
        let evidence = evidence(&function, &env).unwrap();
        assert_eq!(evidence.observations.len(), 3);
        let checked = check_selection(
            evidence.source.program.clone(),
            &SourceContext::Hir {
                function: &function,
                env: &env,
            },
        )
        .unwrap();
        (evidence, allocate(checked, &options).unwrap())
    };
    assert_eq!(plan.debug_locations.len(), 3);
    assert!(
        plan.debug_locations[&DebugLocationId(0)]
            .locations
            .iter()
            .all(|l| matches!(l, Location::Cell(_)))
    );
    assert!(
        plan.debug_locations[&DebugLocationId(1)]
            .locations
            .iter()
            .all(|l| matches!(l, Location::Stack(_)))
    );
    let frame = assign_frame(&plan, &observed.source.source_locals).unwrap();
    assert!(frame.aligned_size() > 0);
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut expansion = ExpansionEnv::default();
    expansion.resolve_debug(&plan, &frame);
    let DebugDecorator::Variable(info) = &expansion.debug[&DebugLocationId(0)] else {
        panic!("variable");
    };
    let masm::DebugVarLocation::Local(offset) = info.value_location() else {
        panic!("actual spilled location: {:?}", info.value_location());
    };
    assert_eq!(
        miden_debug::resolve_variable_value(
            info.value_location(),
            &[],
            |_| None,
            |candidate| (candidate == *offset).then_some(miden_core::Felt::new_unchecked(91))
        ),
        Some(miden_core::Felt::new_unchecked(91))
    );
    let mut registry = LoweringRegistry::default();
    for definition in &observed.definitions {
        registry.register(definition).unwrap();
    }
    let body = expand_procedure(&plan, &frame, &registry, &expansion).unwrap();
    let observed_procedure = crate::verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("observed").unwrap())
        .unwrap()
        .to_pretty_string();
    let before = analyze(&plan.selected);
    let entry = f.borrow().entry_block();
    let debug_ops = entry
        .borrow()
        .body()
        .iter()
        .filter(|op| alloc::format!("{}", op.name()).starts_with("di."))
        .map(|op| op.as_operation_ref())
        .collect::<alloc::vec::Vec<_>>();
    for mut op in debug_ops {
        op.borrow_mut().erase();
    }
    let without = {
        let function = f.borrow();
        let evidence = evidence(&function, &env).unwrap();
        let checked = check_selection(
            evidence.source.program,
            &SourceContext::Hir {
                function: &function,
                env: &env,
            },
        )
        .unwrap();
        allocate(checked, &options).unwrap()
    };
    let after = analyze(&without.selected);
    assert_eq!(before.before, after.before);
    assert_eq!(before.after, after.after);
    assert_eq!(plan.spill_slots, without.spill_slots);
    assert_eq!(plan.edge_actions, without.edge_actions);
    for (segment, actions) in &plan.segment_actions {
        let semantic = actions
            .iter()
            .filter(|a| !matches!(a, crate::plan::actions::PlanAction::DebugLocation(_)))
            .cloned()
            .collect::<alloc::vec::Vec<_>>();
        assert_eq!(semantic, without.segment_actions[segment]);
    }
    let frame = assign_frame(&without, &observed.source.source_locals).unwrap();
    let mut without = without;
    resolve_frame_bindings(&mut without, &frame).unwrap();
    let body = expand_procedure(&without, &frame, &registry, &ExpansionEnv::default()).unwrap();
    let plain_procedure = crate::verify::verify(&without, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("plain").unwrap())
        .unwrap()
        .to_pretty_string();
    // Reuse the existing VM fixture host and tuple assertions. Distinct arguments and a
    // protected suffix expose operand/order mistakes in addition to the arithmetic result.
    let inputs = (1..=20).collect::<alloc::vec::Vec<_>>();
    let suffix = (901..=920).collect::<alloc::vec::Vec<_>>();
    let mut expected = alloc::vec![210];
    expected.extend(&suffix);
    let invocation = |name| {
        alloc::format!(
            "{} {} exec.{name} {}",
            crate::plan::tests::lowering_units::push_top(&suffix),
            crate::plan::tests::lowering_units::push_top(&inputs),
            crate::plan::tests::lowering_units::assert_top(&expected)
        )
    };
    let source = alloc::format!(
        "{observed_procedure} {plain_procedure} begin {} {} \
         exec.::miden::core::sys::truncate_stack end",
        invocation("observed"),
        invocation("plain")
    );
    let core = midenc_session::LinkLibrary::core().load(&f.context.session().options).unwrap();
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
    let program = assembler
        .assemble_program("debug_observations", source)
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host().with_library(core).unwrap();
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .unwrap();
}

fn debug_plan(
    f: &crate::select::tests::Parsed,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout) {
    debug_plan_with_tier(f, crate::allocate::schedule::SchedulingTier::Normalization)
}

fn debug_plan_with_tier(
    f: &crate::select::tests::Parsed,
    tier: crate::allocate::schedule::SchedulingTier,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout) {
    use crate::{
        allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
        select::{SelectEnv, evidence},
        verify::{SourceContext, check_selection},
    };
    let function = f.borrow();
    let env = SelectEnv::default();
    let source = evidence(&function, &env).unwrap();
    let selected = check_selection(
        source.source.program,
        &SourceContext::Hir {
            function: &function,
            env: &env,
        },
    )
    .unwrap();
    let plan = allocate(
        selected,
        &AllocationOptions {
            force_tier: Some(tier),
            ..Default::default()
        },
    )
    .unwrap();
    let frame = assign_frame(&plan, &source.source.source_locals).unwrap();
    (plan, frame)
}

#[test]
fn debug_only_dead_value_does_not_gain_storage_or_liveness() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };
    let f = crate::select::tests::parse(
        r#"builtin.function public extern("C") @dead(%a: felt, %b: felt, %c: felt, %d: felt) -> felt {
        %dead = arith.add %a, %b <{ overflow = #builtin.overflow<wrapping> }>;
        %live = arith.add %c, %d <{ overflow = #builtin.overflow<wrapping> }>;
        builtin.ret %live : (felt);
    };"#,
    );
    let entry = f.borrow().entry_block();
    let ops = entry
        .borrow()
        .body()
        .iter()
        .map(|op| op.as_operation_ref())
        .collect::<alloc::vec::Vec<_>>();
    let dead = ops[0].borrow().results().all()[0].borrow().as_value_ref();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(ops[2]);
    builder.debug_value(dead, Variable::default(), SourceSpan::UNKNOWN).unwrap();
    let (plan, frame) = debug_plan(&f);
    let record = &plan.debug_locations[&DebugLocationId(0)];
    assert_eq!(record.locations, [Location::Unavailable]);
    let DebugDecorator::Variable(info) = record.decorator(&frame) else {
        panic!("variable");
    };
    assert_eq!(*info.value_location(), masm::DebugVarLocation::Unavailable);
    let dead_id = record.observation.lanes()[0].value;
    assert!(
        plan.spill_slots
            .iter()
            .all(|slot| *slot != SpillSlotId::canonical(Atom::new(dead_id, 0)))
    );
}

#[test]
fn mixed_width_source_offsets_and_inline_chain_use_final_frame() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, Type,
        dialects::debuginfo::{
            DIBuilder,
            attributes::{
                Expression, ExpressionOp, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain,
                InlineCallChainAttr, InlineCallFrame, Variable,
            },
        },
    };
    let mut f = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @mixed(%a: felt) -> felt { builtin.ret %a : \
         (felt); };",
    );
    f.function.borrow_mut().alloc_local(Type::U32);
    f.function.borrow_mut().alloc_local(Type::U64);
    f.function.borrow_mut().alloc_local(Type::U128);
    let entry = f.borrow().entry_block();
    let mut ret = entry.borrow().terminator().unwrap();
    let frame = |name: &str, line| InlineCallFrame {
        name: midenc_hir::interner::Symbol::intern(name),
        linkage_name: None,
        file: "inlined.rs".into(),
        line: 1,
        column: 1,
        call_file: "caller.rs".into(),
        call_line: line,
        call_column: 2,
    };
    let attr = f
        .context
        .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![
            frame("outer", 10),
            frame("inner", 20)
        ]))
        .as_attribute_ref();
    ret.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, attr);
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(ret);
    for local in [0, 1, 2] {
        builder
            .debug_declare(
                Variable {
                    name: midenc_hir::interner::Symbol::intern(alloc::format!("local{local}")),
                    ..Default::default()
                },
                Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(local)]),
                SourceSpan::UNKNOWN,
            )
            .unwrap();
    }
    let (plan, final_frame) = debug_plan(&f);
    assert_eq!(final_frame.source().offsets, [0, 1, 3]);
    assert_eq!(final_frame.aligned_size(), 8);
    let locations = plan
        .debug_locations
        .values()
        .filter_map(|r| match r.decorator(&final_frame) {
            DebugDecorator::Variable(info) => Some(info.value_location().clone()),
            _ => None,
        })
        .collect::<alloc::vec::Vec<_>>();
    assert_eq!(
        locations,
        [
            masm::DebugVarLocation::Local(-8),
            masm::DebugVarLocation::Local(-7),
            masm::DebugVarLocation::Local(-5)
        ]
    );
    let inline = plan
        .debug_locations
        .values()
        .filter_map(|r| match r.decorator(&final_frame) {
            DebugDecorator::InlineCall(info) => Some(alloc::format!("{info:?}")),
            _ => None,
        })
        .collect::<alloc::vec::Vec<_>>();
    assert_eq!(inline.len(), 2);
    assert!(inline[0].contains("outer"));
    assert!(inline[1].contains("inner"));
}

/// Replay the allocator's raw actions with distinct SSA lane identities. Physical cells
/// are keyed by final offsets, so a colored overwrite cannot hide behind a symbolic slot.
#[test]
fn colored_cells_never_report_overwritten_cached_values() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, ValueRef,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };

    use crate::plan::actions::PlanAction;
    let f = observed_function();
    let entry = f.borrow().entry_block();
    let arguments = entry
        .borrow()
        .arguments()
        .iter()
        .map(|arg| *arg as ValueRef)
        .collect::<alloc::vec::Vec<_>>();
    let ops = entry
        .borrow()
        .body()
        .iter()
        .filter(|op| !alloc::format!("{}", op.name()).starts_with("di."))
        .map(|op| op.as_operation_ref())
        .collect::<alloc::vec::Vec<_>>();
    let mut builder = OpBuilder::new(f.context.clone());
    for op in ops {
        builder.set_insertion_point_before(op);
        for (index, argument) in arguments.iter().enumerate() {
            builder
                .debug_value(
                    *argument,
                    Variable {
                        name: midenc_hir::interner::Symbol::intern(alloc::format!(
                            "argument{index}"
                        )),
                        ..Default::default()
                    },
                    SourceSpan::UNKNOWN,
                )
                .unwrap();
        }
    }
    let (plan, frame) = debug_plan(&f);
    let mut expansion = crate::lower::expand::ExpansionEnv::default();
    expansion.resolve_debug(&plan, &frame);
    let mut stack = plan.contracts[&SegmentId(0)].stack_top_first.clone();
    let mut memory = BTreeMap::new();
    let mut stored = alloc::collections::BTreeSet::new();
    let mut loaded = alloc::collections::BTreeSet::new();
    let mut cached_after_load = 0;
    let mut overwrites = 0;
    for action in &plan.segment_actions[&SegmentId(0)] {
        match *action {
            PlanAction::Dup { depth } => stack.insert(0, stack[usize::from(depth)]),
            PlanAction::Swap { depth } => stack.swap(0, usize::from(depth)),
            PlanAction::MoveUp { depth } => {
                let atom = stack.remove(usize::from(depth));
                stack.insert(0, atom);
            }
            PlanAction::MoveDown { depth } => {
                let atom = stack.remove(0);
                stack.insert(usize::from(depth), atom);
            }
            PlanAction::Drop => {
                stack.remove(0);
            }
            PlanAction::StoreLane(slot) => {
                let atom = stack.remove(0);
                let offset = frame.resolve(FrameRef::Spill(slot)).unwrap();
                if memory.insert(offset, atom).is_some_and(|previous| previous != atom) {
                    overwrites += 1;
                }
                stored.insert(atom);
            }
            PlanAction::LoadLane(slot) => {
                let atom = memory[&frame.resolve(FrameRef::Spill(slot)).unwrap()];
                assert!(stored.contains(&atom));
                loaded.insert(atom);
                stack.insert(0, atom);
            }
            PlanAction::RunLowering { step } => {
                let unit = plan
                    .selected
                    .segments
                    .iter()
                    .flat_map(|segment| &segment.units)
                    .find(|unit| unit.id == step)
                    .unwrap();
                assert_eq!(&stack[..unit.stack_inputs.len()], unit.stack_inputs);
                stack.drain(..unit.stack_inputs.len());
                stack.splice(0..0, unit.outputs.iter().copied());
            }
            PlanAction::DebugLocation(id) => {
                let record = &plan.debug_locations[&id];
                let DebugDecorator::Variable(info) = &expansion.debug[&id] else {
                    continue;
                };
                match info.value_location() {
                    masm::DebugVarLocation::Stack(depth) => {
                        assert_eq!(stack[usize::from(*depth)], record.observation.lanes()[0]);
                    }
                    masm::DebugVarLocation::Local(displacement) => {
                        let offset =
                            (i64::from(frame.aligned_size()) + i64::from(*displacement)) as u16;
                        let expected = record.observation.lanes()[0];
                        assert_eq!(
                            memory.get(&offset),
                            Some(&expected),
                            "stale colored cell at {id:?}"
                        );
                        cached_after_load += usize::from(loaded.contains(&expected));
                    }
                    masm::DebugVarLocation::Unavailable => {}
                    location => panic!("unexpected simple operand location: {location:?}"),
                }
            }
            PlanAction::PushLiteral(_) => panic!("fixture contains no literal operands"),
        }
    }
    assert!(cached_after_load > 0, "a reload must preserve a valid cached copy");
    assert!(overwrites > 0, "disjoint lifetimes must actually reuse physical cells");
}

#[test]
fn allocated_wide_value_crossing_accessible_window_is_unavailable() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, ValueRef,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };
    let args = (0..15)
        .map(|index| alloc::format!("%a{index}: felt"))
        .chain(["%wide: u64".into(), "%tail: felt".into()])
        .collect::<alloc::vec::Vec<_>>()
        .join(", ");
    let f = crate::select::tests::parse(&alloc::format!(
        "builtin.function public extern(\"C\") @split({args}) -> u64 {{ builtin.ret %wide : \
         (u64); }};"
    ));
    let entry = f.borrow().entry_block();
    let ret = entry.borrow().terminator().unwrap();
    let wide = entry.borrow().arguments()[15] as ValueRef;
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(ret);
    builder.debug_value(wide, Variable::default(), SourceSpan::UNKNOWN).unwrap();
    let (plan, frame) = debug_plan(&f);
    let record = &plan.debug_locations[&DebugLocationId(0)];
    assert_eq!(record.locations, [Location::Stack(15), Location::Unavailable]);
    let mut expansion = crate::lower::expand::ExpansionEnv::default();
    expansion.resolve_debug(&plan, &frame);
    let DebugDecorator::Variable(info) = &expansion.debug[&DebugLocationId(0)] else {
        panic!("variable");
    };
    assert_eq!(*info.value_location(), masm::DebugVarLocation::Unavailable);
}

#[test]
fn same_source_value_is_observed_in_a_spill_then_after_reload() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, ValueRef,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };
    let args = (0..20)
        .map(|index| alloc::format!("%a{index}: felt"))
        .collect::<alloc::vec::Vec<_>>()
        .join(", ");
    let mut body = alloc::string::String::from(
        "%s0 = arith.add %a19, %a0 <{ overflow = #builtin.overflow<wrapping> }>; %s1 = arith.add \
         %s0, %a19 <{ overflow = #builtin.overflow<wrapping> }>; %s2 = arith.add %s1, %a19 <{ \
         overflow = #builtin.overflow<wrapping> }>; ",
    );
    for index in 1..19 {
        body.push_str(&alloc::format!(
            "%s{} = arith.add %s{}, %a{index} <{{ overflow = #builtin.overflow<wrapping> }}>; ",
            index + 2,
            index + 1
        ));
    }
    let f = crate::select::tests::parse(&alloc::format!(
        "builtin.function public extern(\"C\") @reload({args}) -> felt {{ {body} builtin.ret %s20 \
         : (felt); }};"
    ));
    let entry = f.borrow().entry_block();
    let argument = entry.borrow().arguments()[19] as ValueRef;
    let ops = entry
        .borrow()
        .body()
        .iter()
        .map(|op| op.as_operation_ref())
        .collect::<alloc::vec::Vec<_>>();
    let mut builder = OpBuilder::new(f.context.clone());
    for op in [ops[1], ops[2]] {
        builder.set_insertion_point_before(op);
        builder.debug_value(argument, Variable::default(), SourceSpan::UNKNOWN).unwrap();
    }
    let (plan, frame) =
        debug_plan_with_tier(&f, crate::allocate::schedule::SchedulingTier::Selective);
    let spilled = &plan.debug_locations[&DebugLocationId(0)];
    let reloaded = &plan.debug_locations[&DebugLocationId(1)];
    assert_eq!(spilled.observation.lanes(), reloaded.observation.lanes());
    assert!(matches!(spilled.locations.as_slice(), [Location::Cell(_)]));
    // A loaded operand is consumed by arithmetic; its cached cell remains a valid
    // description afterwards. Observe both sides of the actual load, without asking
    // allocation to keep a redundant stack copy solely for this marker.
    assert_eq!(spilled.locations, reloaded.locations);
    let [Location::Cell(slot)] = spilled.locations.as_slice() else {
        unreachable!()
    };
    let actions = &plan.segment_actions[&SegmentId(0)];
    let before = actions
        .iter()
        .position(|action| {
            *action == crate::plan::actions::PlanAction::DebugLocation(DebugLocationId(0))
        })
        .unwrap();
    let after = actions
        .iter()
        .position(|action| {
            *action == crate::plan::actions::PlanAction::DebugLocation(DebugLocationId(1))
        })
        .unwrap();
    assert!(
        actions[before + 1..after].contains(&crate::plan::actions::PlanAction::LoadLane(*slot))
    );
    let mut expansion = crate::lower::expand::ExpansionEnv::default();
    expansion.resolve_debug(&plan, &frame);
    for id in [DebugLocationId(0), DebugLocationId(1)] {
        let DebugDecorator::Variable(info) = &expansion.debug[&id] else {
            panic!("variable");
        };
        assert!(matches!(info.value_location(), masm::DebugVarLocation::Local(_)));
    }
}

#[test]
fn branch_and_loop_observations_keep_source_positions() {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan, ValueRef,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
    };

    use crate::{
        allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
        lower::{
            expand::{ExpansionEnv, expand_procedure, resolve_frame_bindings},
            units::LoweringRegistry,
        },
        select::{SelectEnv, evidence},
        verify::{SourceContext, check_selection},
    };
    let f = crate::select::tests::parse(
        r#"builtin.function public extern("C") @control_debug(%p: i1,%x: felt) -> felt {
        %v=scf.if %p then { scf.yield %x : (felt); } else { scf.yield %x : (felt); } : (i1) -> felt;
        %r=scf.while %v before { ^before(%b: felt): scf.condition %p,%b : (i1,felt); } after { ^after(%c: felt): scf.yield %c : (felt); } : (felt) -> felt;
        builtin.ret %r : (felt);
    };"#,
    );
    let entry = f.borrow().entry_block();
    let ops = entry.borrow().body().iter().map(|op| op.as_operation_ref()).collect::<Vec<_>>();
    let outer = entry.borrow().arguments()[1] as ValueRef;
    let mut builder = OpBuilder::new(f.context.clone());
    for operation in &ops[..2] {
        for region in 0..2 {
            let (terminator, value) = {
                let operation = operation.borrow();
                let region = operation.region(region);
                let block = region.entry();
                (
                    block.body().iter().next_back().unwrap().as_operation_ref(),
                    block.arguments().first().map_or(outer, |arg| *arg as ValueRef),
                )
            };
            builder.set_insertion_point_before(terminator);
            builder
                .debug_value(
                    value,
                    Variable {
                        name: midenc_hir::interner::Symbol::intern("control"),
                        ..Default::default()
                    },
                    SourceSpan::UNKNOWN,
                )
                .unwrap();
        }
    }
    let env = SelectEnv::default();
    let source = evidence(&f.borrow(), &env).unwrap();
    assert_eq!(source.observations.len(), 4);
    let checked = check_selection(
        source.source.program,
        &SourceContext::Hir {
            function: &f.borrow(),
            env: &env,
        },
    )
    .unwrap();
    let mut plan = allocate(checked, &AllocationOptions::default()).unwrap();
    assert_eq!(plan.debug_locations.len(), 4);
    let frame = assign_frame(&plan, &source.source.source_locals).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut expansion = ExpansionEnv::default();
    expansion.resolve_debug(&plan, &frame);
    assert_eq!(expansion.debug.len(), 4);
    let mut registry = LoweringRegistry::default();
    for definition in &source.definitions {
        registry.register(definition).unwrap();
    }
    let body = expand_procedure(&plan, &frame, &registry, &expansion).unwrap();
    crate::verify::verify(&plan, &frame, body).unwrap();
}
