//! Prepares dependencies for generated note-constructor bindings.

/// Makes compiled dependency packages available to SDK binding generation.
///
/// This exists for plain `cargo` and IDE builds of the fixture; midenc-driven builds return early.
fn main() {
    miden_sdk_build_script_support::prepare_package_cache();
}
