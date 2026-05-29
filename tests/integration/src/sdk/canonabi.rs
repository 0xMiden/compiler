//! Integration tests for component-model CanonABI variant values.

use std::{fs, path::Path};

use miden_core::program::Program;
use miden_mast_package::Package;
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{
    CompilerTest, CompilerTestBuilder, Project, cargo_proj::project, compiler_test::sdk_crate_path,
    testing::executor_with_std,
};

/// Names and package identifiers used by one generated account/note pair.
struct CanonAbiProjectNames {
    /// The Rust crate name of the account project.
    account_crate: String,
    /// The component package name of the account project without the `miden:` namespace.
    account_slug: String,
    /// The Rust module generated for the account package in note bindings.
    account_package_module: String,
    /// The Rust module generated for the account interface in note bindings.
    account_interface_module: String,
    /// The Rust crate name of the note project.
    note_crate: String,
    /// The component package name of the note project without the `miden:` namespace.
    note_slug: String,
}

impl CanonAbiProjectNames {
    /// Constructs generated project names for `case`.
    fn new(case: &str) -> Self {
        let case = case.replace('-', "_");
        let account_crate = format!("canonabi_{case}_account");
        let account_slug = account_crate.replace('_', "-");
        let account_package_module = account_slug.replace('-', "_");
        let account_interface_module = format!("miden_{account_package_module}");
        let note_crate = format!("canonabi_{case}_note");
        let note_slug = note_crate.replace('_', "-");

        Self {
            account_crate,
            account_slug,
            account_package_module,
            account_interface_module,
            note_crate,
            note_slug,
        }
    }
}

/// Builds a generated account project with the provided component source.
fn build_account_project(names: &CanonAbiProjectNames, source: &str) -> Project {
    let sdk_path = sdk_crate_path();
    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "{account_crate}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:{account_slug}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        account_crate = names.account_crate,
        account_slug = names.account_slug,
        sdk_path = sdk_path.display(),
    );
    let miden_project_toml = format!(
        r#"[package]
name = "{account_crate}"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:{account_slug}/miden-{account_slug}@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
"#,
        account_crate = names.account_crate,
        account_slug = names.account_slug,
    );

    project(&names.account_crate)
        .file("Cargo.toml", &cargo_toml)
        .file("miden-project.toml", &miden_project_toml)
        .file("src/lib.rs", source)
        .build()
}

/// Builds a generated note project that imports the generated account project.
fn build_note_project(
    names: &CanonAbiProjectNames,
    account_root: &Path,
    note_body: &str,
) -> Project {
    let sdk_path = sdk_crate_path();
    let generated_wit = account_root.join("target/generated-wit");
    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "{note_crate}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:{note_slug}"

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.miden.dependencies]
"miden:{account_slug}" = {{ path = "{account_root}" }}

[package.metadata.component.target.dependencies]
"miden:{account_slug}" = {{ path = "{generated_wit}" }}

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        note_crate = names.note_crate,
        note_slug = names.note_slug,
        account_slug = names.account_slug,
        sdk_path = sdk_path.display(),
        account_root = account_root.display(),
        generated_wit = generated_wit.display(),
    );
    let miden_project_toml = format!(
        r#"[package]
name = "{note_crate}"
version = "0.1.0"

[lib]
kind = "note"
namespace = "miden:{note_slug}/miden-{note_slug}@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"
{account_crate} = {{ path = "{account_root}" }}

[package.metadata.miden.dependencies]
{account_crate} = {{ wit = "{generated_wit}" }}
"#,
        note_crate = names.note_crate,
        note_slug = names.note_slug,
        account_crate = names.account_crate,
        account_root = account_root.display(),
        generated_wit = generated_wit.display(),
    );
    let source = format!(
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::{package_module}::{interface_module}::*;

#[note]
struct CanonabiNote;

#[note]
impl CanonabiNote {{
    #[note_script]
    pub fn run(self, _arg: Word) {{
{note_body}
    }}
}}
"#,
        package_module = names.account_package_module,
        interface_module = names.account_interface_module,
        note_body = indent(note_body, 8),
    );

    project(&names.note_crate)
        .file("Cargo.toml", &cargo_toml)
        .file("miden-project.toml", &miden_project_toml)
        .file("src/lib.rs", &source)
        .build()
}

