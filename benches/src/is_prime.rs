//! Benchmark for the is_prime program
//!
//! This benchmark compiles the is_prime Rust program to Miden assembly
//! and measures its execution performance in the Miden VM.

use std::path::PathBuf;

use clap::Parser;
use midenc_benchmark_runner::BenchmarkRunner;

#[derive(Parser)]
struct Config {
    /// The number to test for primality
    #[arg(short = 'i', long, value_name = "NUMBER", default_value = "29")]
    input: usize,
    /// Path to the is_prime source file
    #[arg(
        short = 's',
        long,
        value_name = "PATH",
        default_value = "examples/is-prime/src/lib.rs"
    )]
    source: PathBuf,
    /// Number of iterations to run
    #[arg(short = 'n', long, value_name = "COUNT", default_value = "1")]
    iterations: usize,
}

fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    println!("Is Prime Benchmark");
    println!("==================");
    println!("Input number: {}", config.input);
    println!("Source file: {}", config.source.display());
    println!("Iterations: {}", config.iterations);
    println!();

    let runner = BenchmarkRunner::new()?;

    let mut total_cycles = 0;
    let mut total_compile_time = 0;
    let mut total_execution_time = 0;

    for i in 1..=config.iterations {
        if config.iterations > 1 {
            println!("--- Iteration {i} ---");
        }

        let stats = runner.run_benchmark(
            &config.source,
            &[config.input as u64],
            &format!("is_prime({})", config.input),
        )?;

        total_cycles += stats.vm_cycles;
        total_compile_time += stats.compile_time_ms;
        total_execution_time += stats.execution_time_ms;

        if config.iterations > 1 {
            println!();
        }
    }

    if config.iterations > 1 {
        println!("===============================================================================");
        println!("Average results over {} iterations:", config.iterations);
        println!("-------------------------------------------------------------------------------");
        println!("Average VM cycles: {}", total_cycles / config.iterations);
        println!(
            "Average compilation time: {} ms",
            total_compile_time / config.iterations as u128
        );
        println!(
            "Average execution time: {} ms",
            total_execution_time / config.iterations as u128
        );
        println!("Total compilation time: {total_compile_time} ms");
        println!("Total execution time: {total_execution_time} ms");
        println!("===============================================================================");
    }

    Ok(())
}
