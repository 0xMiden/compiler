use core::cell::Cell;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    BuilderExt, Op, SourceSpan, Type,
    dialects::{builtin::BuiltinOpBuilder, debuginfo::attributes::Variable},
    testing::Test,
};

use super::{super::tests::OpaqueRegion, *};
use crate::HirOpBuilder;

// Count payload entries actually materialized by fact construction, including joins as well as
// clones. This measures work, not Rc layout or elapsed time, and is isolated between test threads.
std::thread_local! {
    pub(super) static MATERIALIZED: Cell<usize> = const { Cell::new(0) };
    // Count all translator value comparisons: source membership, target deduplication, and
    // prospective-result equality. Collection/iteration without comparisons is not counted.
    pub(super) static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

fn materialized(f: impl FnOnce()) -> usize {
    MATERIALIZED.set(0);
    f();
    MATERIALIZED.get()
}

fn recovery(aliases: Vec<Vec<ValueRef>>, bindings: Vec<u8>) -> Recovery {
    Recovery(Some(Rc::new(Facts::new(aliases, bindings))))
}

#[test]
fn snapshots_and_unchanged_joins_do_not_copy_payload() {
    let test = Test::new("recovery_snapshots", &[Type::U32, Type::U32], &[]);
    let entry = test.function().borrow().entry_block();
    let a = entry.borrow().arguments()[0] as ValueRef;
    let b = entry.borrow().arguments()[1] as ValueRef;
    let state = recovery(alloc::vec![alloc::vec![a, b]; 64], alloc::vec![1; 256]);
    let equal = recovery(alloc::vec![alloc::vec![a, b]; 64], alloc::vec![1; 256]);
    let joined = recovery(alloc::vec![alloc::vec![b]; 64], alloc::vec![3; 256]);
    let empty = recovery(Vec::new(), Vec::new());
    let copies = materialized(|| {
        for _ in 0..32 {
            assert_eq!(state.clone(), equal);
            assert_eq!(state.join(&equal), state);
            assert_eq!(state.join(&joined), joined);
            assert_eq!(joined.join(&state), joined);
            assert_eq!(Recovery::default().join(&state), state);
            assert_eq!(state.join(&Recovery::default()), state);
            assert_eq!(empty.join(&empty), empty);
        }
    });
    assert_eq!(copies, 0, "unchanged snapshots and joins must not rematerialize fact payload");
}

#[test]
fn transfers_copy_only_when_facts_change_and_preserve_predecessors() {
    let mut test = Test::new("recovery_transfers", &[Type::U32, Type::U32], &[Type::U32]);
    let (entry, a, b, local, store_a, store_b, record, ordinary, opaque);
    {
        let mut builder = test.function_builder();
        entry = builder.entry_block();
        a = entry.borrow().arguments()[0] as ValueRef;
        b = entry.borrow().arguments()[1] as ValueRef;
        local = builder.alloc_local(Type::U32);
        store_a = builder.store_local(local, a, SourceSpan::UNKNOWN).unwrap().as_operation_ref();
        store_b = builder.store_local(local, b, SourceSpan::UNKNOWN).unwrap().as_operation_ref();
        record = builder
            .builder_mut()
            .debug_declare(
                Variable::default(),
                Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(0)]),
                SourceSpan::UNKNOWN,
            )
            .unwrap()
            .as_operation_ref();
        let constant = builder.u32(7, SourceSpan::UNKNOWN);
        ordinary = constant.borrow().get_defining_op().unwrap();
        opaque = builder.builder_mut().create::<OpaqueRegion, ()>(SourceSpan::UNKNOWN)()
            .unwrap()
            .as_operation_ref();
        builder.ret([a], SourceSpan::UNKNOWN).unwrap();
    }
    let analysis = LocalRecovery {
        locals: [(local, 0)].into_iter().collect(),
        entry,
        references: alloc::vec![record],
        record_updates: [(record, alloc::vec![(0, 2)])].into_iter().collect(),
        dominance: Rc::new(DominanceInfo::new(test.function().borrow().as_operation())),
    };
    let before = recovery(alloc::vec![alloc::vec![a]; 64], alloc::vec![2]);
    let copies = materialized(|| {
        for _ in 0..32 {
            for op in [ordinary, store_a, record] {
                assert_eq!(analysis.transfer(&op.borrow(), &before), before);
            }
        }
    });
    assert_eq!(copies, 0, "identity and redundant store/record transfers must not copy payload");
    let successor = analysis.transfer(&store_b.borrow(), &before);
    assert_eq!(before.0.as_ref().unwrap().aliases[0], [a]);
    assert_eq!(successor.0.as_ref().unwrap().aliases[0], [b]);
    let inactive = recovery(alloc::vec![alloc::vec![a]; 64], alloc::vec![1]);
    let active = analysis.transfer(&record.borrow(), &inactive);
    assert_eq!(inactive.0.as_ref().unwrap().bindings, [1]);
    assert_eq!(active.0.as_ref().unwrap().bindings, [2]);
    let invalidated = analysis.transfer(&opaque.borrow(), &before);
    assert!(invalidated.0.as_ref().unwrap().aliases.iter().all(Vec::is_empty));
    assert_eq!(invalidated.0.as_ref().unwrap().bindings, [3]);
    assert_eq!(before.0.as_ref().unwrap().aliases[0], [a]);
    assert_eq!(before.0.as_ref().unwrap().bindings, [2]);
    let unknown_inactive = recovery(alloc::vec![Vec::new(); 64], alloc::vec![1]);
    assert_eq!(
        materialized(|| {
            assert_eq!(analysis.transfer(&opaque.borrow(), &invalidated), invalidated);
            assert_eq!(analysis.transfer(&opaque.borrow(), &unknown_inactive), unknown_inactive);
            assert_eq!(
                analysis.transfer(&store_b.borrow(), &Recovery::default()),
                Recovery::default()
            );
        }),
        0,
        "already unknown opaque facts and unreachable points must not copy payload"
    );
}

