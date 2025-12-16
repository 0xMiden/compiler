//! Common benchmarking framework for Miden compiler programs
//!
//! This module provides utilities for compiling Rust programs to Miden assembly
//! and measuring their execution performance in the Miden VM.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};

/// Execution statistics for a Miden program
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Total VM cycles executed
    pub vm_cycles: usize,
    /// Compilation time in milliseconds
    pub compile_time_ms: u128,
    /// Execution time in milliseconds
    pub execution_time_ms: u128,
}

impl ExecutionStats {
    /// Create execution stats from midenc output
    pub fn from_midenc_output(
        output: &str,
        compile_time_ms: u128,
        execution_time_ms: u128,
    ) -> Result<Self> {
        // Parse the VM cycles from midenc output
        let vm_cycles = Self::parse_vm_cycles(output)?;

        Ok(Self {
            vm_cycles,
            compile_time_ms,
            execution_time_ms,
        })
    }

    /// Parse VM cycles from midenc run output
    fn parse_vm_cycles(output: &str) -> Result<usize> {
        for line in output.lines() {
            if line.contains("VM cycles:") {
                // Look for pattern like "VM cycles: 805 extended to 1024 steps"
                if let Some(cycles_part) = line.split("VM cycles:").nth(1)
                    && let Some(cycles_str) = cycles_part.split_whitespace().next()
                {
                    return cycles_str
                        .parse()
                        .with_context(|| format!("Failed to parse VM cycles from: {cycles_str}"));
                }
            }
        }
        Err(anyhow::anyhow!("Could not find VM cycles in output"))
    }

    /// Print formatted execution statistics
    pub fn print(&self, program_name: &str) {
        println!("===============================================================================");
        println!("Benchmark results for: {program_name}");
        println!("-------------------------------------------------------------------------------");
        println!(
            "VM cycles: {} extended to {} steps",
            self.vm_cycles,
            self.vm_cycles.next_power_of_two()
        );
        println!("Compilation time: {} ms", self.compile_time_ms);
        println!("Execution time: {} ms", self.execution_time_ms);
        println!("===============================================================================");
    }
}

/// A benchmark runner for Miden programs
pub struct BenchmarkRunner;

impl BenchmarkRunner {
    /// Create a new benchmark runner
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// Compile a Rust source file to Miden assembly using cargo miden
    pub fn compile_rust_to_masm(&self, source_path: &Path) -> Result<PathBuf> {
        let compile_start = Instant::now();

        // Convert to absolute path if relative
        let abs_source_path = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            std::env::current_dir()?.join(source_path)
        };

        // Get the directory containing the source file
        let project_dir = abs_source_path.parent()
            .and_then(|p| p.parent()) // Go up from src/ to project root
            .ok_or_else(|| anyhow::anyhow!("Could not determine project directory"))?;

        // Use cargo miden to build the project
        let mut cmd = std::process::Command::new("cargo");
        cmd.arg("miden")
            .arg("build")
            .arg("--release")
            .arg("--manifest-path")
            .arg(project_dir.join("Cargo.toml"))
            .current_dir(project_dir);

        let output = cmd.output().with_context(|| "Failed to execute cargo miden build")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("cargo miden build failed: {stderr}"));
        }

        let compile_time = compile_start.elapsed();
        println!("Compilation completed in {} ms", compile_time.as_millis());

        // Find the generated .masp file
        let target_dir = project_dir.join("target").join("miden").join("release");
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Could not determine project name"))?;

        // Convert hyphens to underscores for the MASP filename
        let masp_filename = project_name.replace('-', "_");
        let masp_path = target_dir.join(format!("{masp_filename}.masp"));

        if !masp_path.exists() {
            return Err(anyhow::anyhow!("Expected MASP file not found: {}", masp_path.display()));
        }

        Ok(masp_path)
    }

    /// Execute a Miden assembly program using midenc run and return execution statistics
    pub fn execute_masm(&self, masm_path: &Path, inputs: &[u64]) -> Result<ExecutionStats> {
        let execution_start = Instant::now();

        // Create inputs file in TOML format
        let inputs_content = format!(
            r#"[inputs]
stack = [{}]"#,
            inputs.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
        );

        let inputs_file = masm_path.with_extension("inputs");
        std::fs::write(&inputs_file, inputs_content)
            .with_context(|| format!("Failed to write inputs file: {}", inputs_file.display()))?;

        // Use midenc run to execute the program
        let mut cmd = std::process::Command::new("midenc");
        cmd.arg("run").arg(masm_path).arg("--inputs").arg(&inputs_file);

        let output = cmd.output().with_context(|| "Failed to execute midenc run")?;

        let execution_time = execution_start.elapsed();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("midenc run failed: {stderr}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Program executed successfully");
        println!("{stdout}");

        // Clean up inputs file
        let _ = std::fs::remove_file(&inputs_file);

        ExecutionStats::from_midenc_output(&stdout, 0, execution_time.as_millis())
    }

    /// Run a complete benchmark: compile Rust to MASM and execute
    pub fn run_benchmark(
        &self,
        source_path: &Path,
        inputs: &[u64],
        program_name: &str,
    ) -> Result<ExecutionStats> {
        println!("Running benchmark for: {program_name}");

        let compile_start = Instant::now();
        let masm_path = self.compile_rust_to_masm(source_path)?;
        let compile_time = compile_start.elapsed();

        let mut stats = self.execute_masm(&masm_path, inputs)?;
        stats.compile_time_ms = compile_time.as_millis();

        stats.print(program_name);

        Ok(stats)
    }
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self::new().expect("Failed to create benchmark runner")
    }
}
