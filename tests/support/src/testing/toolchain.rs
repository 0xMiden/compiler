//! The Miden toolchain the tests run against.
//!
//! Packages come from `MIDEN_SYSROOT/lib` (or the session's sysroot, which the compiler derives
//! from `MIDENUP_HOME` and `MIDENUP_TOOLCHAIN`), exactly as they do for a user's build under
//! `miden build`. Nothing here embeds or vendors a package.

use std::{
    cell::RefCell,
    collections::BTreeMap,
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
use midenc_session::Session;
use sha2::{Digest, Sha256};

/// The workspace file that pins the toolchain, relative to this crate's manifest directory.
const TOOLCHAIN_FILE: &str = "../../miden-toolchain.toml";

/// Why no packages could be loaded.
#[derive(Debug, thiserror::Error)]
#[error(
    "no Miden toolchain packages at {path}: {reason}. Set MIDEN_SYSROOT to a toolchain directory \
     (its lib/ holds the .masp files), or install one with `midenup install {channel} --profile \
     empty --component core --component protocol` per miden-toolchain.toml"
)]
pub struct ToolchainError {
    path: PathBuf,
    reason: String,
    /// The channel `miden-toolchain.toml` pins, so the suggested command names the toolchain the
    /// repository actually wants. A placeholder when the file cannot be read — a missing
    /// toolchain file is not what this error is reporting.
    channel: String,
}

impl ToolchainError {
    fn new(path: impl Into<PathBuf>, reason: impl std::fmt::Display) -> Self {
        Self {
            path: path.into(),
            reason: reason.to_string(),
            channel: pinned_channel().unwrap_or_else(|_| String::from("<channel>")),
        }
    }
}

/// The sysroot named by the environment, or by the repository's toolchain file under midenup's
/// home.
///
/// `MIDEN_SYSROOT` wins outright — under `cargo make` it is always set, so nothing below runs
/// there. Otherwise the sysroot is `$MIDENUP_HOME/toolchains/<channel>`, with `MIDENUP_HOME`
/// defaulting to midenup's own default home and `<channel>` taken from `MIDENUP_TOOLCHAIN` or,
/// failing that, from the workspace's `miden-toolchain.toml` — the same derivation
/// `Makefile.toml` performs, so a direct `cargo nextest run` points at the toolchain the
/// repository pins rather than at a constant that has to be kept in step with it.
pub fn sysroot() -> PathBuf {
    if let Some(dir) = std::env::var_os("MIDEN_SYSROOT") {
        return PathBuf::from(dir);
    }
    let home = match std::env::var_os("MIDENUP_HOME") {
        Some(home) => PathBuf::from(home),
        None => home_dir().join(".local/share/midenup"),
    };
    home.join("toolchains").join(channel())
}

/// The toolchain channel: `MIDENUP_TOOLCHAIN`, else the one `miden-toolchain.toml` pins.
fn channel() -> String {
    if let Some(channel) = std::env::var_os("MIDENUP_TOOLCHAIN").filter(|c| !c.is_empty()) {
        return channel.to_string_lossy().into_owned();
    }
    pinned_channel().unwrap_or_else(|err| panic!("{err}"))
}

/// The `channel` value of the workspace's `miden-toolchain.toml`.
///
/// Matched by line rather than parsed: this crate has no `toml` dependency, and `Makefile.toml`
/// extracts the same value with a `sed` expression of the same shape.
fn pinned_channel() -> Result<String, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(TOOLCHAIN_FILE);
    let contents = std::fs::read_to_string(&manifest)
        .map_err(|err| format!("cannot read {}: {err}", manifest.display()))?;
    contents
        .lines()
        .filter_map(|line| line.trim().strip_prefix("channel"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .filter_map(|value| value.trim().strip_prefix('"'))
        .filter_map(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .next()
        .ok_or_else(|| format!("no `channel = \"...\"` entry in {}", manifest.display()))
}

/// Midenup's default home lives under the user's home directory, so an unset `HOME` has no
/// answer; say which variable to set instead of panicking on an unnamed `Option`.
fn home_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => panic!(
            "HOME is unset, so midenup's default home cannot be located: set MIDEN_SYSROOT to a \
             toolchain directory, or MIDENUP_HOME to midenup's home"
        ),
    }
}

/// Every package in `sysroot/lib`.
///
/// A sysroot is read once per test thread: a property test evaluates its program hundreds of
/// times, the core library's package alone is several megabytes, and the toolchain does not
/// change under a running test.
pub fn packages_in(sysroot: &Path) -> Result<Vec<Arc<Package>>, ToolchainError> {
    thread_local! {
        static LOADED: RefCell<BTreeMap<PathBuf, Vec<Arc<Package>>>> =
            const { RefCell::new(BTreeMap::new()) };
    }

    if let Some(packages) = LOADED.with_borrow(|loaded| loaded.get(sysroot).cloned()) {
        return Ok(packages);
    }
    let packages = read_packages_in(sysroot)?;
    LOADED.with_borrow_mut(|loaded| loaded.insert(sysroot.to_path_buf(), packages.clone()));
    Ok(packages)
}

