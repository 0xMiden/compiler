use alloc::{format, rc::Rc, string::String, vec::Vec};
use core::hash::Hasher;

use super::{
    DebugDeclare, DebugKill, DebugValue,
    attributes::{Expression, ExpressionOp, FrameBase},
    bindings::{BindingIdentity, enclosing_function},
};
use crate::{
    BlockRef, Context, Forward, FxHashMap, FxHashSet, FxHasher, Operation, OperationRef, RawWalk,
    RegionRef, Report, SmallVec, Usable, ValueRef,
    dialects::builtin::attributes::LocalVariableAttr,
    effects::{MemoryEffect, MemoryEffectOpInterface},
    equivalence::{DefaultValueHasher, OperationEquivalenceFlags},
};

mod observed;

use self::observed::ObservedCaptures;

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
    /// Surviving dependent records must remain unchanged at their capture points.
    DiscardUnreachable,
}

#[derive(Clone, PartialEq, Eq)]
struct Record {
    fingerprint: u64,
    binding: BindingIdentity,
    expression: Option<Expression>,
    value: Option<ValueRef>,
    is_kill: bool,
    block: Option<BlockRef>,
    region: Option<RegionRef>,
    function: Option<OperationRef>,
}

impl Record {
    fn read(op: OperationRef) -> Option<Self> {
        let operation = op.borrow();
        let binding = BindingIdentity::of(&operation)?;
        let expression = super::bindings::debug_record_expression(&operation);
        let value = operation
            .downcast_ref::<DebugValue>()
            .map(|record| record.value().as_value_ref());
        let is_kill = operation.is::<DebugKill>();
        Some(Self {
            fingerprint: fingerprint(&operation),
            binding,
            expression,
            value,
            is_kill,
            block: operation.parent(),
            region: operation.parent_region(),
            function: enclosing_function(op),
        })
    }

