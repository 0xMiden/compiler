use std::path::Path;

mod auth_component_no_auth;
mod auth_component_rpo_falcon512;
mod basic_wallet_package_sizes;
mod collatz;
mod counter_contract_debug_build;
mod counter_metadata;
mod counter_note;
mod fibonacci;
mod is_prime;
mod storage_metadata;

fn persist_cargo_miden_dependency(
    project_path: impl AsRef<Path>,
    package: &miden_mast_package::Package,
) {
    package
        .write_masp_file(project_path.as_ref().join("target").join("miden").join("release"))
        .expect("failed to persist compiled Miden dependency package");
}
