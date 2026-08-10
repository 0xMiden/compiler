//! Consumer adapters for author codec components.

use std::{collections::HashSet, sync::Arc};

use miden_field::Felt;
use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::PACKAGE_NOTE_CODEC_SECTION_ID;
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, Linker},
};

use crate::{CodecRegistry, ConsumerTypeCodec, Error, NoteStorageSchema, Result};

/// Maximum Wasm instructions available to one codec operation.
const CALL_FUEL: u64 = 10_000_000;

/// Maximum bytes available to one codec component linear memory.
const MAX_COMPONENT_MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Maximum FQNs accepted from `supported-types`.
const MAX_SUPPORTED_TYPES: usize = 128;

/// Maximum bytes accepted in one reported FQN.
const MAX_FQN_BYTES: usize = 512;

/// Maximum felts accepted from `parse`.
const MAX_RETURNED_FELTS: usize = 4_096;

/// Maximum bytes accepted in one component-returned string.
const MAX_RETURNED_STRING_BYTES: usize = 16 * 1024;

wasmtime::component::bindgen!({
    path: "../note-codec/wit",
    world: "note-codec",
});

impl CodecRegistry {
    /// Loads the note codec component from a package and registers all reported types.
    pub fn load_from_package(package: &Package) -> Result<Self> {
        let schema = NoteStorageSchema::from_package(package)?;
        let bytes = crate::section::unique_package_section(package, PACKAGE_NOTE_CODEC_SECTION_ID)?;
        Self::load_from_component(bytes, &schema.custom_type_fqns())
    }

