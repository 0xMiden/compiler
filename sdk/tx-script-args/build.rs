use std::env;

fn main() {
    // Mirror the SDK convention: the `miden` cfg marks builds targeting the Miden VM, driven by
    // the compiler pipeline via `MIDENC_TARGET_IS_MIDEN_VM` (or `--cfg miden` in rustflags).
    println!("cargo::rerun-if-env-changed=MIDENC_TARGET_IS_MIDEN_VM");
    println!("cargo::rustc-check-cfg=cfg(miden)");
    if env::var_os("MIDENC_TARGET_IS_MIDEN_VM").is_some() {
        println!("cargo::rustc-cfg=miden");
    }
}
