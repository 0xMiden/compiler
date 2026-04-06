# `miden-objtool`

Provides the `miden-objtool` CLI to analyze compilation artifacts.

## Compatibility

The compilation artifacts to be examined must have been produced by a `midenup` toolchain version compatible with the `compiler` version used to build `miden-objtool`. Otherwise you may run into errors like `unsupported version`.

## Installation

Running `cargo make install-miden-objtool` from the repository root installs the `objtool` binary globally via the cargo bin directory. Alternatively, `cargo make install` installs multiple tools, including `objtool`.

Once installed, `miden-objtool` can be executed with:

```sh
miden-objtool decorators ./mypkg.masp
```

# Commands

## `decorators`

Note that this computes sizes in memory and does *not* write packages stripped of decorators to disk.

**Example usage**

```sh
miden-objtool decorators ./mypkg.masp

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
miden-objtool decorators --help

Compare serialized MAST forest sizes after stripping decorators

Usage: miden-objtool decorators <PATH>

Arguments:
  <PATH>  Path to the input .masp file

Options:
  -h, --help  Print help
```
