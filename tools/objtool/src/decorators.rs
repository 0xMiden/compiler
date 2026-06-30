use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use miden_core::{
    mast::MastForest,
    serde::{Deserializable, Serializable},
};
use miden_mast_package::{Package, TargetType};

#[derive(Debug, Clone, Args)]
#[command(arg_required_else_help = true)]
pub struct DecoratorsCommand {
    /// Path to the input .masp file
    #[arg(required = true)]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum ArtifactKind {
    Program,
    Library,
}

impl ArtifactKind {
    fn from_package(package: &Package) -> Self {
        if package.is_program() {
            ArtifactKind::Program
        } else {
            ArtifactKind::Library
        }
    }
}

impl std::fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Program => write!(f, "program"),
            Self::Library => write!(f, "library"),
        }
    }
}

pub fn run(command: &DecoratorsCommand) -> Result<()> {
    let input_bytes = fs::read(&command.path)
        .with_context(|| format!("failed to read input file '{}'", command.path.display()))?;
    let masp_size = input_bytes.len();

    let package = Package::read_from_bytes(&input_bytes)
        .with_context(|| format!("failed to decode package '{}'", command.path.display()))?;

    let original_forest = package.mast.mast_forest().as_ref().clone();
    let original_forest_size = forest_size(&original_forest);

    let mut stripped_forest = original_forest.clone();
    stripped_forest.clear_debug_info();

    let (compacted_forest, _) = stripped_forest.clone().compact();

    let report = Report {
        input: command.path.display().to_string(),
        package_kind: package.kind,
        artifact_kind: ArtifactKind::from_package(&package),
        metric_points: vec![
            MetricPoint::reference("original masp", masp_size),
            MetricPoint::baseline("original forest", original_forest_size),
            MetricPoint::delta(
                "without decorators",
                forest_size(&stripped_forest),
                original_forest_size,
            ),
            MetricPoint::delta(
                "compacted forest",
                forest_size(&compacted_forest),
                original_forest_size,
            ),
        ],
    };

    println!("{report}");

    Ok(())
}

fn forest_size(forest: &MastForest) -> usize {
    forest.to_bytes().len()
}

fn bytes_to_kb(bytes: usize) -> f64 {
    bytes as f64 / 1024.0
}

#[derive(Debug, Clone)]
struct Report {
    input: String,
    package_kind: TargetType,
    artifact_kind: ArtifactKind,
    metric_points: Vec<MetricPoint>,
}

#[derive(Debug, Clone)]
struct MetricPoint {
    label: &'static str,
    bytes: usize,
    delta: Option<i64>,
    delta_percent: Option<f64>,
}

impl MetricPoint {
    fn reference(label: &'static str, bytes: usize) -> Self {
        Self {
            label,
            bytes,
            delta: None,
            delta_percent: None,
        }
    }

    fn baseline(label: &'static str, bytes: usize) -> Self {
        Self {
            label,
            bytes,
            delta: Some(0),
            delta_percent: Some(0.0),
        }
    }

    fn delta(label: &'static str, bytes: usize, baseline: usize) -> Self {
        let delta = i64::try_from(bytes).unwrap() - i64::try_from(baseline).unwrap();
        let delta_percent = if baseline == 0 {
            0.0
        } else {
            (delta as f64 / baseline as f64) * 100.0
        };

        Self {
            label,
            bytes,
            delta: Some(delta),
            delta_percent: Some(delta_percent),
        }
    }
}

fn format_delta(row: &MetricPoint) -> String {
    match row.delta {
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

fn format_delta_percent(row: &MetricPoint) -> String {
    match row.delta_percent {
        Some(percent) => format!("{percent:+.2}%"),
        None => "-".to_string(),
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Input: {}", self.input)?;
        writeln!(f, "Package kind: {}", self.package_kind)?;
        writeln!(f, "Artifact: {}", self.artifact_kind)?;
        writeln!(f)?;

        let kb_strings: Vec<String> = self
            .metric_points
            .iter()
            .map(|row| format!("{:.2}", bytes_to_kb(row.bytes)))
            .collect();
        let delta_strings: Vec<String> = self.metric_points.iter().map(format_delta).collect();
        let delta_percent_strings: Vec<String> =
            self.metric_points.iter().map(format_delta_percent).collect();

        let metric_width = self
            .metric_points
            .iter()
            .map(|row| row.label.len())
            .chain(std::iter::once("Metric".len()))
            .max()
            .unwrap_or("Metric".len());
        let kb_width = kb_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("KB".len()))
            .max()
            .unwrap_or("KB".len());
        let delta_width = delta_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("Delta".len()))
            .max()
            .unwrap_or("Delta".len());
        let delta_percent_width = delta_percent_strings
            .iter()
            .map(String::len)
            .chain(std::iter::once("Delta %".len()))
            .max()
            .unwrap_or("Delta %".len());

        writeln!(
            f,
            "{:<metric_width$}  {:>kb_width$}  {:>delta_width$}  {:>delta_percent_width$}",
            "Metric", "KB", "Delta", "Delta %",
        )?;

        for ((row, kb), (delta, delta_percent)) in self
            .metric_points
            .iter()
            .zip(kb_strings.iter())
            .zip(delta_strings.iter().zip(delta_percent_strings.iter()))
        {
            writeln!(
                f,
                "{:<metric_width$}  {:>kb_width$}  {:>delta_width$}  {:>delta_percent_width$}",
                row.label, kb, delta, delta_percent,
            )?;
        }

        Ok(())
    }
}
