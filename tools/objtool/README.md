# `miden-objtool`

Provides the `miden-objtool` CLI to analyze compilation artifacts.

## Compatibility

The compilation artifacts to be examined must have been produced by a `midenup` toolchain version compatible with the `compiler` version used to build `miden-objtool`. Otherwise you may run into errors like `unsupported version`.

## Installation

Running `cargo make install-miden-objtool` from the repository root installs the `miden-objtool` binary globally via the cargo bin directory. Alternatively, `cargo make install` installs multiple tools, including `miden-objtool`.

## Dumping debug information

Inspect the debug metadata embedded in a compiled package:

```sh
miden-objtool dump debug-info ./mypkg.masp
```

The package must contain debug information. Use `--summary` for record counts, or `--section`
to select one section:

```sh
miden-objtool dump debug-info ./mypkg.masp --summary
miden-objtool dump debug-info ./mypkg.masp --section functions
miden-objtool dump debug-info ./mypkg.masp --section locations
```

The available sections are `strings`, `types`, `files`, `functions`, `variables`, and `locations`.
`--raw` prints indices instead of resolved names; `--verbose` includes additional function details.

For the complete CLI options:

```sh
miden-objtool dump debug-info --help
```
