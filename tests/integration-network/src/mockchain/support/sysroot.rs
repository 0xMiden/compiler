//! A test sysroot that extends the installed Miden toolchain with the `miden-standards` account
//! components.
//!
//! The installed toolchain does not ship the standard account-component packages yet, so the tests
//! that depend on a standard component by name compile against an overlay of the toolchain that
//! adds them, taken from the `miden-standards` crate the workspace builds against.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use miden_core::Word;
use miden_mast_package::Package;
use miden_standards::{
    StandardsLib,
    account::{
        access::{Authority, Ownable2Step, Pausable, PausableManager, RoleBasedAccessControl},
        auth::{
            AuthGuardedMultisig, AuthMultisig, AuthMultisigSmart, AuthNetworkAccount,
            AuthSingleSig, AuthTxFeeCollector, NoAuth,
        },
        faucets::{FungibleFaucet, NonFungibleFaucet},
        fees::{BasicConstantFeePolicy, ConstantFeeManager},
        inspection::{AccountSchemaCommitment, CodeInspection},
        note_creator::NoteCreator,
        oracle::PriceOracle,
        policies::{
            AllowlistManager, BasicAllowlist, BasicBlocklist, BlocklistManager, BurnAllowAll,
            BurnOwnerOnly, MinBurnAmount, MintAllowAll, MintOwnerOnly, TokenPolicyManager,
            TransferAllowAll,
        },
        upgrade::UpgradeManager,
        wallets::BasicWallet,
    },
};
use midenc_frontend_wasm_metadata::package_cache::package_file_name;
use midenc_integration_test_support::{
    cargo_proj::test_target_dir,
    testing::toolchain::{package_files_in, sysroot},
};
use sha2::{Digest, Sha256};

/// The name of the standards library package every standard account component links against.
const STANDARDS_PACKAGE: &str = "miden-standards";

/// The version of the overlay's directory layout, part of the overlay key.
///
/// Bump it whenever the way an overlay is staged changes (its layout, or how its files are
/// linked or written), so an overlay staged by an older test build is never reused.
const OVERLAY_FORMAT_VERSION: u32 = 1;

/// The package of every account component the `miden-standards` crate defines.
const STANDARD_COMPONENTS: [fn() -> &'static Package; 33] = [
    || Authority::code().as_package(),
    || Ownable2Step::code().as_package(),
    || Pausable::code().as_package(),
    || PausableManager::code().as_package(),
    || RoleBasedAccessControl::code().as_package(),
    || AuthGuardedMultisig::code().as_package(),
    || AuthMultisig::code().as_package(),
    || AuthMultisigSmart::code().as_package(),
    || AuthNetworkAccount::code().as_package(),
    || AuthSingleSig::code().as_package(),
    || AuthTxFeeCollector::code().as_package(),
    || NoAuth::code().as_package(),
    || FungibleFaucet::code().as_package(),
    || NonFungibleFaucet::code().as_package(),
    || BasicConstantFeePolicy::code().as_package(),
    || ConstantFeeManager::code().as_package(),
    || AccountSchemaCommitment::code().as_package(),
    || CodeInspection::code().as_package(),
    || NoteCreator::code().as_package(),
    || PriceOracle::code().as_package(),
    || AllowlistManager::code().as_package(),
    || BasicAllowlist::code().as_package(),
    || BasicBlocklist::code().as_package(),
    || BlocklistManager::code().as_package(),
    || BurnAllowAll::code().as_package(),
    || BurnOwnerOnly::code().as_package(),
    || MinBurnAmount::code().as_package(),
    || MintAllowAll::code().as_package(),
    || MintOwnerOnly::code().as_package(),
    || TokenPolicyManager::code().as_package(),
    || TransferAllowAll::code().as_package(),
    || UpgradeManager::code().as_package(),
    || BasicWallet::code().as_package(),
];

/// The account-component packages of the `miden-standards` crate.
///
/// The installed toolchain does not ship these yet, so the tests take them from the crate the
/// workspace builds against.
fn standard_component_packages() -> Vec<Arc<Package>> {
    STANDARD_COMPONENTS.iter().map(|package| Arc::new(package().clone())).collect()
}

/// A sysroot that extends the installed toolchain with the `miden-standards` account-component
/// packages.
///
/// Its `lib/` holds the `miden-standards` crate's own build of the standards library, one
/// `<name>.masp` per entry of [`standard_component_packages`], so a project can depend on a
/// standard component by name, and every other package of [`sysroot`]: a toolchain package with
/// the name of one of these is replaced by it.
///
/// The standards library is taken from the crate because the components and the mock-chain
/// runtime both come from the crate, so the standards library compiled Rust code links against
/// must be the crate's build of it; the toolchain's prebuilt package may be a different build of
/// the same version.
///
/// The directory lives under the workspace's target directory, keyed by everything its contents
/// are derived from, and is shared by every test process that agrees on that key.
///
/// Panics if a package in the overlay (a standard component, the standards library or a kept
/// toolchain package) depends on a package the overlay lacks or provides with another digest, if
/// two toolchain files carry the same package, or if the toolchain cannot be read or the overlay
/// cannot be staged.
pub(crate) fn sysroot_with_standard_components() -> PathBuf {
    static OVERLAY: OnceLock<PathBuf> = OnceLock::new();
    OVERLAY
        .get_or_init(|| {
            let root = test_target_dir().join("miden-sysroot-overlay");
            stage_overlay(&sysroot(), &root).unwrap_or_else(|err| panic!("{err}"))
        })
        .clone()
}