/// Reads and deserializes every package in `sysroot/lib`.
fn read_packages_in(sysroot: &Path) -> Result<Vec<Arc<Package>>, ToolchainError> {
    let lib = sysroot.join("lib");
    let entries = std::fs::read_dir(&lib).map_err(|err| ToolchainError::new(&lib, err))?;
    let mut packages = Vec::new();
    for entry in entries {
        let path = entry.map_err(|err| ToolchainError::new(&lib, err))?.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("masp")) {
            let bytes = std::fs::read(&path).map_err(|err| ToolchainError::new(&path, err))?;
            let package = Package::read_from_bytes_trusted(&bytes)
                .map_err(|err| ToolchainError::new(&path, err))?;
            packages.push(Arc::new(package));
        }
    }
    if packages.is_empty() {
        return Err(ToolchainError::new(lib, "no .masp files"));
    }
    Ok(packages)
}

/// The packages of the session's toolchain, panicking with the install instructions if absent.
pub fn packages(session: &Session) -> Vec<Arc<Package>> {
    let root = session.options.sysroot.clone().unwrap_or_else(sysroot);
    packages_in(&root).unwrap_or_else(|err| panic!("{err}"))
}

/// The name of the standards library package every standard account component links against.
const STANDARDS_PACKAGE: &str = "miden-standards";

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
pub fn standard_component_packages() -> Vec<Arc<Package>> {
    STANDARD_COMPONENTS.iter().map(|package| Arc::new(package().clone())).collect()
}

/// A sysroot that extends the installed toolchain with the `miden-standards` account-component
/// packages.
///
/// Its `lib/` holds every package of [`sysroot`] except `miden-standards`, the `miden-standards`
/// crate's own build of the standards library in its place, and one `<name>.masp` per entry of
/// [`standard_component_packages`], so a project can depend on a standard component by name.
///
/// The standards library is taken from the crate because the components and the mock-chain
/// runtime both come from the crate, so the standards library compiled Rust code links against
/// must be the crate's build of it; the toolchain's prebuilt package may be a different build of
/// the same version.
///
/// The directory lives under the workspace's target directory, keyed by everything its contents
/// are derived from, and is shared by every test process that agrees on that key.
///
/// Panics if a component depends on a package whose digest differs from the one in the overlay.
pub fn sysroot_with_standard_components() -> PathBuf {
    static OVERLAY: OnceLock<PathBuf> = OnceLock::new();
    OVERLAY
        .get_or_init(|| {
            let root = crate::cargo_proj::test_target_dir().join("miden-sysroot-overlay");
            stage_overlay(&sysroot(), &root).unwrap_or_else(|err| panic!("{err}"))
        })
        .clone()
}

