// Build the linker stubs of the protocol and standards packages, and of the compiler-handled
// procedures this crate binds by hand, and link them for dependents. `stubs/protocol.rs` and
// `stubs/standards.rs` are generated from the packages (see `tests/generated.rs`);
// `stubs/intrinsics.rs` is written by hand. The recipe, and why the stubs are an archive at all,
// is `miden_sdk_build_script_support::stubs`.

use std::{env, path::PathBuf};

use miden_sdk_build_script_support::stubs::{StubArchive, compile_stub_archive};

fn main() {
    println!("cargo::rerun-if-env-changed=MIDENC_TARGET_IS_MIDEN_VM");
    println!("cargo::rustc-check-cfg=cfg(miden)");
    if env::var_os("MIDENC_TARGET_IS_MIDEN_VM").is_some() {
        println!("cargo::rustc-cfg=miden");
    }

    let stubs = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("stubs");
    for (crate_name, source) in [
        ("miden_base_sys_protocol_stubs", "protocol.rs"),
        ("miden_base_sys_standards_stubs", "standards.rs"),
        ("miden_base_sys_intrinsics_stubs", "intrinsics.rs"),
    ] {
        compile_stub_archive(&StubArchive {
            crate_name: crate_name.into(),
            source: stubs.join(source),
        });
    }
}
