// Build the compiler-intrinsic stubs and link them for dependents. The recipe, and why the
// stubs are an archive at all, is `miden_sdk_build_script_support::stubs`.

use std::{env, path::PathBuf};

use miden_sdk_build_script_support::stubs::{StubArchive, compile_stub_archive};

fn main() {
    println!("cargo::rerun-if-env-changed=MIDENC_TARGET_IS_MIDEN_VM");
    println!("cargo::rustc-check-cfg=cfg(miden)");
    if env::var_os("MIDENC_TARGET_IS_MIDEN_VM").is_some() {
        println!("cargo::rustc-cfg=miden");
    }

    let stubs = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("stubs");
    // The stub crate root only declares modules; watch the whole tree, not just the root.
    println!("cargo:rerun-if-changed={}", stubs.display());
    compile_stub_archive(&StubArchive {
        crate_name: "miden_intrinsics_sys_stubs".into(),
        source: stubs.join("intrinsics_root.rs"),
    });
}