#[test]
fn branch_joins_preserve_alias_order_binding_union_and_predecessors() {
    let test = Test::new("recovery_joins", &[Type::U32, Type::U32, Type::U32], &[]);
    let entry = test.function().borrow().entry_block();
    let a = entry.borrow().arguments()[0] as ValueRef;
    let b = entry.borrow().arguments()[1] as ValueRef;
    let c = entry.borrow().arguments()[2] as ValueRef;
    let left = recovery(alloc::vec![alloc::vec![a, b, c], alloc::vec![b]], alloc::vec![1, 2]);
    let right = recovery(alloc::vec![alloc::vec![c, a], alloc::vec![c]], alloc::vec![2, 1]);
    let joined = left.join(&right);
    assert_eq!(joined.0.as_ref().unwrap().aliases, [alloc::vec![a, c], Vec::new()]);
    assert_eq!(joined.0.as_ref().unwrap().bindings, [3, 3]);
    assert_eq!(left.0.as_ref().unwrap().aliases, [alloc::vec![a, b, c], alloc::vec![b]]);
    assert_eq!(right.0.as_ref().unwrap().aliases, [alloc::vec![c, a], alloc::vec![c]]);
    assert_eq!(left.0.as_ref().unwrap().bindings, [1, 2]);
    assert_eq!(right.0.as_ref().unwrap().bindings, [2, 1]);
    let reordered = recovery(alloc::vec![alloc::vec![c, a], Vec::new()], alloc::vec![3, 3]);
    assert_eq!(left.join(&reordered), joined, "right reuse must preserve the left alias order");
}

