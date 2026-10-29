//! Consumer adapters for author codec components.

use std::{collections::HashSet, sync::Arc};

use miden_field::Felt;
use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::{PACKAGE_NOTE_CODEC_SECTION_ID, package_note_codec_section_id};
use wasmtime::{
    Config, Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, Linker},
};

use crate::{
    CodecFailure, CodecRegistry, ConsumerTypeCodec, Error, NoteStorageSchema, Result,
    codec_structure::{
        MAX_CORE_INSTANCES, MAX_INSTANTIATED_MEMORIES, MAX_INSTANTIATED_TABLES,
        NOTE_CODEC_WASM_FEATURES,
    },
    validate_note_codec_component,
};

/// Maximum bytes of Wasm stack available to one component call.
const MAX_WASM_STACK_BYTES: usize = 512 * 1024;

/// Ceiling on the host copy budget of one call: wasmtime's own default.
const MAX_HOSTCALL_FUEL: usize = 128 << 20;

/// The message wasmtime reports when a lift would exceed the host copy budget.
///
/// Wasmtime keeps that error type private, so the message is the only handle on it.
const HOST_COPY_BUDGET_EXHAUSTED: &str = "too much data is being copied between the host and the \
                                          guest: fuel allocated for hostcalls has been exhausted";

/// The prefix of the message wasmtime reports when a store instance, table, or memory count is
/// reached.
const STORE_COUNT_LIMIT_PREFIX: &str = "resource limit exceeded:";

/// Maximum instances one codec call store may hold.
///
/// The value is the structural budget for the core instances one instantiation creates. The load
/// policy counts what instantiation creates, so a component that loads instantiates within this
/// count.
const MAX_STORE_INSTANCES: usize = MAX_CORE_INSTANCES;

/// Maximum tables one codec call store may hold.
///
/// The value is the structural budget for the tables one instantiation creates.
const MAX_STORE_TABLES: usize = MAX_INSTANTIATED_TABLES;

/// Maximum linear memories one codec call store may hold.
///
/// The value is the structural budget for the linear memories one instantiation creates.
const MAX_STORE_MEMORIES: usize = MAX_INSTANTIATED_MEMORIES;

wasmtime::component::bindgen!({
    path: "wit",
    world: "note-codec",
});

/// Runtime limits a host applies to bundled note codecs. Limits are host policy.
/// A package carries no limit values and cannot raise them. Hosts in one
/// deployment should run identical limits, so a codec behaves the same everywhere.
///
/// The store counts are fixed policy instead of host configuration. One call store admits at
/// most `MAX_STORE_INSTANCES` instances, `MAX_STORE_TABLES` tables, and `MAX_STORE_MEMORIES`
/// linear memory, and one call gets `MAX_WASM_STACK_BYTES` of Wasm stack. The structural policy
/// in [`validate_note_codec_component`] counts what one instantiation creates and applies the
/// same values, so a component that loads instantiates within these counts.
///
/// The result caps also size the host copy budget of one call, up to a ceiling of 128 MiB. A
/// result that lifts past the budget, and an argument of a refused import call that lifts past
/// it, fails as [`CodecFailure::LimitExceeded`] with a generic message rather than a cap's own
/// message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecLimits {
    /// Fuel budget for one codec call, about one unit per Wasm instruction.
    pub fuel: u64,
    /// Cap on one guest linear memory in bytes. Wasmtime applies the cap to each memory, and a
    /// store admits `MAX_STORE_MEMORIES` of them.
    pub max_memory_bytes: usize,
    /// Cap on the elements of one table. Wasmtime applies the cap to each table, and a store
    /// admits `MAX_STORE_TABLES` of them.
    pub max_table_elements: usize,
    /// Cap on the component size in bytes.
    pub max_component_bytes: usize,
    /// Cap on the number of types one component may report.
    pub max_supported_types: usize,
    /// Cap on one reported type name in bytes.
    pub max_fqn_bytes: usize,
    /// Cap on the felts one `parse` call may return.
    pub max_returned_felts: usize,
    /// Cap on one returned string in bytes.
    pub max_returned_string_bytes: usize,
}

impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            max_memory_bytes: 16 * 1024 * 1024,
            max_table_elements: 4_096,
            max_component_bytes: crate::schema::MAX_NOTE_CODEC_COMPONENT_BYTES,
            max_supported_types: 128,
            max_fqn_bytes: 512,
            max_returned_felts: 4_096,
            max_returned_string_bytes: 16 * 1024,
        }
    }
}

impl CodecLimits {
    /// The host copy budget of one call, in the units wasmtime charges a lift: the guest bytes
    /// of each string, and the host size of each list element.
    ///
    /// The budget admits the largest result any operation may return within the caps in any
    /// string encoding, and refuses a result past that before the host copies it. It never
    /// exceeds wasmtime's own default, so caps beyond that ceiling refuse results that lift
    /// past it.
    fn hostcall_fuel(&self) -> usize {
        // Wasmtime charges a lifted string by its size in guest memory, which for a codec that
        // lifts with UTF-16 is up to twice its UTF-8 length, and a lifted list one owned element
        // per entry before any entry is read. Every host call starts from the full budget, so
        // the largest single result bounds it.
        let guest_string = |utf8_bytes: usize| utf8_bytes.saturating_mul(2);
        let discovery = self
            .max_supported_types
            .saturating_mul(size_of::<String>().saturating_add(guest_string(self.max_fqn_bytes)));
        let felts = self.max_returned_felts.saturating_mul(size_of::<u64>());
        discovery
            .max(felts)
            .max(guest_string(self.max_returned_string_bytes))
            .min(MAX_HOSTCALL_FUEL)
    }
}

impl CodecRegistry {
    /// Loads the note codec component from a package under the default limits.
    pub fn load_from_package(package: &Package) -> Result<Self> {
        Self::load_from_package_with_limits(package, CodecLimits::default())
    }

    /// Loads the note codec component from a package and registers all reported types.
    ///
    /// A package without a note codec section is the common case, so it is not an error: the
    /// registry comes back with the standard codecs alone, the same base the bundled-codec path
    /// registers on top of. More than one note codec section is still an error.
    pub fn load_from_package_with_limits(package: &Package, limits: CodecLimits) -> Result<Self> {
        // The section check comes first. A package without a codec section needs no schema, and
        // a unit `#[note]` struct emits neither section.
        let section_id = package_note_codec_section_id();
        if !package.sections.iter().any(|section| section.id == section_id) {
            return Ok(Self::default());
        }
        let bytes = crate::section::unique_package_section(
            package,
            section_id,
            PACKAGE_NOTE_CODEC_SECTION_ID,
        )?;
        let schema = NoteStorageSchema::from_package(package)?;
        Self::load_from_component(bytes, &schema.custom_type_fqns(), limits)
    }

