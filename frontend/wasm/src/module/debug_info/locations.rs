use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use midenc_hir::{
    dialects::debuginfo::attributes::{Expression, ExpressionOp},
    interner::Symbol,
};

use super::{
    LocalDebugInfo, LocationDescriptor, LocationScheduleEntry, is_supported_location_expression,
};

pub(super) fn normalize_locations(locations: Vec<LocationDescriptor>) -> Vec<LocationDescriptor> {
    let mut changes: BTreeMap<u64, Vec<(usize, bool)>> = BTreeMap::new();
    for (index, location) in locations.iter().enumerate() {
        if location.end.is_some_and(|end| end <= location.start) {
            continue;
        }
        changes.entry(location.start).or_default().push((index, true));
        if let Some(end) = location.end {
            changes.entry(end).or_default().push((index, false));
        }
    }
    let mut active = BTreeSet::new();
    let mut result: Vec<LocationDescriptor> = Vec::new();
    let mut changes = changes.into_iter().peekable();
    while let Some((start, updates)) = changes.next() {
        for (index, available) in updates {
            if available {
                active.insert(index);
            } else {
                active.remove(&index);
            }
        }
        if active.is_empty() {
            continue;
        }
        let storage = if active.len() == 1 {
            locations[*active.first().unwrap()].storage.clone()
        } else {
            Expression::default()
        };
        let end = changes.peek().map(|(offset, _)| *offset);
        if let Some(previous) = result.last_mut()
            && previous.end == Some(start)
            && previous.storage == storage
            && !is_transient(&storage)
        {
            previous.end = end;
        } else {
            result.push(LocationDescriptor {
                start,
                end,
                storage,
            });
        }
    }
    result
}

fn location_at(info: &LocalDebugInfo, offset: u64) -> Option<&Expression> {
    let index = info.locations.partition_point(|location| location.start <= offset);
    info.locations
        .get(index.wrapping_sub(1))
        .filter(|location| location.end.is_none_or(|end| offset < end))
        .map(|location| &location.storage)
}

pub(crate) fn is_transient(expression: &Expression) -> bool {
    expression
        .operations
        .iter()
        .any(|operation| matches!(operation, ExpressionOp::OperandStackSlot(_)))
}

pub(super) fn build_location_schedule(
    locals: &mut [Option<LocalDebugInfo>],
) -> Vec<LocationScheduleEntry> {
    let mut names: BTreeMap<Symbol, Vec<usize>> = BTreeMap::new();
    for (index, local) in locals.iter_mut().enumerate() {
        let Some(local) = local else { continue };
        local.locations = normalize_locations(core::mem::take(&mut local.locations));
        if local.scope.is_some() || !local.locations.is_empty() {
            names.entry(local.attr.name).or_default().push(index);
        }
    }
    let mut schedule = Vec::new();
    for indices in names.values() {
        let mut changes: BTreeMap<u64, BTreeSet<usize>> = BTreeMap::new();
        for &index in indices {
            let local = locals[index].as_ref().unwrap();
            if let Some(scope) = &local.scope {
                for range in &scope.ranges {
                    changes.entry(range.start).or_default().insert(index);
                    if let Some(end) = range.end {
                        changes.entry(end).or_default().insert(index);
                    }
                }
            }
            for location in &local.locations {
                changes.entry(location.start).or_default().insert(index);
                if let Some(end) = location.end {
                    changes.entry(end).or_default().insert(index);
                }
            }
        }
        let mut active: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut previous = None;
        for (offset, updates) in changes {
            for index in updates {
                let local = locals[index].as_ref().unwrap();
                let depth = local.scope.as_ref().map_or(0, |scope| scope.depth);
                let visible = local.scope.as_ref().map_or_else(
                    || location_at(local, offset).is_some(),
                    |scope| scope.contains(offset),
                );
                if visible {
                    active.entry(depth).or_default().insert(index);
                } else if let Some(bindings) = active.get_mut(&depth) {
                    bindings.remove(&index);
                    if bindings.is_empty() {
                        active.remove(&depth);
                    }
                }
            }
            let current = active.last_key_value().map(|(_, innermost)| {
                let index = *innermost.first().unwrap();
                let local = locals[index].as_ref().unwrap();
                let unambiguous = innermost.len() == 1
                    && active.values().flatten().all(|other| {
                        let other = locals[*other].as_ref().unwrap();
                        match (&other.scope, &local.scope) {
                            (Some(outer), Some(inner)) => outer.is_ancestor_of(inner),
                            (None, _) => true,
                            (Some(_), None) => false,
                        }
                    });
                let storage = unambiguous
                    .then(|| location_at(local, offset))
                    .flatten()
                    .filter(|storage| is_supported_location_expression(storage))
                    .cloned();
                (index, storage)
            });
            let refresh = current.as_ref().is_some_and(|(index, storage)| {
                let local = locals[*index].as_ref().unwrap();
                let location = local.locations.partition_point(|location| location.start <= offset);
                storage.as_ref().is_some_and(is_transient)
                    && local
                        .locations
                        .get(location.wrapping_sub(1))
                        .is_some_and(|location| location.start == offset)
            });
            if previous == current && !refresh {
                continue;
            }
            if let Some((var_index, _)) = previous {
                schedule.push(LocationScheduleEntry {
                    offset,
                    var_index,
                    storage: None,
                });
            }
            if let Some((var_index, storage)) = &current
                && (storage.is_some() || previous.is_none())
            {
                schedule.push(LocationScheduleEntry {
                    offset,
                    var_index: *var_index,
                    storage: storage.clone(),
                });
            }
            previous = current;
        }
    }
    schedule.sort_by_key(|entry| (entry.offset, entry.storage.is_some()));
    schedule
}

