# Miden project starter templates

The official starter project templates for the [Miden cargo extension](https://0xmiden.github.io/compiler/usage/cargo-miden.html)'s
`new` command. They were maintained in `0xMiden/rust-templates` until they moved
here; that repository is no longer the source.

These are released as `templates/v*`, independently of the compiler. Editing
them is not enough to put a change in front of users — see
[CONTRIBUTING.md](../../../CONTRIBUTING.md#changing-the-project-templates).

## Pre-requisites

Install cargo extension:

```bash
cargo install cargo-miden
```

## Usage

### Create a new project

Run

```bash
cargo miden new [project-name]
```

to create a new project based on the default template.