/// Stages the overlay of `toolchain` under `root`, or reuses an identical one already there.
fn stage_overlay(toolchain: &Path, root: &Path) -> Result<PathBuf, String> {
    let standards = StandardsLib::default().package();
    let components = standard_component_packages();

    let lib = toolchain.join("lib");
    let mut toolchain_files = fs::read_dir(&lib)
        .map_err(|err| format!("cannot read {}: {err}", lib.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("cannot read {}: {err}", lib.display()))?;
    toolchain_files.retain(|path| {
        path.extension().is_some_and(|ext| ext == "masp")
            && path.file_stem().is_some_and(|stem| stem != STANDARDS_PACKAGE)
    });
    toolchain_files.sort();

    // What each dependency name resolves to in the overlay: the toolchain packages, with the
    // standards library replaced by the crate's.
    let mut provided: BTreeMap<String, Word> = packages_in(toolchain)
        .map_err(|err| err.to_string())?
        .iter()
        .filter(|package| &*package.name != STANDARDS_PACKAGE)
        .map(|package| (package.name.to_string(), package.dependency_commitment()))
        .collect();
    provided.insert(STANDARDS_PACKAGE.to_string(), standards.dependency_commitment());
    for component in &components {
        check_component_dependencies(
            component,
            &provided,
            &standards.version.to_string(),
            toolchain,
        )?;
    }

    // The key names everything the overlay's contents are derived from, so a stale overlay is never
    // picked up after the toolchain or the crate changes.
    let mut key = Sha256::new();
    key.update(toolchain.as_os_str().as_encoded_bytes());
    for path in &toolchain_files {
        key.update(path.as_os_str().as_encoded_bytes());
    }
    for (name, digest) in &provided {
        key.update(name);
        key.update(word_hex(*digest));
    }
    for component in &components {
        key.update(word_hex(component.dependency_commitment()));
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
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging_lib).map_err(|err| io(&staging_lib, err))?;
    for source in &toolchain_files {
        let target = staging_lib.join(source.file_name().expect("a .masp file has a name"));
        link_or_copy(source, &target).map_err(|err| io(&target, err))?;
    }
    for package in core::iter::once(&standards).chain(&components) {
        package.write_masp_file(&staging_lib).map_err(|err| io(&staging_lib, err))?;
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

/// Checks that every dependency of `component` resolves to the package it was built against.
///
/// `provided` maps each package name in the overlay to its dependency commitment;
/// `standards_version` is the version of the `miden-standards` crate the component comes from, and
/// `toolchain` the toolchain the overlay extends.
fn check_component_dependencies(
    component: &Package,
    provided: &BTreeMap<String, Word>,
    standards_version: &str,
    toolchain: &Path,
) -> Result<(), String> {
    for dependency in component.manifest.dependencies() {
        let name: &str = &dependency.name;
        let found = provided
            .get(name)
            .map_or_else(|| String::from("no such package"), |digest| word_hex(*digest));
        if provided.get(name) == Some(&dependency.digest) {
            continue;
        }
        return Err(format!(
            "component package {} of the {STANDARDS_PACKAGE} crate {standards_version} depends on \
             {name} {}, but the sysroot overlay of the toolchain at {} provides {name} {found}: \
             align the toolchain channel in miden-toolchain.toml with the {STANDARDS_PACKAGE} \
             crate version",
            component.name,
            word_hex(dependency.digest),
            toolchain.display(),
        ));
    }
    Ok(())
}

/// The hex rendering of `word`.
fn word_hex(word: Word) -> String {
    word.to_hex()
}

/// Symlinks `target` to `source`, copying where symlinks are unavailable.
fn link_or_copy(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if std::os::unix::fs::symlink(source, target).is_ok() {
        return Ok(());
    }
    fs::copy(source, target).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sysroot_lib_directory_contains_the_core_and_protocol_packages() {
        let packages = packages_in(&sysroot()).unwrap_or_else(|err| panic!("{err}"));
        let mut names: Vec<&str> = packages.iter().map(|p| p.name.as_ref()).collect();
        names.sort();
        for expected in ["miden-core", "miden-protocol", "miden-standards", "miden-tx-kernel"] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
    }

    #[test]
    fn a_missing_sysroot_reports_how_to_install_one() {
        let err = packages_in(std::path::Path::new("/nonexistent/toolchain"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("MIDEN_SYSROOT"), "{err}");
        assert!(err.contains("midenup"), "{err}");
    }

    /// The channel is read from the workspace's toolchain file, so both the derived sysroot and
    /// the install instructions follow it rather than a constant kept in step by hand.
    #[test]
    fn the_channel_comes_from_the_workspace_toolchain_file() {
        let channel = pinned_channel().expect("the workspace pins a toolchain channel");
        assert!(!channel.is_empty(), "the pinned channel must not be empty");

        let err = packages_in(std::path::Path::new("/nonexistent/toolchain"))
            .unwrap_err()
            .to_string();
        assert!(err.contains(&format!("midenup install {channel}")), "{err}");
    }

    /// The overlay is the toolchain with the crate's standards library plus one account-component
    /// package per standard component.
    #[test]
    fn the_overlay_adds_the_standard_components_to_the_toolchain() {
        let toolchain = packages_in(&sysroot()).unwrap_or_else(|err| panic!("{err}"));
        let overlay =
            packages_in(&sysroot_with_standard_components()).unwrap_or_else(|err| panic!("{err}"));

        let mut toolchain_names: Vec<&str> = toolchain.iter().map(|p| p.name.as_ref()).collect();
        toolchain_names.sort();
        let (mut components, mut rest): (Vec<_>, Vec<_>) = overlay
            .iter()
            .partition(|p| p.kind == miden_mast_package::TargetType::AccountComponent);
        rest.sort_by(|a, b| a.name.cmp(&b.name));
        let rest_names: Vec<&str> = rest.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(rest_names, toolchain_names);
        let standards = rest.iter().find(|p| &*p.name == STANDARDS_PACKAGE).unwrap();
        assert_eq!(
            standards.dependency_commitment(),
            StandardsLib::default().package().dependency_commitment(),
            "the overlay's standards library must be the crate's"
        );

        components.sort_by(|a, b| a.name.cmp(&b.name));
        let mut expected: Vec<String> =
            standard_component_packages().iter().map(|p| p.name.to_string()).collect();
        expected.sort();
        let names: Vec<String> = components.iter().map(|p| p.name.to_string()).collect();
        assert_eq!(names.len(), 33, "{names:?}");
        assert_eq!(names, expected);
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
        check_component_dependencies(component, &provided, "1.2.3", toolchain)
            .expect("matching digests pass");

        let actual = provided["miden-protocol"];
        let drifted = Word::default();
        provided.insert("miden-protocol".to_string(), drifted);
        let err =
            check_component_dependencies(component, &provided, "1.2.3", toolchain).unwrap_err();
        for expected in [
            word_hex(actual).as_str(),
            word_hex(drifted).as_str(),
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
