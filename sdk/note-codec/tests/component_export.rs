//! Component export test for the author codec world.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::TempDir;
use wit_component::{ComponentEncoder, DecodedWasm};
use wit_parser::WorldItem;

const WASM_TARGET: &str = "wasm32-unknown-unknown";

#[test]
fn minimal_codec_crate_encodes_to_zero_import_component() {
    if !wasm_target_is_installed() {
        eprintln!("skipping component export test: {WASM_TARGET} is not installed");
        return;
    }

    let fixture = TempDir::new().expect("failed to create temporary codec crate");
    write_fixture(fixture.path());
    let target_dir = workspace_root().join("target/note-codec-component-test");
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
        .expect("failed to run cargo for the component fixture");
    assert_command_succeeded("building the component fixture", &output);

    let module = fs::read(
        target_dir.join(format!("{WASM_TARGET}/release/note_codec_component_fixture.wasm")),
    )
    .expect("component fixture did not produce a Wasm module");
    let component = ComponentEncoder::default()
        .module(&module)
        .expect("the fixture is not a component-ready core module")
        .validate(true)
        .encode()
        .expect("failed to encode the codec component");
    let DecodedWasm::Component(resolve, world_id) =
        wit_component::decode(&component).expect("failed to decode the encoded component")
    else {
        panic!("ComponentEncoder did not produce a component");
    };
    let world = &resolve.worlds[world_id];
    assert!(world.imports.is_empty(), "codec component imports: {:#?}", world.imports);
    assert_eq!(world.exports.len(), 1);

    let interface_id = world
        .exports
        .values()
        .find_map(|item| match item {
            WorldItem::Interface { id, .. } => Some(*id),
            _ => None,
        })
        .expect("the component does not export the note codec interface");
    let interface = &resolve.interfaces[interface_id];
    assert_eq!(interface.name.as_deref(), Some("codec"));
    let package_id = interface.package.expect("codec interface has no package");
    let package = &resolve.packages[package_id].name;
    assert_eq!(package.namespace, "miden");
    assert_eq!(package.name, "note-codec");
    assert_eq!(package.version.as_ref().map(ToString::to_string).as_deref(), Some("1.0.0"));
    assert_eq!(
        interface.functions.keys().map(String::as_str).collect::<Vec<_>>(),
        ["supported-types", "parse", "display", "validate"]
    );
}

/// Returns true when rustup reports the component target as installed.
fn wasm_target_is_installed() -> bool {
    let output = match Command::new("rustup").args(["target", "list"]).output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("`rustup target list` failed:\n{}", String::from_utf8_lossy(&output.stderr));
            return false;
        }
        Err(error) => {
            eprintln!("could not run `rustup target list`: {error}");
            return false;
        }
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with(WASM_TARGET) && line.contains("(installed)"))
}

/// Writes the minimal author codec crate used by the componentization test.
fn write_fixture(root: &Path) {
    fs::create_dir(root.join("src")).expect("failed to create fixture source directory");
    let codec_path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = format!(
        r#"[package]
name = "note-codec-component-fixture"
version = "0.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
codec-facade = {{ package = "miden-note-codec", path = {:?} }}

[workspace]
"#,
        codec_path
    );
    fs::write(root.join("Cargo.toml"), manifest).expect("failed to write fixture manifest");
    fs::write(root.join("src/lib.rs"), FIXTURE_SOURCE).expect("failed to write fixture source");
}

/// Returns the compiler workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("note-codec must be inside sdk/")
        .to_owned()
}

/// Reports all command output when one fixture command fails.
fn assert_command_succeeded(action: &str, output: &Output) {
    assert!(
        output.status.success(),
        "failed while {action}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

const FIXTURE_SOURCE: &str = r##"
use codec_facade::AuthorTypeCodec;

codec_facade::from_wit_text!(r#"
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

#[codec_facade::note_codec]
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

codec_facade::export_codecs!();
"##;
