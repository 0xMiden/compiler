//! End-to-end test for package discovery and generated p2id consumer bindings.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;
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
    let bindings_dir = workspace.join("sdk/note-bindings");
    let schema_dir = workspace.join("sdk/note-schema");
    let field_repr_dir = workspace.join("sdk/field-repr/repr");
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "note-bindings-consumer"
version = "0.1.0"
edition = "2024"

[dependencies]
miden-field = "0.28"
miden-field-repr = {{ path = {field_repr_dir:?} }}
miden-note-bindings = {{ path = {bindings_dir:?} }}
miden-note-schema = {{ path = {schema_dir:?} }}
miden-protocol = {{ version = "=0.16.0-alpha.4", features = ["std"] }}
"#,
            field_repr_dir = field_repr_dir.to_string_lossy(),
            bindings_dir = bindings_dir.to_string_lossy(),
            schema_dir = schema_dir.to_string_lossy(),
        ),
    )
    .unwrap();

    let source = format!(
        r#"use std::collections::BTreeMap;

use miden_protocol::{{account::AccountId, address::NetworkId}};

mod project_bindings {{
    miden_note_bindings::from_project!({project_dir:?});
}}

mod package_bindings {{
    miden_note_bindings::from_package!({package_path:?});
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
    typed.validate_with().unwrap();
    assert_eq!(
        typed.display_with().unwrap(),
        format!("{{{{target-account-id: {{bech32}}}}}}"),
    );

    let decoded = project_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(decoded, typed);

    let exact = package_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(exact.target_account_id, account_id);
}}
"#,
        project_dir = p2id_dir.to_string_lossy(),
        package_path = package_path.to_string_lossy(),
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
