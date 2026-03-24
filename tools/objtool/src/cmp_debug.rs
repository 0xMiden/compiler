use std::{
    ffi::OsStr,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use cargo_metadata::MetadataCommand;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct CmpDebugCommand {
    /// Path to the project directory to be built with `cargo miden build`
    pub path: PathBuf,
    /// Whether to build with release profile (default: false)
    #[arg(long)]
    pub release: bool,
}

pub fn run(command: CmpDebugCommand) -> Result<()> {
    let project_dir = validate_project_dir(&command.path)?;
    let target_miden_dir = resolve_target_miden_dir(&project_dir)?;
    let profile = Profile::new(command.release);

    println!("Note: cleaning '{}' before and after each build.", target_miden_dir.display());
    println!(
        "Note: current assembler behavior may produce identical sizes for all `--debug` modes."
    );
    println!();

    let mut metric_points = Vec::with_capacity(DebugMode::all().len());
    let mut baseline = None;
    for mode in DebugMode::all() {
        let bytes = build_and_measure(&project_dir, &target_miden_dir, profile, *mode)?;
        let metric_point = match baseline {
            Some(baseline) => MetricPoint::delta(*mode, bytes, baseline),
            None => {
                baseline = Some(bytes);
                MetricPoint::baseline(*mode, bytes)
            }
        };
        metric_points.push(metric_point);
    }

    let report = Report {
        project_dir: project_dir.display().to_string(),
        profile,
        metric_points,
    };

    println!("{report}");

    Ok(())
}

fn validate_project_dir(path: &Path) -> Result<PathBuf> {
    ensure!(path.exists(), "project directory '{}' does not exist", path.display());
    ensure!(path.is_dir(), "project path '{}' is not a directory", path.display());

    let cargo_toml = path.join("Cargo.toml");
    ensure!(
        cargo_toml.exists(),
        "project directory '{}' does not contain a Cargo.toml",
        path.display()
    );

    path.canonicalize()
        .with_context(|| format!("failed to canonicalize '{}'", path.display()))
}

fn resolve_target_miden_dir(project_dir: &Path) -> Result<PathBuf> {
    let cargo_toml = project_dir.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&cargo_toml)
        .no_deps()
        .exec()
        .with_context(|| {
            format!("failed to load cargo metadata from '{}'", cargo_toml.display())
        })?;

    Ok(PathBuf::from(metadata.target_directory.as_str()).join("miden"))
}

fn build_and_measure(
    project_dir: &Path,
    target_miden_dir: &Path,
    profile: Profile,
    debug_mode: DebugMode,
) -> Result<u64> {
    clean_target_miden(target_miden_dir)?;

    let build_result = (|| {
        run_build(project_dir, profile, debug_mode)?;
        let artifact = find_artifact(target_miden_dir, profile)?;
        measure_artifact(&artifact)
    })();

    let cleanup_result = clean_target_miden(target_miden_dir);

    match (build_result, cleanup_result) {
        (Ok(bytes), Ok(())) => Ok(bytes),
        (Err(build_err), Ok(())) => Err(build_err),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Err(build_err), Err(cleanup_err)) => Err(anyhow::anyhow!(
            "{build_err:#}\ncleanup after build failure also failed: {cleanup_err:#}"
        )),
    }
}

fn clean_target_miden(target_miden_dir: &Path) -> Result<()> {
    if target_miden_dir.exists() {
        fs::remove_dir_all(target_miden_dir)
            .with_context(|| format!("failed to remove '{}'", target_miden_dir.display()))?;
    }
    Ok(())
}

fn run_build(project_dir: &Path, profile: Profile, debug_mode: DebugMode) -> Result<()> {
    println!("Building with --debug={debug_mode}");

    let cargo_path = std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut command = Command::new(&cargo_path);
    command
        .current_dir(project_dir)
        .arg("miden")
        .arg("build")
        .arg("--debug")
        .arg(debug_mode.as_arg());

    if profile.is_release() {
        command.arg("--release");
    }

    let status = command.status().with_context(|| {
        format!("failed to run command in '{}': {command:?}", project_dir.display())
    })?;

    if !status.success() {
        bail!(
            "command failed in '{}' with {}: {command:?}",
            project_dir.display(),
            format_exit_status(status.code())
        );
    }

    Ok(())
}

fn find_artifact(target_miden_dir: &Path, profile: Profile) -> Result<PathBuf> {
    let output_dir = profile.output_dir(target_miden_dir);
    let entries = fs::read_dir(&output_dir).with_context(|| {
        format!("failed to read build output directory '{}'", output_dir.display())
    })?;

    let mut artifacts = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .with_context(|| format!("failed to read an entry from '{}'", output_dir.display()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| path.extension() == Some(OsStr::new("masp")))
        .collect::<Vec<_>>();
    artifacts.sort();

    match artifacts.as_slice() {
        [] => {
            bail!("expected exactly one .masp artifact in '{}', found none", output_dir.display())
        }
        [artifact] => Ok(artifact.clone()),
        _ => {
            let artifact_list = artifacts
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "expected exactly one .masp artifact in '{}', found {}: {artifact_list}",
                output_dir.display(),
                artifacts.len(),
            )
        }
    }
}