    /// Loads a note codec component with every unresolved import stubbed as a trap.
    fn load_from_component(
        bytes: &[u8],
        custom_type_fqns: &HashSet<String>,
        limits: CodecLimits,
    ) -> Result<Self> {
        let runtime = Arc::new(ComponentRuntime::new(bytes, limits)?);
        let supported_types = runtime.supported_types()?;
        let mut registry = Self::default();
        validate_reported_fqns(&supported_types, custom_type_fqns, &registry)?;

        for fqn in supported_types {
            registry.register_shared(
                fqn.clone(),
                Arc::new(ComponentCodec {
                    fqn,
                    runtime: Arc::clone(&runtime),
                }),
            );
        }

        Ok(registry)
    }
}

/// A compiled component used to create an isolated instance for each operation.
struct ComponentRuntime {
    /// The engine that holds the accepted Wasm proposals and the fuel setting.
    engine: Engine,
    /// The compiled codec component.
    component: Component,
    /// The host limits applied to every instance and every call.
    limits: CodecLimits,
}

/// Store state that owns the component resource limits.
struct ComponentStore {
    /// The inner limiter that decides every memory and table question.
    limits: StoreLimits,
    /// Set when a memory or table growth was refused, so a failed call reports its class.
    limit_hit: bool,
}

// The inner limiter decides every question. This wrapper only records that a growth was
// refused, which a call failure reports as `CodecFailure::LimitExceeded`.
impl ResourceLimiter for ComponentStore {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.limits.memory_growing(current, desired, maximum);
        if !matches!(allowed, Ok(true)) {
            self.limit_hit = true;
        }
        allowed
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.limit_hit = true;
        self.limits.memory_grow_failed(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.limits.table_growing(current, desired, maximum);
        if !matches!(allowed, Ok(true)) {
            self.limit_hit = true;
        }
        allowed
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.limit_hit = true;
        self.limits.table_grow_failed(error)
    }

    fn instances(&self) -> usize {
        self.limits.instances()
    }

    fn tables(&self) -> usize {
        self.limits.tables()
    }

    fn memories(&self) -> usize {
        self.limits.memories()
    }
}

/// One isolated codec component call context.
struct ComponentInstance {
    /// The store that holds the fuel budget and the resource limits of this call.
    store: Store<ComponentStore>,
    /// The generated typed bindings of the codec world.
    bindings: NoteCodec,
}

impl ComponentRuntime {
    /// Compiles a component under the explicit engine policy and the given limits.
    fn new(bytes: &[u8], limits: CodecLimits) -> Result<Self> {
        // One entry point applies the byte cap, the Wasm feature policy, and the structural
        // limits. The producer applies the same policy when it attaches a codec.
        validate_note_codec_component(bytes, limits.max_component_bytes)?;
        let mut config = Config::new();
        config.consume_fuel(true);
        // `NOTE_CODEC_WASM_FEATURES` decides what loads, and the call above already applied it.
        // The engine repeats exactly that set as a backstop, because wasmtime's defaults follow
        // its Cargo features, which another crate in the same build can turn on by unification.
        config.wasm_features(NOTE_CODEC_WASM_FEATURES, true);
        config.wasm_features(!NOTE_CODEC_WASM_FEATURES, false);
        // Floats stay on: codecs parse and format decimal text. NaN canonicalization keeps
        // float results identical across hosts.
        config.cranelift_nan_canonicalization(true);
        config.max_wasm_stack(MAX_WASM_STACK_BYTES);
        config.wasm_backtrace_max_frames(None);
        let engine = Engine::new(&config)
            .map_err(|error| component_error("create the Wasmtime engine", error))?;
        let component = Component::new(&engine, bytes)
            .map_err(|error| component_error("compile the note codec component", error))?;
        Ok(Self {
            engine,
            component,
            limits,
        })
    }

    /// Instantiates the component with trapping WASI stubs and fresh per-call limits.
    fn instantiate(&self) -> Result<ComponentInstance> {
        let mut linker = Linker::new(&self.engine);
        // The wasip2 standard library imports WASI interfaces the codec never needs at
        // runtime. Stub every import as a trap so nothing outside the component is callable.
        linker
            .define_unknown_imports_as_traps(&self.component)
            .map_err(|error| component_error("stub the note codec imports", error))?;
        let state = ComponentStore {
            limits: component_store_limits(&self.limits),
            limit_hit: false,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state as &mut dyn ResourceLimiter);
        store
            .set_fuel(self.limits.fuel)
            .map_err(|error| component_error("set the note codec fuel budget", error))?;
        store.set_hostcall_fuel(self.limits.hostcall_fuel());
        // Instantiation runs the guest `_initialize` and every data-segment initializer under
        // the same fuel and store limits as an exported call, so it fails in the same classes.
        let instance = match linker.instantiate(&mut store, &self.component) {
            Ok(instance) => instance,
            Err(error) => return Err(start_failure(&store, error)),
        };
        let bindings = match NoteCodec::new(&mut store, &instance) {
            Ok(bindings) => bindings,
            Err(error) => return Err(start_failure(&store, error)),
        };
        Ok(ComponentInstance { store, bindings })
    }

    /// Queries the component's supported FQNs once during registry construction.
    ///
    /// The count and byte caps bound the names the registry keeps. A `list<string>` is the one
    /// result whose elements can alias one region of guest memory, so the call's host copy
    /// budget refuses a list the caps could not admit in any encoding before it reaches the
    /// host. Which of the two refusals the host sees depends on the encoding.
    fn supported_types(&self) -> Result<Vec<String>> {
        let ComponentInstance {
            mut store,
            bindings,
        } = self.instantiate()?;
        let fqns = match bindings.miden_note_codec_codec().call_supported_types(&mut store) {
            Ok(fqns) => fqns,
            Err(error) => {
                return Err(Error::codec(
                    classify_failure(&store, &error),
                    format!("note codec component failed in `supported-types`: {error:#}"),
                ));
            }
        };
        if fqns.len() > self.limits.max_supported_types {
            return Err(Error::codec(
                CodecFailure::LimitExceeded,
                format!(
                    "note codec component reported {} types; the limit is {}",
                    fqns.len(),
                    self.limits.max_supported_types
                ),
            ));
        }
        for fqn in &fqns {
            ensure_returned_string_limit("type FQN", fqn, self.limits.max_fqn_bytes)?;
        }
        Ok(fqns)
    }
}

/// Builds the resource limits attached to every isolated component call store.
fn component_store_limits(limits: &CodecLimits) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(limits.max_memory_bytes)
        .table_elements(limits.max_table_elements)
        .instances(MAX_STORE_INSTANCES)
        .tables(MAX_STORE_TABLES)
        .memories(MAX_STORE_MEMORIES)
        .trap_on_grow_failure(true)
        .build()
}

/// A registry entry that dispatches one FQN through isolated component instances.
struct ComponentCodec {
    /// The fully qualified WIT name this entry answers for.
    fqn: String,
    /// The compiled component shared by every registry entry of one package.
    runtime: Arc<ComponentRuntime>,
}