#[test]
fn edge_translation_preserves_snapshots_and_reestablishes_only_mapped_loop_arguments() {
    let mut test = Test::new("recovery_edges", &[Type::U32, Type::U32], &[Type::U32]);
    let (entry, a, b, ret);
    {
        let mut builder = test.function_builder();
        entry = builder.entry_block();
        a = entry.borrow().arguments()[0] as ValueRef;
        b = entry.borrow().arguments()[1] as ValueRef;
        ret = builder.ret([a], SourceSpan::UNKNOWN).unwrap().as_operation_ref();
    }
    let dominance = DominanceInfo::new(test.function().borrow().as_operation());
    let before = recovery(alloc::vec![alloc::vec![a, b]], alloc::vec![2]);
    let point = ProgramPoint::after(ret);
    let copies = materialized(|| {
        for _ in 0..32 {
            assert_eq!(translate(&before, core::iter::empty(), point, &dominance), before);
        }
    });
    assert_eq!(copies, 0, "identity edges must not rematerialize fact payload");
    assert_eq!(
        materialized(|| {
            assert_eq!(
                translate(
                    &before,
                    [(a, a), (b, b)].into_iter(),
                    ProgramPoint::at_start_of(entry),
                    &dominance
                ),
                before
            );
            assert_eq!(
                translate(&Recovery::default(), core::iter::empty(), point, &dominance),
                Recovery::default()
            );
        }),
        0,
        "a mapped reentry which reestablishes the same names must reuse its snapshot"
    );
    let reentry = translate(
        &before,
        [(a, b), (a, b)].into_iter(),
        ProgramPoint::at_start_of(entry),
        &dominance,
    );
    assert_eq!(reentry.0.as_ref().unwrap().aliases[0], [b]);
    assert_eq!(reentry.0.as_ref().unwrap().bindings, [2]);
    let unmapped =
        translate(&before, core::iter::empty(), ProgramPoint::at_start_of(entry), &dominance);
    assert!(unmapped.0.as_ref().unwrap().aliases[0].is_empty());
    assert_eq!(before.0.as_ref().unwrap().aliases[0], [a, b]);
}

#[test]
fn wide_edges_compare_only_relevant_aliases_for_duplicate_targets() {
    use midenc_dialect_cf::ControlFlowOpBuilder;

    let mut measurements = Vec::new();
    for selected in [16, 32] {
        let width = selected * selected;
        let mut test = Test::new("wide_recovery_edge", &[Type::U32, Type::U32], &[Type::U32]);
        let context = test.context_rc();
        let (value, successor, arguments, branch);
        {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            value = entry.borrow().arguments()[0] as ValueRef;
            let unrelated = entry.borrow().arguments()[1] as ValueRef;
            successor = builder.create_block();
            arguments = (0..width)
                .map(|_| context.append_block_argument(successor, Type::U32, SourceSpan::UNKNOWN))
                .collect::<Vec<_>>();
            let operands = core::iter::repeat_n(unrelated, width - selected)
                .chain(core::iter::repeat_n(value, selected));
            branch = builder.br(successor, operands, SourceSpan::UNKNOWN).unwrap();
            builder.switch_to_block(successor);
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }
        let dominance = DominanceInfo::new(test.function().borrow().as_operation());
        let before = recovery(alloc::vec![alloc::vec![value]], alloc::vec![2]);
        let branch = branch.borrow();
        let operands = branch.get_successor_operands(0);
        let mappings = arguments
            .iter()
            .enumerate()
            .map(|(i, target)| (operands.get(i).unwrap().into_value_ref().unwrap(), *target));
        COMPARISONS.set(0);
        let after = translate(&before, mappings, ProgramPoint::at_start_of(successor), &dominance);
        let comparisons = COMPARISONS.get();
        let expected = core::iter::once(value)
            .chain(arguments[width - selected..].iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(after.0.as_ref().unwrap().aliases[0], expected);
        assert_eq!(after.0.as_ref().unwrap().bindings, [2]);
        assert_eq!(before.0.as_ref().unwrap().aliases[0], [value]);
        assert_eq!(before.0.as_ref().unwrap().bindings, [2]);
        let bound = 4 * width + 2 * selected * selected;
        std::eprintln!(
            "wide edge M={width}, K={selected}: {comparisons} comparisons; bound {bound}"
        );
        measurements.push((width, selected, comparisons, bound));
    }
    for (width, selected, comparisons, bound) in measurements {
        assert!(
            comparisons <= bound,
            "wide edge M={width}, K={selected} used {comparisons} comparisons, above the O(M + \
             K^2) bound {bound}"
        );
    }
}
