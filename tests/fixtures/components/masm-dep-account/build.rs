fn main() {
    miden_sdk_build_script_support::prepare_package_cache();
    miden_sdk_build_script_support::generate_bindings(&miden_sdk_build_script_support::Bindings {
        package: "masm-dep",
        root: "",
        support: "::miden::support",
        with: &[],
    });
}