impl ComponentCodec {
    /// Calls one component operation in a fresh fuel- and memory-limited store.
    fn with_runtime<T>(
        &self,
        operation: &str,
        call: impl FnOnce(&NoteCodec, &mut Store<ComponentStore>) -> wasmtime::Result<T>,
    ) -> Result<T> {
        let mut instance = self.runtime.instantiate()?;
        match call(&instance.bindings, &mut instance.store) {
            Ok(value) => Ok(value),
            Err(error) => Err(self.call_failure(operation, &instance.store, error)),
        }
    }

    /// Reports why one component call did not return a value.
    fn call_failure(
        &self,
        operation: &str,
        store: &Store<ComponentStore>,
        error: wasmtime::Error,
    ) -> Error {
        let fqn = &self.fqn;
        match classify_failure(store, &error) {
            CodecFailure::LimitExceeded => Error::codec(
                CodecFailure::LimitExceeded,
                format!("note codec `{fqn}` exceeded a resource limit in `{operation}`"),
            ),
            CodecFailure::OutOfFuel => Error::codec(
                CodecFailure::OutOfFuel,
                format!("note codec `{fqn}` ran out of fuel in `{operation}`"),
            ),
            class => Error::codec(
                class,
                format!("note codec `{fqn}` trapped in `{operation}`: {error:#}"),
            ),
        }
    }

    /// Returns the limits the host applies to this codec.
    fn limits(&self) -> &CodecLimits {
        &self.runtime.limits
    }
}

impl ConsumerTypeCodec for ComponentCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let result = self.with_runtime("parse", |bindings, store| {
            bindings.miden_note_codec_codec().call_parse(store, &self.fqn, value)
        })?;
        let values = match result {
            Ok(values) => values,
            Err(message) => {
                ensure_returned_string_limit(
                    "codec error",
                    &message,
                    self.limits().max_returned_string_bytes,
                )?;
                return Err(codec_rejection("parse", &self.fqn, message));
            }
        };
        ensure_returned_felt_limit(&self.fqn, values.len(), self.limits().max_returned_felts)?;
        component_values_to_felts(&self.fqn, &values)
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        let values = felts.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();
        let result = self.with_runtime("display", |bindings, store| {
            bindings.miden_note_codec_codec().call_display(store, &self.fqn, &values)
        })?;
        let display = match result {
            Ok(display) => display,
            Err(message) => {
                ensure_returned_string_limit(
                    "codec error",
                    &message,
                    self.limits().max_returned_string_bytes,
                )?;
                return Err(codec_rejection("display", &self.fqn, message));
            }
        };
        ensure_returned_string_limit(
            "display value",
            &display,
            self.limits().max_returned_string_bytes,
        )?;
        Ok(display)
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        let values = felts.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();
        let result = self.with_runtime("validate", |bindings, store| {
            bindings.miden_note_codec_codec().call_validate(store, &self.fqn, &values)
        })?;
        match result {
            Ok(()) => Ok(()),
            Err(message) => {
                ensure_returned_string_limit(
                    "codec error",
                    &message,
                    self.limits().max_returned_string_bytes,
                )?;
                Err(codec_rejection("validate", &self.fqn, message))
            }
        }
    }
}

/// Validates the component's claimed type authority before registration.
fn validate_reported_fqns(
    reported: &[String],
    custom_type_fqns: &HashSet<String>,
    standard_registry: &CodecRegistry,
) -> Result<()> {
    let mut seen = HashSet::new();
    for fqn in reported {
        if fqn.trim().is_empty() {
            return Err(Error::new("note codec component reported an empty type FQN"));
        }
        if !seen.insert(fqn) {
            return Err(Error::new(format!(
                "note codec component reported type FQN `{fqn}` more than once"
            )));
        }
        if standard_registry.contains(fqn) {
            return Err(Error::new(format!(
                "note codec component cannot replace the standard codec for `{fqn}`"
            )));
        }
        if !custom_type_fqns.contains(fqn) {
            return Err(Error::new(format!(
                "note codec component reported `{fqn}`, but that custom type does not appear in \
                 the package note storage schema"
            )));
        }
    }
    Ok(())
}

/// Classifies why a component call, or the instantiation that precedes it, failed.
///
/// The store records a refused memory or table growth, and it outranks the trap the guest saw:
/// the trap is only how the refusal surfaced.
fn classify_failure(store: &Store<ComponentStore>, error: &wasmtime::Error) -> CodecFailure {
    if store.data().limit_hit {
        return CodecFailure::LimitExceeded;
    }
    match error.downcast_ref::<Trap>() {
        Some(&Trap::OutOfFuel) => CodecFailure::OutOfFuel,
        Some(_) => CodecFailure::Trapped,
        // Wasmtime applies the store instance, table, and memory counts without asking the
        // limiter, so that failure carries no trap and no recorded refusal.
        None if reports_store_count_limit(error) => CodecFailure::LimitExceeded,
        None if reports_host_copy_budget(error) => CodecFailure::LimitExceeded,
        None => CodecFailure::Trapped,
    }
}

/// Returns true when one wasmtime error reports a store count that was reached.
fn reports_store_count_limit(error: &wasmtime::Error) -> bool {
    // Causes are matched one at a time from their start: the import stub error of a codec
    // embeds the import name, which the codec chooses.
    error
        .chain()
        .any(|cause| cause.to_string().starts_with(STORE_COUNT_LIMIT_PREFIX))
}

/// Returns true when one wasmtime error reports that a lift would exceed the host copy budget.
fn reports_host_copy_budget(error: &wasmtime::Error) -> bool {
    error.chain().any(|cause| cause.to_string() == HOST_COPY_BUDGET_EXHAUSTED)
}

/// Reports a component that did not reach its first exported call.
fn start_failure(store: &Store<ComponentStore>, error: wasmtime::Error) -> Error {
    Error::codec(
        classify_failure(store, &error),
        format!("note codec failed to start: {error:#}"),
    )
}

/// Enforces a byte-size cap on one component-returned string.
fn ensure_returned_string_limit(kind: &str, value: &str, limit: usize) -> Result<()> {
    if value.len() > limit {
        return Err(Error::codec(
            CodecFailure::LimitExceeded,
            format!(
                "note codec component returned a {kind} of {} bytes; the limit is {limit}",
                value.len()
            ),
        ));
    }
    Ok(())
}

/// Enforces the structural felt count cap on one `parse` result.
fn ensure_returned_felt_limit(fqn: &str, count: usize, limit: usize) -> Result<()> {
    if count > limit {
        return Err(Error::codec(
            CodecFailure::LimitExceeded,
            format!("codec `{fqn}` returned {count} felts from `parse`; the limit is {limit}"),
        ));
    }
    Ok(())
}

/// Converts component integers into canonical field elements.
fn component_values_to_felts(fqn: &str, values: &[u64]) -> Result<Vec<Felt>> {
    values
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| {
            Felt::new(value).map_err(|error| {
                Error::new(format!(
                    "codec `{fqn}` returned a noncanonical felt at index {index}: {error}"
                ))
            })
        })
        .collect()
}