/// Stages the overlay of `toolchain` under `root`, or reuses an identical one already there.
fn stage_overlay(toolchain: &Path, root: &Path) -> Result<PathBuf, String> {
    // The toolchain packages are symlinked into the overlay, and a relative symlink target would
    // resolve against the overlay's `lib/` rather than the current directory.
    let toolchain = toolchain
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", toolchain.display()))?;
    let toolchain = toolchain.as_path();
    let standards = StandardsLib::default().package();
    let components = standard_component_packages();
    let written: Vec<&Package> = core::iter::once(&*standards)
        .chain(components.iter().map(|component| &**component))
        .collect();
    let written_names: BTreeSet<&str> = written.iter().map(|package| &*package.name).collect();

    // The toolchain packages the overlay keeps, linked from the files they were read from: those
    // the overlay does not replace.
    let installed = package_files_in(toolchain).map_err(|err| err.to_string())?;
    let mut installed_names: BTreeMap<&str, &Path> = BTreeMap::new();
    for (path, package) in &installed {
        if let Some(other) = installed_names.insert(&package.name, path) {
            return Err(format!(
                "toolchain files {} and {} both carry package {}",
                other.display(),
                path.display(),
                package.name
            ));
        }
    }
    let kept: Vec<(&Path, &Package)> = installed
        .iter()
        .map(|(path, package)| (path.as_path(), &**package))
        .filter(|(_, package)| !written_names.contains(&*package.name))
        .collect();
    // Writing a package onto a linked file would follow the symlink and overwrite the installed
    // toolchain's file, so no kept file may have the file name of a written package.
    let written_files: BTreeSet<String> =
        written.iter().map(|package| package_file_name(&package.name)).collect();
    if let Some((path, _)) = kept.iter().find(|(path, _)| {
        path.file_name()
            .is_some_and(|name| written_files.contains(&*name.to_string_lossy()))
    }) {
        return Err(format!(
            "toolchain file {} carries another package than the one the overlay writes under its \
             name",
            path.display()
        ));
    }
    let toolchain_files: Vec<&Path> = kept.iter().map(|(path, _)| *path).collect();

    // What each dependency name resolves to in the overlay: the toolchain packages, with the
    // packages the overlay writes replacing any of the same name.
    let provided: BTreeMap<String, Word> = kept
        .iter()
        .map(|(_, package)| *package)
        .chain(written.iter().copied())
        .map(|package| (package.name.to_string(), package.dependency_commitment()))
        .collect();
    let standards_version = standards.version.to_string();
    for package in &written {
        let origin = format!(
            "package {} of the {STANDARDS_PACKAGE} crate {standards_version}",
            package.name
        );
        check_dependencies(package, &origin, &provided, toolchain)?;
    }
    // A kept toolchain package may depend on a package the overlay replaces.
    for (_, package) in &kept {
        let origin = format!("toolchain package {}", package.name);
        check_dependencies(package, &origin, &provided, toolchain)?;
    }

    // The key names everything the overlay's contents are derived from, so a stale overlay is never
    // picked up after the toolchain, the crate or the staging format changes.
    let mut key = Sha256::new();
    key.update(OVERLAY_FORMAT_VERSION.to_le_bytes());
    key.update(toolchain.as_os_str().as_encoded_bytes());
    for path in &toolchain_files {
        key.update(path.as_os_str().as_encoded_bytes());
    }
    for (name, digest) in &provided {
        key.update(name);
        key.update(digest.to_hex());
    }
    let key: String = key.finalize().iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    let dirname = toolchain.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    let overlay = root.join(format!("{dirname}-{key}"));
    if overlay.is_dir() {
        return Ok(overlay);
    }

    // Built in a private sibling and renamed into place, so a concurrent test process either sees
    // no overlay or a complete one.
    let staging = root.join(format!(".{dirname}-{key}.{}", std::process::id()));
    let staging_lib = staging.join("lib");
    let io = |path: &Path, err: std::io::Error| format!("cannot stage {}: {err}", path.display());
    // A directory already here was left by a crashed process that had the same pid; it may be
    // incomplete, so it is never reused. Failing to remove a missing directory is expected.
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging_lib).map_err(|err| io(&staging_lib, err))?;
    for source in &toolchain_files {
        let target = staging_lib.join(source.file_name().expect("a .masp file has a name"));
        link_or_copy(source, &target).map_err(|err| io(&target, err))?;
    }
    for package in &written {
        let target = staging_lib.join(package_file_name(&package.name));
        package.write_to_file(&target).map_err(|err| io(&target, err))?;
    }
    match fs::rename(&staging, &overlay) {
        Ok(()) => Ok(overlay),
        // Another process renamed its identical overlay into place first.
        Err(_) if overlay.is_dir() => {
            let _ = fs::remove_dir_all(&staging);
            Ok(overlay)
        }
        Err(err) => Err(io(&overlay, err)),
    }
}

