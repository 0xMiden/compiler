# Rust MIR frontend

This crate is an experimental frontend that reads Rust MIR and builds Miden HIR from it directly, without the WebAssembly path.

The frontend links against rustc and runs the compiler in-process through `rustc_public`, the stable-MIR API. It compiles the input file with `--emit=metadata`, so rustc stops before code generation and the MIR stays in memory. The `wasm32-unknown-unknown` target supplies the 32-bit data layout; no WebAssembly is produced.

## Development environment

This crate changes the shared development environment in two ways:

1. **The `rustc-dev` and `llvm-tools` toolchain components.** The `rust-toolchain.toml` at the workspace root includes both in its `components` list. `rustc-dev` installs the private compiler libraries (`rustc_public`, `rustc_public_bridge`, `rustc_driver`, ...) that this crate links against, and also the compiler sources at `<sysroot>/lib/rustlib/rustc-src/rust`. `llvm-tools` installs the LLVM shared library into the one directory that rustc puts on the linker search path; without it, binaries and tests of this crate fail to link. `rustup` installs the components automatically on the first `cargo` command after a checkout.
2. **`#![feature(rustc_private)]`.** The crate compiles only on the nightly toolchain that the workspace already pins. The compiler crates come from the sysroot, not from crates.io, so a `rustc_public` API change can occur on each toolchain bump.

At run time the driver finds the sysroot with `rustc --print sysroot`. Run tests from inside the workspace, so that `rustup` resolves the pinned toolchain.

## rust-analyzer setup

rust-analyzer needs two settings to resolve `rustc_public` and the other private compiler crates. Both are necessary:

1. **Package metadata.** The `Cargo.toml` of this crate contains:

   ```toml
   [package.metadata.rust-analyzer]
   rustc_private = true
   ```

   This marks the package as a user of the private compiler crates. It is committed and needs no action.

2. **The `rustc.source` setting.** Your editor must set `rust-analyzer.rustc.source` to `"discover"`. With this value rust-analyzer finds the compiler sources that the `rustc-dev` component installs under `<sysroot>/lib/rustlib/rustc-src`. Without this setting the `extern crate rustc_public` items stay unresolved, even with the package metadata in place.

   VS Code (`.vscode/settings.json`, kept out of git):

   ```json
   { "rust-analyzer.rustc.source": "discover" }
   ```

   Neovim (`nvim-lspconfig`):

   ```lua
   require("lspconfig").rust_analyzer.setup({
     settings = {
       ["rust-analyzer"] = {
         rustc = { source = "discover" },
       },
     },
   })
   ```

Restart the rust-analyzer server after you change the setting. The first load indexes the rustc compiler workspace, which takes extra time and can download the dependencies of that workspace once.
