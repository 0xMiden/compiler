//! Benchmark for the is_prime program
//!
//! This benchmark compiles the is_prime Rust program to Miden assembly
//! and measures its execution performance in the Miden VM.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Arg, Command};
use miden_compiler_benches::BenchmarkRunner;

fn main() -> Result<()> {
    let matches = Command::new("is_prime_benchmark")
        .about("Benchmark the is_prime program in Miden VM")
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("NUMBER")
                .help("The number to test for primality")
                .default_value("29"),
        )
        .arg(
            Arg::new("source")
                .short('s')
                .long("source")
                .value_name("PATH")
                .help("Path to the is_prime source file")
                .default_value("examples/is-prime/src/lib.rs"),
        )
        .arg(
            Arg::new("iterations")
                .short('n')
                .long("iterations")
                .value_name("COUNT")
                .help("Number of iterations to run")
                .default_value("1"),
        )
        .get_matches();

    let input_number: u64 = matches
        .get_one::<String>("input")
        .unwrap()
        .parse()
        .expect("Input must be a valid number");

    let source_path = PathBuf::from(matches.get_one::<String>("source").unwrap());

    let iterations: usize = matches
        .get_one::<String>("iterations")
        .unwrap()
        .parse()
        .expect("Iterations must be a valid number");

    println!("Is Prime Benchmark");
    println!("==================");
    println!("Input number: {}", input_number);
    println!("Source file: {}", source_path.display());
    println!("Iterations: {}", iterations);
    println!();

    let runner = BenchmarkRunner::new()?;

    let mut total_cycles = 0;
    let mut total_compile_time = 0;
    let mut total_execution_time = 0;

    for i in 1..=iterations {
        if iterations > 1 {
            println!("--- Iteration {} ---", i);
        }

        let stats = runner.run_benchmark(
            &source_path,
            &[input_number],
            &format!("is_prime({})", input_number),
        )?;

        total_cycles += stats.vm_cycles;
        total_compile_time += stats.compile_time_ms;
        total_execution_time += stats.execution_time_ms;

        if iterations > 1 {
            println!();
        }
    }

    if iterations > 1 {
        println!("===============================================================================");
        println!("Average results over {} iterations:", iterations);
        println!("-------------------------------------------------------------------------------");
        println!("Average VM cycles: {}", total_cycles / iterations);
        println!("Average compilation time: {} ms", total_compile_time / iterations as u128);
        println!("Average execution time: {} ms", total_execution_time / iterations as u128);
        println!("Total compilation time: {} ms", total_compile_time);
        println!("Total execution time: {} ms", total_execution_time);
        println!("===============================================================================");
    }

    Ok(())
}
