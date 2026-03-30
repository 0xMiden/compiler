# `objtool`

Provides the `objtool` CLI to analyze compilation artifacts.

## Compatibility

The compilation artifacts to be examined must have been produced by a `midenup` toolchain version compatible with the `compiler` version used to build `objtool`. Otherwise you may run into errors like `unsupported version`.

## Usage

It can be built and run from the repository root, for example:

```sh
cargo run -p objtool -- decorators ./mypkg.masp
```

When using cargo run, arguments for objtool go after the `--` separator.

Alternatively, you can build the package, move it to a directory in your `PATH` and then invoke it directly:

```sh
cargo build -p objtool --release
mv target/release/objtool <dir_on_your_path>
objtool decorators ./mypkg.masp
```

# Commands

## `decorators`

Note that this computes sizes in memory and does *not* write packages stripped of decorators to disk.

**Example usage**

```sh
cargo run -p objtool -- decorators ./mypkg.masp

Package kind: library
Artifact: library

Metric                 KB   Delta  Delta %
original masp       36.65       -        -
original forest     36.13    0.00   +0.00%
without decorators  17.95  -18.19  -50.33%
compacted forest    13.10  -23.03  -63.74%
```

**Help**

```sh
cargo run -p objtool -- decorators --help

Compare serialized MAST forest sizes after stripping decorators

Usage: objtool decorators <PATH>

Arguments:
  <PATH>  Path to the input .masp file

Options:
  -h, --help  Print help
```
