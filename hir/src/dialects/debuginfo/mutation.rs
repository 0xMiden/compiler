use alloc::{format, rc::Rc, string::String, vec::Vec};
use core::hash::Hasher;

use super::{
    DebugDeclare, DebugKill, DebugValue,
    attributes::{
        Expression, ExpressionOp, FrameBase, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain,
        InlineCallChainAttr, Variable,
    },
};
use crate::{
    BlockRef, Context, Forward, FxHashMap, FxHashSet, FxHasher, Operation, OperationRef, RawWalk,
    RegionRef, Report, SmallVec, Usable, ValueRef,
    dialects::builtin::{Function, attributes::LocalVariableAttr},
    effects::{MemoryEffect, MemoryEffectOpInterface},
    equivalence::{DefaultValueHasher, OperationEquivalenceFlags},
};

#[cfg(test)]
mod tests;

/// The debug-info contract of a rewrite. There is deliberately no default policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugInfoUpdate {
    /// Preserve records, their values, and recovery expressions without moving captures.
    Preserve,
    /// Relocate unchanged captures along an explicitly equivalent control-flow mapping.
    Relocate,
    /// Redirect debug uses to semantically equivalent SSA values without changing expressions.
    Equivalent,
    /// Explicitly rewrite recovery expressions or replace records at their capture points.
    Salvage,
    /// Replace affected locations with unavailable records, retaining their ownership.
    Invalidate,
    /// Discard records inside code the transform has proven unreachable.
    DiscardUnreachable,
}

#[derive(Clone, PartialEq, Eq)]
struct Record {
    fingerprint: u64,
    variable: Variable,
    expression: Option<Expression>,
    inline_calls: Option<InlineCallChain>,
    value: Option<ValueRef>,
    is_kill: bool,
    block: Option<BlockRef>,
    region: Option<RegionRef>,
    function: Option<OperationRef>,
}

impl Record {
    fn read(op: OperationRef) -> Option<Self> {
        let operation = op.borrow();
        let (variable, expression, value, is_kill) =
            if let Some(value) = operation.downcast_ref::<DebugValue>() {
                (
                    value.variable().as_value().clone(),
                    Some(value.expression().as_value().clone()),
                    Some(value.value().as_value_ref()),
                    false,
                )
            } else if let Some(declare) = operation.downcast_ref::<DebugDeclare>() {
                (
                    declare.variable().as_value().clone(),
                    Some(declare.expression().as_value().clone()),
                    None,
                    false,
                )
            } else {
                let kill = operation.downcast_ref::<DebugKill>()?;
                (kill.variable().as_value().clone(), None, None, true)
            };
        Some(Self {
            fingerprint: fingerprint(&operation),
            variable,
            expression,
            value,
            is_kill,
            inline_calls: operation
                .get_typed_attribute::<InlineCallChainAttr>(INLINE_CALL_CHAIN_ATTR_NAME)
                .map(|chain| chain.borrow().as_value().clone()),
            block: operation.parent(),
            region: operation.parent_region(),
            function: enclosing_function(op),
        })
    }

    fn same_binding(&self, other: &Self) -> bool {
        self.variable == other.variable && self.inline_calls == other.inline_calls
    }

    fn local_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.expression.iter().flat_map(|expression| {
            expression.operations.iter().filter_map(|operation| match operation {
                ExpressionOp::LocalSlot(index)
                | ExpressionOp::FrameBase {
                    base: FrameBase::LocalSlot(index),
                    ..
                } => Some(*index),
                _ => None,
            })
        })
    }

    fn uses_definition(&self, op: OperationRef) -> bool {
        self.value.is_some_and(|value| value.borrow().get_defining_op() == Some(op))
    }
}

fn fingerprint(operation: &Operation) -> u64 {
    let mut hasher = FxHasher::default();
    operation.hash_with_options(
        OperationEquivalenceFlags::NONE,
        DefaultValueHasher,
        DefaultValueHasher,
        &mut hasher,
    );
    hasher.finish()
}

