//! Shared fixtures for stored-procedure dispatch tests.
//!
//! Generates the temporary cargo-miden projects of a dispatch scenario (target component,
//! dispatcher component, trigger notes) and resolves the procedure roots and slot names the host
//! writes into the dispatcher's storage.

use std::{path::Path, sync::Arc};

use miden_core::{Felt, Word, serde::Deserializable};
use miden_mast_package::{Package, SectionId};
use miden_protocol::account::{
    AccountComponentMetadata, StorageSlotName,
    component::{StorageSlotSchema, storage::SchemaType},
};
use midenc_integration_test_support::{cargo_proj::Project, project};

use super::super::support::*;

/// Interface segment of every generated dispatcher component (`[lib].namespace` and trait name).
pub(super) const DISPATCHER_INTERFACE: &str = "dispatcher";

/// Names derived from a dispatch test for its target, dispatcher, and note projects.
pub(super) struct DispatchProjectNames {
    pub base_name: String,
    pub target_account_name: String,
    pub dispatcher_account_name: String,
    pub target_account_package: String,
    pub dispatcher_account_package: String,
}

impl DispatchProjectNames {
    /// Builds Cargo crate names and WIT package names from `test_name`.
    pub fn new(test_name: &str) -> Self {
        let base_name = test_name.replace('_', "-");
        let target_account_name = format!("{base_name}-target-account");
        let dispatcher_account_name = format!("{base_name}-dispatcher-account");
        let target_account_package = format!("miden:{target_account_name}");
        let dispatcher_account_package = format!("miden:{dispatcher_account_name}");
        Self {
            base_name,
            target_account_name,
            dispatcher_account_name,
            target_account_package,
            dispatcher_account_package,
        }
    }

    /// Returns the storage slot name of the dispatcher's `field` stored-procedure slot.
    pub fn dispatcher_slot(&self, field: &str) -> StorageSlotName {
        storage_slot_name_for_field(&self.dispatcher_account_package, DISPATCHER_INTERFACE, field)
    }
}

/// Generates and compiles the target (sibling) component exporting `interface`.
pub(super) fn build_target_package(
    names: &DispatchProjectNames,
    interface: &str,
    source: &str,
) -> (Project, Arc<Package>) {
    let project = project(&names.target_account_name)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                &names.target_account_name,
                &names.target_account_package,
                interface,
            ),
        )
        .file(
            "Cargo.toml",
            &account_cargo_toml_for(&names.target_account_name, &names.target_account_package),
        )
        .file("src/lib.rs", source)
        .build();
    let package = compile_rust_package(project.root(), true);
    (project, package)
}

/// Generates and compiles the dispatcher component, which depends on nothing: its call targets
/// are runtime data.
pub(super) fn build_dispatcher_package(
    names: &DispatchProjectNames,
    source: &str,
) -> (Project, Arc<Package>) {
    build_dispatcher_package_with_dependencies(names, &[], source)
}

