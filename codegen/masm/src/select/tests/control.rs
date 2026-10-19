use super::*;
use crate::allocate::analysis::analyze;

#[test]
fn real_hir_nested_before_control_has_distinct_entry_and_condition() {
    let f = parse(include_str!("fixtures/nested-before.hir"));
    let env = SelectEnv::default();
    let original = f.borrow().as_operation().to_pretty_string();
    let selected = checked(&f, &env);
    let ControlTree::Sequence(tree) = &selected.structure else {
        panic!("sequence")
    };
    let ControlTree::Loop {
        header,
        on_true,
        before,
        ..
    } = &tree[2]
    else {
        panic!("loop")
    };
    let site = selected.edges.iter().find(|e| e.id == *on_true).unwrap().source;
    assert_ne!(*header, site);
    assert!(matches!(before.as_ref(), ControlTree::Sequence(_)));
    let analysis = analyze(&selected);
    assert!(
        analysis
            .loops
            .values()
            .any(|l| l.members.contains(&site) && l.header == *header)
    );
    assert_eq!(original, f.borrow().as_operation().to_pretty_string());
    for (go, q, expected) in [(0, 0, 22), (0, 1, 11), (1, 0, 22), (1, 1, 11)] {
        execute(&f, &env, &[go, q, 11, 22], &[expected]);
    }
}