fn enclosing_function(op: OperationRef) -> Option<OperationRef> {
    let mut current = Some(op);
    while let Some(operation) = current {
        if operation.borrow().is::<Function>() {
            return Some(operation);
        }
        current = operation.parent_op();
    }
    None
}

fn is_under(op: OperationRef, root: OperationRef) -> bool {
    let mut current = Some(op);
    while let Some(operation) = current {
        if operation == root {
            return true;
        }
        current = operation.parent_op();
    }
    false
}

fn records_under(root: OperationRef) -> FxHashMap<OperationRef, Record> {
    let mut records = FxHashMap::default();
    root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        if let Some(record) = Record::read(op) {
            records.insert(op, record);
        }
    });
    records
}

struct Dependency {
    fingerprint: Option<u64>,
    records: SmallVec<[OperationRef; 2]>,
    local: Option<(OperationRef, u32)>,
}

type LocalSources = FxHashMap<(OperationRef, u32), SmallVec<[(OperationRef, u64); 2]>>;
type LocalRecords = FxHashMap<(OperationRef, u32), FxHashSet<OperationRef>>;

fn index_local_sources(root: OperationRef) -> LocalSources {
    let mut sources = LocalSources::default();
    root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        let operation = op.borrow();
        if operation.has_regions() {
            return;
        }
        let Some(effects) = operation.as_trait::<dyn MemoryEffectOpInterface>() else {
            return;
        };
        let Some(function) = enclosing_function(op) else {
            return;
        };
        for effect in effects.effects() {
            if !matches!(effect.effect(), MemoryEffect::Write | MemoryEffect::Free) {
                continue;
            }
            let Some(local) = effect
                .attribute()
                .and_then(|attribute| attribute.try_downcast_attr::<LocalVariableAttr>().ok())
            else {
                continue;
            };
            let index = local.borrow().as_value().as_usize() as u32;
            sources
                .entry((function, index))
                .or_default()
                .push((op, fingerprint(&operation)));
        }
    });
    sources
}

struct Audit {
    root: OperationRef,
    records: FxHashMap<OperationRef, Record>,
    dependencies: FxHashMap<OperationRef, Dependency>,
    local_sources: Option<LocalSources>,
    local_records: LocalRecords,
    approved_sources: FxHashMap<OperationRef, u64>,
    errors: Vec<String>,
}

impl Audit {
    fn new(root: OperationRef, records: FxHashMap<OperationRef, Record>) -> Self {
        let mut dependencies = FxHashMap::<OperationRef, Dependency>::default();
        let mut local_records = LocalRecords::default();
        for (&op, record) in &records {
            if let Some(value) = record.value
                && let Some(definition) = value.borrow().get_defining_op()
            {
                dependencies
                    .entry(definition)
                    .or_insert_with(|| Dependency {
                        fingerprint: Some(fingerprint(&definition.borrow())),
                        records: SmallVec::new(),
                        local: None,
                    })
                    .records
                    .push(op);
            }
            if let Some(function) = enclosing_function(op) {
                for index in record.local_slots() {
                    local_records.entry((function, index)).or_default().insert(op);
                }
            }
        }
        let local_sources = (!local_records.is_empty()).then(|| index_local_sources(root));
        if let Some(sources) = &local_sources {
            for (&local, writes) in sources {
                if local_records.contains_key(&local) {
                    for &(op, fingerprint) in writes {
                        dependencies
                            .entry(op)
                            .or_insert_with(|| Dependency {
                                fingerprint: Some(fingerprint),
                                records: SmallVec::new(),
                                local: Some(local),
                            })
                            .local = Some(local);
                    }
                }
            }
        }
        Self {
            root,
            records,
            dependencies,
            local_sources,
            local_records,
            approved_sources: FxHashMap::default(),
            errors: Vec::new(),
        }
    }