/// Builds a compiler test for a generated Cargo-Miden project.
fn build_generated_test(root: impl AsRef<Path>) -> CompilerTest {
    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden(root, WasmTranslationConfig::default(), []);
    builder.with_release(true);
    builder.build()
}

/// Rebuilds an executable program from a compiled note-script package.
fn note_script_program(package: &Package) -> Program {
    let note_script =
        NoteScript::from_package(package).expect("compiled package should contain a note script");
    Program::new(note_script.mast(), note_script.entrypoint())
}

/// Reads the single generated WIT file emitted by the account project.
fn read_generated_wit(project: &Project) -> String {
    let generated_wit_dir = project.root().join("target/generated-wit");
    let mut wit_paths = fs::read_dir(&generated_wit_dir)
        .unwrap_or_else(|err| {
            panic!("failed to read generated WIT dir {}: {err}", generated_wit_dir.display())
        })
        .map(|entry| entry.expect("failed to inspect generated WIT entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wit"))
        .collect::<Vec<_>>();
    wit_paths.sort();
    assert_eq!(wit_paths.len(), 1, "expected one generated WIT file, got {wit_paths:?}");
    fs::read_to_string(&wit_paths[0]).unwrap_or_else(|err| {
        panic!("failed to read generated WIT {}: {err}", wit_paths[0].display())
    })
}

/// Asserts that exported Rust enums are encoded as WIT variants.
fn assert_generated_wit_uses_variants(project: &Project) {
    let wit = read_generated_wit(project);
    assert!(
        wit.contains("variant request {"),
        "generated WIT did not define `request` as a variant:\n{wit}"
    );
    assert!(
        wit.contains("variant response {"),
        "generated WIT did not define `response` as a variant:\n{wit}"
    );
    assert!(
        !wit.contains("enum request {"),
        "generated WIT encoded `request` as an enum:\n{wit}"
    );
    assert!(
        !wit.contains("enum response {"),
        "generated WIT encoded `response` as an enum:\n{wit}"
    );
}

/// Runs a generated account/note pair by executing the compiled note script directly.
fn run_variant_case(case: &str, account_source: &str, note_body: &str) {
    let names = CanonAbiProjectNames::new(case);
    let account_project = build_account_project(&names, account_source);
    let account_root = account_project.root();
    let mut account_test = build_generated_test(&account_root);
    let account_package = account_test.compile_package();
    assert!(account_package.is_library());
    assert_generated_wit_uses_variants(&account_project);

    let note_project = build_note_project(&names, &account_root, note_body);
    let mut note_test = build_generated_test(note_project.root());
    let note_package = note_test.compile_package();
    assert!(note_package.is_library());

    let program = note_script_program(note_package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(note_package.manifest.dependencies())
        .expect("failed to add generated note dependencies");
    let _trace = exec.execute(&program, note_test.session.source_manager.clone());
}

/// Indents every line of `source` by `spaces`.
fn indent(source: &str, spaces: usize) -> String {
    let padding = " ".repeat(spaces);
    source
        .lines()
        .map(|line| format!("{padding}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tests a variant with two cases that both carry one felt.
#[test]
fn variant_with_two_felt_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Request variants carrying felt values.
#[export_type]
pub enum Request {
    /// Carries the first felt value.
    First(Felt),
    /// Carries the second felt value.
    Second(Felt),
}

/// Response variants carrying felt values.
#[export_type]
pub enum Response {
    /// Returns the first transformed felt value.
    First(Felt),
    /// Returns the second transformed felt value.
    Second(Felt),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms one felt variant into one felt result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::First(value) => Response::First(value + felt!(1)),
            Request::Second(value) => Response::Second(value + felt!(2)),
        }
    }
}
"#;
    let note_body = r#"let first = roundtrip(Request::First(felt!(11)));
match first {
    Response::First(value) => assert_eq!(value, felt!(12)),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let second = roundtrip(Request::Second(felt!(20)));
match second {
    Response::Second(value) => assert_eq!(value, felt!(22)),
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("two_felts", account_source, note_body);
}

/// Tests a variant with one unit case and one felt case.
#[test]
fn variant_with_unit_and_felt_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Request variants with and without payloads.
#[export_type]
pub enum Request {
    /// Carries no value.
    Empty,
    /// Carries a single felt value.
    Value(Felt),
}

/// Response variants with and without payloads.
#[export_type]
pub enum Response {
    /// Returns no value.
    Empty,
    /// Returns a single felt value.
    Value(Felt),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a unit or felt variant into the matching result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::Empty => Response::Empty,
            Request::Value(value) => Response::Value(value + felt!(5)),
        }
    }
}
"#;
    let note_body = r#"let empty = roundtrip(Request::Empty);
match empty {
    Response::Empty => (),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let value = roundtrip(Request::Value(felt!(37)));
match value {
    Response::Value(value) => assert_eq!(value, felt!(42)),
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("unit_felt", account_source, note_body);
}

/// Tests a variant with one felt case and one word case.
#[test]
fn variant_with_felt_and_word_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, Word, component, export_type, felt};

/// Request variants carrying either a felt or a word.
#[export_type]
pub enum Request {
    /// Carries a single felt value.
    Scalar(Felt),
    /// Carries a full word value.
    Elements(Word),
}

/// Response variants carrying either a felt or a word.
#[export_type]
pub enum Response {
    /// Returns a single felt value.
    Scalar(Felt),
    /// Returns a full word value.
    Elements(Word),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms either a felt or word variant into the matching result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::Scalar(value) => Response::Scalar(value + felt!(3)),
            Request::Elements(word) => Response::Elements(Word::new([
                word.a + felt!(1),
                word.b + felt!(2),
                word.c + felt!(3),
                word.d + felt!(4),
            ])),
        }
    }
}
"#;
    let note_body = r#"let scalar = roundtrip(Request::Scalar(felt!(7)));
