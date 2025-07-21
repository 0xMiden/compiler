# Miden Compiler Benchmarks

Benchmarks for measuring VM cycles and performance of Miden programs.

## Usage

```bash
# Run is_prime benchmark
cargo run --bin is_prime

# Custom input
cargo run --bin is_prime -- --input 97

# Multiple iterations
cargo run --bin is_prime -- --input 29 --iterations 5

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