pub(crate) struct LocationIndex {
    names: BTreeMap<Symbol, Vec<usize>>,
    transient: BTreeSet<Symbol>,
}

impl LocationIndex {
    pub fn new(schedule: &[LocationScheduleEntry], locals: &[Option<LocalDebugInfo>]) -> Self {
        let mut names: BTreeMap<Symbol, Vec<usize>> = BTreeMap::new();
        let mut transient = BTreeSet::new();
        for (index, entry) in schedule.iter().enumerate() {
            let name = locals[entry.var_index].as_ref().unwrap().attr.name;
            names.entry(name).or_default().push(index);
            if entry.storage.as_ref().is_some_and(is_transient) {
                transient.insert(name);
            }
        }
        Self { names, transient }
    }

    fn state(
        &self,
        name: Symbol,
        offset: u64,
        schedule: &[LocationScheduleEntry],
    ) -> LocationScheduleEntry {
        let entries = &self.names[&name];
        let index = entries.partition_point(|index| schedule[*index].offset <= offset);
        if let Some(index) = index.checked_sub(1) {
            let mut entry = schedule[entries[index]].clone();
            entry.offset = offset;
            entry
        } else {
            LocationScheduleEntry {
                offset,
                var_index: schedule[entries[0]].var_index,
                storage: None,
            }
        }
    }

