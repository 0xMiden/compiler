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
    validate_note_codec_structure,
};

/// Maximum bytes of stack available to one component call.
const MAX_WASM_STACK_BYTES: usize = 512 * 1024;

wasmtime::component::bindgen!({
    path: "wit",
    world: "note-codec",
});

/// Runtime limits a host applies to bundled note codecs. Limits are host policy.
/// A package carries no limit values and cannot raise them. Hosts in one
/// deployment should run identical limits, so a codec behaves the same everywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecLimits {
    /// Fuel budget for one codec call, about one unit per Wasm instruction.
    pub fuel: u64,
    /// Cap on the guest linear memory in bytes.
    pub max_memory_bytes: usize,
    /// Cap on the total table elements.
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

impl CodecRegistry {
    /// Loads the note codec component from a package under the default limits.
    pub fn load_from_package(package: &Package) -> Result<Self> {
        Self::load_from_package_with_limits(package, CodecLimits::default())
    }

    /// Loads the note codec component from a package and registers all reported types.
    pub fn load_from_package_with_limits(package: &Package, limits: CodecLimits) -> Result<Self> {
        let schema = NoteStorageSchema::from_package(package)?;
        let bytes = crate::section::unique_package_section(
            package,
            package_note_codec_section_id(),
            PACKAGE_NOTE_CODEC_SECTION_ID,
        )?;
        Self::load_from_component(bytes, &schema.custom_type_fqns(), limits)
    }

    /// Loads a note codec component whose only imports are stubbed WASI interfaces.
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
    engine: Engine,
    component: Component,
    limits: CodecLimits,
}

/// Store state that owns the component resource limits.
struct ComponentStore {
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
    store: Store<ComponentStore>,
    bindings: NoteCodec,
}

impl ComponentRuntime {
    /// Compiles a component under the explicit engine policy and the given limits.
    fn new(bytes: &[u8], limits: CodecLimits) -> Result<Self> {
        ensure_component_byte_limit(bytes.len(), limits.max_component_bytes)?;
        validate_note_codec_structure(bytes)?;
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        // Proposals the wasm32-wasip2 target emits by default.
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);
        // Validation policy: every other proposal is off by name. The engine defaults change
        // between versions, and they decide which components load on every host.
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        config.wasm_tail_call(false);
        config.wasm_extended_const(false);
        config.wasm_custom_page_sizes(false);
        config.wasm_wide_arithmetic(false);
        config.wasm_shared_everything_threads(false);
        config.wasm_stack_switching(false);
        config.wasm_exceptions(false);
        // Floats stay on: codecs parse and format decimal text. NaN canonicalization keeps
        // float results identical across hosts.
        config.cranelift_nan_canonicalization(true);
        config.max_wasm_stack(MAX_WASM_STACK_BYTES);
        config.wasm_backtrace(false);
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
        let bindings = NoteCodec::instantiate(&mut store, &self.component, &linker)
            .map_err(|error| component_error("instantiate the note codec", error))?;
        Ok(ComponentInstance { store, bindings })
    }

    /// Queries the component's supported FQNs once during registry construction.
    fn supported_types(&self) -> Result<Vec<String>> {
        let mut instance = self.instantiate()?;
        let fqns = instance
            .bindings
            .miden_note_codec_codec()
            .call_supported_types(&mut instance.store)
            .map_err(|error| component_error("call `supported-types`", error))?;
        if fqns.len() > self.limits.max_supported_types {
            return Err(Error::new(format!(
                "note codec component reported {} types; the limit is {}",
                fqns.len(),
                self.limits.max_supported_types
            )));
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
        .instances(32)
        .tables(32)
        .memories(1)
        .trap_on_grow_failure(true)
        .build()
}

/// Rejects oversized component bytes before validation or JIT compilation begins.
fn ensure_component_byte_limit(byte_len: usize, limit: usize) -> Result<()> {
    if byte_len > limit {
        return Err(Error::new(format!(
            "note codec component is {byte_len} bytes; the pre-compilation limit is {limit}"
        )));
    }
    Ok(())
}

/// A registry entry that dispatches one FQN through isolated component instances.
struct ComponentCodec {
    fqn: String,
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

    /// Classifies why one component call did not return a value.
    fn call_failure(
        &self,
        operation: &str,
        store: &Store<ComponentStore>,
        error: wasmtime::Error,
    ) -> Error {
        let fqn = &self.fqn;
        if store.data().limit_hit {
            Error::codec(
                CodecFailure::LimitExceeded,
                format!("note codec `{fqn}` exceeded a resource limit in `{operation}`"),
            )
        } else if error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) {
            Error::codec(
                CodecFailure::OutOfFuel,
                format!("note codec `{fqn}` ran out of fuel in `{operation}`"),
            )
        } else {
            Error::codec(
                CodecFailure::Trapped,
                format!("note codec `{fqn}` trapped in `{operation}`: {error:#}"),
            )
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

/// Enforces a byte-size cap on one component-returned string.
fn ensure_returned_string_limit(kind: &str, value: &str, limit: usize) -> Result<()> {
    if value.len() > limit {
        return Err(Error::new(format!(
            "note codec component returned a {kind} of {} bytes; the limit is {limit}",
            value.len()
        )));
    }
    Ok(())
}

/// Enforces the structural felt count cap on one `parse` result.
fn ensure_returned_felt_limit(fqn: &str, count: usize, limit: usize) -> Result<()> {
    if count > limit {
        return Err(Error::new(format!(
            "codec `{fqn}` returned {count} felts from `parse`; the limit is {limit}"
        )));
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
fn component_error(action: &str, error: impl core::fmt::Display) -> Error {
    Error::new(format!("failed to {action}: {error:#}"))
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
        assert!(
            ensure_returned_felt_limit(
                FIXTURE_FQN,
                limits.max_returned_felts + 1,
                limits.max_returned_felts
            )
            .unwrap_err()
            .to_string()
            .contains("the limit is")
        );
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

    /// Builds the minimal author codec used by the Phase 4a component spike.
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
