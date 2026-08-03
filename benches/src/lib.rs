//! End-to-end benchmarks for compiler example projects.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use miden_assembly::{DefaultSourceManager, SourceManager};
use miden_core::serde::Serializable;
use miden_debug::{ExecutionConfig, Executor, flamegraph::FlamegraphProfile};
use miden_mast_package::Package;
use serde::{Deserialize, Serialize};

pub const RESULTS_FILE: &str = "results.json";

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub commit: String,
    pub benchmarks: Vec<BenchmarkResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub struct BenchmarkResult {
    pub name: String,
    pub mast_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycles: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flamegraph: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct BenchmarkCase {
    name: String,
    execute: bool,
}

pub struct BenchmarkRunner {
    workspace_root: PathBuf,
    output_dir: PathBuf,
    build_dir: PathBuf,
    cargo_miden: Option<PathBuf>,
}

impl BenchmarkRunner {
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
        build_dir: impl Into<PathBuf>,
        cargo_miden: Option<PathBuf>,
    ) -> Result<Self> {
        let workspace_root = workspace_root
            .into()
            .canonicalize()
            .context("failed to canonicalize the compiler workspace root")?;
        let output_dir = absolute_path(output_dir.into())?;
        let build_dir = absolute_path(build_dir.into())?;
        let cargo_miden = cargo_miden
            .map(|path| path.canonicalize().context("failed to locate cargo-miden"))
            .transpose()?;

        Ok(Self {
            workspace_root,
            output_dir,
            build_dir,
            cargo_miden,
        })
    }

    pub fn run(&self, commit: String) -> Result<BenchmarkReport> {
        recreate_dir(&self.output_dir.join("packages"))?;
        recreate_dir(&self.output_dir.join("flamegraphs"))?;

        let cases = discover_cases(&self.workspace_root)?;
        let mut benchmarks = Vec::with_capacity(cases.len());
        for case in cases {
            eprintln!("Benchmarking {}", case.name);
            benchmarks.push(self.run_case(&case)?);
        }

        let report = BenchmarkReport {
            schema_version: 1,
            commit,
            benchmarks,
        };
        let output = self.output_dir.join(RESULTS_FILE);
        let mut contents = serde_json::to_vec_pretty(&report)?;
        contents.push(b'\n');
        fs::write(&output, contents)
            .with_context(|| format!("failed to write {}", output.display()))?;
        Ok(report)
    }

    fn run_case(&self, case: &BenchmarkCase) -> Result<BenchmarkResult> {
        let project_dir = self.workspace_root.join("examples").join(&case.name);
        ensure!(
            project_dir.join("miden-project.toml").is_file(),
            "missing example {}",
            case.name
        );

        let optimized = self.compile(&project_dir, "none")?;
        let saved_package = self.output_dir.join("packages").join(format!("{}.masp", case.name));
        fs::copy(&optimized, &saved_package).with_context(|| {
            format!(
                "failed to copy optimized package from {} to {}",
                optimized.display(),
                saved_package.display()
            )
        })?;
        let optimized_package = load_package(&saved_package)?;
        let mast_size = serialized_mast_size(&optimized_package);

        let (cycles, flamegraph) = if case.execute {
            let inputs = project_dir.join("inputs.toml");
            let optimized_profile = self.profile(optimized_package, &inputs, &project_dir, None)?;

            let debuggable = self.compile(&project_dir, "full")?;
            let relative_flamegraph = format!("flamegraphs/{}.svg", case.name);
            let flamegraph_path = self.output_dir.join(&relative_flamegraph);
            self.profile(
                load_package(&debuggable)?,
                &inputs,
                &project_dir,
                Some(&flamegraph_path),
            )?;

            (Some(optimized_profile.total_cycles()), Some(relative_flamegraph))
        } else {
            (None, None)
        };

        Ok(BenchmarkResult {
            name: case.name.clone(),
            mast_size,
            cycles,
            flamegraph,
        })
    }

    fn compile(&self, project_dir: &Path, debug: &str) -> Result<PathBuf> {
        let mut command = if let Some(cargo_miden) = self.cargo_miden.as_ref() {
            let mut command = Command::new(cargo_miden);
            command.arg("miden");
            command
        } else {
            let mut command = Command::new("cargo");
            command.arg("miden");
            command
        };
        command
            .arg("build")
            .arg("--release")
            .arg("--manifest-path")
            .arg(project_dir.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(self.build_dir.join("miden-target"))
            .arg("--debug")
            .arg(debug)
            .arg("--optimize")
            .arg("max")
            .arg("--color")
            .arg("never")
            .env("CARGO_TARGET_DIR", self.build_dir.join("cargo-target"))
            .current_dir(project_dir);

        let output = command
            .output()
            .with_context(|| format!("failed to compile {}", project_dir.display()))?;
        if !output.status.success() {
            bail!("failed to compile {}:\n{}", project_dir.display(), command_output(&output));
        }

        parse_compiled_package(&output, project_dir)
    }

    fn profile(
        &self,
        package: Arc<Package>,
        inputs_path: &Path,
        project_dir: &Path,
        flamegraph_path: Option<&Path>,
    ) -> Result<FlamegraphProfile> {
        ensure!(package.is_program(), "{} is not executable", package.name);

        let config = ExecutionConfig::parse_file(inputs_path)
            .with_context(|| format!("failed to parse {}", inputs_path.display()))?;
        let mut executor = Executor::from_config(config);
        let packages_dir = project_dir.join("target/miden/packages");
        let mut dependencies = fs::read_dir(&packages_dir)
            .with_context(|| format!("failed to read {}", packages_dir.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        dependencies.retain(|path| path.extension().is_some_and(|ext| ext == "masp"));
        dependencies.sort();
        for dependency in dependencies {
            executor
                .with_package(load_package(&dependency)?)
                .map_err(|err| anyhow!(err.to_string()))?;
        }

        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let mut debug_executor = executor.into_debug(package, source_manager);
        let profile = FlamegraphProfile::collect(&mut debug_executor)
            .map_err(|err| anyhow!("execution failed at cycle {}: {err}", debug_executor.cycle))?;
        if let Some(path) = flamegraph_path {
            profile.write_svg(path).map_err(|err| anyhow!(err.to_string()))?;
        }
        Ok(profile)
    }
}

fn discover_cases(workspace_root: &Path) -> Result<Vec<BenchmarkCase>> {
    let examples_dir = workspace_root.join("examples");
    let mut cases = Vec::new();
    for entry in fs::read_dir(&examples_dir)
        .with_context(|| format!("failed to read {}", examples_dir.display()))?
    {
        let path = entry?.path();
        if !path.join("miden-project.toml").is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("example path is not valid UTF-8: {}", path.display()))?
            .to_string();
        cases.push(BenchmarkCase {
            name,
            execute: path.join("inputs.toml").is_file(),
        });
    }
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    ensure!(!cases.is_empty(), "no Miden example projects found");
    Ok(cases)
}

pub fn git_commit(workspace_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .context("failed to execute git rev-parse")?;
    ensure!(output.status.success(), "git rev-parse failed: {}", command_output(&output));
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn recreate_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))
}