    pub fn repairs(
        &self,
        source: Option<u64>,
        target: u64,
        schedule: &[LocationScheduleEntry],
        locals: &[Option<LocalDebugInfo>],
    ) -> Vec<LocationScheduleEntry> {
        let mut names = BTreeSet::new();
        if let Some(source) = source {
            let start = schedule.partition_point(|entry| entry.offset <= source.min(target));
            let end = schedule.partition_point(|entry| entry.offset <= source.max(target));
            for entry in &schedule[start..end] {
                names.insert(locals[entry.var_index].as_ref().unwrap().attr.name);
            }
        } else {
            names.extend(self.names.keys().copied());
        }
        names.extend(self.transient.iter().copied().filter(|name| {
            self.state(*name, target, schedule).storage.as_ref().is_some_and(is_transient)
        }));
        let mut repairs = Vec::new();
        for name in names {
            let mut desired = self.state(name, target, schedule);
            let transient = desired.storage.as_ref().is_some_and(is_transient);
            let current = source.map(|source| self.state(name, source, schedule));
            if let Some(current) = &current
                && current.var_index != desired.var_index
            {
                repairs.push(LocationScheduleEntry {
                    offset: target,
                    var_index: current.var_index,
                    storage: None,
                });
            }
            let changed = current.is_none_or(|current| {
                current.storage != desired.storage
                    || (desired.storage.is_some() && current.var_index != desired.var_index)
            });
            if changed || transient {
                if transient {
                    desired.storage = None;
                }
                repairs.push(desired);
            }
        }
        repairs
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use super::*;
    use crate::module::debug_info::{
        Variable,
        scope::{ScopeRange, VariableScope},
    };

    fn scope(start: u64, end: u64, parent: Option<Rc<VariableScope>>) -> Rc<VariableScope> {
        VariableScope::new(
            vec![ScopeRange {
                start,
                end: Some(end),
            }],
            parent,
        )
    }

    fn binding(scope: Rc<VariableScope>, value: Option<u64>) -> Option<LocalDebugInfo> {
        Some(LocalDebugInfo {
            inline_calls: Vec::new(),
            attr: Variable::new(Symbol::intern("x"), Symbol::intern("scope.rs"), 1, None),
            locations: value
                .map(|value| LocationDescriptor {
                    start: 0,
                    end: None,
                    storage: Expression::with_ops(vec![
                        ExpressionOp::ConstU64(value),
                        ExpressionOp::StackValue,
                    ]),
                })
                .into_iter()
                .collect(),
            expression: None,
            scope: Some(scope),
        })
    }

    fn values_at(schedule: &[LocationScheduleEntry], offset: u64) -> Vec<(usize, bool)> {
        schedule
            .iter()
            .filter(|entry| entry.offset == offset)
            .map(|entry| (entry.var_index, entry.storage.is_some()))
            .collect()
    }

    #[test]
    fn leaving_an_inner_scope_restores_the_outer_binding() {
        let outer = scope(0, 100, None);
        let inner = scope(10, 20, Some(outer.clone()));
        let mut locals = vec![binding(outer, Some(15)), binding(inner, Some(25))];
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(values_at(&schedule, 0), [(0, true)]);
        assert_eq!(values_at(&schedule, 10), [(0, false), (1, true)]);
        assert_eq!(values_at(&schedule, 20), [(1, false), (0, true)]);
        assert_eq!(values_at(&schedule, 100), [(0, false)]);
    }

    #[test]
    fn an_unavailable_inner_binding_still_hides_the_outer_binding() {
        let outer = scope(0, 100, None);
        let inner = scope(10, 20, Some(outer.clone()));
        let mut locals = vec![binding(outer, Some(15)), binding(inner, None)];
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(values_at(&schedule, 10), [(0, false)]);
        assert_eq!(values_at(&schedule, 20), [(1, false), (0, true)]);
        assert!(!schedule.iter().any(|entry| entry.var_index == 1 && entry.storage.is_some()));
    }

    #[test]
    fn overlapping_sibling_scopes_do_not_invent_a_shadow_order() {
        let outer = scope(0, 100, None);
        let first = scope(10, 30, Some(outer.clone()));
        let second = scope(20, 40, Some(outer.clone()));
        let mut locals =
            vec![binding(outer, Some(1)), binding(first, Some(2)), binding(second, Some(3))];
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(values_at(&schedule, 20), [(1, false)]);
        assert_eq!(values_at(&schedule, 30), [(1, false), (2, true)]);
        assert_eq!(values_at(&schedule, 40), [(2, false), (0, true)]);
    }

    #[test]
    fn discontiguous_scope_fragments_restore_the_outer_binding_in_gaps() {
        let outer = scope(0, 100, None);
        let inner = VariableScope::new(
            vec![
                ScopeRange {
                    start: 10,
                    end: Some(20),
                },
                ScopeRange {
                    start: 30,
                    end: Some(40),
                },
            ],
            Some(outer.clone()),
        );
        let mut locals = vec![binding(outer, Some(15)), binding(inner, Some(25))];
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(values_at(&schedule, 20), [(1, false), (0, true)]);
        assert_eq!(values_at(&schedule, 30), [(0, false), (1, true)]);
        assert_eq!(values_at(&schedule, 40), [(1, false), (0, true)]);
    }

    #[test]
    fn overlapping_location_descriptions_are_unavailable() {
        let locations = normalize_locations(vec![
            LocationDescriptor {
                start: 0,
                end: Some(20),
                storage: Expression::with_ops(vec![ExpressionOp::ConstU64(1)]),
            },
            LocationDescriptor {
                start: 10,
                end: Some(30),
                storage: Expression::with_ops(vec![ExpressionOp::ConstU64(2)]),
            },
        ]);
        assert_eq!(locations.len(), 3);
        assert_eq!((locations[1].start, locations[1].end), (10, Some(20)));
        assert!(locations[1].storage.is_empty());
        assert_eq!(locations[2].storage.operations, [ExpressionOp::ConstU64(2)]);
    }

    #[test]
    fn adjacent_stack_locations_capture_again_at_the_boundary() {
        let storage =
            Expression::with_ops(vec![ExpressionOp::OperandStackSlot(0), ExpressionOp::StackValue]);
        let mut locals = vec![Some(LocalDebugInfo {
            inline_calls: Vec::new(),
            attr: Variable::new(Symbol::intern("x"), Symbol::intern("stack.rs"), 1, None),
            locations: vec![
                LocationDescriptor {
                    start: 0,
                    end: Some(10),
                    storage: storage.clone(),
                },
                LocationDescriptor {
                    start: 10,
                    end: Some(20),
                    storage,
                },
            ],
            expression: None,
            scope: None,
        })];
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(values_at(&schedule, 10), [(0, false), (0, true)]);
    }

    #[test]
    fn many_disjoint_scopes_keep_each_declaration_separate() {
        let outer = scope(0, 3001, None);
        let mut locals = vec![binding(outer.clone(), Some(0))];
        for index in 0..1000 {
            locals.push(binding(
                scope(index * 3 + 1, index * 3 + 2, Some(outer.clone())),
                Some(index + 1),
            ));
        }
        let schedule = build_location_schedule(&mut locals);
        assert_eq!(schedule.len(), 4002);
        assert_eq!(values_at(&schedule, 2999), [(1000, false), (0, true)]);
    }

    #[test]
    fn edge_repairs_invalidate_skipped_updates_and_loop_backedges() {
        let outer = scope(0, 100, None);
        let mut locals = vec![binding(outer, Some(7))];
        locals[0].as_mut().unwrap().locations[0].end = Some(10);
        let schedule = build_location_schedule(&mut locals);
        let index = LocationIndex::new(&schedule, &locals);
        for (source, target, available) in [(5, 15, false), (15, 5, true)] {
            let repairs = index.repairs(Some(source), target, &schedule, &locals);
            assert_eq!(repairs.len(), 1);
            assert_eq!(repairs[0].storage.is_some(), available);
        }
        assert!(index.repairs(Some(15), 20, &schedule, &locals).is_empty());
    }

    #[test]
    fn edge_repairs_kill_the_departing_binding_when_crossing_a_shadow_boundary() {
        let outer = scope(0, 100, None);
        let inner = scope(20, 40, Some(outer.clone()));
        let mut locals = vec![binding(outer, Some(7)), binding(inner, Some(7))];
        locals[1].as_mut().unwrap().attr.line = 2;
        let schedule = build_location_schedule(&mut locals);
        let index = LocationIndex::new(&schedule, &locals);
        for (source, target, expected) in
            [(30, 50, vec![(1, false), (0, true)]), (50, 30, vec![(0, false), (1, true)])]
        {
            let repairs = index.repairs(Some(source), target, &schedule, &locals);
            assert_eq!(values_at(&repairs, target), expected);
        }
    }

    #[test]
    fn stable_locations_are_not_repeated_on_every_cfg_edge() {
        let mut locals = vec![binding(scope(0, 100, None), Some(7))];
        let schedule = build_location_schedule(&mut locals);
        let index = LocationIndex::new(&schedule, &locals);
        assert!(index.repairs(Some(1), 90, &schedule, &locals).is_empty());
        assert!(index.repairs(Some(90), 1, &schedule, &locals).is_empty());
    }

    #[test]
    fn joins_do_not_reuse_captured_operand_stack_values() {
        let mut locals = vec![Some(LocalDebugInfo {
            inline_calls: Vec::new(),
            attr: Variable::new(Symbol::intern("x"), Symbol::intern("stack.rs"), 1, None),
            locations: vec![LocationDescriptor {
                start: 0,
                end: Some(100),
                storage: Expression::with_ops(vec![
                    ExpressionOp::OperandStackSlot(0),
                    ExpressionOp::StackValue,
                ]),
            }],
            expression: None,
            scope: None,
        })];
        let schedule = build_location_schedule(&mut locals);
        let index = LocationIndex::new(&schedule, &locals);
        let repairs = index.repairs(Some(5), 10, &schedule, &locals);
        assert_eq!(repairs.len(), 1);
        assert!(repairs[0].storage.is_none());
    }
}
