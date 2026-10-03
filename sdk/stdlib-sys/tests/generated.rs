//! The committed generated files are exactly what the generator produces for the toolchain's
//! core package. `UPDATE_BINDINGS=1` rewrites them.

use std::{collections::BTreeMap, path::Path};

use miden_sdk_bindgen::{Generated, Options, generate};
use midenc_integration_test_support::testing::{bindings::check_generated, toolchain};
use midenc_package_interface::PackageInterface;

const ROOT: &str = "::miden::core";
const SUPPORT: &str = "::miden_intrinsics_sys::support";

fn generated() -> Generated {
    let packages =
        toolchain::packages_in(&toolchain::sysroot()).unwrap_or_else(|err| panic!("{err}"));
    let core = packages
        .iter()
        .find(|package| AsRef::<str>::as_ref(&package.name) == "miden-core")
        .expect("miden-core in the sysroot");
    let iface = PackageInterface::from_package(core);
    let options = Options {
        root: ROOT.into(),
        support: SUPPORT.into(),
        with: BTreeMap::new(),
    };
    generate(&iface, &[], &options).unwrap_or_else(|err| panic!("{err}"))
}

#[test]
fn committed_bindings_are_current() {
    let generated = generated();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    check_generated(manifest_dir, "src/raw/core.rs", &generated.bindings);
    check_generated(manifest_dir, "stubs/core.rs", &generated.stubs);
}

/// The core procedures without bindings: the 76 the interface cannot lower (69 take or return a
/// `u128` or `u256` by value, 7 need more operand stack elements than the budget). The two
/// `load_128_mem`, which take a *pointer* to a `u128`, are bound: in memory a `u128` is the
/// same sixteen little-endian bytes on both sides. Core has no string constants.
#[test]
fn the_skipped_core_exports_are_the_known_ones() {
    let skipped = generated().skipped;
    let paths: Vec<&str> = skipped.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(skipped.len(), 76, "{paths:#?}");
    for load in ["k1_base", "k1_scalar"] {
        let path = format!("::miden::core::precompiles::fields::{load}::load_128_mem");
        assert!(!paths.contains(&path.as_str()), "{path} is skipped: {paths:#?}");
    }
}
