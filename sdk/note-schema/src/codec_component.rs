//! Consumer adapters for author codec components.

use std::sync::{Arc, Mutex};

use miden_field::Felt;
use miden_mast_package::{Package, SectionId};
use midenc_frontend_wasm_metadata::PACKAGE_NOTE_CODEC_SECTION_ID;
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};

use crate::{CodecRegistry, ConsumerTypeCodec, Error, Result};

wasmtime::component::bindgen!({
    path: "../note-codec/wit",
    world: "note-codec",
});

impl CodecRegistry {
    /// Loads the note codec component from a package and registers all reported types.
    pub fn load_from_package(package: &Package) -> Result<Self> {
        let section_id = SectionId::custom(PACKAGE_NOTE_CODEC_SECTION_ID).map_err(|error| {
            Error::new(format!(
                "invalid note codec section id `{PACKAGE_NOTE_CODEC_SECTION_ID}`: {error}"
            ))
        })?;
        let bytes = package
            .sections
            .iter()
            .find(|section| section.id == section_id)
            .ok_or_else(|| {
                Error::new(format!(
                    "package does not contain the `{PACKAGE_NOTE_CODEC_SECTION_ID}` section"
                ))
            })?
            .data
            .as_ref();

        Self::load_from_component(bytes)
    }

    /// Loads a zero-import note codec component.
    fn load_from_component(bytes: &[u8]) -> Result<Self> {
        let mut runtime = ComponentRuntime::instantiate(bytes)?;
        let supported_types = runtime.supported_types()?;
        let runtime = Arc::new(Mutex::new(runtime));
        let mut registry = Self::default();

        for fqn in supported_types {
            if fqn.trim().is_empty() {
                return Err(Error::new("note codec component reported an empty type FQN"));
            }
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

/// One instantiated note codec component and its mutable store.
struct ComponentRuntime {
    store: Store<()>,
    bindings: NoteCodec,
}

impl ComponentRuntime {
    /// Instantiates a component without defining any host imports.
    fn instantiate(bytes: &[u8]) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config)
            .map_err(|error| component_error("create the Wasmtime engine", error))?;
        let component = Component::new(&engine, bytes)
            .map_err(|error| component_error("compile the note codec component", error))?;
        let linker = Linker::new(&engine);
        let mut store = Store::new(&engine, ());
        let bindings = NoteCodec::instantiate(&mut store, &component, &linker)
            .map_err(|error| component_error("instantiate the zero-import note codec", error))?;
        Ok(Self { store, bindings })
    }

    /// Queries the component's supported FQNs once during registry construction.
    fn supported_types(&mut self) -> Result<Vec<String>> {
        self.bindings
            .miden_note_codec_codec()
            .call_supported_types(&mut self.store)
            .map_err(|error| component_error("call `supported-types`", error))
    }
}

/// A registry entry that dispatches one FQN into a shared component instance.
struct ComponentCodec {
    fqn: String,
    runtime: Arc<Mutex<ComponentRuntime>>,
}

impl ComponentCodec {
    /// Calls one component operation while holding exclusive access to its store.
    fn with_runtime<T>(
        &self,
        operation: &str,
        call: impl FnOnce(&NoteCodec, &mut Store<()>) -> wasmtime::Result<T>,
    ) -> Result<T> {
        let mut runtime = self.runtime.lock().map_err(|_| {
            Error::new(format!(
                "note codec component state is unavailable while calling `{operation}` for `{}`",
                self.fqn
            ))
        })?;
        let ComponentRuntime { bindings, store } = &mut *runtime;
        call(bindings, store).map_err(|error| {
            component_error(&format!("call `{operation}` for codec `{}`", self.fqn), error)
        })
    }
}

impl ConsumerTypeCodec for ComponentCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let result = self.with_runtime("parse", |bindings, store| {
            bindings.miden_note_codec_codec().call_parse(store, &self.fqn, value)
        })?;
        let values = result.map_err(|message| codec_rejection("parse", &self.fqn, message))?;
        component_values_to_felts(&self.fqn, &values)
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        let values = felts.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();
        self.with_runtime("display", |bindings, store| {
            bindings.miden_note_codec_codec().call_display(store, &self.fqn, &values)
        })?
        .map_err(|message| codec_rejection("display", &self.fqn, message))
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        let values = felts.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();
        self.with_runtime("validate", |bindings, store| {
            bindings.miden_note_codec_codec().call_validate(store, &self.fqn, &values)
        })?
        .map_err(|message| codec_rejection("validate", &self.fqn, message))
    }
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
    Error::new(format!("failed to {action}: {error}"))
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
        sync::Arc,
    };

    use miden_core::{
        mast::{BasicBlockNodeBuilder, DenseMastForestBuilder, MastNodeExt},
        operations::Operation,
    };
    use miden_mast_package::{
        PackageExport, PackageId, PathBuf as MastPathBuf, ProcedureExport, Section, TargetType,
        Version,
    };
    use tempfile::TempDir;
    use wit_component::ComponentEncoder;

    use super::*;

    const WASM_TARGET: &str = "wasm32-unknown-unknown";

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
            SectionId::custom(PACKAGE_NOTE_CODEC_SECTION_ID).unwrap(),
            component,
        ));

        let registry = CodecRegistry::load_from_package(&package).unwrap();
        let fqn = "example:codec-schema/note-storage@1.0.0.ratio";
        let codec = registry.codec(fqn).expect("fixture ratio codec was not registered");
        let encoded = codec.parse("3/2").unwrap();
        assert_eq!(encoded, [Felt::new(3).unwrap(), Felt::ZERO, Felt::new(2).unwrap(), Felt::ZERO]);
        codec.validate(&encoded).unwrap();
        assert_eq!(codec.display(&encoded).unwrap(), "3/2");

        let invalid = codec.parse("3/0").unwrap();
        assert!(codec.validate(&invalid).unwrap_err().to_string().contains("denominator"));
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
