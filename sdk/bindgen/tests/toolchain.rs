//! Generation over the real toolchain packages: it must succeed, the output must be Rust, and
//! what is skipped must be exactly what the interface skips, the string constants, and the
//! procedures whose types have no Rust form.

use std::collections::BTreeMap;

use miden_assembly_syntax::ast::ConstantValue;
use miden_mast_package::TargetType;
use miden_sdk_bindgen::{External, Generated, Options, Skipped, generate};
use midenc_integration_test_support::testing::toolchain;
use midenc_package_interface::PackageInterface;

/// The generated bindings and stubs of `iface` must parse, and report every procedure the
/// interface skips and every string constant. Returns the other entries: the bindable procedures
/// the bindings could not bind.
fn check<'g>(iface: &PackageInterface, generated: &'g Generated) -> Vec<&'g Skipped> {
    let name = &iface.name;
    syn::parse_file(&generated.bindings).unwrap_or_else(|err| panic!("{name}: {err}"));
    syn::parse_file(&generated.stubs).unwrap_or_else(|err| panic!("{name}: {err}"));
    let strings = iface
        .constants
        .iter()
        .filter(|c| matches!(c.value, ConstantValue::String(_)))
        .map(|c| c.path.to_string());
    let expected: Vec<String> = iface
        .skipped()
        .map(|(procedure, _)| procedure.path.to_string())
        .chain(strings)
        .collect();
    for path in &expected {
        assert!(generated.skipped.iter().any(|s| &s.path == path), "{name}: {path} not reported");
    }
    generated.skipped.iter().filter(|s| !expected.contains(&s.path)).collect()
}

#[test]
fn every_toolchain_library_generates_parseable_rust() {
    let packages =
        toolchain::packages_in(&toolchain::sysroot()).unwrap_or_else(|err| panic!("{err}"));
    let interfaces: Vec<PackageInterface> = packages
        .iter()
        // The kernel's procedures are not `exec`-bindable, and its modules sit under `$kernel`.
        .filter(|package| package.kind == TargetType::Library)
        .map(|package| PackageInterface::from_package(package))
        .collect();
    let protocol = interfaces
        .iter()
        .find(|iface| AsRef::<str>::as_ref(&iface.name) == "miden-protocol")
        .expect("the toolchain has the protocol package");
    let with_protocol = BTreeMap::from([(
        "miden-protocol".to_string(),
        External {
            root: "::miden::protocol".into(),
            rust_path: "crate::raw::protocol".into(),
        },
    )]);

    let mut generated = Vec::new();
    for iface in &interfaces {
        let name: &str = iface.name.as_ref();
        let root = format!("::miden::{}", name.trim_start_matches("miden-"));
        let options = Options {
            root,
            support: "crate::__support".into(),
            with: BTreeMap::new(),
        };
        let output = generate(iface, &[], &options).unwrap_or_else(|err| panic!("{name}: {err}"));
        let unbound: Vec<(&str, &str)> = check(iface, &output)
            .into_iter()
            .map(|skipped| (skipped.path.as_str(), skipped.reason.as_str()))
            .collect();
        match name {
            "miden-core" => {
                assert!(unbound.is_empty(), "{name}: {unbound:?}");
                assert!(
                    output.bindings.contains(
                        "pub unsafe fn hash_elements(arg0: ElementPtr<Felt>, arg1: u32) -> Word {"
                    ),
                    "{name}: an element pointer parameter is an `ElementPtr`"
                );
                // A pointer to a `u128` has a Rust form: the two layouts agree in memory.
                assert!(
                    output
                        .bindings
                        .contains("pub unsafe fn load_128_mem(arg0: ElementPtr<u128>) -> Expr {"),
                    "{name}: a `u128` pointee is written as `u128`"
                );
            }
            "miden-protocol" => {
                assert!(unbound.is_empty(), "{name}: {unbound:?}");
                assert!(
                    output.bindings.contains("pub fn get_id() -> super::types::AccountId {"),
                    "{name}: a struct result refers to the type export"
                );
            }
            _ => assert!(unbound.is_empty(), "{name}: {unbound:?}"),
        }
        // Every symbol the bindings declare has a stub.
        let declared = output.bindings.matches("#[link_name = ").count();
        assert_eq!(declared, output.stubs.matches("#[unsafe(export_name = ").count(), "{name}");
        assert_eq!(declared, iface.bindable().count() - unbound.len(), "{name}");
        // The standards use the protocol's types; with the protocol's bindings named, they refer
        // to those instead of declaring their own.
        if name == "miden-standards" {
            let options = Options {
                with: with_protocol.clone(),
                ..options
            };
            let output = generate(iface, &[protocol], &options)
                .unwrap_or_else(|err| panic!("{name}: {err}"));
            let unbound = check(iface, &output);
            assert!(unbound.is_empty(), "{name}: {unbound:?}");
            assert!(
                output.bindings.contains("crate::raw::protocol::types::AccountId"),
                "{name}: the standards' `AccountId` fields refer to the protocol's"
            );
        }
        generated.push(name.to_string());
    }
    generated.sort();
    assert_eq!(generated, ["miden-core", "miden-protocol", "miden-standards"]);
}