/// Checks that every dependency of `package`, a package in the overlay (one the overlay writes or a
/// kept toolchain package), resolves to the package it was built against.
///
/// `origin` describes the package in the error, e.g. `package miden-standards of the
/// miden-standards crate 0.17.0`; `provided` maps each package name in the overlay to its
/// dependency commitment, and `toolchain` is the toolchain the overlay extends.
fn check_dependencies(
    package: &Package,
    origin: &str,
    provided: &BTreeMap<String, Word>,
    toolchain: &Path,
) -> Result<(), String> {
    for dependency in package.manifest.dependencies() {
        let name: &str = &dependency.name;
        if provided.get(name) == Some(&dependency.digest) {
            continue;
        }
        let found = provided
            .get(name)
            .map_or_else(|| String::from("no such package"), |digest| digest.to_hex());
        return Err(format!(
            "{origin} depends on {name} {}, but the sysroot overlay of the toolchain at {} \
             provides {name} {found}: align the toolchain channel in miden-toolchain.toml with \
             the {STANDARDS_PACKAGE} crate version",
            dependency.digest.to_hex(),
            toolchain.display(),
        ));
    }
    Ok(())
}

/// Symlinks `target` to `source`, or copies `source` to `target` where symlinks are unsupported.
///
/// Any other failure is returned, `target` already existing included: a copy onto an existing
/// symlink would follow it and truncate the toolchain file it points to.
fn link_or_copy(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    match std::os::unix::fs::symlink(source, target) {
        Ok(()) => return Ok(()),
        // `EPERM` and `ENOSYS`: the file system does not support symlinks.
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) => {}
        Err(err) => return Err(err),
    }
    fs::copy(source, target).map(drop)
}

#[cfg(test)]
mod tests {
    use miden_mast_package::TargetType;

    use super::*;

    /// The overlay is the toolchain with the crate's standards library plus one account-component
    /// package per standard component.
    #[test]
    fn the_overlay_adds_the_standard_components_to_the_toolchain() {
        let names = |packages: Vec<&Package>| {
            let mut names: Vec<String> = packages.iter().map(|p| p.name.to_string()).collect();
            names.sort();
            names
        };
        let is_component = |p: &&Package| p.kind == TargetType::AccountComponent;
        let toolchain = package_files_in(&sysroot()).unwrap_or_else(|err| panic!("{err}"));
        let toolchain: Vec<&Package> = toolchain.iter().map(|(_, p)| &**p).collect();
        let overlay = package_files_in(&sysroot_with_standard_components())
            .unwrap_or_else(|err| panic!("{err}"));
        let overlay: Vec<&Package> = overlay.iter().map(|(_, p)| &**p).collect();

        // The toolchain's other packages are kept, its standards library replaced.
        let (components, rest): (Vec<&Package>, Vec<&Package>) =
            overlay.iter().copied().partition(is_component);
        let toolchain_rest: Vec<&Package> =
            toolchain.iter().copied().filter(|p| !is_component(p)).collect();
        assert_eq!(names(rest.clone()), names(toolchain_rest));
        let standards = rest.iter().find(|p| &*p.name == STANDARDS_PACKAGE).unwrap();
        assert_eq!(
            standards.dependency_commitment(),
            StandardsLib::default().package().dependency_commitment(),
            "the overlay's standards library must be the crate's"
        );

        // The standard components are added to any the toolchain ships.
        let mut expected: Vec<String> = standard_component_packages()
            .iter()
            .map(|p| p.name.to_string())
            .chain(toolchain.iter().copied().filter(is_component).map(|p| p.name.to_string()))
            .collect();
        expected.sort();
        expected.dedup();
        assert_eq!(names(components), expected);
    }

    /// A component built against another package than the overlay provides is reported with both
    /// digests.
    #[test]
    fn a_drifted_dependency_names_both_digests() {
        let component = &standard_component_packages()[0];
        let mut provided: BTreeMap<String, Word> = component
            .manifest
            .dependencies()
            .map(|dependency| (dependency.name.to_string(), dependency.digest))
            .collect();
        let toolchain = Path::new("/some/toolchain");
        let origin = format!("package {} of the miden-standards crate 1.2.3", component.name);
        check_dependencies(component, &origin, &provided, toolchain)
            .expect("matching digests pass");

        let actual = provided["miden-protocol"];
        let drifted = Word::default();
        provided.insert("miden-protocol".to_string(), drifted);
        let err = check_dependencies(component, &origin, &provided, toolchain).unwrap_err();
        for expected in [
            actual.to_hex().as_str(),
            drifted.to_hex().as_str(),
            "miden-protocol",
            &component.name,
            "1.2.3",
            "/some/toolchain",
            "miden-toolchain.toml",
        ] {
            assert!(err.contains(expected), "missing {expected} in {err}");
        }
    }
}