/// Creates a host error for a component runtime failure.
///
/// The engine, the compilation, and the import stubs all fail this way, and a consumer sees
/// one class for a codec the engine did not run.
fn component_error(action: &str, error: impl core::fmt::Display) -> Error {
    Error::codec(CodecFailure::Trapped, format!("failed to {action}: {error:#}"))
}

/// Creates a host error for an author codec rejection.
fn codec_rejection(operation: &str, fqn: &str, message: String) -> Error {
    Error::codec(
        CodecFailure::Rejected,
        format!("codec `{fqn}` rejected `{operation}`: {message}"),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::{Command, Output},
        sync::{Arc, OnceLock},
    };

    use miden_core::{
        mast::{BasicBlockNodeBuilder, DenseMastForestBuilder, MastNodeExt},
        operations::Operation,
    };
    use miden_mast_package::{
        PackageExport, PackageId, PathBuf as MastPathBuf, ProcedureExport, Section, TargetType,
        Version,
    };
    use midenc_frontend_wasm_metadata::{
        NESTED_CARGO_SCRUB_ENV, package_note_storage_schema_section_id,
    };
    use midenc_integration_test_support::wasm_target_is_installed;
    use tempfile::TempDir;

    use super::*;

    const WASM_TARGET: &str = "wasm32-wasip2";
    /// The versioned interface a note codec component exports.
    const CODEC_INTERFACE: &str = "miden:note-codec/codec@1.0.0";
    const FIXTURE_FQN: &str = "example:codec-schema/note-storage@1.0.0.ratio";
    const DIGEST_FQN: &str = "miden:base/core-types@1.0.0.digest";
    const FIXTURE_SCHEMA: &str = r#"
package example:codec-schema@1.0.0;

interface note-storage {
    record ratio {
        numerator: u64,
        denominator: u64,
    }

    record codec-note {
        ratio: ratio,
    }

    type storage = codec-note;
}
"#;
    const EMBEDDED_CORE_SCHEMA: &str = r#"
package example:embedded-core-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{digest};
    record embedded-core-note { value: digest }
    type storage = embedded-core-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record word { a: felt, b: felt, c: felt, d: felt }
        record digest { inner: word }
    }
}
"#;

    #[test]
    fn local_note_codec_wit_matches_canonical_document() {
        let local = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/wit/note-codec.wit"));
        assert_eq!(local, miden_note_codec_wit::NOTE_CODEC_WIT);
    }

    #[test]
    fn component_boundary_rejects_noncanonical_felts() {
        let error =
            component_values_to_felts("example:test/codec.value", &[Felt::ORDER]).unwrap_err();

        assert!(error.to_string().contains("noncanonical felt at index 0"));
    }

    #[test]
    fn oversized_component_is_rejected_before_compilation() {
        let limits = CodecLimits::default();
        let bytes = vec![0; limits.max_component_bytes + 1];
        let error = ComponentRuntime::new(&bytes, limits.clone())
            .err()
            .expect("an oversized component must fail before compilation")
            .to_string();

        assert!(error.contains("pre-compilation limit"));
        assert!(error.contains(&(limits.max_component_bytes + 1).to_string()));
        assert!(error.contains(&limits.max_component_bytes.to_string()));
    }

    #[test]
    fn default_limits_match_the_published_policy() {
        let limits = CodecLimits::default();

        assert_eq!(limits.fuel, 10_000_000);
        assert_eq!(limits.max_memory_bytes, 16 * 1024 * 1024);
        assert_eq!(limits.max_table_elements, 4_096);
        assert_eq!(limits.max_component_bytes, crate::MAX_NOTE_CODEC_COMPONENT_BYTES);
        assert_eq!(limits.max_supported_types, 128);
        assert_eq!(limits.max_fqn_bytes, 512);
        assert_eq!(limits.max_returned_felts, 4_096);
        assert_eq!(limits.max_returned_string_bytes, 16 * 1024);
    }

    #[test]
    fn component_store_limits_bound_table_elements() {
        let policy = CodecLimits::default();
        let mut limits = component_store_limits(&policy);
        assert!(
            ResourceLimiter::table_growing(&mut limits, 0, policy.max_table_elements, None)
                .unwrap()
        );

        let error =
            ResourceLimiter::table_growing(&mut limits, 0, policy.max_table_elements + 1, None)
                .unwrap_err()
                .to_string();
        assert!(error.contains("growing table"), "unexpected table-limit error: {error}");
    }

    /// Builds a component that exports the note codec interface with a chosen
    /// `supported-types` body.
    ///
    /// `core_declarations` adds core module text, so a test can install a trapping start
    /// function. The other three interface functions carry their real signatures, which the
    /// generated bindings check, and share one core function that is never called.
    fn codec_shaped_component(core_declarations: &str, supported_types_body: &str) -> Vec<u8> {
        codec_shaped_component_with(
            core_declarations,
            supported_types_body,
            "",
            "",
            "",
            "(i32.const 1024)",
        )
    }

    /// Builds the codec-shaped component with guest cleanup for the discovery call (a no-op
    /// when the body is empty), extra canonical options on the discovery lift and on the value
    /// lifts, and one body shared by `parse`, `display` and `validate` that returns the result
    /// area.
    fn codec_shaped_component_with(
        core_declarations: &str,
        supported_types_body: &str,
        post_return_body: &str,
        supported_types_options: &str,
        operation_options: &str,
        operation_body: &str,
    ) -> Vec<u8> {
        let text = format!(
            r#"(component
                (core module $m
                    (memory (export "memory") 1)
                    {core_declarations}
                    (func (export "supported-types") (result i32) {supported_types_body})
                    (func (export "post-return") (param i32) {post_return_body})
                    (func (export "operation") (param i32 i32 i32 i32) (result i32)
                        {operation_body})
                    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
                        (i32.const 1024)))
                (core instance $i (instantiate $m))
                (func $supported (result (list string))
                    (canon lift (core func $i "supported-types")
                        (memory (core memory $i "memory"))
                        (realloc (core func $i "cabi_realloc"))
                        {supported_types_options}
                        (post-return (core func $i "post-return"))))
                (func $parse (param "type-fqn" string) (param "value" string)
                    (result (result (list u64) (error string)))
                    (canon lift (core func $i "operation")
                        (memory (core memory $i "memory"))
                        (realloc (core func $i "cabi_realloc"))
                        {operation_options}))
                (func $display (param "type-fqn" string) (param "value" (list u64))
                    (result (result string (error string)))
                    (canon lift (core func $i "operation")
                        (memory (core memory $i "memory"))
                        (realloc (core func $i "cabi_realloc"))
                        {operation_options}))
                (func $validate (param "type-fqn" string) (param "value" (list u64))
                    (result (result (error string)))
                    (canon lift (core func $i "operation")
                        (memory (core memory $i "memory"))
                        (realloc (core func $i "cabi_realloc"))
                        {operation_options}))
                (instance $codec
                    (export "supported-types" (func $supported))
                    (export "parse" (func $parse))
                    (export "display" (func $display))
                    (export "validate" (func $validate)))
                (export "{CODEC_INTERFACE}" (instance $codec)))"#
        );
        wat::parse_str(text).unwrap()
    }

    /// Renders one value as the four little-endian bytes of a WAT data string.
    fn le(value: usize) -> String {
        u32::try_from(value)
            .unwrap()
            .to_le_bytes()
            .map(|byte| format!("\\{byte:02x}"))
            .concat()
    }

    /// Returns core module text that reports `count` string descriptors, all aliasing the
    /// string at `ptr` whose length word is `length`: code units, tagged for `latin1+utf16`. The bytes `abcd` live at 0, the list header at 8
    /// and the descriptors from 16 on, so a string other than `abcd` must start past
    /// `16 + 8 * count`.
    fn descriptors(count: usize, ptr: usize, length: usize) -> String {
        let descriptor = format!("{}{}", le(ptr), le(length)).repeat(count);
        format!(
            r#"(data (i32.const 0) "abcd")
               (data (i32.const 8) "\10\00\00\00{}")
               (data (i32.const 16) "{descriptor}")"#,
            le(count)
        )
    }

    /// Pins the host copy budget to the largest single result the caps admit, and its ceiling
    /// to wasmtime's own default.
    #[test]
    fn hostcall_fuel_covers_the_largest_result_within_the_caps() {
        let string = size_of::<String>();
        let defaults = CodecLimits::default();
        // 128 names of 512 UTF-8 bytes, each up to 1024 bytes in guest UTF-16, plus one owned
        // `String` per name.
        assert_eq!(defaults.hostcall_fuel(), 128 * (string + 1024));

        let felts = CodecLimits {
            max_returned_felts: 100_000,
            ..defaults.clone()
        };
        assert_eq!(felts.hostcall_fuel(), 100_000 * 8);

        let strings = CodecLimits {
            max_returned_string_bytes: 1_000_000,
            ..defaults.clone()
        };
        assert_eq!(strings.hostcall_fuel(), 2 * 1_000_000);

        let extreme = CodecLimits {
            max_supported_types: usize::MAX,
            max_fqn_bytes: usize::MAX,
            max_returned_felts: usize::MAX,
            max_returned_string_bytes: usize::MAX,
            ..defaults
        };
        assert_eq!(extreme.hostcall_fuel(), MAX_HOSTCALL_FUEL);
        assert_eq!(Store::new(&Engine::default(), ()).hostcall_fuel(), MAX_HOSTCALL_FUEL);
    }

    /// Accepts a discovery list at both caps.
    #[test]
    fn discovery_accepts_the_maximum_count_of_maximal_fqns() {
        // Every descriptor covers the same zeroed region past the descriptor table, so each
        // name is valid UTF-8 of exactly the byte cap, and the count is exactly the count cap.
        let limits = CodecLimits::default();
        let component = codec_shaped_component(
            &descriptors(limits.max_supported_types, 2048, limits.max_fqn_bytes),
            "(i32.const 8)",
        );

        let fqns = ComponentRuntime::new(&component, limits.clone())
            .unwrap()
            .supported_types()
            .expect("a list at both caps must be accepted");

        assert_eq!(fqns.len(), limits.max_supported_types);
        assert!(fqns.iter().all(|fqn| fqn.len() == limits.max_fqn_bytes));
    }

    /// Accepts a discovery list at both caps in each encoding that stores a name in up to
    /// twice its UTF-8 length, and refuses one code unit more per name inside the call.
    #[test]
    fn discovery_accepts_utf16_names_at_both_caps() {
        // At the default caps a budget that counted names at their UTF-8 length plus the other
        // results' terms would also admit this list, so a count cap of 1024 leaves the
        // discovery term alone to decide. Each name is zeroed UTF-16 code units, one UTF-8 byte
        // each. `latin1+utf16` tags a UTF-16 length with the top bit; read untagged, one unit
        // over would lift as Latin-1 within the budget and reach the byte cap instead.
        let limits = CodecLimits {
            max_supported_types: 1024,
            ..CodecLimits::default()
        };
        const UTF16_TAG: usize = 1 << 31;
        for (encoding, tag) in
            [("string-encoding=utf16", 0), ("string-encoding=latin1+utf16", UTF16_TAG)]
        {
            let discover = |units: usize| {
                let component = codec_shaped_component_with(
                    &descriptors(limits.max_supported_types, 16384, units | tag),
                    "(i32.const 8)",
                    "",
                    encoding,
                    "",
                    "(i32.const 1024)",
                );
                ComponentRuntime::new(&component, limits.clone()).unwrap().supported_types()
            };

            let fqns = discover(limits.max_fqn_bytes)
                .unwrap_or_else(|error| panic!("a {encoding} list at both caps failed: {error}"));
            assert_eq!(fqns.len(), limits.max_supported_types);
            assert!(fqns.iter().all(|fqn| fqn.len() == limits.max_fqn_bytes));

            let over = discover(limits.max_fqn_bytes + 1)
                .expect_err("a list past the host copy budget must be refused");
            assert!(
                over.to_string().contains("failed in `supported-types`"),
                "{encoding}: unexpected error: {over}"
            );
            assert_eq!(over.codec_failure(), Some(CodecFailure::LimitExceeded));
        }
    }

    /// Rejects one name one byte over the byte cap with the cap's own message.
    #[test]
    fn discovery_rejects_a_name_over_the_byte_cap() {
        let limits = CodecLimits::default();
        let length = limits.max_fqn_bytes + 1;
        let component = codec_shaped_component(&descriptors(1, 2048, length), "(i32.const 8)");

        let error = ComponentRuntime::new(&component, limits)
            .unwrap()
            .supported_types()
            .expect_err("a name over the byte cap must be rejected");

        assert!(
            error.to_string().contains(&format!("type FQN of {length} bytes")),
            "unexpected error: {error}"
        );
        assert_eq!(error.codec_failure(), Some(CodecFailure::LimitExceeded));
    }

    /// Lifts a UTF-16 `display` value at the string cap and refuses one code unit more inside
    /// the call, where the value string is the largest result the caps admit, and refuses a
    /// UTF-8 value one byte over the cap, which fits the budget, with the cap's own message.
    #[test]
    fn display_results_lift_utf16_up_to_the_string_cap() {
        let limits = CodecLimits {
            max_supported_types: 1,
            max_fqn_bytes: 8,
            max_returned_felts: 4,
            max_returned_string_bytes: 64,
            ..CodecLimits::default()
        };
        assert_eq!(limits.hostcall_fuel(), 2 * limits.max_returned_string_bytes);
        // `result<string, string>` at 512: the `ok` tag, then the pointer and the length in
        // code units of a zeroed string at 2048, which decodes to one UTF-8 byte per unit.
        let display = |encoding: &str, units: usize| {
            let component = codec_shaped_component_with(
                &format!(r#"(data (i32.const 512) "\00\00\00\00{}{}")"#, le(2048), le(units)),
                "(i32.const 8)",
                "",
                "",
                encoding,
                "(i32.const 512)",
            );
            ComponentCodec {
                fqn: "example:codec/types@1.0.0.value".to_owned(),
                runtime: Arc::new(ComponentRuntime::new(&component, limits.clone()).unwrap()),
            }
            .display(&[])
        };

        let cap = limits.max_returned_string_bytes;
        let at_cap = display("string-encoding=utf16", cap)
            .expect("a UTF-16 display value at the string cap must lift");
        assert_eq!(at_cap.len(), cap);

        let over_budget = display("string-encoding=utf16", cap + 1)
            .expect_err("a display value past the host copy budget must be refused");
        assert!(
            over_budget.to_string().contains("exceeded a resource limit in `display`"),
            "unexpected error: {over_budget}"
        );
        assert_eq!(over_budget.codec_failure(), Some(CodecFailure::LimitExceeded));

        let over_cap =
            display("", cap + 1).expect_err("a display value over the string cap must be rejected");
        assert!(
            over_cap
                .to_string()
                .contains(&format!("display value of {} bytes; the limit is {cap}", cap + 1)),
            "unexpected error: {over_cap}"
        );
        assert_eq!(over_cap.codec_failure(), Some(CodecFailure::LimitExceeded));
    }

    /// Classifies wasmtime's store-count and host-copy errors as limits by matching each cause
    /// on its own, so the import name a codec chooses, which the import stub error embeds,
    /// cannot pass as either.
    #[test]
    fn limit_errors_are_classified_by_their_own_cause() {
        let store = Store::new(
            &Engine::default(),
            ComponentStore {
                limits: component_store_limits(&CodecLimits::default()),
                limit_hit: false,
            },
        );
        let store_count =
            wasmtime::format_err!("resource limit exceeded: instance count too high at 33");
        let host_copy = wasmtime::format_err!("{HOST_COPY_BUDGET_EXHAUSTED}");
        let spoofed = wasmtime::format_err!(
            "unknown import: `url=<resource limit exceeded: instance count too high at 9 \
             {HOST_COPY_BUDGET_EXHAUSTED}>` has not been defined"
        );

        assert_eq!(classify_failure(&store, &store_count), CodecFailure::LimitExceeded);
        assert_eq!(classify_failure(&store, &host_copy), CodecFailure::LimitExceeded);
        assert_eq!(classify_failure(&store, &spoofed), CodecFailure::Trapped);
    }

    /// Refuses a discovery list whose names alias more guest bytes than the budget inside the
    /// call.
    #[test]
    fn discovery_host_copies_are_budgeted_before_the_byte_cap() {
        // The count is within its cap and every descriptor stays in bounds of the one-page
        // memory, but together the names alias far more bytes than the host copy budget. The
        // budget refuses the list inside the call, before the byte cap sees the first name.
        let limits = CodecLimits::default();
        let component = codec_shaped_component(
            &descriptors(limits.max_supported_types, 2048, 60_000),
            "(i32.const 8)",
        );

        let error = ComponentRuntime::new(&component, limits)
            .unwrap()
            .supported_types()
            .expect_err("a list past the host copy budget must be refused");

        assert!(
            error.to_string().contains("failed in `supported-types`"),
            "unexpected error: {error}"
        );
        assert_eq!(
            error.codec_failure(),
            Some(CodecFailure::LimitExceeded),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn discovery_rejects_a_reported_count_over_the_limit() {
        // Every descriptor points at the same four bytes, so the list is over the count limit
        // while every element stays within the byte limit.
        let limits = CodecLimits::default();
        let reported = limits.max_supported_types + 1;
        let component = codec_shaped_component(&descriptors(reported, 0, 4), "(i32.const 8)");

        let error = ComponentRuntime::new(&component, limits.clone())
            .unwrap()
            .supported_types()
            .expect_err("a component over the supported-type limit must be rejected");

        assert!(
            error.to_string().contains(&format!("reported {reported} types")),
            "unexpected limit error: {error}"
        );
        assert!(
            error
                .to_string()
                .contains(&format!("the limit is {}", limits.max_supported_types)),
            "unexpected limit error: {error}"
        );
        assert_eq!(error.codec_failure(), Some(CodecFailure::LimitExceeded));
    }

    #[test]
    fn a_trapping_start_function_is_classified() {
        let component = codec_shaped_component(
            r#"(func $boom unreachable)
               (start $boom)"#,
            "(i32.const 8)",
        );

        let error = ComponentRuntime::new(&component, CodecLimits::default())
            .unwrap()
            .supported_types()
            .expect_err("a trapping start function must fail instantiation");

        assert!(error.to_string().contains("failed to start"), "unexpected error: {error}");
        assert_eq!(error.codec_failure(), Some(CodecFailure::Trapped));
    }

    #[test]
    fn discovery_that_runs_out_of_fuel_is_classified() {
        let component = codec_shaped_component("", "(loop $spin (br $spin)) (i32.const 8)");

        let error = ComponentRuntime::new(&component, CodecLimits::default())
            .unwrap()
            .supported_types()
            .expect_err("an endless `supported-types` must exhaust its fuel");

        assert!(
            error.to_string().contains("failed in `supported-types`"),
            "unexpected error: {error}"
        );
        assert_eq!(error.codec_failure(), Some(CodecFailure::OutOfFuel));
    }

    /// Classifies a post-return that spins, grows memory past the limit, or traps like the call
    /// it cleans up after.
    #[test]
    fn discovery_post_return_failures_are_classified() {
        for (post_return_body, expected) in [
            ("(loop $spin (br $spin))", CodecFailure::OutOfFuel),
            ("(i32.const 256) (memory.grow) drop", CodecFailure::LimitExceeded),
            ("unreachable", CodecFailure::Trapped),
        ] {
            let component = codec_shaped_component_with(
                "",
                "(i32.const 8)",
                post_return_body,
                "",
                "",
                "(i32.const 1024)",
            );
            let error = ComponentRuntime::new(&component, CodecLimits::default())
                .unwrap()
                .supported_types()
                .expect_err("a failing post-return must reject codec discovery");

            assert!(
                error.to_string().contains("failed in `supported-types`"),
                "unexpected error: {error}"
            );
            assert_eq!(error.codec_failure(), Some(expected), "unexpected error: {error}");
        }
    }

    #[test]
    fn a_package_without_a_codec_section_keeps_the_standard_codecs() {
        let mut package = test_package();
        package.sections.push(Section::new(
            package_note_storage_schema_section_id(),
            FIXTURE_SCHEMA.as_bytes().to_vec(),
        ));

        let registry = CodecRegistry::load_from_package(&package).unwrap();

        assert!(registry.contains(crate::FELT_FQN));
        assert!(registry.contains(crate::WORD_FQN));
        assert!(!registry.contains(FIXTURE_FQN));
    }

    #[test]
    fn a_package_without_a_schema_or_a_codec_section_keeps_the_standard_codecs() {
        // A unit `#[note]` struct emits neither section, and a host may probe any package.
        let package = test_package();

        let registry = CodecRegistry::load_from_package(&package).unwrap();

        assert!(registry.contains(crate::FELT_FQN));
        assert!(registry.contains(crate::WORD_FQN));
        assert!(!registry.contains(FIXTURE_FQN));
    }

    #[test]
    fn simd_components_do_not_load() {
        let component = wat::parse_str(
            r#"(component
                (core module
                    (func (export "simd")
                        v128.const i32x4 0 0 0 0
                        drop)))"#,
        )
        .unwrap();

        let error = ComponentRuntime::new(&component, CodecLimits::default())
            .err()
            .expect("a SIMD component must not load")
            .to_string();

        assert!(error.contains("SIMD"), "unexpected engine policy error: {error}");
    }

    #[test]
    fn threads_components_do_not_load() {
        let component = wat::parse_str(
            r#"(component
                (core module
                    (memory 1 1 shared)))"#,
        )
        .unwrap();

        let error = ComponentRuntime::new(&component, CodecLimits::default())
            .err()
            .expect("a threads component must not load")
            .to_string();

        assert!(error.contains("threads"), "unexpected engine policy error: {error}");
    }

    #[test]
    fn gc_components_do_not_load() {
        let component = wat::parse_str(
            r#"(component
                (core module
                    (type (struct))))"#,
        )
        .unwrap();

        let error = ComponentRuntime::new(&component, CodecLimits::default())
            .err()
            .expect("a GC component must not load")
            .to_string();

        assert!(error.contains("gc"), "unexpected engine policy error: {error}");
    }

    #[test]
    fn nonstandard_embedded_core_type_is_author_codec_eligible() {
        let schema = NoteStorageSchema::from_wit_text(EMBEDDED_CORE_SCHEMA).unwrap();
        let custom_types = schema.custom_type_fqns();

        assert!(custom_types.contains(DIGEST_FQN));
        assert!(!custom_types.contains(crate::FELT_FQN));
        assert!(!custom_types.contains(crate::WORD_FQN));
        validate_reported_fqns(&[DIGEST_FQN.to_owned()], &custom_types, &CodecRegistry::default())
            .unwrap();
    }

    #[test]
    fn package_component_registers_and_dispatches_author_codec() {
        if !wasm_target_is_installed() {
            eprintln!("skipping component adapter test: {WASM_TARGET} is not installed");
            return;
        }

        let component = build_fixture_component();
        let mut package = test_package();
        package.sections.push(Section::new(
            package_note_storage_schema_section_id(),
            FIXTURE_SCHEMA.as_bytes().to_vec(),
        ));
        package.sections.push(Section::new(package_note_codec_section_id(), component));

        let registry = CodecRegistry::load_from_package(&package).unwrap();
        let codec = registry.codec(FIXTURE_FQN).expect("fixture ratio codec was not registered");
        let encoded = codec.parse("3/2").unwrap();
        assert_eq!(encoded, [Felt::new(3).unwrap(), Felt::ZERO, Felt::new(2).unwrap(), Felt::ZERO]);
        codec.validate(&encoded).unwrap();
        assert_eq!(codec.display(&encoded).unwrap(), "3/2");

        let invalid = codec.parse("3/0").unwrap();
        assert!(codec.validate(&invalid).unwrap_err().to_string().contains("denominator"));
    }

    #[test]
    fn component_calls_recover_after_traps_and_enforce_fuel() {
        if !wasm_target_is_installed() {
            eprintln!("skipping component adapter test: {WASM_TARGET} is not installed");
            return;
        }

        let schema = NoteStorageSchema::from_wit_text(FIXTURE_SCHEMA).unwrap();
        let registry = CodecRegistry::load_from_component(
            &build_fixture_component(),
            &schema.custom_type_fqns(),
            CodecLimits::default(),
        )
        .unwrap();
        let codec = registry.codec(FIXTURE_FQN).unwrap();

        let trap = codec.parse("trap").unwrap_err();
        assert!(trap.to_string().contains("trapped in `parse`"), "unexpected error: {trap}");
        assert_eq!(trap.codec_failure(), Some(CodecFailure::Trapped));
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");

        let oversized_input = "x".repeat(CodecLimits::default().max_memory_bytes + 1);
        let limit = codec.parse(&oversized_input).unwrap_err();
        assert!(
            limit.to_string().contains("exceeded a resource limit in `parse`"),
            "unexpected error: {limit}"
        );
        assert_eq!(limit.codec_failure(), Some(CodecFailure::LimitExceeded));
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");

        let fuel = codec.parse("loop").unwrap_err();
        assert!(fuel.to_string().contains("ran out of fuel"), "unexpected budget error: {fuel}");
        assert_eq!(fuel.codec_failure(), Some(CodecFailure::OutOfFuel));
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");

        // An author rejection is not a sandbox failure; it carries the codec's own message.
        let rejected = codec.validate(&codec.parse("3/0").unwrap()).unwrap_err();
        assert!(rejected.to_string().contains("denominator"), "unexpected error: {rejected}");
        assert_eq!(rejected.codec_failure(), Some(CodecFailure::Rejected));
    }

    #[test]
    fn limits_reject_a_component_that_reports_too_many_types() {
        if !wasm_target_is_installed() {
            eprintln!("skipping component adapter test: {WASM_TARGET} is not installed");
            return;
        }

        let mut package = test_package();
        package.sections.push(Section::new(
            package_note_storage_schema_section_id(),
            FIXTURE_SCHEMA.as_bytes().to_vec(),
        ));
        package
            .sections
            .push(Section::new(package_note_codec_section_id(), build_fixture_component()));
        let limits = CodecLimits {
            max_supported_types: 0,
            ..CodecLimits::default()
        };

        let error = CodecRegistry::load_from_package_with_limits(&package, limits)
            .err()
            .expect("a zero supported-type limit must reject the fixture codec")
            .to_string();

        assert!(error.contains("reported 1 types"), "unexpected limit error: {error}");
        assert!(error.contains("the limit is 0"), "unexpected limit error: {error}");
    }

    #[test]
    fn reported_fqns_cannot_replace_standard_or_unrelated_types() {
        let standard = CodecRegistry::default();
        let allowed = HashSet::from([FIXTURE_FQN.to_owned()]);

        let collision =
            validate_reported_fqns(&[crate::ACCOUNT_ID_FQN.to_owned()], &allowed, &standard)
                .unwrap_err()
                .to_string();
        assert!(collision.contains("cannot replace the standard codec"));

        let unrelated = validate_reported_fqns(
            &["example:other/schema@1.0.0.value".to_owned()],
            &allowed,
            &standard,
        )
        .unwrap_err()
        .to_string();
        assert!(unrelated.contains("does not appear in the package note storage schema"));

        let duplicate = validate_reported_fqns(
            &[FIXTURE_FQN.to_owned(), FIXTURE_FQN.to_owned()],
            &allowed,
            &standard,
        )
        .unwrap_err()
        .to_string();
        assert!(duplicate.contains("more than once"));
    }

    #[test]
    fn returned_values_are_size_limited() {
        let limits = CodecLimits::default();
        let long = "x".repeat(limits.max_returned_string_bytes + 1);
        assert!(
            ensure_returned_string_limit("display value", &long, limits.max_returned_string_bytes)
                .unwrap_err()
                .to_string()
                .contains("the limit is")
        );
        let felts = ensure_returned_felt_limit(
            FIXTURE_FQN,
            limits.max_returned_felts + 1,
            limits.max_returned_felts,
        )
        .unwrap_err();
        assert!(felts.to_string().contains("the limit is"), "unexpected error: {felts}");
        assert_eq!(felts.codec_failure(), Some(CodecFailure::LimitExceeded));
    }

    #[test]
    fn package_readers_reject_duplicate_schema_and_codec_sections() {
        let schema_id = package_note_storage_schema_section_id();
        let codec_id = package_note_codec_section_id();
        let mut duplicate_schema = test_package();
        duplicate_schema
            .sections
            .push(Section::new(schema_id.clone(), FIXTURE_SCHEMA.as_bytes().to_vec()));
        duplicate_schema
            .sections
            .push(Section::new(schema_id, FIXTURE_SCHEMA.as_bytes().to_vec()));
        assert!(
            NoteStorageSchema::from_package(&duplicate_schema)
                .err()
                .expect("duplicate schema sections must fail")
                .to_string()
                .contains("more than one `note_storage_schema` section")
        );

        let mut duplicate_codec = test_package();
        duplicate_codec.sections.push(Section::new(
            package_note_storage_schema_section_id(),
            FIXTURE_SCHEMA.as_bytes().to_vec(),
        ));
        duplicate_codec.sections.push(Section::new(codec_id.clone(), Vec::new()));
        duplicate_codec.sections.push(Section::new(codec_id, Vec::new()));
        assert!(
            CodecRegistry::load_from_package(&duplicate_codec)
                .err()
                .expect("duplicate codec sections must fail")
                .to_string()
                .contains("more than one `note_codec` section")
        );
    }

    /// Builds a valid package with one procedure export.
    fn test_package() -> Package {
        let mut builder = DenseMastForestBuilder::new();
        let node_id = builder
            .push_node(BasicBlockNodeBuilder::new(vec![Operation::Add]))
            .expect("failed to build package procedure");
        builder.mark_root(node_id);
        let (forest, remapping) = builder.build_with_id_map().expect("failed to build package");
        let node_id = remapping.get(node_id).expect("package root was removed");
        let export = ProcedureExport::new(
            MastPathBuf::absolute("component-codec-test::run").into(),
            Some(node_id),
            forest[node_id].digest(),
            None,
        );

        Package::create(
            PackageId::from("component-codec-test"),
            Version::new(0, 0, 0),
            TargetType::Library,
            Arc::new(forest),
            [PackageExport::Procedure(export)],
            [],
        )
        .expect("failed to create test package")
    }

    /// Builds the minimal author codec the component adapter tests dispatch through.
    pub(crate) fn build_fixture_component() -> Vec<u8> {
        static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
        COMPONENT.get_or_init(build_fixture_component_uncached).clone()
    }

    /// Builds the component fixture once for this test process.
    fn build_fixture_component_uncached() -> Vec<u8> {
        let fixture = TempDir::new().expect("failed to create component fixture directory");
        write_fixture(fixture.path());
        let target_dir = workspace_root().join("target/note-schema-component-test");
        let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        command
            .args([
                "build",
                "--manifest-path",
                fixture.path().join("Cargo.toml").to_str().unwrap(),
                "--release",
                "--target",
                WASM_TARGET,
                "--offline",
            ])
            .env("CARGO_TARGET_DIR", &target_dir)
            // Share the hash-keyed intermediates with the other test builds; only the
            // name-keyed final artifacts stay in the directory above. Convention from
            // `tests/support`.
            .env("CARGO_BUILD_BUILD_DIR", workspace_root().join("target/miden_build_cache"));
        for &variable in NESTED_CARGO_SCRUB_ENV {
            command.env_remove(variable);
        }
        // Pin the guest rustflags after the scrub, the way the compiler builds a codec crate.
        command.env("RUSTFLAGS", crate::NOTE_CODEC_GUEST_RUSTFLAGS);
        let output = command.output().expect("failed to start fixture build");
        assert_command_succeeded("building the component adapter fixture", &output);

        fs::read(
            target_dir.join(format!("{WASM_TARGET}/release/note_schema_component_fixture.wasm")),
        )
        .expect("component fixture did not produce its Wasm component")
    }

    /// Writes a standalone codec crate for the component adapter test.
    fn write_fixture(root: &Path) {
        fs::create_dir(root.join("src")).unwrap();
        // Seed the workspace lockfile so the offline build resolves to the versions that the
        // workspace already downloaded instead of racing the registry index.
        fs::copy(workspace_root().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
        let codec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../note-codec");
        let manifest = format!(
            r#"[package]
name = "note-schema-component-fixture"
version = "0.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden-note-codec = {{ path = {:?} }}

[workspace]
"#,
            codec_path
        );
        fs::write(root.join("Cargo.toml"), manifest).unwrap();
        fs::write(root.join("src/lib.rs"), fixture_source()).unwrap();
    }

    /// Returns the compiler workspace root.
    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_owned()
    }

    /// Includes both output streams when one fixture command fails.
    fn assert_command_succeeded(action: &str, output: &Output) {
        assert!(
            output.status.success(),
            "failed while {action}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    /// Builds the fixture source around the schema shared with host-side assertions.
    fn fixture_source() -> String {
        [
            r##"
use miden_note_codec::AuthorTypeCodec;

miden_note_codec::from_wit_text!(r#""##,
            FIXTURE_SCHEMA,
            r##""#);

#[miden_note_codec::note_codec]
impl AuthorTypeCodec for Ratio {
    fn parse(value: &str) -> Result<Self, String> {
        if value == "trap" {
            panic!("fixture trap");
        }
        if value == "loop" {
            loop {
                std::hint::black_box(());
            }
        }
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or_else(|| "a ratio must use `numerator/denominator`".to_owned())?;
        Ok(Self {
            numerator: numerator.parse::<u64>().map_err(|error| error.to_string())?,
            denominator: denominator.parse::<u64>().map_err(|error| error.to_string())?,
        })
    }

    fn display(&self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    fn validate(&self) -> Result<(), String> {
        if self.denominator == 0 {
            Err("the denominator must not be zero".to_owned())
        } else {
            Ok(())
        }
    }
}

miden_note_codec::export_codecs!();
"##,
        ]
        .concat()
    }
}
