use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::{ArgAction, Args};
use miden_assembly::{Assembler, Path, ProjectTargetSelector};
use miden_package_registry::InMemoryPackageRegistry;
use miden_project::{Package, Target, TargetType};
use miden_protocol::ProtocolLib;

#[derive(Debug, Clone, Args)]
pub struct Masm2MaspCommand {
    /// Path to the input .masm file
    pub masm_path: PathBuf,
    /// Target type (e.g. account-component)
    pub target_type: String,
    /// Path of output directory to create, must *not* yet exist
    // TODO since using `virtual` target, try without creating out_dir and manifest file
    pub out_path: PathBuf,
    /// Don't link against the default protocol library
    #[arg(long = "no-protocol-lib", action = ArgAction::SetFalse, default_value_t = true)]
    link_protocol_lib: bool,
}

pub fn run(command: Masm2MaspCommand) -> Result<()> {
    let target_type: TargetType = command
        .target_type
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse target_type: {e}"))?;
    if target_type != TargetType::AccountComponent {
        // TODO add support for more `TargeType`s as needed
        anyhow::bail!("only AccountComponent target type is currently supported");
    }

    if command.out_path.exists() {
        anyhow::bail!("output directory '{}' already exists", command.out_path.display());
    }

    fs::create_dir_all(&command.out_path)
        .with_context(|| format!("failed to create directory '{}'", command.out_path.display()))?;
    // TODO cleanup the output directory if any of the below fails

    let masm_filename = command
        .masm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .with_context(|| format!("invalid masm file path '{}'", command.masm_path.display()))?;

    let package_name = masm_filename.to_string();
    let namespace_str = masm_filename.replace('-', "::");

    let namespace_path: Arc<Path> = Path::new(&namespace_str).to_path_buf().into();
    let target = Target::r#virtual(target_type, package_name.as_str(), namespace_path)
        .with_path(format!("{masm_filename}.masm"));

    let version = miden_project::semver::Version::new(0, 1, 0);
    let package = Package::new(package_name.as_str(), target).with_version(version);

    let manifest_toml = package
        .to_toml()
        .map_err(|e| anyhow::anyhow!("failed to generate manifest: {e}"))?;

    let manifest_path = command.out_path.join("miden-project.toml");
    fs::write(&manifest_path, &manifest_toml)
        .with_context(|| format!("failed to write '{}'", manifest_path.display()))?;

    let masm_dest = command.out_path.join(format!("{masm_filename}.masm"));
    fs::copy(&command.masm_path, &masm_dest)
        .with_context(|| format!("failed to copy masm file to '{}'", masm_dest.display()))?;

    let mut store = InMemoryPackageRegistry::default();
    let mut assembler = Assembler::default();
    if command.link_protocol_lib {
        // TODO record the protocol dependency in `miden-project.toml`
        assembler = assembler
            .with_dynamic_library(ProtocolLib::default())
            .map_err(|e| anyhow::anyhow!("failed to link protocol library: {e}"))?;
    }
    let mut project_assembler = assembler
        .for_project_at_path(&manifest_path, &mut store)
        .map_err(|e| anyhow::anyhow!("failed to load project: {e}"))?;

    let package = project_assembler
        .assemble(ProjectTargetSelector::Library, "release")
        .map_err(|e| anyhow::anyhow!("failed to assemble package: {e}"))?;

    package.write_masp_file(&command.out_path).with_context(|| {
        format!("failed to write .masp file to '{}'", command.out_path.display())
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use miden_assembly::debuginfo::DefaultSourceManager;
    use miden_core::serde::Deserializable;
    use miden_project::Project;

    use super::*;

    fn collect_masp_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| {
                let e = entry.ok()?;
                let path = e.path();
                if path.extension().is_some_and(|ext| ext == "masp") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect()
    }

    const TEST_MASM_SIMPLE: &str = r#"pub proc foo
    push.1
    push.2
    add
end
"#;

    fn create_test_masm(content: &str, filename: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let masm_path = dir.path().join(filename);
        fs::write(&masm_path, content).unwrap();
        (dir, masm_path)
    }

    #[test]
    fn existing_destination_fails() {
        let dir = tempfile::tempdir().unwrap();
        let existing_dir = dir.path().join("output");
        fs::create_dir(&existing_dir).unwrap();

        let masm_path = dir.path().join("input.masm");
        fs::write(&masm_path, "").unwrap();

        let result = run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: existing_dir,
            link_protocol_lib: true,
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn happy_path_creates_expected_files() {
        let (_masm_dir, masm_path) = create_test_masm(TEST_MASM_SIMPLE, "test-account.masm");

        let output_dir = tempfile::tempdir().unwrap();
        let dest = output_dir.path().join("test-account-project");

        run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: dest.clone(),
            link_protocol_lib: true,
        })
        .unwrap();

        assert!(dest.join("miden-project.toml").exists());
        assert!(dest.join("test-account.masm").exists());

        let masp_files = collect_masp_files(&dest);
        assert_eq!(masp_files.len(), 1);
        assert_eq!(masp_files[0].file_stem().unwrap().to_str().unwrap(), "test-account");
    }

    #[test]
    fn generated_manifest_loads() {
        let (_masm_dir, masm_path) = create_test_masm(TEST_MASM_SIMPLE, "test-account.masm");

        let output_dir = tempfile::tempdir().unwrap();
        let dest = output_dir.path().join("test-account-project");

        run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: dest.clone(),
            link_protocol_lib: true,
        })
        .unwrap();

        let manifest_path = dest.join("miden-project.toml");
        let source_manager = DefaultSourceManager::default();
        let project = Project::load(&manifest_path, &source_manager).unwrap();
        assert_eq!(&**project.package().name().inner(), "test-account");
    }

    #[test]
    fn built_package_kind_is_account_component() {
        let (_masm_dir, masm_path) = create_test_masm(TEST_MASM_SIMPLE, "test-account.masm");

        let output_dir = tempfile::tempdir().unwrap();
        let dest = output_dir.path().join("test-account-project");

        run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: dest.clone(),
            link_protocol_lib: true,
        })
        .unwrap();

        let masp_files = collect_masp_files(&dest);
        assert_eq!(masp_files.len(), 1);

        let bytes = fs::read(&masp_files[0]).unwrap();
        let package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
        assert_eq!(package.kind, TargetType::AccountComponent);
    }

    #[test]
    fn protocol_imports_are_supported() {
        let (_masm_dir, masm_path) = create_test_masm(
            r#"use miden::protocol::asset::ASSET_SIZE

pub proc foo
    push.ASSET_SIZE
    drop
end
"#,
            "test-account.masm",
        );

        let output_dir = tempfile::tempdir().unwrap();
        let dest = output_dir.path().join("test-account-project");

        run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: dest.clone(),
            link_protocol_lib: true,
        })
        .unwrap();

        let masp_files = collect_masp_files(&dest);
        assert_eq!(masp_files.len(), 1);
    }

    #[test]
    fn masp_filename_matches_package_name() {
        let (_masm_dir, masm_path) = create_test_masm(TEST_MASM_SIMPLE, "test-account.masm");

        let output_dir = tempfile::tempdir().unwrap();
        let dest = output_dir.path().join("my-custom-name");

        run(Masm2MaspCommand {
            masm_path,
            target_type: "account-component".to_string(),
            out_path: dest.clone(),
            link_protocol_lib: true,
        })
        .unwrap();

        let masp_files = collect_masp_files(&dest);
        assert_eq!(masp_files.len(), 1);
        assert_eq!(masp_files[0].file_stem().unwrap().to_str().unwrap(), "test-account");
    }
}