    /// Loads a zero-import note codec component.
    fn load_from_component(bytes: &[u8], custom_type_fqns: &HashSet<String>) -> Result<Self> {
        let runtime = Arc::new(ComponentRuntime::new(bytes)?);
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
}

/// Store state that owns the component resource limits.
struct ComponentStore {
    limits: StoreLimits,
}

/// One isolated codec component call context.
struct ComponentInstance {
    store: Store<ComponentStore>,
    bindings: NoteCodec,
}

impl ComponentRuntime {
    /// Compiles a component with fuel accounting enabled.
    fn new(bytes: &[u8]) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config)
            .map_err(|error| component_error("create the Wasmtime engine", error))?;
        let component = Component::new(&engine, bytes)
            .map_err(|error| component_error("compile the note codec component", error))?;
        Ok(Self { engine, component })
    }

    /// Instantiates a zero-import component with fresh per-call limits.
    fn instantiate(&self) -> Result<ComponentInstance> {
        let linker = Linker::new(&self.engine);
        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_COMPONENT_MEMORY_BYTES)
            .instances(32)
            .tables(32)
            .memories(1)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&self.engine, ComponentStore { limits });
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(CALL_FUEL)
            .map_err(|error| component_error("set the note codec fuel budget", error))?;
        let bindings = NoteCodec::instantiate(&mut store, &self.component, &linker)
            .map_err(|error| component_error("instantiate the zero-import note codec", error))?;
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
        if fqns.len() > MAX_SUPPORTED_TYPES {
            return Err(Error::new(format!(
                "note codec component reported {} types; the limit is {MAX_SUPPORTED_TYPES}",
                fqns.len()
            )));
        }
        for fqn in &fqns {
            ensure_returned_string_limit("type FQN", fqn, MAX_FQN_BYTES)?;
        }
        Ok(fqns)
    }
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
        call(&instance.bindings, &mut instance.store).map_err(|error| {
            component_error(&format!("call `{operation}` for codec `{}`", self.fqn), error)
        })
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
                ensure_returned_string_limit("codec error", &message, MAX_RETURNED_STRING_BYTES)?;
                return Err(codec_rejection("parse", &self.fqn, message));
            }
        };
        ensure_returned_felt_limit(&self.fqn, values.len())?;
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
                ensure_returned_string_limit("codec error", &message, MAX_RETURNED_STRING_BYTES)?;
                return Err(codec_rejection("display", &self.fqn, message));
            }
        };
        ensure_returned_string_limit("display value", &display, MAX_RETURNED_STRING_BYTES)?;
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
                ensure_returned_string_limit("codec error", &message, MAX_RETURNED_STRING_BYTES)?;
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
fn ensure_returned_felt_limit(fqn: &str, count: usize) -> Result<()> {
    if count > MAX_RETURNED_FELTS {
        return Err(Error::new(format!(
            "codec `{fqn}` returned {count} felts from `parse`; the limit is {MAX_RETURNED_FELTS}"
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
    Error::new(format!("codec `{fqn}` rejected `{operation}`: {message}"))
}

#[cfg(test)]
mod tests {
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
        PackageExport, PackageId, PathBuf as MastPathBuf, ProcedureExport, Section, SectionId,
        TargetType, Version,
    };
    use midenc_frontend_wasm_metadata::PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID;
    use tempfile::TempDir;
    use wit_component::ComponentEncoder;

    use super::*;

    const WASM_TARGET: &str = "wasm32-unknown-unknown";
    const FIXTURE_FQN: &str = "example:codec-schema/note-storage@1.0.0.ratio";
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

    #[test]
    fn component_boundary_rejects_noncanonical_felts() {
        let error =
            component_values_to_felts("example:test/codec.value", &[Felt::ORDER]).unwrap_err();

        assert!(error.to_string().contains("noncanonical felt at index 0"));
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
            SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).unwrap(),
            FIXTURE_SCHEMA.as_bytes().to_vec(),
        ));
        package.sections.push(Section::new(
            SectionId::custom(PACKAGE_NOTE_CODEC_SECTION_ID).unwrap(),
            component,
        ));

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
        )
        .unwrap();
        let codec = registry.codec(FIXTURE_FQN).unwrap();

        assert!(codec.parse("trap").unwrap_err().to_string().contains("call `parse`"));
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");

        let oversized_input = "x".repeat(MAX_COMPONENT_MEMORY_BYTES + 1);
        assert!(codec.parse(&oversized_input).is_err());
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");

        let fuel_error = codec.parse("loop").unwrap_err().to_string();
        assert!(
            fuel_error.contains("fuel") || fuel_error.contains("interrupt"),
            "unexpected budget error: {fuel_error}"
        );
        assert_eq!(codec.display(&codec.parse("3/2").unwrap()).unwrap(), "3/2");
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
        let long = "x".repeat(MAX_RETURNED_STRING_BYTES + 1);
        assert!(
            ensure_returned_string_limit("display value", &long, MAX_RETURNED_STRING_BYTES)
                .unwrap_err()
                .to_string()
                .contains("the limit is")
        );
        assert!(
            ensure_returned_felt_limit(FIXTURE_FQN, MAX_RETURNED_FELTS + 1)
                .unwrap_err()
                .to_string()
                .contains("the limit is")
        );
    }

    #[test]
    fn package_readers_reject_duplicate_schema_and_codec_sections() {
        let schema_id = SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).unwrap();
        let codec_id = SectionId::custom(PACKAGE_NOTE_CODEC_SECTION_ID).unwrap();
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
            SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).unwrap(),
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
        let (forest, remapping) = builder.finish_with_id_map().expect("failed to build package");
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
    fn build_fixture_component() -> Vec<u8> {
        static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
        COMPONENT.get_or_init(build_fixture_component_uncached).clone()
    }

    /// Builds the component fixture once for this test process.
    fn build_fixture_component_uncached() -> Vec<u8> {
        let fixture = TempDir::new().expect("failed to create component fixture directory");
        write_fixture(fixture.path());
        let target_dir = workspace_root().join("target/note-schema-component-test");
        let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
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
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .expect("failed to start fixture build");
        assert_command_succeeded("building the component adapter fixture", &output);

        let module = fs::read(
            target_dir.join(format!("{WASM_TARGET}/release/note_schema_component_fixture.wasm")),
        )
        .expect("component fixture did not produce its Wasm module");
        ComponentEncoder::default()
            .module(&module)
            .expect("fixture module is not component-ready")
            .validate(true)
            .encode()
            .expect("failed to encode fixture component")
    }

    /// Writes a standalone codec crate for the component adapter test.
    fn write_fixture(root: &Path) {
        fs::create_dir(root.join("src")).unwrap();
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
        fs::write(root.join("src/lib.rs"), FIXTURE_SOURCE).unwrap();
    }

    /// Returns true when rustup reports the component target as installed.
    fn wasm_target_is_installed() -> bool {
        let Ok(output) = Command::new("rustup").args(["target", "list"]).output() else {
            return false;
        };
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.starts_with(WASM_TARGET) && line.contains("(installed)"))
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

    const FIXTURE_SOURCE: &str = r##"
use miden_note_codec::AuthorTypeCodec;

miden_note_codec::from_wit_text!(r#"
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
"#);

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
"##;
}
