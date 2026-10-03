//! The committed generated files are exactly what the generator produces for the toolchain's
//! protocol and standards packages. `UPDATE_BINDINGS=1` rewrites them.

use std::{collections::BTreeMap, path::Path};

use miden_sdk_bindgen::{External, Generated, Options, generate};
use midenc_integration_test_support::testing::{bindings::check_generated, toolchain};
use midenc_package_interface::PackageInterface;

const PROTOCOL_ROOT: &str = "::miden::protocol";
const STANDARDS_ROOT: &str = "::miden::standards";
const SUPPORT: &str = "::miden_intrinsics_sys::support";
/// Where `src/raw/mod.rs` mounts the protocol's bindings, which the standards' bindings name.
const PROTOCOL_RUST_PATH: &str = "crate::raw::protocol";

/// The protocol's and the standards' generated text, in that order.
fn generated() -> (Generated, Generated) {
    let packages =
        toolchain::packages_in(&toolchain::sysroot()).unwrap_or_else(|err| panic!("{err}"));
    let interface = |name: &str| {
        let package = packages
            .iter()
            .find(|package| AsRef::<str>::as_ref(&package.name) == name)
            .unwrap_or_else(|| panic!("{name} in the sysroot"));
        PackageInterface::from_package(package)
    };
    let protocol_iface = interface("miden-protocol");
    let standards_iface = interface("miden-standards");

    let protocol_options = Options {
        root: PROTOCOL_ROOT.into(),
        support: SUPPORT.into(),
        with: BTreeMap::new(),
    };
    let protocol = generate(&protocol_iface, &[], &protocol_options)
        .unwrap_or_else(|err| panic!("miden-protocol: {err}"));

    let standards_options = Options {
        root: STANDARDS_ROOT.into(),
        support: SUPPORT.into(),
        with: BTreeMap::from([(
            "miden-protocol".to_string(),
            External {
                root: PROTOCOL_ROOT.into(),
                rust_path: PROTOCOL_RUST_PATH.into(),
            },
        )]),
    };
    let standards = generate(&standards_iface, &[&protocol_iface], &standards_options)
        .unwrap_or_else(|err| panic!("miden-standards: {err}"));
    (protocol, standards)
}

#[test]
fn committed_bindings_are_current() {
    let (protocol, standards) = generated();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    check_generated(manifest_dir, "src/raw/protocol.rs", &protocol.bindings);
    check_generated(manifest_dir, "stubs/protocol.rs", &protocol.stubs);
    check_generated(manifest_dir, "src/raw/standards.rs", &standards.bindings);
    check_generated(manifest_dir, "stubs/standards.rs", &standards.stubs);
}

/// The procedures without bindings: the protocol's variadic FPI executor (the SDK reaches it
/// through the compiler's `execute_foreign_procedure_indirect` instead), and of the standards'
/// procedures four that need more operand stack elements than the budget and three without a
/// typed signature. Neither package has string constants.
#[test]
fn the_skipped_exports_are_the_known_ones() {
    let (protocol, standards) = generated();
    // Each path relative to its package's root.
    let paths = |generated: &Generated, root: &str| -> Vec<String> {
        let root = format!("{root}::");
        generated
            .skipped
            .iter()
            .map(|skipped| skipped.path.strip_prefix(&root).unwrap_or(&skipped.path).to_string())
            .collect()
    };
    assert_eq!(paths(&protocol, PROTOCOL_ROOT), ["tx::execute_foreign_procedure"]);
    assert_eq!(
        paths(&standards, STANDARDS_ROOT),
        [
            "auth::create_tx_summary",
            "auth::create_tx_summary_with_block",
            "auth::eip712::verify_raw",
            "auth::hash_and_insert_tx_summary",
            "note::note_reclaim::assert_reclaimable",
            "note::note_target::assert_active_account_is_network_target_account",
            "note::note_target::assert_active_account_is_target_account",
        ]
    );
}