fn load_package(path: &Path) -> Result<Arc<Package>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Package::read_from_bytes_unchecked(&bytes)
        .map(Arc::new)
        .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))
}

fn serialized_mast_size(package: &Package) -> u64 {
    package
        .mast_forest()
        .to_bytes()
        .len()
        .try_into()
        .expect("serialized MAST size exceeds u64::MAX")
}

fn parse_compiled_package(output: &Output, project_dir: &Path) -> Result<PathBuf> {
    let text = command_output(output);
    let path = text
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("Compiled "))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("compiler did not report an output package:\n{text}"))?;
    let path = if path.is_absolute() {
        path
    } else {
        project_dir.join(path)
    };
    ensure!(path.is_file(), "compiled package does not exist: {}", path.display());
    Ok(path)
}

fn command_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}\n{stderr}")
}

#[cfg(test)]
mod tests {
    use std::process::ExitStatus;

    use miden_assembly::Assembler;

    use super::*;

    #[cfg(unix)]
    fn success() -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }

    #[test]
    #[cfg(unix)]
    fn extracts_compiled_package_path() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("example:example.masp");
        fs::write(&package, []).unwrap();
        let output = Output {
            status: success(),
            stdout: format!("Compiled {}\n", package.display()).into_bytes(),
            stderr: Vec::new(),
        };

        assert_eq!(parse_compiled_package(&output, dir.path()).unwrap(), package);
    }

    #[test]
    fn discovers_example_projects_in_name_order() {
        let workspace = tempfile::tempdir().unwrap();
        let examples = workspace.path().join("examples");
        for name in ["zeta", "alpha"] {
            let project = examples.join(name);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join("miden-project.toml"), []).unwrap();
        }
        fs::write(examples.join("zeta/inputs.toml"), []).unwrap();

        assert_eq!(
            discover_cases(workspace.path()).unwrap(),
            vec![
                BenchmarkCase {
                    name: "alpha".to_string(),
                    execute: false,
                },
                BenchmarkCase {
                    name: "zeta".to_string(),
                    execute: true,
                },
            ]
        );
    }

    #[test]
    fn mast_size_excludes_package_metadata() {
        let mut package = Assembler::default()
            .assemble_program("benchmark-test", "begin\n    push.1\nend")
            .unwrap();
        let expected = serialized_mast_size(&package);
        package.description = Some("metadata".repeat(1_000));

        assert_eq!(serialized_mast_size(&package), expected);
    }
}
