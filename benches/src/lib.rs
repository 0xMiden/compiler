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

/// Marker wrapping a compile-step failure, so `--skip-failed-builds` skips exactly those.
#[derive(Debug)]
struct BuildFailure(anyhow::Error);

impl std::fmt::Display for BuildFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for BuildFailure {}

pub struct BenchmarkRunner {
    workspace_root: PathBuf,
    output_dir: PathBuf,
    build_dir: PathBuf,
    cargo_miden: Option<PathBuf>,
    /// Whether a case whose build fails is skipped instead of failing the whole run.
    ///
    /// The baseline side of a benchmark comparison drives the previous compiler over the
    /// candidate's examples and SDK, so a candidate change that old compilers cannot build
    /// — for example, macros that require package features the old compiler does not
    /// produce — would otherwise fail the baseline outright. Skipped cases appear in the
    /// comparison report without a baseline value. The candidate run must never set this:
    /// a candidate build failure is a real regression.
    skip_failed_builds: bool,
}

impl BenchmarkRunner {
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
        build_dir: impl Into<PathBuf>,
        cargo_miden: Option<PathBuf>,
        skip_failed_builds: bool,
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
            skip_failed_builds,
        })
    }

    pub fn run(&self, commit: String) -> Result<BenchmarkReport> {
        recreate_dir(&self.output_dir.join("packages"))?;
        recreate_dir(&self.output_dir.join("flamegraphs"))?;

        let cases = discover_cases(&self.workspace_root)?;
        let mut benchmarks = Vec::with_capacity(cases.len());
        for case in cases {
            eprintln!("Benchmarking {}", case.name);
            match self.run_case(&case) {
                Ok(benchmark) => benchmarks.push(benchmark),
                // Only compile-step failures are skippable: a malformed inputs.toml or an
                // executor failure is a harness problem, not an old-compiler limitation,
                // and must fail the run.
                Err(err) if self.skip_failed_builds && err.is::<BuildFailure>() => {
                    eprintln!("Skipping {}: {err:#}", case.name);
                }
                Err(err) => return Err(err),
            }
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

        let optimized = self
            .compile(&project_dir, "none")
            .map_err(|err| anyhow::Error::new(BuildFailure(err)))?;
        let saved_package = self
            .output_dir
            .join("packages")
            .join(format!("{}.{}", case.name, Package::EXTENSION));
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

            let debuggable = self
                .compile(&project_dir, "full")
                .map_err(|err| anyhow::Error::new(BuildFailure(err)))?;
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

    /// Compiles an example, keeping its dependency packages readable for [`Self::profile`].
    ///
    /// The build runs with `MIDENC_PACKAGE_CACHE` naming a runner-owned directory. The
    /// current compiler adopts the directory as its package cache and leaves the packages
    /// in place after it exits. Old compiler versions — one runner binary drives both
    /// during a benchmark comparison — ignore the inherited variable and persist their
    /// packages in the project cache instead, which the legacy scan picks up.
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
            .env("MIDENC_PACKAGE_CACHE", self.package_cache_dir(project_dir))
            .current_dir(project_dir);

        let output = command
            .output()
            .with_context(|| format!("failed to compile {}", project_dir.display()))?;
        if !output.status.success() {
            bail!("failed to compile {}:\n{}", project_dir.display(), command_output(&output));
        }

        parse_compiled_package(&output, project_dir)
    }

    /// The per-example package-cache directory this run hands to the compiler.
    fn package_cache_dir(&self, project_dir: &Path) -> PathBuf {
        let case = project_dir.file_name().unwrap_or_default();
        self.build_dir.join("package-cache").join(case)
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
        // Load exactly one package generation. The current compiler adopts the runner-owned
        // cache directory (see `Self::compile`), so when that directory holds packages it is
        // the generation this run produced and the only one loaded. The legacy project-cache
        // scan serves old compiler versions only — combining generations could pair a stale
        // package with the current one under the same name and version but a different
        // digest, which package installation rejects.
        let export_dir = self.package_cache_dir(project_dir);
        let mut dependencies: Vec<PathBuf> = if export_dir.is_dir() {
            read_dir_paths(&export_dir)?
                .into_iter()
                .filter(|path| is_package_path(path))
                .collect()
        } else {
            Vec::new()
        };
        if dependencies.is_empty() {
            let packages_dir = project_dir.join("target/miden/packages");
            dependencies = collect_dependency_packages(&packages_dir)?;
        }
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

/// Collects the legacy-layout dependency packages that the executor must load for an example.
///
/// One runner binary drives both compiler versions during a benchmark comparison. The current
/// compiler adopts the runner-provided `MIDENC_PACKAGE_CACHE` directory and leaves its packages
/// there (see `BenchmarkRunner::compile`). Old compiler versions
/// persist packages in the project cache: either directly in `target/miden/packages/` or in a
/// fingerprinted subdirectory of it, so the scan reads the cache directory and one level of
/// subdirectories, and tolerates the directory being absent. Package resolution at execution
/// time is digest-addressed, so packages from a stale cache entry are inert.
fn collect_dependency_packages(packages_dir: &Path) -> Result<Vec<PathBuf>> {
    if !packages_dir.exists() {
        return Ok(Vec::new());
    }
    let mut dependencies = Vec::new();
    for path in read_dir_paths(packages_dir)? {
        if path.is_dir() {
            dependencies
                .extend(read_dir_paths(&path)?.into_iter().filter(|path| is_package_path(path)));
        } else if is_package_path(&path) {
            dependencies.push(path);
        }
    }
    dependencies.sort();
    Ok(dependencies)
}

/// Lists the entry paths of a directory.
fn read_dir_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read {}", dir.display()))
}

/// Returns true when a path names a serialized Miden package file.
fn is_package_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == Package::EXTENSION)
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
    fn collects_packages_from_flat_and_fingerprinted_cache_layouts() {
        let cache = tempfile::tempdir().unwrap();
        fs::write(cache.path().join("legacy.masp"), []).unwrap();
        fs::write(cache.path().join("1234567890abcdef.lock"), []).unwrap();
        let fingerprint = cache.path().join("1234567890abcdef");
        fs::create_dir_all(&fingerprint).unwrap();
        fs::write(fingerprint.join("miden-core.masp"), []).unwrap();
        fs::write(fingerprint.join("README.txt"), []).unwrap();

        assert_eq!(
            collect_dependency_packages(cache.path()).unwrap(),
            vec![fingerprint.join("miden-core.masp"), cache.path().join("legacy.masp")]
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