match scalar {
    Response::Scalar(value) => assert_eq!(value, felt!(10)),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let word = Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]);
let elements = roundtrip(Request::Elements(word));
match elements {
    Response::Elements(value) => {
        assert_eq!(value.a, felt!(2));
        assert_eq!(value.b, felt!(4));
        assert_eq!(value.c, felt!(6));
        assert_eq!(value.d, felt!(8));
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("felt_word", account_source, note_body);
}

/// Tests a variant with felt, word, and unit cases.
#[test]
fn variant_with_felt_word_and_unit_cases() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, Word, component, export_type, felt};

/// Request variants carrying a scalar, a word, or no value.
#[export_type]
pub enum Request {
    /// Carries a single felt value.
    Scalar(Felt),
    /// Carries a full word value.
    Vector(Word),
    /// Carries no value.
    Empty,
}

/// Response variants carrying a scalar, a word, or no value.
#[export_type]
pub enum Response {
    /// Returns a single felt value.
    Scalar(Felt),
    /// Returns a full word value.
    Vector(Word),
    /// Returns no value.
    Empty,
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a scalar, word, or unit variant into the matching result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::Scalar(value) => Response::Scalar(value + felt!(8)),
            Request::Vector(word) => Response::Vector(Word::new([
                word.a + felt!(2),
                word.b + felt!(4),
                word.c + felt!(6),
                word.d + felt!(8),
            ])),
            Request::Empty => Response::Empty,
        }
    }
}
"#;
    let note_body = r#"let scalar = roundtrip(Request::Scalar(felt!(13)));
match scalar {
    Response::Scalar(value) => assert_eq!(value, felt!(21)),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let word = Word::new([felt!(3), felt!(6), felt!(9), felt!(12)]);
let vector = roundtrip(Request::Vector(word));
match vector {
    Response::Vector(value) => {
        assert_eq!(value.a, felt!(5));
        assert_eq!(value.b, felt!(10));
        assert_eq!(value.c, felt!(15));
        assert_eq!(value.d, felt!(20));
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}

let empty = roundtrip(Request::Empty);
match empty {
    Response::Empty => (),
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("felt_word_unit", account_source, note_body);
}

/// Tests a variant with one word case and one u64 case.
#[test]
fn variant_with_word_and_u64_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Word, component, export_type, felt};

/// Request variants carrying either a word or a 64-bit integer.
#[export_type]
pub enum Request {
    /// Carries a full word value.
    Elements(Word),
    /// Carries a 64-bit integer value.
    Amount(u64),
}

/// Response variants carrying either a word or a 64-bit integer.
#[export_type]
pub enum Response {
    /// Returns a full word value.
    Elements(Word),
    /// Returns a 64-bit integer value.
    Amount(u64),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms either a word or u64 variant into the matching result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::Elements(word) => Response::Elements(Word::new([
                word.a + felt!(10),
                word.b + felt!(20),
                word.c + felt!(30),
                word.d + felt!(40),
            ])),
            Request::Amount(value) => Response::Amount(value + 9),
        }
    }
}
"#;
    let note_body = r#"let word = Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]);