    fn same_binding(&self, other: &Self) -> bool {
        self.binding == other.binding
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
    hash_operation(operation, OperationEquivalenceFlags::NONE)
}

fn semantic_fingerprint(operation: &Operation) -> u64 {
    hash_operation(operation, OperationEquivalenceFlags::IGNORE_LOCATIONS)
}

fn hash_operation(operation: &Operation, flags: OperationEquivalenceFlags) -> u64 {
    let mut hasher = FxHasher::default();
    operation.hash_with_options(flags, DefaultValueHasher, DefaultValueHasher, &mut hasher);
    hasher.finish()
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

/// Classify a debug capture using immutable name metadata, without borrowing the operation.
pub fn is_debug_record(op: OperationRef) -> bool {
    let name = op.name();
    name.is::<DebugValue>() || name.is::<DebugDeclare>() || name.is::<DebugKill>()
}

fn is_in_owner(record: OperationRef, owner: CaptureOwner) -> bool {
    let mut current = Some(CaptureOwner::Operation(record));
    while let Some(ancestor) = current {
        if ancestor == owner {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

fn records_under(root: OperationRef) -> FxHashMap<OperationRef, Record> {
    if !root.context_ref().debug_info_mutations.borrow().has_observed_records()
        && !is_debug_record(root)
    {
        return FxHashMap::default();
    }
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
    locals: SmallVec<[LocalKey; 1]>,
}

type LocalKey = (OperationRef, u32);
type LocalSources = FxHashMap<LocalKey, SmallVec<[(OperationRef, u64); 2]>>;
type LocalRecords = FxHashMap<LocalKey, FxHashSet<OperationRef>>;

fn local_writes(op: OperationRef) -> SmallVec<[(u32, u64); 1]> {
    #[cfg(test)]
    tests::LOCAL_WRITE_INSPECTIONS.with(|count| count.set(count.get() + 1));
    let operation = op.borrow();
    if operation.has_regions() {
        return SmallVec::new();
    }
    let Some(effects) = operation.as_trait::<dyn MemoryEffectOpInterface>() else {
        return SmallVec::new();
    };
    effects
        .effects()
        .filter_map(|effect| {
            if !matches!(effect.effect(), MemoryEffect::Write | MemoryEffect::Free) {
                return None;
            }
            let local = effect.attribute()?.try_downcast_attr::<LocalVariableAttr>().ok()?;
            let index = local.borrow().as_value().as_usize() as u32;
            Some((index, semantic_fingerprint(&operation)))
        })
        .collect()
}

fn index_local_sources(root: OperationRef) -> LocalSources {
    let mut sources = LocalSources::default();
    root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        #[cfg(test)]
        tests::LOCAL_SOURCE_VISITS.with(|count| count.set(count.get() + 1));
        let Some(function) = enclosing_function(op) else {
            return;
        };
        for (index, fingerprint) in local_writes(op) {
            sources.entry((function, index)).or_default().push((op, fingerprint));
        }
    });
    sources
}

#[derive(Clone)]
struct LocalSourceSnapshot {
    source: OperationRef,
    // Unowned writes in a detached container acquire its eventual enclosing function.
    function: Option<OperationRef>,
    slot: u32,
    fingerprint: u64,
}

/// Snapshot at the authoring boundary, before attachment can expose captures to raw edits.
/// This rare path inspects both existing function storage and prebuilt detached writers.
fn authored_local_sources(
    op: OperationRef,
    record: &Record,
    root: OperationRef,
) -> Vec<LocalSourceSnapshot> {
    let slots = record.local_slots().collect::<FxHashSet<_>>();
    if slots.is_empty() {
        return Vec::new();
    }
    let mut sources = Vec::new();
    let mut visited = FxHashSet::default();
    let mut visit = |op: OperationRef| {
        if !visited.insert(op) {
            return;
        }
        for (slot, fingerprint) in local_writes(op) {
            if slots.contains(&slot) {
                sources.push(LocalSourceSnapshot {
                    source: op,
                    function: enclosing_function(op),
                    slot,
                    fingerprint,
                });
            }
        }
    };
    root.raw_prewalk_all::<Forward, _>(&mut visit);
    if let Some(function) = record.function {
        function.raw_prewalk_all::<Forward, _>(&mut visit);
    } else {
        let mut owner = CaptureOwner::Operation(op);
        while let Some(parent) = owner.parent() {
            owner = parent;
        }
        match owner {
            CaptureOwner::Operation(op) => op.raw_prewalk_all::<Forward, _>(&mut visit),
            CaptureOwner::Region(region) => region.raw_prewalk_all::<Forward, _>(&mut visit),
            CaptureOwner::Block(block) => {
                let operations = block
                    .borrow()
                    .body()
                    .iter()
                    .map(|op| op.as_operation_ref())
                    .collect::<Vec<_>>();
                for op in operations {
                    op.raw_prewalk_all::<Forward, _>(&mut visit);
                }
            }
        }
    }
    sources
}

#[derive(Clone)]
struct AuthoredCapture {
    record: Record,
    definition: Option<(OperationRef, u64)>,
    local_sources: Vec<LocalSourceSnapshot>,
}

struct Audit {
    id: u64,
    pending: FxHashSet<OperationRef>,
    authored: FxHashMap<OperationRef, AuthoredCapture>,
    root: OperationRef,
    records: FxHashMap<OperationRef, Record>,
    dependencies: FxHashMap<OperationRef, Dependency>,
    local_records: LocalRecords,
    approved_sources: FxHashMap<(OperationRef, LocalKey), u64>,
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
                        fingerprint: Some(semantic_fingerprint(&definition.borrow())),
                        records: SmallVec::new(),
                        locals: SmallVec::new(),
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
                                locals: SmallVec::new(),
                            })
                            .locals
                            .push(local);
                    }
                }
            }
        }
        Self {
            id: 0,
            pending: FxHashSet::default(),
            authored: FxHashMap::default(),
            root,
            records,
            dependencies,
            local_records,
            approved_sources: FxHashMap::default(),
            errors: Vec::new(),
        }
    }

    fn has_obligation(&self, op: OperationRef) -> bool {
        self.records.contains_key(&op)
            || self.dependencies.get(&op).is_some_and(|dependency| {
                dependency.locals.iter().any(|local| {
                    self.local_records.get(local).is_some_and(|records| !records.is_empty())
                }) || dependency.records.iter().any(|record| {
                    self.records.get(record).is_some_and(|record| record.uses_definition(op))
                })
            })
    }

    /// Registrations at one completion boundary share a fresh source snapshot. Separate authoring
    /// calls must rescan: list hooks cannot observe in-place operand or property edits.
    fn register(&mut self, records: impl IntoIterator<Item = (OperationRef, Record)>) {
        let mut sources = None;
        for (op, record) in records {
            self.pending.remove(&op);
            self.unregister(op);
            if let Some(value) = record.value
                && let Some(definition) = value.borrow().get_defining_op()
            {
                let dependency =
                    self.dependencies.entry(definition).or_insert_with(|| Dependency {
                        fingerprint: Some(semantic_fingerprint(&definition.borrow())),
                        records: SmallVec::new(),
                        locals: SmallVec::new(),
                    });
                if !dependency.records.contains(&op) {
                    dependency.records.push(op);
                }
            }
            if let Some(function) = record.function {
                for index in record.local_slots() {
                    let local = (function, index);
                    if !self.local_records.contains_key(&local) {
                        let sources = sources.get_or_insert_with(|| index_local_sources(self.root));
                        for &(source, fingerprint) in sources.get(&local).into_iter().flatten() {
                            self.dependencies
                                .entry(source)
                                .or_insert_with(|| Dependency {
                                    fingerprint: Some(fingerprint),
                                    records: SmallVec::new(),
                                    locals: SmallVec::new(),
                                })
                                .locals
                                .push(local);
                        }
                    }
                    self.local_records.entry(local).or_default().insert(op);
                }
            }
            self.records.insert(op, record);
        }
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CaptureOwner {
    Operation(OperationRef),
    Block(BlockRef),
    Region(RegionRef),
}

impl CaptureOwner {
    /// Intrusive parent links remain accessible while their entities are mutably borrowed.
    fn parent(self) -> Option<Self> {
        match self {
            Self::Operation(op) => op.parent().map(Self::Block),
            Self::Block(block) => block.parent().map(Self::Region),
            Self::Region(region) => region.parent().map(Self::Operation),
        }
    }
}

struct CaptureMapping {
    target: OperationRef,
    expected: Record,
}

#[derive(Clone, Copy)]
struct OwnerAnchor {
    block: Option<BlockRef>,
    region: Option<RegionRef>,
    function: Option<OperationRef>,
}

impl OwnerAnchor {
    fn of(op: OperationRef) -> Self {
        Self {
            block: op.parent(),
            region: op.parent().and_then(|block| block.parent()),
            function: enclosing_function(op),
        }
    }

    fn contains(&self, record: &Record) -> bool {
        self.block == record.block
            && self.region == record.region
            && self.function == record.function
    }
}

#[derive(Clone, Copy)]
struct InsertionPoint {
    block: Option<BlockRef>,
    next: Option<OperationRef>,
}

struct Permission {
    id: u64,
    operations: FxHashSet<OperationRef>,
    audits: FxHashSet<u64>,
    new_records: FxHashMap<OperationRef, Option<OperationRef>>,
    inserted: FxHashMap<OperationRef, InsertionPoint>,
    /// Snapshot capture ownership so list hooks can track container motion without borrowing it.
    captures_by_owner: FxHashMap<CaptureOwner, SmallVec<[OperationRef; 2]>>,
    moved_records: FxHashSet<OperationRef>,
    moved_owners: FxHashSet<CaptureOwner>,
    update: DebugInfoUpdate,
    mappings: FxHashMap<OperationRef, CaptureMapping>,
}

#[derive(Default)]
pub(crate) struct DebugInfoMutationState {
    audits: Vec<Audit>,
    permissions: Vec<Permission>,
    next_id: u64,
    has_observed_records: bool,
    observed: ObservedCaptures,
}

impl DebugInfoMutationState {
    pub(super) fn has_observed_records(&self) -> bool {
        self.has_observed_records
    }

    pub(crate) fn inserted(&mut self, op: OperationRef) {
        if is_debug_record(op) {
            self.has_observed_records = true;
            self.observed.observe(op);
            self.observe_attachment(op);
        } else {
            self.attached(CaptureOwner::Operation(op));
        }
        let point = InsertionPoint {
            block: op.parent(),
            next: op.next(),
        };
        for permission in &mut self.permissions {
            if !permission.operations.is_empty() {
                permission.inserted.entry(op).or_insert(point);
            }
        }
    }

    fn observe_attachment(&mut self, record: OperationRef) {
        for audit in &mut self.audits {
            if is_under(record, audit.root) && !audit.records.contains_key(&record) {
                if let Some(mut authored) = audit.authored.remove(&record) {
                    authored.record.block = record.parent();
                    authored.record.region = record.parent().and_then(|block| block.parent());
                    authored.record.function = enclosing_function(record);
                    if let Some((definition, fingerprint)) = authored.definition {
                        audit
                            .dependencies
                            .entry(definition)
                            .or_insert_with(|| Dependency {
                                fingerprint: Some(fingerprint),
                                records: SmallVec::new(),
                                locals: SmallVec::new(),
                            })
                            .records
                            .push(record);
                    }
                    if let Some(function) = authored.record.function {
                        for slot in authored.record.local_slots() {
                            let local = (function, slot);
                            if !audit.local_records.contains_key(&local) {
                                for source in &authored.local_sources {
                                    if source.slot == slot
                                        && source.function.is_none_or(|owner| owner == function)
                                    {
                                        let dependency = audit
                                            .dependencies
                                            .entry(source.source)
                                            .or_insert_with(|| Dependency {
                                                fingerprint: Some(source.fingerprint),
                                                records: SmallVec::new(),
                                                locals: SmallVec::new(),
                                            });
                                        if !dependency.locals.contains(&local) {
                                            dependency.locals.push(local);
                                        }
                                    }
                                }
                            }
                            audit.local_records.entry(local).or_default().insert(record);
                        }
                    }
                    audit.records.insert(record, authored.record);
                } else {
                    audit.pending.insert(record);
                }
            }
        }
    }

    fn attached(&mut self, owner: CaptureOwner) {
        let records = self.observed.reparent(owner);
        for record in records {
            self.observe_attachment(record);
        }
    }

    pub(crate) fn inserted_block(&mut self, block: BlockRef) {
        self.attached(CaptureOwner::Block(block));
    }

    pub(crate) fn inserted_region(&mut self, region: RegionRef) {
        self.attached(CaptureOwner::Region(region));
    }

    pub(crate) fn removed(&mut self, op: OperationRef) {
        self.moved(CaptureOwner::Operation(op));
        self.require_intent(op, "removing or moving");
    }

    pub(crate) fn removed_block(&mut self, block: BlockRef) {
        self.moved(CaptureOwner::Block(block));
    }

    pub(crate) fn removed_region(&mut self, region: RegionRef) {
        self.moved(CaptureOwner::Region(region));
    }

    fn moved(&mut self, owner: CaptureOwner) {
        let records = self.observed.reparent(owner);
        for record in records {
            self.require_intent(record, "removing or moving its owner");
            for permission in &mut self.permissions {
                if permission.inserted.contains_key(&record) {
                    permission.moved_records.insert(record);
                }
            }
        }
        for permission in &mut self.permissions {
            if let Some(records) = permission.captures_by_owner.get(&owner) {
                permission.moved_records.extend(records.iter().copied());
                permission.moved_owners.insert(owner);
            }
        }
    }

    pub(crate) fn implicit_erase(&mut self, op: OperationRef) {
        self.require_intent(op, "erasing");
    }

    fn require_intent(&mut self, op: OperationRef, action: &str) {
        for audit in &mut self.audits {
            if audit.has_obligation(op)
                && !self.permissions.iter().any(|permission| {
                    permission.audits.contains(&audit.id)
                        && permission.operations.contains(&op)
                        && (!audit.records.contains_key(&op)
                            || permission.mappings.contains_key(&op)
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
///
/// A guard does not authorize arbitrary capture insertion. Use DIBuilder for new source captures,
/// or name an exact replacement/coalescence with this guard before erasing its original. Records
/// authored in detached containers during the audit retain their creation snapshots on attachment.
/// Attaching records built before the audit requires a current explicit mapping; merely moving
/// initially tracked records uses the ordinary relocation contract.
#[must_use]
pub struct DebugInfoMutation {
    context: Rc<Context>,
    update: DebugInfoUpdate,
    records: FxHashMap<OperationRef, Record>,
    descendants: FxHashSet<OperationRef>,
    dependencies: FxHashSet<OperationRef>,
    discard_witnesses: FxHashMap<OperationRef, SmallVec<[CaptureOwner; 2]>>,
    anchors: FxHashMap<OperationRef, OwnerAnchor>,
    original_owners: FxHashMap<OperationRef, SmallVec<[OperationRef; 2]>>,
    audit_records: FxHashMap<u64, FxHashSet<OperationRef>>,
    id: u64,
    active: bool,
    roots: Vec<OperationRef>,
    pending: Vec<PendingCaptureMutation>,
}

struct PendingCaptureMutation {
    anchor: OperationRef,
    binding: super::bindings::SourceBinding,
    witnesses: Vec<CaptureOwner>,
    parent: Option<OperationRef>,
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
            discard_witnesses: FxHashMap::default(),
            anchors: FxHashMap::default(),
            original_owners: FxHashMap::default(),
            audit_records: FxHashMap::default(),
            id: 0,
            active,
            roots: if super::capture_invalidation::has_pending(root.context_ref()) {
                roots.to_vec()
            } else {
                Vec::new()
            },
            pending: Vec::new(),
        };
        if super::capture_invalidation::has_pending(&mutation.context) {
            let mut seen = FxHashSet::default();
            for &root in roots {
                for anchor in super::transform::collect_debug_ops(&root) {
                    if super::capture_invalidation::is_pending_capture(anchor)
                        && seen.insert(anchor)
                    {
                        let witnesses = roots
                            .iter()
                            .copied()
                            .filter(|root| {
                                root.borrow().has_regions()
                                    && root.parent().is_some()
                                    && is_under(anchor, *root)
                            })
                            .map(CaptureOwner::Operation)
                            .collect();
                        if !super::capture_invalidation::validate_anchor(anchor) {
                            super::capture_invalidation::poison(
                                &mutation.context,
                                "pending capture changed before its mutation guard",
                            );
                        }
                        let binding =
                            super::capture_invalidation::expected_binding(anchor).unwrap();
                        let parent = binding.function.and_then(|function| function.parent_op());
                        mutation.pending.push(PendingCaptureMutation {
                            anchor,
                            binding,
                            witnesses,
                            parent,
                        });
                    }
                }
            }
        }
        if !active {
            return mutation;
        }
        if mutation
            .context
            .debug_info_mutations
            .borrow()
            .audits
            .iter()
            .all(|audit| audit.records.is_empty())
        {
            let mut state = mutation.context.debug_info_mutations.borrow_mut();
            state.next_id += 1;
            mutation.id = state.next_id;
            let audits = state.audits.iter().map(|audit| audit.id).collect();
            state.permissions.push(Permission {
                id: mutation.id,
                operations: FxHashSet::default(),
                audits,
                new_records: FxHashMap::default(),
                inserted: FxHashMap::default(),
                captures_by_owner: FxHashMap::default(),
                moved_records: FxHashSet::default(),
                moved_owners: FxHashSet::default(),
                update,
                mappings: FxHashMap::default(),
            });
            drop(state);
            return mutation;
        }
        for root in roots {
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
                let owned_records = mutation.audit_records.entry(audit.id).or_default();
                let mut visited_locals = FxHashSet::default();
                for op in affected.clone() {
                    if let Some(record) = audit.records.get(&op) {
                        mutation.records.insert(op, record.clone());
                        owned_records.insert(op);
                        operations.insert(op);
                    }
                    if let Some(dependency) = audit.dependencies.get(&op) {
                        mutation.dependencies.insert(op);
                        operations.insert(op);
                        for record in dependency.records.iter().chain(
                            dependency
                                .locals
                                .iter()
                                .filter(|local| visited_locals.insert(**local))
                                .filter_map(|local| audit.local_records.get(local))
                                .flatten(),
                        ) {
                            if let Some(before) = audit.records.get(record) {
                                mutation.records.insert(*record, before.clone());
                                owned_records.insert(*record);
                                operations.insert(*record);
                            }
                        }
                    }
                }
            }
        }
        for &record in mutation.records.keys() {
            mutation.anchors.insert(record, OwnerAnchor::of(record));
            mutation.original_owners.entry(record).or_default().push(record);
            for &root in roots {
                if is_under(record, root) {
                    mutation.anchors.insert(root, OwnerAnchor::of(root));
                    mutation.original_owners.entry(record).or_default().push(root);
                }
            }
        }
        if update == DebugInfoUpdate::DiscardUnreachable {
            for &record in mutation.records.keys() {
                for &root in roots {
                    if root.borrow().has_regions() && is_under(record, root) {
                        mutation
                            .discard_witnesses
                            .entry(record)
                            .or_default()
                            .push(CaptureOwner::Operation(root));
                    }
                }
            }
        }
        let mut captures_by_owner =
            FxHashMap::<CaptureOwner, SmallVec<[OperationRef; 2]>>::default();
        if update != DebugInfoUpdate::Relocate {
            for record in mutation.records.keys().copied() {
                let mut owner = Some(record);
                while let Some(op) = owner {
                    captures_by_owner.entry(CaptureOwner::Operation(op)).or_default().push(record);
                    let Some(block) = op.parent() else { break };
                    captures_by_owner.entry(CaptureOwner::Block(block)).or_default().push(record);
                    let Some(region) = block.parent() else { break };
                    captures_by_owner.entry(CaptureOwner::Region(region)).or_default().push(record);
                    owner = region.parent();
                }
            }
        }
        let mut state = mutation.context.debug_info_mutations.borrow_mut();
        state.next_id += 1;
        mutation.id = state.next_id;
        let audits = state.audits.iter().map(|audit| audit.id).collect();
        let authorized = state
            .audits
            .iter()
            .map(|audit| {
                state
                    .permissions
                    .iter()
                    .filter(|permission| permission.audits.contains(&audit.id))
                    .flat_map(|permission| permission.operations.iter().copied())
                    .collect::<FxHashSet<_>>()
            })
            .collect::<Vec<_>>();
        for (index, audit) in state.audits.iter_mut().enumerate() {
            for (&op, before) in &mutation.records {
                if !audit.records.contains_key(&op) {
                    continue;
                }
                if !authorized[index].contains(&op) && Record::read(op).as_ref() != Some(before) {
                    audit.errors.push(format!(
                        "unhandled debug-info mutation for variable '{}': the record changed \
                         before its DebugInfoUpdate was declared",
                        before.binding.variable.name
                    ));
                }
            }
            for op in &mutation.dependencies {
                if !authorized[index].contains(op)
                    && let Some(dependency) = audit.dependencies.get(op)
                    && audit.has_obligation(*op)
                    && is_under(*op, audit.root).then(|| semantic_fingerprint(&op.borrow()))
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
            id: mutation.id,
            operations,
            audits,
            new_records: FxHashMap::default(),
            inserted: FxHashMap::default(),
            captures_by_owner,
            moved_records: FxHashSet::default(),
            moved_owners: FxHashSet::default(),
            update,
            mappings: FxHashMap::default(),
        });
        drop(state);
        mutation
    }

    /// Coalesce exactly one original capture into a named surviving capture.
    ///
    /// The caller asserts equivalent execution and supplies old-to-new SSA value pairs. Both
    /// captures must belong to this rewrite, agree on binding/kind/expression, and remain in the
    /// same function and enclosing rewrite scope. Register the mapping before erasing the original.
    pub fn coalesce_capture(
        &mut self,
        original: OperationRef,
        retained: OperationRef,
        values: &[(ValueRef, ValueRef)],
    ) -> Result<(), Report> {
        if super::capture_invalidation::is_pending_capture(original)
            || super::capture_invalidation::is_pending_capture(retained)
        {
            return Err(Report::msg("cannot coalesce a pending capture clear"));
        }
        if !self.active {
            return Ok(());
        }
        let invalid = || Report::msg("invalid debug-info capture coalescence mapping");
        let before = self.records.get(&original).ok_or_else(invalid)?;
        let target_before = self.records.get(&retained).ok_or_else(invalid)?;
        let after = Record::read(retained).ok_or_else(invalid)?;
        let value_matches = before.value == after.value
            || before.value.zip(after.value).is_some_and(|pair| {
                values.contains(&pair) && pair.0.borrow().ty() == pair.1.borrow().ty()
            });
        let scopes_match = self.original_owners.get(&original).into_iter().flatten().any(|owner| {
            self.original_owners.get(&retained).into_iter().flatten().any(|target_owner| {
                self.anchors[owner].region == self.anchors[target_owner].region
                    && self.anchors[owner].function == self.anchors[target_owner].function
            })
        });
        if self.update != DebugInfoUpdate::Equivalent
            || original == retained
            || !before.same_binding(&after)
            || before.expression != after.expression
            || original.name() != retained.name()
            || !value_matches
            || !scopes_match
            || before.function != after.function
            || target_before != &after
        {
            return Err(invalid());
        }
        self.map_capture(original, retained, after)
    }

    /// Replace a capture immediately before its original record or an explicitly declared
    /// ancestor rewrite root. The owner and its containers must not have moved since this guard
    /// was declared; the owner may subsequently be erased. The replacement must remain at its
    /// registered point. Intentional relocation requires a separate, preceding Relocate guard.
    pub fn replace_capture(
        &mut self,
        original: OperationRef,
        replacement: OperationRef,
        owner: OperationRef,
    ) -> Result<(), Report> {
        if !self.active {
            if super::capture_invalidation::is_pending_capture(original) {
                if !matches!(self.update, DebugInfoUpdate::Salvage | DebugInfoUpdate::Invalidate)
                    || replacement.next() != Some(owner)
                    || !(owner == original || self.roots.contains(&owner))
                    || !is_under(original, owner)
                {
                    super::capture_invalidation::poison(
                        &self.context,
                        "invalid pending owner-anchored replacement",
                    );
                    return Err(Report::msg("invalid pending owner-anchored replacement"));
                }
                super::capture_invalidation::transfer(original, replacement)?;
            }
            return Ok(());
        }
        let invalid = || Report::msg("invalid debug-info owner-anchored capture replacement");
        let before = self.records.get(&original).ok_or_else(invalid)?;
        let after = Record::read(replacement).ok_or_else(invalid)?;
        let anchor = self.anchors.get(&owner).ok_or_else(invalid)?;
        let current_owner = OwnerAnchor::of(owner);
        if !matches!(self.update, DebugInfoUpdate::Salvage | DebugInfoUpdate::Invalidate)
            || !self
                .original_owners
                .get(&original)
                .is_some_and(|owners| owners.contains(&owner))
            || !before.same_binding(&after)
            || (self.update == DebugInfoUpdate::Invalidate && !after.is_kill)
            || !anchor.contains(&after)
            || anchor.block != current_owner.block
            || anchor.region != current_owner.region
            || anchor.function != current_owner.function
            || replacement.next() != Some(owner)
        {
            return Err(invalid());
        }
        let state = self.context.debug_info_mutations.borrow();
        let permission = state
            .permissions
            .iter()
            .find(|permission| permission.id == self.id)
            .ok_or_else(invalid)?;
        // Only a capture inserted during this guard can replace an original. Existing retained
        // captures are handled by coalescence, which has a different semantic contract.
        if !permission
            .inserted
            .get(&replacement)
            .is_some_and(|point| point.block == anchor.block && point.next == Some(owner))
            || permission.moved_records.contains(&replacement)
            || permission.moved_owners.iter().any(|&moved| is_in_owner(owner, moved))
            || (!permission.new_records.contains_key(&replacement)
                && !state.audits.iter().any(|audit| {
                    permission.audits.contains(&audit.id) && audit.pending.contains(&replacement)
                }))
        {
            return Err(invalid());
        }
        drop(state);
        self.map_capture(original, replacement, after)?;
        super::capture_invalidation::transfer(original, replacement)
    }

    fn map_capture(
        &mut self,
        original: OperationRef,
        target: OperationRef,
        expected: Record,
    ) -> Result<(), Report> {
        let invalid = || {
            Report::msg(
                "capture mapping must name a live original and target in every owning audit",
            )
        };
        let before = self.records.get(&original).ok_or_else(invalid)?;
        if Record::read(original).as_ref() != Some(before) {
            return Err(invalid());
        }
        let mut state = self.context.debug_info_mutations.borrow_mut();
        let Some(index) = state.permissions.iter().position(|permission| permission.id == self.id)
        else {
            return Err(invalid());
        };
        let permission = &state.permissions[index];
        if permission.audits.is_empty()
            || permission.mappings.contains_key(&original)
            || state.audits.iter().any(|audit| {
                permission.audits.contains(&audit.id)
                    && self
                        .audit_records
                        .get(&audit.id)
                        .is_some_and(|records| records.contains(&original))
                    && (!is_under(original, audit.root) || !is_under(target, audit.root))
            })
        {
            return Err(invalid());
        }
        let permission = &mut state.permissions[index];
        permission.mappings.insert(original, CaptureMapping { target, expected });
        // Include target ancestry so a container round trip cannot hide motion after registration.
        let mut current = Some(target);
        while let Some(op) = current {
            permission
                .captures_by_owner
                .entry(CaptureOwner::Operation(op))
                .or_default()
                .push(target);
            let Some(block) = op.parent() else { break };
            permission
                .captures_by_owner
                .entry(CaptureOwner::Block(block))
                .or_default()
                .push(target);
            let Some(region) = block.parent() else { break };
            permission
                .captures_by_owner
                .entry(CaptureOwner::Region(region))
                .or_default()
                .push(target);
            current = region.parent();
        }
        Ok(())
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
                for PendingCaptureMutation {
                    anchor, witnesses, ..
                } in &mut mutation.pending
                {
                    for &block in blocks {
                        if is_in_owner(*anchor, CaptureOwner::Block(block)) {
                            witnesses.push(CaptureOwner::Block(block));
                        }
                    }
                }
                for &record in mutation.records.keys() {
                    for &block in blocks {
                        if is_in_owner(record, CaptureOwner::Block(block)) {
                            mutation
                                .discard_witnesses
                                .entry(record)
                                .or_default()
                                .push(CaptureOwner::Block(block));
                        }
                    }
                }
            }
            mutation
        })
    }
}

impl Drop for DebugInfoMutation {
    fn drop(&mut self) {
        for PendingCaptureMutation {
            anchor,
            binding,
            witnesses,
            parent,
        } in &self.pending
        {
            if !super::capture_invalidation::is_pending_capture(*anchor) {
                continue;
            }
            let root = binding.function;
            let attached = root.is_some_and(|root| is_under(*anchor, root));
            let current = super::bindings::SourceBinding::of(*anchor);
            if self.update == DebugInfoUpdate::Relocate
                && let Some(current) = current.as_ref()
                && current.identity == binding.identity
                && current.function.is_some()
            {
                super::capture_invalidation::relocated(*anchor, current.clone());
                continue;
            }
            if self.update == DebugInfoUpdate::DiscardUnreachable
                && current.as_ref().is_some_and(|current| {
                    current.identity == binding.identity
                        && current.function.is_none_or(|function| Some(function) == root)
                })
                && witnesses.iter().any(|owner| {
                    // The capture must have disappeared with this container, not escaped to
                    // another one before its old owner was removed. A separately erased record
                    // is also covered by the declared container's subsequent removal.
                    (anchor.parent().is_none() || is_in_owner(*anchor, *owner))
                        && match owner {
                            CaptureOwner::Operation(owner) => {
                                owner.parent_op().is_none()
                                    && (!attached
                                        || root
                                            .is_some_and(|root| *owner == root && parent.is_some()))
                            }
                            CaptureOwner::Block(block) => {
                                !attached && block.parent().and_then(|r| r.parent()).is_none()
                            }
                            CaptureOwner::Region(_) => false,
                        }
                })
            {
                super::capture_invalidation::cancel(*anchor);
            } else if !attached || current.as_ref() != Some(binding) {
                super::capture_invalidation::poison(
                    &self.context,
                    "pending capture changed or disappeared without an exact mapping or \
                     unreachable owner",
                );
            }
        }
        if !self.active {
            return;
        }
        let mut state = self.context.debug_info_mutations.borrow_mut();
        let Some(index) = state.permissions.iter().position(|permission| permission.id == self.id)
        else {
            return;
        };
        let permission = state.permissions.remove(index);
        for audit in &mut state.audits {
            if permission.audits.contains(&audit.id) {
                self.complete(audit, &permission);
            }
        }
    }
}

impl DebugInfoMutation {
    fn complete(&self, audit: &mut Audit, permission: &Permission) {
        let Some(originals) = self.audit_records.get(&audit.id) else {
            return;
        };
        let root = audit.root;
        let replacements = permission
            .new_records
            .iter()
            .filter_map(|(&op, &origin)| {
                let original = origin?;
                (originals.contains(&original) && !is_under(original, root) && is_under(op, root))
                    .then(|| Record::read(op).map(|record| (original, op, record)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let mut updates = Vec::new();
        let mut valid_contract = true;
        for &op in originals {
            let before = &self.records[&op];
            let after = is_under(op, root).then(|| Record::read(op)).flatten();
            let mapped = after.is_none()
                && permission.mappings.get(&op).is_some_and(|mapping| {
                    is_under(mapping.target, root)
                        && Record::read(mapping.target).as_ref() == Some(&mapping.expected)
                        && !permission.moved_records.contains(&mapping.target)
                });
            let valid = if permission.mappings.contains_key(&op) {
                mapped
            } else {
                match (&after, self.update) {
                    (Some(after), DebugInfoUpdate::Preserve) => {
                        before == after && !permission.moved_records.contains(&op)
                    }
                    (Some(after), DebugInfoUpdate::DiscardUnreachable) => {
                        before == after && !permission.moved_records.contains(&op)
                    }
                    (Some(after), DebugInfoUpdate::Equivalent) => {
                        before.same_binding(after)
                            && before.expression == after.expression
                            && before.is_kill == after.is_kill
                            && before.block == after.block
                            && before.region == after.region
                            && before.function == after.function
                            && !permission.moved_records.contains(&op)
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
                        self.discard_witnesses.get(&op).is_some_and(|owners| {
                            owners.iter().any(|owner| match owner {
                                CaptureOwner::Operation(owner) => !is_under(*owner, root),
                                CaptureOwner::Block(block) => block
                                    .parent()
                                    .and_then(|region| region.parent())
                                    .is_none_or(|owner| !is_under(owner, root)),
                                CaptureOwner::Region(_) => false,
                            })
                        })
                    }
                    (None, DebugInfoUpdate::Salvage | DebugInfoUpdate::Invalidate) => {
                        replacements.iter().any(|(original, _, replacement)| {
                            *original == op
                                && before.same_binding(replacement)
                                && before.block == replacement.block
                                && before.region == replacement.region
                                && before.function == replacement.function
                                && (self.update != DebugInfoUpdate::Invalidate
                                    || replacement.is_kill)
                        })
                    }
                    _ => false,
                }
            };
            if valid {
                updates.push((op, after));
            } else {
                valid_contract = false;
                audit.errors.push(format!(
                    "debug-info {:?} contract failed for variable '{}' in '{}'",
                    self.update,
                    before.binding.variable.name,
                    op.name()
                ));
            }
        }
        // A failed contract must not refresh its dependency baselines or approve inserted writers.
        if !valid_contract {
            return;
        }
        let affected_locals = originals
            .iter()
            .flat_map(|op| {
                let record = &self.records[op];
                record
                    .local_slots()
                    .filter_map(|slot| record.function.map(|function| (function, slot)))
            })
            .collect::<FxHashSet<_>>();
        let mut registrations = Vec::new();
        for (op, record) in updates {
            if let Some(record) = record {
                registrations.push((op, record));
            } else {
                audit.unregister(op);
            }
        }
        registrations.extend(replacements.into_iter().map(|(_, op, record)| (op, record)));
        for (&original, mapping) in &permission.mappings {
            if originals.contains(&original) && is_under(mapping.target, root) {
                registrations.push((mapping.target, mapping.expected.clone()));
            }
        }
        audit.register(registrations);
        for &op in &self.dependencies {
            if let Some(dependency) = audit.dependencies.get_mut(&op) {
                dependency.fingerprint =
                    is_under(op, root).then(|| semantic_fingerprint(&op.borrow()));
            }
        }
        // New storage effects are authorized only for the original affected locals and only by
        // a successful salvage/invalidation contract. Preserve/equivalence do not create writes.
        if !affected_locals.is_empty()
            && matches!(self.update, DebugInfoUpdate::Salvage | DebugInfoUpdate::Invalidate)
        {
            let candidates = permission
                .inserted
                .keys()
                .chain(self.descendants.iter())
                .copied()
                .collect::<FxHashSet<_>>();
            for op in candidates {
                if !is_under(op, root) {
                    continue;
                }
                let Some(function) = enclosing_function(op) else {
                    continue;
                };
                for (slot, fingerprint) in local_writes(op) {
                    let local = (function, slot);
                    if affected_locals.contains(&local) {
                        audit.approved_sources.insert((op, local), fingerprint);
                    }
                }
            }
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
    let definition = record
        .value
        .and_then(|value| value.borrow().get_defining_op())
        .map(|definition| (definition, semantic_fingerprint(&definition.borrow())));
    let origin = op
        .next()
        .filter(|next| state.audits.iter().any(|audit| audit.records.contains_key(next)));
    for permission in &mut state.permissions {
        permission.new_records.insert(op, origin);
    }
    for audit in &mut state.audits {
        if is_under(op, audit.root) {
            audit.register(core::iter::once((op, record.clone())));
        } else {
            audit.authored.insert(
                op,
                AuthoredCapture {
                    record: record.clone(),
                    definition,
                    local_sources: authored_local_sources(op, &record, audit.root),
                },
            );
        }
    }
}

pub(crate) struct DebugInfoAudit {
    phase_generation: u64,
    context: Rc<Context>,
    active: bool,
    id: u64,
}

impl DebugInfoAudit {
    pub(crate) fn begin(root: OperationRef) -> Self {
        let context = root.borrow().context_rc();
        let records = records_under(root);
        let mut state = context.debug_info_mutations.borrow_mut();
        state.next_id += 1;
        let id = state.next_id;
        let mut audit = Audit::new(root, records);
        audit.id = id;
        state.audits.push(audit);
        drop(state);
        Self {
            phase_generation: super::capture_invalidation::generation(&context),
            context,
            active: true,
            id,
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), Report> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let mut state = self.context.debug_info_mutations.borrow_mut();
        let index = state.audits.iter().position(|audit| audit.id == self.id).unwrap();
        let mut audit = state.audits.remove(index);
        if let Err(error) =
            super::capture_invalidation::audit_boundary(&self.context, self.phase_generation)
        {
            audit.errors.push(format!("{error}"));
        }
        for permission in &mut state.permissions {
            if permission.audits.remove(&self.id) {
                audit.errors.push("debug-info rewrite guard outlived its audit".into());
            }
        }
        drop(state);
        for op in &audit.pending {
            audit.errors.push(format!(
                "unhandled debug-info insertion in '{}': use DIBuilder or an explicit capture \
                 mapping",
                op.name()
            ));
        }
        let actual = records_under(audit.root);
        for (op, expected) in &audit.records {
            if actual.get(op) != Some(expected) {
                audit.errors.push(format!(
                    "unhandled debug-info mutation for variable '{}': record changed, moved, or \
                     disappeared without an explicit DebugInfoUpdate",
                    expected.binding.variable.name
                ));
            }
        }
        for (op, record) in &actual {
            if !audit.records.contains_key(op) {
                audit.errors.push(format!(
                    "unhandled debug-info insertion for variable '{}': use DIBuilder or an \
                     explicit DebugInfoUpdate",
                    record.binding.variable.name
                ));
            }
        }
        for (op, dependency) in &audit.dependencies {
            if !audit.has_obligation(*op) {
                continue;
            }
            let actual = is_under(*op, audit.root).then(|| semantic_fingerprint(&op.borrow()));
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
            for local in dependency.locals {
                if !audit.dependencies.get(&op).is_some_and(|before| before.locals.contains(&local))
                    && audit.approved_sources.get(&(op, local)).copied() != dependency.fingerprint
                {
                    audit.errors.push(format!(
                        "unhandled debug-info storage insertion in '{}': a tracked local write \
                         requires an explicit DebugInfoUpdate",
                        op.name()
                    ));
                }
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

    /// A failed pass skips capture validation, but still must close every phase it opened.
    pub(crate) fn abort(mut self) -> Result<(), Report> {
        self.close()
    }

    fn close(&mut self) -> Result<(), Report> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let mut state = self.context.debug_info_mutations.borrow_mut();
        state.audits.retain(|audit| audit.id != self.id);
        for permission in &mut state.permissions {
            permission.audits.remove(&self.id);
        }
        drop(state);
        super::capture_invalidation::audit_boundary(&self.context, self.phase_generation)
    }
}

impl Drop for DebugInfoAudit {
    fn drop(&mut self) {
        // Unwinding/abandonment backstop only: lifecycle validation poisons and releases state;
        // fallible IR materialization belongs to the explicit phase/pass completion paths.
        let _ = self.close();
    }
}
