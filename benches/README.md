# Miden Compiler Benchmarks

Benchmarks for measuring VM cycles and performance of Miden programs.

## Usage

### From project root (recommended)

```bash
# Run is_prime benchmark
cargo make bench --bin is_prime

# Custom input
cargo make bench --bin is_prime -- --input 97

# Multiple iterations
cargo make bench --bin is_prime -- --input 97 --iterations 5

# Custom source file
cargo make bench --bin is_prime -- --source examples/is-prime/src/lib.rs --input 29
```

### Direct cargo commands

```bash
# Run is_prime benchmark
cargo run -p midenc-benchmark-runner --bin is_prime

# Custom input
cargo run -p midenc-benchmark-runner --bin is_prime -- --input 97

# Multiple iterations
cargo run -p midenc-benchmark-runner --bin is_prime -- --input 29 --iterations 5

# Criterion benchmarks
cargo bench
```

## Benchmark Results

| Input         | VM Cycles | Prime? |
| ------------- | --------- | ------ |
| 13            | 533       | ✓      |
| 97            | 805       | ✓      |
| 4,397         | 3,525     | ✓      |
| 285,191       | 24,741    | ✓      |
| 87,019,979    | 423,221   | ✓      |
| 2,147,483,647 | 2,101,189 | ✓      |

## Adding benchmarks

1. Add binary to `src/`
2. Add `[[bin]]` entry to `Cargo.toml`
3. Use `BenchmarkRunner` from the lib