let elements = roundtrip(Request::Elements(word));
match elements {
    Response::Elements(value) => {
        assert_eq!(value.a, felt!(11));
        assert_eq!(value.b, felt!(22));
        assert_eq!(value.c, felt!(33));
        assert_eq!(value.d, felt!(44));
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}

let amount = roundtrip(Request::Amount(100));
match amount {
    Response::Amount(value) => {
        if value != 109 { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("word_u64", account_source, note_body);
}

/// Tests a variant with one u8 case and one u64 case.
#[test]
fn variant_with_u8_and_u64_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type};

/// Request variants carrying differently sized integer values.
#[export_type]
pub enum Request {
    /// Carries an 8-bit integer value.
    Tiny(u8),
    /// Carries a 64-bit integer value.
    Wide(u64),
}

/// Response variants carrying differently sized integer values.
#[export_type]
pub enum Response {
    /// Returns an 8-bit integer value.
    Tiny(u8),
    /// Returns a 64-bit integer value.
    Wide(u64),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms either an 8-bit or 64-bit variant into the matching result variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::Tiny(value) => Response::Tiny(value + 5),
            Request::Wide(value) => Response::Wide(value + 13),
        }
    }
}
"#;
    let note_body = r#"let tiny = roundtrip(Request::Tiny(17));
match tiny {
    Response::Tiny(value) => {
        if value != 22 { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}

let wide = roundtrip(Request::Wide(1000));
match wide {
    Response::Wide(value) => {
        if value != 1013 { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("u8_u64", account_source, note_body);
}

/// Tests variants whose record payloads have different flat shapes and field orders.
#[test]
fn variant_with_different_struct_payload_shapes() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type};

/// Payload with a 64-bit field before a 32-bit field.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct PayloadA {
    /// Wide field first.
    pub x: u64,
    /// Narrow field second.
    pub y: u32,
}

/// Payload with a 32-bit field before a 64-bit field.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct PayloadB {
    /// Narrow field first.
    pub x: u32,
    /// Wide field second.
    pub y: u64,
}

/// Request variants carrying differently shaped records.
#[export_type]
pub enum Request {
    /// Carries the first payload layout.
    A(PayloadA),
    /// Carries the second payload layout.
    B(PayloadB),
}

/// Response variants carrying differently shaped records.
#[export_type]
pub enum Response {
    /// Returns the first payload layout.
    A(PayloadA),
    /// Returns the second payload layout.
    B(PayloadB),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms the active record payload using its own field layout.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::A(payload) => Response::A(PayloadA {
                x: payload.x + 17,
                y: payload.y + 19,
            }),
            Request::B(payload) => Response::B(PayloadB {
                x: payload.x + 23,
                y: payload.y + 29,
            }),
        }
    }
}
"#;
    let note_body = r#"let payload_a = PayloadA { x: 1_000, y: 70 };
let result_a = roundtrip(Request::A(payload_a));
match result_a {
    Response::A(value) => {
        if value.x != 1_017 { assert_eq!(felt!(0), felt!(1)); }
        if value.y != 89 { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}

let payload_b = PayloadB { x: 90, y: 2_000 };
let result_b = roundtrip(Request::B(payload_b));
match result_b {
    Response::B(value) => {
        if value.x != 113 { assert_eq!(felt!(0), felt!(1)); }
        if value.y != 2_029 { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("different_struct_shapes", account_source, note_body);
}

/// Tests nested variants used as outer variant payloads.
#[test]
fn variant_with_nested_variant_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, Word, component, export_type, felt};

/// Inner variants carried by the outer request and response variants.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub enum Inner {
    /// Carries a single felt value.
    One(Felt),
    /// Carries a full word value.
    Many(Word),
}

/// Request variants with either no payload or an inner variant payload.
#[export_type]
pub enum Request {
    /// Carries no value.
    None,
    /// Carries a nested variant.
    Some(Inner),
}

/// Response variants with either no payload or an inner variant payload.
#[export_type]
pub enum Response {
    /// Returns no value.
    None,
    /// Returns a nested variant.
    Some(Inner),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a nested variant payload and returns it through the outer variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::None => Response::None,
            Request::Some(Inner::One(value)) => Response::Some(Inner::One(value + felt!(9))),
            Request::Some(Inner::Many(word)) => Response::Some(Inner::Many(Word::new([
                word.a + felt!(3),
                word.b + felt!(6),
                word.c + felt!(9),
                word.d + felt!(12),
            ]))),
        }
    }
}
"#;
    let note_body = r#"let none = roundtrip(Request::None);