    fn has_obligation(&self, op: OperationRef) -> bool {
        self.records.contains_key(&op)
            || self.dependencies.get(&op).is_some_and(|dependency| {
                dependency
                    .local
                    .and_then(|local| self.local_records.get(&local))
                    .is_some_and(|records| !records.is_empty())
                    || dependency.records.iter().any(|record| {
                        self.records.get(record).is_some_and(|record| record.uses_definition(op))
                    })
            })
    }

    fn register(&mut self, op: OperationRef, record: Record) {
        self.unregister(op);
        if let Some(value) = record.value
            && let Some(definition) = value.borrow().get_defining_op()
        {
            let dependency = self.dependencies.entry(definition).or_insert_with(|| Dependency {
                fingerprint: Some(fingerprint(&definition.borrow())),
                records: SmallVec::new(),
                local: None,
            });
            if !dependency.records.contains(&op) {
                dependency.records.push(op);
            }
        }
        if let Some(function) = record.function {
            for index in record.local_slots() {
                let local = (function, index);
                if !self.local_records.contains_key(&local) {
                    let sources =
                        self.local_sources.get_or_insert_with(|| index_local_sources(self.root));
                    for &(source, fingerprint) in sources.get(&local).into_iter().flatten() {
                        self.dependencies
                            .entry(source)
                            .or_insert_with(|| Dependency {
                                fingerprint: Some(fingerprint),
                                records: SmallVec::new(),
                                local: Some(local),
                            })
                            .local = Some(local);
                    }
                }
                self.local_records.entry(local).or_default().insert(op);
            }
        }
        self.records.insert(op, record);
    }

    fn unregister(&mut self, op: OperationRef) {
        if let Some(record) = self.records.remove(&op)
            && let Some(function) = record.function
        {
            for index in record.local_slots() {
                self.local_records.get_mut(&(function, index)).unwrap().remove(&op);
            }
        }
    }
}

struct Permission {
    operations: FxHashSet<OperationRef>,
    audit_depth: usize,
    new_records: FxHashMap<OperationRef, Option<OperationRef>>,
    inserted: FxHashMap<OperationRef, Option<OperationRef>>,
    update: DebugInfoUpdate,
}

#[derive(Default)]
pub(crate) struct DebugInfoMutationState {
    audits: Vec<Audit>,
    permissions: Vec<Permission>,
}

impl DebugInfoMutationState {
    pub(crate) fn inserted(&mut self, op: OperationRef) {
        let origin = op
            .next()
            .filter(|next| self.audits.iter().any(|audit| audit.records.contains_key(next)));
        for permission in &mut self.permissions {
            permission.inserted.insert(op, origin);
        }
    }

    pub(crate) fn removed(&mut self, op: OperationRef) {
        self.require_intent(op, "removing or moving");
    }

    pub(crate) fn implicit_erase(&mut self, op: OperationRef) {
        self.require_intent(op, "erasing");
    }

    fn require_intent(&mut self, op: OperationRef, action: &str) {
        for (index, audit) in self.audits.iter_mut().enumerate() {
            if audit.has_obligation(op)
                && !self.permissions.iter().any(|permission| {
                    permission.audit_depth > index
                        && permission.operations.contains(&op)
                        && (!audit.records.contains_key(&op)
                            || !matches!(
                                permission.update,
                                DebugInfoUpdate::Preserve | DebugInfoUpdate::Equivalent
                            ))
                })
            {
                audit.errors.push(format!(
                    "unhandled debug-info mutation: {action} '{}' requires an explicit \
                     DebugInfoUpdate",
                    op.name()
                ));
            }
        }
    }
}