/// Generates and compiles a dispatcher component declaring the given sibling package
/// dependencies (package name, project root), for fixtures comparing stored dispatch against
/// direct sibling calls.
pub(super) fn build_dispatcher_package_with_dependencies(
    names: &DispatchProjectNames,
    dependencies: &[(&str, &Path)],
    source: &str,
) -> (Project, Arc<Package>) {
    let mut miden_project_toml = account_miden_project_toml_with_interface(
        &names.dispatcher_account_name,
        &names.dispatcher_account_package,
        DISPATCHER_INTERFACE,
    );
    append_miden_project_dependencies(&mut miden_project_toml, dependencies);
    let mut cargo_toml =
        account_cargo_toml_for(&names.dispatcher_account_name, &names.dispatcher_account_package);
    append_cargo_dependency_metadata(&mut cargo_toml, dependencies);
    let project = project(&names.dispatcher_account_name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", source)
        .build();
    let package = compile_rust_package(project.root(), true);
    (project, package)
}

/// Returns the non-zero storage key the counter fixtures use (matching the note sources).
pub(super) fn counter_storage_key() -> Word {
    Word::new([
        Felt::new(13).unwrap(),
        Felt::new(21).unwrap(),
        Felt::new(34).unwrap(),
        Felt::new(55).unwrap(),
    ])
}

/// Target counter component: a read, an increment, and an order-sensitive two-argument update.
pub(super) const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

/// Sibling account component whose storage map holds one counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "sibling counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Sibling account component exposing read and update procedures over its counter.
#[component]
trait CounterContract {
    /// Returns the counter value stored under the provided key.
    #[account_procedure]
    fn get_count(&self, key: Word) -> Felt;
    /// Increments the counter value stored under the provided key, returning the new value.
    #[account_procedure]
    fn increment_count(&mut self, key: Word) -> Felt;
    /// Adds `delta` to the counter value stored under the provided key, returning the new value.
    #[account_procedure]
    fn add_to_count(&mut self, key: Word, delta: Felt) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self, key: Word) -> Felt {
        self.count_map.get(key)
    }

    fn increment_count(&mut self, key: Word) -> Felt {
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }

    fn add_to_count(&mut self, key: Word, delta: Felt) -> Felt {
        let new_value = self.count_map.get(key) + delta;
        self.count_map.set(key, new_value);
        new_value
    }
}
"#;

/// Generates and compiles a note project named `<base>-<suffix>` depending on the dispatcher.
pub(super) fn build_note_package(
    names: &DispatchProjectNames,
    suffix: &str,
    dispatcher_project_root: &Path,
    source: &str,
) -> Arc<Package> {
    let note_name = format!("{}-{suffix}", names.base_name);
    let note_package = format!("miden:{note_name}");
    let project = project(&note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                &note_name,
                &note_package,
                &names.dispatcher_account_package,
                dispatcher_project_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(
                &note_name,
                &names.dispatcher_account_package,
                dispatcher_project_root,
            ),
        )
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(project.root(), true)
}

/// Returns the MAST root of the lifted component-model export `leaf` of `package`.
///
/// A package manifest exposes each export twice under one leaf name: the core Wasm function
/// under the `namespace::interface` module and the lifted component-model wrapper under the
/// component-id module (whose path segment contains `/`). Only the wrapper is a valid `dyncall`
/// target, so the lookup selects by path.
pub(super) fn lifted_export_root(package: &Package, leaf: &str) -> Word {
    let matches = package
        .manifest
        .exports()
        .filter_map(|export| export.as_procedure())
        .filter(|export| {
            let path = export.path.as_ref().as_str();
            // Leaf names containing `-` are rendered quoted (`::"increment-count"`)
            let last = path.rsplit("::").next().map(|last| last.trim_matches('"'));
            path.contains('/') && last == Some(leaf)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one lifted export named `{leaf}`, got {:?}",
        package
            .manifest
            .exports()
            .filter_map(|export| export.as_procedure())
            .map(|export| export.path.as_ref().as_str().to_string())
            .collect::<Vec<_>>()
    );
    matches[0].digest
}

/// Asserts that every stored-procedure slot of `package` is described as a plain `word` value
/// slot in the embedded account component metadata.
pub(super) fn assert_word_value_slots(package: &Package, slots: &[StorageSlotName]) {
    let metadata_bytes = package
        .sections
        .iter()
        .find(|section| section.id == SectionId::ACCOUNT_COMPONENT_METADATA)
        .map(|section| section.data.as_ref())
        .expect("dispatcher package should embed account component metadata");
    let metadata = AccountComponentMetadata::read_from_bytes(metadata_bytes)
        .expect("account component metadata should deserialize");
    for slot in slots {
        match metadata.storage_schema().slots().get(slot) {
            Some(StorageSlotSchema::Value(value)) => assert_eq!(
                value.word().word_type(),
                SchemaType::native_word(),
                "slot `{slot}` should be a `word` value slot"
            ),
            other => panic!("slot `{slot}` should be a value slot, got {other:?}"),
        }
    }
}
