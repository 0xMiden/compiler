//! Prepares dependencies for generated note-constructor bindings.

/// Makes compiled dependency packages available to SDK binding generation.
fn main() {
    miden_sdk_build_script_support::prepare_package_cache();
}