/// Declare the debug-info contract for one rewrite, including its affected debug users.
///
/// Keep this guard alive for exactly that rewrite, not for a whole pass. Raw edits outside an
/// acknowledged rewrite are diagnosed even when ordinary IR verification is disabled.
#[must_use]
pub struct DebugInfoMutation {
    context: Rc<Context>,
    update: DebugInfoUpdate,
    records: FxHashMap<OperationRef, Record>,
    descendants: FxHashSet<OperationRef>,
    dependencies: FxHashSet<OperationRef>,
    discard_roots: SmallVec<[OperationRef; 2]>,
    discard_blocks: SmallVec<[BlockRef; 2]>,
    active: bool,
}

impl DebugInfoMutation {
    pub fn new(root: OperationRef, update: DebugInfoUpdate) -> Self {
        Self::for_operations(&[root], update)
    }

    /// Declare a shared contract for a rewrite, including affected result users.
    pub fn for_operations(roots: &[OperationRef], update: DebugInfoUpdate) -> Self {
        let root = *roots.first().expect("a debug-info mutation must name its rewrite roots");
        let context = root.borrow().context_rc();
        let active = !context.debug_info_mutations.borrow().audits.is_empty();
        let mut mutation = Self {
            context,
            update,
            records: FxHashMap::default(),
            descendants: FxHashSet::default(),
            dependencies: FxHashSet::default(),
            discard_roots: SmallVec::new(),
            discard_blocks: SmallVec::new(),
            active,
        };
        if !active {
            return mutation;
        }
        for root in roots {
            if root.borrow().has_regions() && Record::read(*root).is_none() {
                mutation.discard_roots.push(*root);
            }
            root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
                mutation.descendants.insert(op);
            });
        }
        let users = mutation
            .descendants
            .iter()
            .flat_map(|operation| {
                operation
                    .borrow()
                    .results()
                    .iter()
                    .flat_map(|result| {
                        result.borrow().iter_uses().map(|operand| operand.owner).collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let affected = mutation.descendants.iter().copied().chain(users);
        let mut operations = mutation.descendants.clone();
        {
            let state = mutation.context.debug_info_mutations.borrow();
            for audit in &state.audits {
                let mut visited_locals = FxHashSet::default();
                for op in affected.clone() {
                    if let Some(record) = audit.records.get(&op) {
                        mutation.records.insert(op, record.clone());
                        operations.insert(op);
                    }
                    if let Some(dependency) = audit.dependencies.get(&op) {
                        mutation.dependencies.insert(op);
                        operations.insert(op);
                        for record in dependency.records.iter().chain(
                            dependency
                                .local
                                .filter(|local| visited_locals.insert(*local))
                                .and_then(|local| audit.local_records.get(&local))
                                .into_iter()
                                .flatten(),
                        ) {
                            if let Some(before) = audit.records.get(record) {
                                mutation.records.insert(*record, before.clone());
                                operations.insert(*record);
                            }
                        }
                    }
                }
            }
        }
        let mut state = mutation.context.debug_info_mutations.borrow_mut();
        let audit_depth = state.audits.len();
        let authorized = (0..audit_depth)
            .map(|index| {
                state
                    .permissions
                    .iter()
                    .filter(|permission| permission.audit_depth > index)
                    .flat_map(|permission| permission.operations.iter().copied())
                    .collect::<FxHashSet<_>>()
            })
            .collect::<Vec<_>>();
        for (index, audit) in state.audits.iter_mut().enumerate() {
            for (&op, before) in &mutation.records {
                if !authorized[index].contains(&op) && Record::read(op).as_ref() != Some(before) {
                    audit.errors.push(format!(
                        "unhandled debug-info mutation for variable '{}': the record changed \
                         before its DebugInfoUpdate was declared",
                        before.variable.name
                    ));
                }
            }
            for op in &mutation.dependencies {
                if !authorized[index].contains(op)
                    && let Some(dependency) = audit.dependencies.get(op)
                    && audit.has_obligation(*op)
                    && is_under(*op, audit.root).then(|| fingerprint(&op.borrow()))
                        != dependency.fingerprint
                {
                    audit.errors.push(format!(
                        "unhandled debug-info dependency mutation in '{}': the source changed \
                         before its DebugInfoUpdate was declared",
                        op.name()
                    ));
                }
            }
        }
        state.permissions.push(Permission {
            operations,
            audit_depth,
            new_records: FxHashMap::default(),
            inserted: FxHashMap::default(),
            update,
        });
        drop(state);
        mutation
    }

    /// Declare how a value replacement handles debug uses and affected value producers.
    pub fn for_value(value: ValueRef, update: DebugInfoUpdate) -> Option<Self> {
        let users = value
            .borrow()
            .iter_uses()
            .map(|operand| operand.owner)
            .collect::<SmallVec<[OperationRef; 4]>>();
        (!users.is_empty()).then(|| Self::for_operations(&users, update))
    }

    /// Declare the capture mapping for the blocks participating in one control-flow rewrite.
    pub fn for_blocks(blocks: &[BlockRef], update: DebugInfoUpdate) -> Option<Self> {
        let operations = blocks
            .iter()
            .flat_map(|block| {
                block.borrow().body().iter().map(|op| op.as_operation_ref()).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        (!operations.is_empty()).then(|| {
            let mut mutation = Self::for_operations(&operations, update);
            if update == DebugInfoUpdate::DiscardUnreachable {
                mutation.discard_blocks.extend_from_slice(blocks);
            }
            mutation
        })
    }
}

impl Drop for DebugInfoMutation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let (permission, root) = {
            let mut state = self.context.debug_info_mutations.borrow_mut();
            (state.permissions.pop().unwrap(), state.audits.last().unwrap().root)
        };
        let mut replacement_origins = permission.inserted.clone();
        replacement_origins
            .extend(permission.new_records.iter().map(|(&op, &origin)| (op, origin)));
        let replacements = replacement_origins
            .keys()
            .filter_map(|&op| {
                is_under(op, root)
                    .then(|| Record::read(op).map(|record| (op, record)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let mut updates = Vec::new();
        let mut errors = Vec::new();
        for (&op, before) in &self.records {
            let after = is_under(op, root).then(|| Record::read(op)).flatten();
            let valid = match (&after, self.update) {
                (Some(after), DebugInfoUpdate::Preserve) => before == after,
                (Some(after), DebugInfoUpdate::Equivalent) => {
                    before.same_binding(after)
                        && before.expression == after.expression
                        && before.is_kill == after.is_kill
                        && before.block == after.block
                        && before.region == after.region
                        && before.function == after.function
                }
                (Some(after), DebugInfoUpdate::Relocate) => {
                    before.same_binding(after)
                        && before.expression == after.expression
                        && before.is_kill == after.is_kill
                }
                (Some(after), DebugInfoUpdate::Salvage) => before.same_binding(after),
                (Some(after), DebugInfoUpdate::Invalidate) => {
                    before.same_binding(after) && after.is_kill
                }
                (None, DebugInfoUpdate::DiscardUnreachable) => {
                    self.descendants.contains(&op)
                        && (self.discard_roots.iter().any(|discarded| !is_under(*discarded, root))
                            || self.discard_blocks.iter().any(|block| block.parent().is_none()))
                }
                (None, DebugInfoUpdate::Salvage | DebugInfoUpdate::Invalidate) => {
                    replacements.iter().any(|(replacement_op, replacement)| {
                        before.same_binding(replacement)
                            && replacement_origins.get(replacement_op) == Some(&Some(op))
                            && (self.update != DebugInfoUpdate::Invalidate || replacement.is_kill)
                    })
                }
                _ => false,
            };
            if valid {
                updates.push((op, after));
            } else {
                errors.push(format!(
                    "debug-info {:?} contract failed for variable '{}' in '{}'",
                    self.update,
                    before.variable.name,
                    op.name()
                ));
            }
        }
        let sources = self
            .dependencies
            .iter()
            .map(|&op| (op, is_under(op, root).then(|| fingerprint(&op.borrow()))))
            .collect::<Vec<_>>();
        let inserted = permission
            .inserted
            .keys()
            .filter(|&&op| is_under(op, root))
            .map(|&op| (op, fingerprint(&op.borrow())))
            .collect::<Vec<_>>();
        let mut state = self.context.debug_info_mutations.borrow_mut();
        for audit in &mut state.audits {
            audit.errors.extend(errors.iter().cloned());
            for (op, record) in &updates {
                if let Some(record) = record {
                    audit.register(*op, record.clone());
                } else {
                    audit.unregister(*op);
                }
            }
            for (op, record) in &replacements {
                audit.register(*op, record.clone());
            }
            for (op, fingerprint) in &sources {
                if let Some(dependency) = audit.dependencies.get_mut(op) {
                    dependency.fingerprint = *fingerprint;
                }
            }
            audit.approved_sources.extend(inserted.iter().copied());
        }
    }
}

pub(crate) fn register_debug_record(op: OperationRef) {
    let context = op.borrow().context_rc();
    let mut state = context.debug_info_mutations.borrow_mut();
    if state.audits.is_empty() {
        return;
    }
    let record = Record::read(op).unwrap();
    let origin = op
        .next()
        .filter(|next| state.audits.iter().any(|audit| audit.records.contains_key(next)));
    for permission in &mut state.permissions {
        permission.new_records.insert(op, origin);
    }
    for audit in &mut state.audits {
        if is_under(op, audit.root) {
            audit.register(op, record.clone());
        }
    }
}

pub(crate) struct DebugInfoAudit {
    context: Rc<Context>,
    active: bool,
}

impl DebugInfoAudit {
    pub(crate) fn begin(root: OperationRef) -> Self {
        let context = root.borrow().context_rc();
        let records = records_under(root);
        context.debug_info_mutations.borrow_mut().audits.push(Audit::new(root, records));
        Self {
            context,
            active: true,
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), Report> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let mut audit = self.context.debug_info_mutations.borrow_mut().audits.pop().unwrap();
        let actual = records_under(audit.root);
        for (op, expected) in &audit.records {
            if actual.get(op) != Some(expected) {
                audit.errors.push(format!(
                    "unhandled debug-info mutation for variable '{}': record changed, moved, or \
                     disappeared without an explicit DebugInfoUpdate",
                    expected.variable.name
                ));
            }
        }
        for (op, record) in &actual {
            if !audit.records.contains_key(op) {
                audit.errors.push(format!(
                    "unhandled debug-info insertion for variable '{}': use DIBuilder or an \
                     explicit DebugInfoUpdate",
                    record.variable.name
                ));
            }
        }
        for (op, dependency) in &audit.dependencies {
            if !audit.has_obligation(*op) {
                continue;
            }
            let actual = is_under(*op, audit.root).then(|| fingerprint(&op.borrow()));
            if actual != dependency.fingerprint {
                audit.errors.push(format!(
                    "unhandled debug-info dependency mutation in '{}': a tracked value or local \
                     location changed without an explicit DebugInfoUpdate",
                    op.name()
                ));
            }
        }
        let current = Audit::new(audit.root, actual);
        for (op, dependency) in current.dependencies {
            if dependency.local.is_some()
                && !audit.dependencies.contains_key(&op)
                && audit.approved_sources.get(&op).copied() != dependency.fingerprint
            {
                audit.errors.push(format!(
                    "unhandled debug-info storage insertion in '{}': a tracked local write \
                     requires an explicit DebugInfoUpdate",
                    op.name()
                ));
            }
        }
        audit.errors.sort();
        audit.errors.dedup();
        if audit.errors.is_empty() {
            Ok(())
        } else {
            Err(Report::msg(audit.errors.join("\n")))
        }
    }
}

impl Drop for DebugInfoAudit {
    fn drop(&mut self) {
        if self.active {
            self.context.debug_info_mutations.borrow_mut().audits.pop();
        }
    }
}