match none {
    Response::None => (),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let one = roundtrip(Request::Some(Inner::One(felt!(24))));
match one {
    Response::Some(Inner::One(value)) => assert_eq!(value, felt!(33)),
    _ => assert_eq!(felt!(0), felt!(1)),
}

let word = Word::new([felt!(2), felt!(4), felt!(6), felt!(8)]);
let many = roundtrip(Request::Some(Inner::Many(word)));
match many {
    Response::Some(Inner::Many(value)) => {
        assert_eq!(value.a, felt!(5));
        assert_eq!(value.b, felt!(10));
        assert_eq!(value.c, felt!(15));
        assert_eq!(value.d, felt!(20));
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("nested_variant", account_source, note_body);
}

/// Tests variants whose payloads are mixed-scalar records.
#[test]
fn variant_with_mixed_struct_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Payload with mixed scalar fields and canonical ABI alignment requirements.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct MixedPayload {
    /// A 64-bit integer field.
    pub amount: u64,
    /// A field element.
    pub value: Felt,
    /// A 32-bit integer field.
    pub count: u32,
    /// A 16-bit integer field.
    pub small: u16,
    /// An 8-bit integer field.
    pub tiny: u8,
    /// A boolean field.
    pub flag: bool,
}

/// Request variants carrying mixed payload records.
#[export_type]
pub enum Request {
    /// Carries the first mixed payload.
    First(MixedPayload),
    /// Carries the second mixed payload.
    Second(MixedPayload),
}

/// Response variants carrying mixed payload records.
#[export_type]
pub enum Response {
    /// Returns the first transformed mixed payload.
    First(MixedPayload),
    /// Returns the second transformed mixed payload.
    Second(MixedPayload),
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a mixed record payload and returns it in the matching variant.
    pub fn roundtrip(&self, request: Request) -> Response {
        match request {
            Request::First(payload) => Response::First(MixedPayload {
                amount: payload.amount + 7,
                value: payload.value + felt!(7),
                count: payload.count + 7,
                small: payload.small + 7,
                tiny: payload.tiny + 7,
                flag: !payload.flag,
            }),
            Request::Second(payload) => Response::Second(MixedPayload {
                amount: payload.amount + 11,
                value: payload.value + felt!(11),
                count: payload.count + 11,
                small: payload.small + 11,
                tiny: payload.tiny + 11,
                flag: !payload.flag,
            }),
        }
    }
}
"#;
    let note_body = r#"let first = MixedPayload {
    amount: 100,
    value: felt!(10),
    count: 20,
    small: 30,
    tiny: 40,
    flag: false,
};
let first_result = roundtrip(Request::First(first));
match first_result {
    Response::First(value) => {
        if value.amount != 107 { assert_eq!(felt!(0), felt!(1)); }
        assert_eq!(value.value, felt!(17));
        if value.count != 27 { assert_eq!(felt!(0), felt!(1)); }
        if value.small != 37 { assert_eq!(felt!(0), felt!(1)); }
        if value.tiny != 47 { assert_eq!(felt!(0), felt!(1)); }
        if !value.flag { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}

let second = MixedPayload {
    amount: 200,
    value: felt!(20),
    count: 30,
    small: 40,
    tiny: 50,
    flag: true,
};
let second_result = roundtrip(Request::Second(second));
match second_result {
    Response::Second(value) => {
        if value.amount != 211 { assert_eq!(felt!(0), felt!(1)); }
        assert_eq!(value.value, felt!(31));
        if value.count != 41 { assert_eq!(felt!(0), felt!(1)); }
        if value.small != 51 { assert_eq!(felt!(0), felt!(1)); }
        if value.tiny != 61 { assert_eq!(felt!(0), felt!(1)); }
        if value.flag { assert_eq!(felt!(0), felt!(1)); }
    }
    _ => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_variant_case("mixed_struct", account_source, note_body);
}
