// Build the core library's linker stubs and link them for dependents. The stub source
// `stubs/core.rs` is generated from the core package (see `tests/generated.rs`); the recipe, and
// why the stubs are an archive at all, is `miden_sdk_build_script_support::stubs`. The
// compiler-intrinsic stubs are built by `miden-intrinsics-sys`.

use std::{env, path::PathBuf};

use miden_sdk_build_script_support::stubs::{StubArchive, compile_stub_archive};

fn main() {
    println!("cargo::rerun-if-env-changed=MIDENC_TARGET_IS_MIDEN_VM");
    println!("cargo::rustc-check-cfg=cfg(miden)");
    if env::var_os("MIDENC_TARGET_IS_MIDEN_VM").is_some() {
        println!("cargo::rustc-cfg=miden");
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    compile_stub_archive(&StubArchive {
        crate_name: "miden_stdlib_sys_core_stubs".into(),
        source: manifest_dir.join("stubs/core.rs"),
    });
}