fn wide_while(before: usize, after: usize) -> String {
    let names = |prefix: &str, n: usize| {
        (0..n).map(|i| alloc::format!("%{prefix}{i}")).collect::<Vec<_>>().join(", ")
    };
    let params = |prefix: &str, n: usize| {
        (0..n)
            .map(|i| alloc::format!("%{prefix}{i}: felt"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let types = |n: usize| vec!["felt"; n].join(", ");
    let forwarded = (0..after)
        .map(|i| alloc::format!("%b{}", i % before))
        .collect::<Vec<_>>()
        .join(", ");
    let yielded = (0..before)
        .map(|i| alloc::format!("%c{}", i % after))
        .collect::<Vec<_>>()
        .join(", ");
    alloc::format!(
        "builtin.function public extern(\"C\") @wide(%p: i1, {}) -> ({}) {{ {} = scf.while {} \
         before {{ ^before({}): scf.condition %p, {} : (i1, {}); }} after {{ ^after({}): \
         scf.yield {} : ({}); }} : ({}) -> ({}); builtin.ret {} : ({}); }};",
        params("a", before),
        types(after),
        names("r", after),
        names("a", before),
        params("b", before),
        forwarded,
        types(after),
        params("c", after),
        yielded,
        types(before),
        types(before),
        types(after),
        names("r", after),
        types(after)
    )
}

#[test]
fn real_hir_wide_if_switch_and_while_keep_all_columns() {
    for (before, after) in [(18, 20), (20, 18), (16, 16)] {
        let f = parse(&wide_while(before, after));
        let env = SelectEnv::default();
        let original = f.borrow().as_operation().to_pretty_string();
        let selected = checked(&f, &env);
        assert!(selected.segments.iter().any(|s| s.parameters.len() == before));
        assert!(selected.segments.iter().any(|s| s.parameters.len() == after));
        let analysis = analyze(&selected);
        assert_eq!(analysis.loops.len(), 1);
        assert_eq!(original, f.borrow().as_operation().to_pretty_string());
        assert_eq!(selected.abi.results(), after as u32);
    }
    for kind in ["if", "switch"] {
        let types = vec!["felt"; 20].join(", ");
        let params = (0..20).map(|i| alloc::format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
        let values = (0..20).map(|i| alloc::format!("%a{i}")).collect::<Vec<_>>().join(", ");
        let results = (0..20).map(|i| alloc::format!("%r{i}")).collect::<Vec<_>>().join(", ");
        let region = alloc::format!("{{ scf.yield {values} : ({types}); }}");
        let control = if kind == "if" {
            alloc::format!("scf.if %p then {region} else {region} : (i1) -> ({types})")
        } else {
            alloc::format!("scf.index_switch %p case 7 {region} default {region} : ({types})")
        };
        let ty = if kind == "if" { "i1" } else { "u32" };
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @wide(%p: {ty}, {params}) -> ({types}) {{ \
             {results} = {control}; builtin.ret {results} : ({types}); }};"
        ));
        let original = f.borrow().as_operation().to_pretty_string();
        let selected = checked(&f, &SelectEnv::default());
        assert!(selected.segments.iter().any(|s| s.parameters.len() == 20));
        assert_eq!(original, f.borrow().as_operation().to_pretty_string());
    }
}

#[test]
fn real_hir_switch_case_region_pairs_and_dispatch_policy_are_checked() {
    let f = parse(include_str!("fixtures/switch-cases.hir"));
    for (index, expected) in [(6, 22), (7, 22), (8, 11), (9, 11), (10, 22), (u32::MAX as usize, 22)]
    {
        execute(&f, &SelectEnv::default(), &[index, 11, 22], &[expected]);
    }
    let default = parse(include_str!("fixtures/default-switch.hir"));
    let selected = checked(&default, &SelectEnv::default());
    assert!(selected.segments.iter().all(|s| s.units.is_empty()));
    execute(&default, &SelectEnv::default(), &[u32::MAX as usize, 42], &[42]);
    let sparse = include_str!("fixtures/switch-cases.hir").replace("case 8", "case 4294967295");
    let f = parse(&sparse);
    for (index, expected) in [(7, 22), (9, 11), (8, 22), (u32::MAX as usize, 11)] {
        execute(&f, &SelectEnv::default(), &[index, 11, 22], &[expected]);
    }
}

#[test]
fn real_hir_terminal_dispatch_and_trapping_arms_have_exact_coverage() {
    let f = parse(include_str!("fixtures/terminal-control.hir"));
    for (flag, expected) in [(0, 77), (1, 33)] {
        execute(&f, &SelectEnv::default(), &[flag, 33], &[expected]);
    }
    let f = parse(
        "builtin.function public extern(\"C\") @terminal(%p: i1) { scf.if %p then { builtin.ret; \
         } else { builtin.ret; } : (i1) -> (); builtin.ret; };",
    );
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.edges.iter().filter(|e| e.kind == EdgeKind::Return).count(), 2);
    let f = parse(
        "builtin.function public extern(\"C\") @trap(%p: i1, %a: felt) -> felt { %x = scf.if %p \
         then { ub.unreachable; } else { scf.yield %a : (felt); } : (i1) -> felt; builtin.ret %x \
         : (felt); };",
    );
    execute(&f, &SelectEnv::default(), &[0, 44], &[44]);
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn real_hir_missing_else_captures_duplicates_and_permutations() {
    let f = parse(
        "builtin.function public extern(\"C\") @identity(%p: i1, %a: felt) -> felt { scf.if %p \
         then { scf.yield : (); } else {} : (i1) -> (); builtin.ret %a : (felt); };",
    );
    for flag in [0, 1] {
        execute(&f, &SelectEnv::default(), &[flag, 55], &[55]);
    }
    let f = parse(&wide_while(2, 3));
    let selected = checked(&f, &SelectEnv::default());
    let analysis = analyze(&selected);
    assert_eq!(analysis.loops.len(), 1);
    assert!(
        selected
            .edges
            .iter()
            .any(|e| e.bindings.len() == 3 && e.bindings[0].source == e.bindings[2].source)
    );
}

#[test]
fn real_hir_inner_loops_in_before_and_trapping_before_paths_are_represented() {
    let f = parse(include_str!("fixtures/nested-before-inner.hir"));
    let env = SelectEnv::default();
    let selected = checked(&f, &env);
    let analysis = analyze(&selected);
    assert_eq!(analysis.loops.len(), 2);
    for (go, q, expected) in [(0, 0, 22), (0, 1, 11), (1, 0, 22), (1, 1, 11)] {
        execute(&f, &env, &[go, q, 11, 22], &[expected]);
    }
    let f = parse(include_str!("fixtures/trapping-before.hir"));
    execute(&f, &env, &[0, 0, 73], &[73]);
    let text = include_str!("fixtures/trapping-before.hir")
        .replace("scf.yield %value : (felt);", "ub.unreachable;");
    let f = parse(&text);
    let selected = checked(&f, &env);
    assert!(analyze(&selected).loops.is_empty());
    assert_eq!(selected.edges.iter().filter(|e| e.kind == EdgeKind::Trap).count(), 2);
    assert!(selected.source_map.len() > selected.segments.len());
    let (plan, frame, body) = ready(&f, &env);
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn unsupported_early_return_fallthrough_and_residual_cycles_fail_closed() {
    for text in [
        "builtin.function public extern(\"C\") @early(%p: i1,%a: felt) -> felt { %x=scf.if %p \
         then {builtin.ret %a : (felt);} else {scf.yield %a : (felt);} : (i1) -> felt; \
         builtin.ret %x : (felt); };",
        "builtin.function public extern(\"C\") @cycle(%p: i1) { cf.cond_br %p ^loop, ^exit : \
         (i1); ^loop: cf.cond_br %p ^loop,^exit : (i1); ^exit: builtin.ret; };",
    ] {
        let f = parse(text);
        let function = f.borrow();
        let env = SelectEnv::default();
        let result = select_function(&function, &env).and_then(|selected| {
            check_selection(
                selected,
                &SourceContext::Hir {
                    function: &function,
                    env: &env,
                },
            )
            .map(|_| ())
            .map_err(Into::into)
        });
        assert!(result.is_err());
    }
}

#[test]
fn simultaneous_loop_parameter_permutation_preserves_bits_and_dead_columns() {
    let f = parse(include_str!("fixtures/permuted-loop.hir"));
    let env = SelectEnv::default();
    let selected = checked(&f, &env);
    let analysis = analyze(&selected);
    let entry = &selected.segments[0];
    assert_eq!(entry.parameters.len(), 4);
    assert!(!analysis.live_in[&entry.id].contains(entry.parameters[3]));
    let backedge = selected.edges.iter().find(|e| e.kind == EdgeKind::Backedge).unwrap();
    let after = selected.segments.iter().find(|s| s.id == backedge.source).unwrap();
    assert_eq!(backedge.bindings[1].source, after.parameters[1]);
    assert_eq!(backedge.bindings[2].source, after.parameters[0]);
    assert_eq!(backedge.bindings.len(), 4);
    execute(&f, &env, &[0, 11, 22, 999], &[11, 22]);
    execute(&f, &env, &[1, 11, 22, 999], &[22, 11]);
}

#[test]
fn switch_region_parameters_are_rejected_for_every_dispatch_policy() {
    use midenc_hir::{SourceSpan, Type};
    for (text, region_index) in [
        (include_str!("fixtures/default-switch.hir").to_string(), 0),
        (include_str!("fixtures/switch-cases.hir").replace("case 8", "case 6"), 0),
        (include_str!("fixtures/switch-cases.hir").to_string(), 0),
        (include_str!("fixtures/switch-cases.hir").to_string(), 1),
        (include_str!("fixtures/switch-cases.hir").to_string(), 3),
    ] {
        let f = parse(&text);
        let op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let region = op.borrow().region(region_index).as_region_ref();
        let block = region.borrow().entry_block_ref().unwrap();
        f.context.append_block_argument(block, Type::Felt, SourceSpan::UNKNOWN);
        assert_malformed_source_rejected(&f, &SelectEnv::default());
    }
}
