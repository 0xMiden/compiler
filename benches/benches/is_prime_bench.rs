//! Criterion benchmark for the is_prime program
//!
//! This provides detailed performance analysis using the Criterion benchmarking framework.

use std::{hint::black_box, path::PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use midenc_benchmark_runner::BenchmarkRunner;

fn bench_is_prime_compilation(c: &mut Criterion) {
    let runner = BenchmarkRunner::new().expect("Failed to create benchmark runner");
    let source_path = PathBuf::from("../examples/is-prime/src/lib.rs");

    c.bench_function("is_prime_compilation", |b| {
        b.iter(|| {
            runner
                .compile_rust_to_masm(black_box(&source_path))
                .expect("Compilation failed")
        })
    });
}

fn bench_is_prime_execution(c: &mut Criterion) {
    let runner = BenchmarkRunner::new().expect("Failed to create benchmark runner");
    let source_path = PathBuf::from("../examples/is-prime/src/lib.rs");

    // Pre-compile the program
    let masm_path = runner.compile_rust_to_masm(&source_path).expect("Failed to compile program");

    let mut group = c.benchmark_group("is_prime_execution");

    // Test with different input values
    for input in [7, 29, 97, 997, 9973].iter() {
        group.bench_with_input(format!("is_prime({input})"), input, |b, &input| {
            b.iter(|| {
                runner
                    .execute_masm(black_box(&masm_path), black_box(&[input as u64]))
                    .expect("Execution failed")
            })
        });
    }

    group.finish();
}

fn bench_is_prime_full_pipeline(c: &mut Criterion) {
    let runner = BenchmarkRunner::new().expect("Failed to create benchmark runner");
    let source_path = PathBuf::from("../examples/is-prime/src/lib.rs");

    let mut group = c.benchmark_group("is_prime_full_pipeline");

    // Test full compilation + execution pipeline
    for input in [29, 97, 997].iter() {
        group.bench_with_input(format!("full_pipeline_is_prime({input})"), input, |b, &input| {
            b.iter(|| {
                runner
                    .run_benchmark(
                        black_box(&source_path),
                        black_box(&[input as u64]),
                        &format!("is_prime({input})"),
                    )
                    .expect("Benchmark failed")
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_is_prime_compilation,
    bench_is_prime_execution,
    bench_is_prime_full_pipeline
);
criterion_main!(benches);
