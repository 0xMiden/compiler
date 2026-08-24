//! End-to-end test for package discovery and generated p2id consumer bindings.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use miden_mast_package::{Package, Section, SectionId};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_frontend_wasm_metadata::PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID;
use midenc_integration_test_support::CompilerTest;

/// Compiles one Cargo Miden project without debug output.
fn compile_project(project_path: &Path) -> Arc<Package> {
    let mut test = CompilerTest::rust_source_cargo_miden(
        project_path,
        WasmTranslationConfig::default(),
        ["--debug".to_owned(), "none".to_owned()],
    );
    test.compile_package()
}

/// Returns the compiler workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Returns the native rustc host target.
fn host_target() -> String {
    let output = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-vV")
        .output()
        .expect("failed to query the rustc host target");
    assert!(output.status.success(), "rustc -vV failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc -vV did not report a host target")
        .to_owned()
}

/// Copies the workspace patch table into an isolated consumer manifest.
fn workspace_patch_section(workspace: &Path) -> String {
    let manifest = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    let mut section = String::new();
    let mut copying = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed == "[patch.crates-io]" {
            copying = true;
        } else if copying && trimmed.starts_with('[') {
            break;
        }
        if copying {
            section.push_str(line);
            section.push('\n');
        }
    }
    assert!(!section.is_empty(), "workspace manifest has no [patch.crates-io] section");
    section
}

#[test]
fn generated_p2id_bindings_compile_and_run_in_a_consumer_crate() {
    let workspace = workspace_root();
    let examples = workspace.join("examples");
    let wallet_dir = examples.join("basic-wallet");
    let wallet = compile_project(&wallet_dir);
    wallet
        .write_masp_file(wallet_dir.join("target/miden/release"))
        .expect("failed to persist the basic-wallet dependency package");

    let p2id_dir = examples.join("p2id-note");
    let p2id = compile_project(&p2id_dir);
    let package_dir = p2id_dir.join("target/miden/release");
    p2id.write_masp_file(&package_dir).expect("failed to persist the p2id package");
    let package_path = package_dir.join("p2id.masp");

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    // Seed the workspace lockfile so the offline build resolves to the versions that the
    // workspace already downloaded instead of racing the registry index.
    fs::copy(workspace.join("Cargo.lock"), temp.path().join("Cargo.lock")).unwrap();
    let second_package_dir = temp.path().join("packages/counter");
    fs::create_dir_all(&second_package_dir).unwrap();
    let mut second_package = (*p2id).clone();
    let schema_id = SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).unwrap();
    second_package.sections.retain(|section| section.id != schema_id);
    second_package.sections.push(Section::new(
        schema_id,
        br#"package example:counter-schema@1.0.0;

interface note-storage {
    record counter-note { value: u64 }
    type storage = counter-note;
}
"#
        .to_vec(),
    ));
    second_package
        .write_masp_file(&second_package_dir)
        .expect("failed to persist the second schema package");
    let second_package_path = second_package_dir.join("p2id.masp");

    let bindings_dir = workspace.join("sdk/note-bindings");
    let patches = workspace_patch_section(&workspace);
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "note-bindings-consumer"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
miden-note-bindings = {{ path = {bindings_dir:?} }}

{patches}
"#,
            bindings_dir = bindings_dir.to_string_lossy(),
        ),
    )
    .unwrap();

    let source = format!(
        r#"use std::collections::BTreeMap;

use miden_note_bindings::{{account::AccountId, address::NetworkId}};

mod project_bindings {{
    miden_note_bindings::from_project!({project_dir:?});
}}

mod package_bindings {{
    miden_note_bindings::from_package!({package_path:?});
    miden_note_bindings::from_package!({second_package_path:?});
}}

fn main() {{
    let account_id =
        AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let bech32 = account_id.to_bech32(NetworkId::Mainnet);
    let mut values = BTreeMap::new();
    values.insert("target_account_id".to_owned(), bech32.clone());

    let typed = project_bindings::P2idNote::from_str_values(&values).unwrap();
    assert_eq!(typed.target_account_id, account_id);
    let storage = typed.to_note_storage().unwrap();
    assert_eq!(
        storage.items(),
        &[account_id.prefix().as_felt(), account_id.suffix()],
    );
    typed.validate().unwrap();
    assert_eq!(
        typed.display().unwrap(),
        format!("{{{{target-account-id: {{bech32}}}}}}"),
    );

    let decoded = project_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(decoded, typed);

    let exact = package_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(exact.target_account_id, account_id);

    let counter = package_bindings::CounterNote {{ value: 9 }};
    let counter_storage = counter.to_note_storage().unwrap();
    assert_eq!(counter_storage.items()[0].as_canonical_u64(), 9);
    assert_eq!(counter_storage.items()[1].as_canonical_u64(), 0);
}}
"#,
        project_dir = p2id_dir.to_string_lossy(),
        package_path = package_path.to_string_lossy(),
        second_package_path = second_package_path.to_string_lossy(),
    );
    fs::write(temp.path().join("src/main.rs"), source).unwrap();

    let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["run", "--quiet", "--target"])
        .arg(host_target())
        .arg("--manifest-path")
        .arg(temp.path().join("Cargo.toml"))
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", workspace.join("target/note-bindings-consumer"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("failed to spawn Cargo for the generated bindings consumer");
    assert!(
        output.status.success(),
        "generated bindings consumer failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