fn measure_artifact(path: &Path) -> Result<u64> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read artifact metadata '{}'", path.display()))?;
    Ok(metadata.len())
}

fn format_exit_status(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("exit code {code}"),
        None => "termination by signal".to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
enum DebugMode {
    None,
    Line,
    Full,
}

impl DebugMode {
    const ALL: [Self; 3] = [Self::None, Self::Line, Self::Full];

    fn all() -> &'static [Self] {
        &Self::ALL
    }

    fn as_arg(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Line => "line",
            Self::Full => "full",
        }
    }
}

impl fmt::Display for DebugMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_arg())
    }
}

#[derive(Debug, Clone, Copy)]
enum Profile {
    Debug,
    Release,
}

impl Profile {
    fn new(release: bool) -> Self {
        if release { Self::Release } else { Self::Debug }
    }

    fn is_release(self) -> bool {
        matches!(self, Self::Release)
    }

    fn output_dir(self, target_miden_dir: &Path) -> PathBuf {
        target_miden_dir.join(match self {
            Self::Debug => "debug",
            Self::Release => "release",
        })
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debug => f.write_str("debug"),
            Self::Release => f.write_str("release"),
        }
    }
}

#[derive(Debug, Clone)]
struct MetricPoint {
    debug_mode: DebugMode,
    bytes: u64,
    delta: Option<i64>,
    delta_percent: Option<f64>,
}

impl MetricPoint {
    fn baseline(debug_mode: DebugMode, bytes: u64) -> Self {
        Self {
            debug_mode,
            bytes,
            delta: Some(0),
            delta_percent: Some(0.0),
        }
    }

    fn delta(debug_mode: DebugMode, bytes: u64, baseline: u64) -> Self {
        let delta = i64::try_from(bytes).unwrap() - i64::try_from(baseline).unwrap();
        let delta_percent = if baseline == 0 {
            0.0
        } else {
            (delta as f64 / baseline as f64) * 100.0
        };

        Self {
            debug_mode,
            bytes,
            delta: Some(delta),
            delta_percent: Some(delta_percent),
        }
    }
}

#[derive(Debug, Clone)]
struct Report {
    project_dir: String,
    profile: Profile,
    metric_points: Vec<MetricPoint>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const LABEL_MIN_WIDTH: usize = 15;

        writeln!(f, "Project: {}", self.project_dir)?;
        writeln!(f, "Profile: {}", self.profile)?;
        writeln!(f)?;

        let kb_strings = self
            .metric_points
            .iter()
            .map(|metric_point| format!("{:.2}", bytes_to_kb(metric_point.bytes)))
            .collect::<Vec<_>>();
        let delta_strings = self.metric_points.iter().map(format_delta).collect::<Vec<_>>();
        let delta_percent_strings =
            self.metric_points.iter().map(format_delta_percent).collect::<Vec<_>>();

        let debug_width = self
            .metric_points
            .iter()
            .map(|metric_point| metric_point.debug_mode.as_arg().len())
            .chain(std::iter::once("Debug".len()))
            .max()
            .unwrap_or("Debug".len())
            .max(LABEL_MIN_WIDTH);
        let kb_width = kb_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("KB".len()))
            .max()
            .unwrap_or("KB".len());
        let delta_width = delta_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("Delta KB".len()))
            .max()
            .unwrap_or("Delta KB".len());
        let delta_percent_width = delta_percent_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("Delta %".len()))
            .max()
            .unwrap_or("Delta %".len());

        writeln!(
            f,
            "{:<debug_width$}  {:>kb_width$}  {:>delta_width$}  {:>delta_percent_width$}",
            "Debug", "KB", "Delta", "Delta %",
        )?;

        for ((metric_point, kb), (delta, delta_percent)) in self
            .metric_points
            .iter()
            .zip(kb_strings.iter())
            .zip(delta_strings.iter().zip(delta_percent_strings.iter()))
        {
            writeln!(
                f,
                "{:<debug_width$}  {:>kb_width$}  {:>delta_width$}  {:>delta_percent_width$}",
                metric_point.debug_mode.as_arg(),
                kb,
                delta,
                delta_percent,
            )?;
        }

        Ok(())
    }
}

fn bytes_to_kb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0
}

fn format_delta(metric_point: &MetricPoint) -> String {
    match metric_point.delta {
        Some(delta) => {
            let delta_kb = delta as f64 / 1024.0;
            if delta_kb.abs() < 0.01 {
                "0.00".to_string()
            } else if delta_kb > 0.0 {
                format!("+{delta_kb:.2}")
            } else {
                format!("{delta_kb:.2}")
            }
        }
        None => "-".to_string(),
    }
}

fn format_delta_percent(metric_point: &MetricPoint) -> String {
    match metric_point.delta_percent {
        Some(delta_percent) => format!("{delta_percent:+.2}%"),
        None => "-".to_string(),
    }
}
