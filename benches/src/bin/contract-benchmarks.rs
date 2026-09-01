use std::path::{Path, PathBuf};

use clap::Parser;
use midenc_benchmark_runner::{BenchmarkRunner, git_commit};

#[derive(Debug, Parser)]
#[command(about = "Run compiler contract examples through deterministic MockChain scenarios")]
struct Args {
    /// Compiler workspace containing the contract example projects.
    #[arg(long, default_value = env!("CARGO_MANIFEST_DIR"))]
    workspace_root: PathBuf,
    /// Directory in which results, packages, replays, and flamegraphs are written.
    #[arg(long, default_value = "target/contract-benchmarks")]
    output_dir: PathBuf,
    /// Directory used for intermediate Cargo and compiler artifacts.
    #[arg(long, default_value = "target/contract-benchmark-build")]
    build_dir: PathBuf,
    /// Explicit cargo-miden binary to use instead of `cargo miden`.
    #[arg(long, env = "CARGO_MIDEN")]
    cargo_miden: Option<PathBuf>,
    /// Commit recorded in results.json; defaults to workspace HEAD.
    #[arg(long)]
    commit: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();
    if args.workspace_root.ends_with("benches") {
        args.workspace_root.pop();
    }
    let commit = resolve_commit(args.commit, &args.workspace_root)?;
    let report = BenchmarkRunner::new(
        &args.workspace_root,
        &args.output_dir,
        &args.build_dir,
        args.cargo_miden,
        false,
    )?
    .run_contracts(commit)?;

    println!("| example | cycles | MAST size |");
    println!("| --- | ---: | ---: |");
    for benchmark in report.benchmarks {
        let cycles = benchmark.cycles.map(|cycles| cycles.to_string()).unwrap_or("n/a".into());
        println!("| {} | {} | {} B |", benchmark.name, cycles, benchmark.mast_size);
    }
    Ok(())
}

fn resolve_commit(commit: Option<String>, workspace_root: &Path) -> anyhow::Result<String> {
    match commit {
        Some(commit) => Ok(commit),
        None => git_commit(workspace_root),
    }
}
