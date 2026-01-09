# Debug Info Metadata Pipeline

This note describes how the Miden compiler now threads source-level variable
metadata through HIR when compiling Wasm input. The goal is to make every HIR
function carry `DI*` attributes and `dbg.*` intrinsics that mirror the DWARF
records present in the Wasm binary, so downstream passes (or tooling consuming
serialized HIR) can reason about user variables.

## High-Level Flow

1. **DWARF ingestion** – while `ModuleEnvironment` parses the module, we retain
   the full set of DWARF sections (`.debug_info`, `.debug_line`, etc.) and the
   wasm name section.
2. **Metadata extraction** – before we translate functions, we walk the DWARF
   using `addr2line` to determine source files and fall back to the wasm module
   path when no debug info is present. We also load parameter/local names from
   the name section. The result is a `FunctionDebugInfo` record containing a
   `DICompileUnitAttr`, `DISubprogramAttr`, and a per-index list of
   `DILocalVariableAttr`s.
3. **Translation-time tracking** – every `FuncTranslator` receives the
   `FunctionDebugInfo` for the function it is translating. `FunctionBuilderExt`
   attaches the compile-unit/subprogram attrs to the function op, records entry
   parameters, and emits `builtin.dbg_value` intrinsics whenever locals change.
4. **Span-aware updates** – as each wasm operator is translated we store the
   real `SourceSpan`. The first non-unknown span is used to retroactively patch
   the compile unit, subprogram, and parameter variable records with real file,
   line, and column information so the resulting HIR references surfaces from
   the actual user file.

The emitted HIR therefore contains both the SSA instructions and the debug
intrinsics that map values back to the user program.

## HIR Metadata Constructs

The core types live in `hir/src/attributes/debug.rs`:

- `DICompileUnitAttr` – captures language, primary file, optional directory,
  producer string, and optimized flag. Stored once per function/module.
- `DISubprogramAttr` – names the function, file, line/column, optional linkage
  name, and flags indicating definition/local status. Does not embed the compile
  unit to avoid redundancy - stored once per function.
- `DILocalVariableAttr` – describes parameters or locals, including the source
  location, optional argument index, and optional `Type`. Does not embed the
  scope to avoid redundancy - the scope is implied by the containing function.
- `DIExpressionAttr` – represents DWARF location expressions that describe how
  to compute or locate a variable's value.
- `DIExpressionOp` – individual operations within a DIExpression, including:
  - `WasmLocal(u32)` - Variable is in a WebAssembly local
  - `WasmGlobal(u32)` - Variable is in a WebAssembly global
  - `WasmStack(u32)` - Variable is on the WebAssembly operand stack
  - `ConstU64(u64)` - Unsigned constant value
  - Additional DWARF operations for complex expressions

These attrs are exported from `midenc_hir` so clients can construct them
programmatically. The debug intrinsic (`builtin.dbg_value` from
`hir/src/dialects/builtin/ops/debug.rs`) consume a `Value` plus the
metadata attributes. The `dbg_value` operation includes a `DIExpressionAttr`
field that describes the location or computation of the variable's value.

## Collecting Metadata from Wasm

`frontend/wasm/src/module/debug_info.rs` is the central collector. The key
steps are:

1. Iterate over the bodies scheduled for translation (`ParsedModule::function_body_inputs`).
2. For each body, determine the source file and first line using `addr2line` and
   store fallbacks (module path or `unknown`) when debug info is missing.
3. Construct `DICompileUnitAttr`/`DISubprogramAttr` and a `Vec<Option<LocalDebugInfo>>`
   that covers both signature parameters and wasm locals. Parameter/local names
   sourced from the name section are used when available; otherwise we emit
   synthesized names (`arg{n}`, `local{n}`).
4. Store the result in a map `FxHashMap<FuncIndex, Rc<RefCell<FunctionDebugInfo>>>`
   attached to `ParsedModule`. We use `RefCell` so later stages can patch the
   attrs once the translator sees more accurate spans.

## Using Metadata During Translation

The translation machinery picks up those records as follows:

- `build_ir.rs` moves the precomputed map onto the `FuncTranslator` invocation.
- `FuncTranslator::translate_body` installs the debug info on its
  `FunctionBuilderExt` before any instructions are emitted.
- `FunctionBuilderExt::set_debug_metadata` attaches compile-unit/subprogram
  attrs to the function op and resets its internal bookkeeping.
- Entry parameters are stored via `register_parameter` so we can emit
  `dbg.value` instructions after we encounter the first real span (parameters
  have no dedicated wasm operator with source ranges).
- Every wasm operator calls `builder.record_debug_span(span)` prior to emission;
  the first non-unknown span updates the compile unit/subprogram attrs and
  triggers parameter `dbg.value` emission so arguments are tied to the correct
  location.
- `def_var_with_dbg` is the canonical entry point for `local.set` and
  `local.tee`. It updates the SSA value and immediately emits a
  `builtin.dbg_value` with the precise span of the store.
- Decoded `DW_AT_location` ranges are normalized into a per-function schedule.
  As the translator visits each wasm offset we opportunistically emit extra
  `dbg.value` intrinsics so source variables track transitions between Wasm
  locals without relying on `builtin.dbg_declare`.
- When present, `DW_AT_decl_line`/`DW_AT_decl_column` on variables override the
  default span so we keep the original lexical definition sites instead of
  inheriting the statement we first observed during translation.

Locals declared in the wasm prologue receive an initial value but no debug
intrinsic until they are defined in user code. Subsequent writes insert
additional `dbg.value` ops so consumers can track value changes over time.

## Example

In the serialized HIR for the test pipeline you now see:

```hir
builtin.dbg_value v0 #[expression = di.expression(DW_OP_WASM_local 0)]
    #[variable = di.local_variable(
        name = arg0,
        file = /path/to/lib.rs,
        line = 25,
        column = 5,
        arg = 1,
        ty = i32
    )]  # /path/to/lib.rs:25:5;
```

The `expression` attribute indicates that the variable is stored in WASM local 0.
When a variable moves between locations, additional `dbg_value` operations are
emitted with updated expressions:

```hir
builtin.dbg_value v22 #[expression = di.expression(DW_OP_WASM_local 3)]
    #[variable = di.local_variable(name = sum, ...)]
```

Both the attribute and the trailing comment reference the same source location
so downstream tooling can disambiguate the variable regardless of how it parses
HIR.

## Kinda Fallback Behavior/Best Effort cases

- If DWARF lookup fails entirely, we still emit attrs but populate
  `file = unknown`, `line = 0`, and omit columns. As soon as a real span is
  observed, those fields are patched.
- If the wasm name section lacks parameter/local names, we keep the generated
  `arg{n}`/`local{n}` placeholders in the HIR. This mirrors LLVM’s behavior when
  debug names are unavailable.

## What we can do next and what are the limitations

- **Location expressions** – We now decode `DW_AT_location` records for locals
  and parameters, interpret simple Wasm location opcodes (including locals,
  globals, and operand-stack slots), and attach them to `dbg.value` operations
  as `DIExpressionAttr`. The system emits additional `dbg.value` intrinsics
  whenever a variable's storage changes, with each operation containing the
  appropriate expression. This allows modeling multi-location lifetimes where
  variables move between different storage locations. Support for more complex
  composite expressions (pieces, arithmetic operations, etc.) is implemented
  but not fully utilized from DWARF parsing yet.
- **Lifetimes** – we reset the compile-unit/subprogram metadata to the first
  span we encounter, but we do not track scopes or lexical block DIEs. Extending
  the collector to read `DW_TAG_lexical_block` and other scope markers would
  allow more precise lifetime modelling.
- **Cross-language inputs** – the language string comes from DWARF or defaults
  to `"wasm"`. If the Wasm file was produced by Rust/C compilers we could read
  `DW_AT_language` to provide richer values.
- **Incremental spans** – parameter debug entries currently use the first
  non-unknown span in the function. For multi-file functions we might wish to
  attach per-parameter spans using `DW_AT_decl_file`/`DW_AT_decl_line` if the
  DWARF provides them.
- **MASM codegen** – The MASM backend emits `Decorator::DebugVar` entries
  containing `DebugVarInfo` with variable names, runtime locations
  (`DebugVarLocation::Stack`, `Local`, etc.), source positions, and type
  information. These decorators are embedded in the MAST instruction stream,
  enabling debuggers to track variable values at specific execution points.

These refinements can be implemented without changing the public HIR surface; we
would only update the metadata collector and the builder helpers.

## Testing

The debug info implementation is validated by lit tests in `tests/lit/debug/`:

- **simple_debug.shtest** – verifies basic debug info for function parameters
- **function_metadata.shtest** – tests debug metadata on multi-parameter functions
- **variable_locations.shtest** – validates debug info tracking for variables in a loop
- more...

Each test compiles a small Rust snippet with DWARF enabled (`-C debuginfo=2`),
runs it through `midenc compile --emit hir`, and uses `FileCheck` to verify that
`builtin.dbg_value` intrinsics are emitted with the correct `di.local_variable`
attributes containing variable names, file paths, line numbers, and types.

To run the debug info tests:

```bash
/opt/homebrew/bin/lit -va tests/lit/debug/
```

Or to run a specific test:

```bash
/opt/homebrew/bin/lit -va tests/lit/debug/simple_debug.shtest
```

## Bottomline

- HIR now exposes DWARF-like metadata via reusable `DI*` attributes including
  `DIExpressionAttr` for location expressions.
- The wasm frontend precomputes function metadata, keeps it mutable during
  translation, and emits `dbg.value` intrinsics with location expressions for
  every parameter/variable assignment.
- Location expressions (DW_OP_WASM_local, etc.) are preserved from DWARF and
  attached to `dbg.value` operations, enabling accurate tracking of variables
  as they move between different storage locations.
- The serialized HIR describes user variables with accurate file/line/column
  information and storage locations, providing a foundation for future tooling
  (debugging, diagnostics correlation, or IR-level analysis).
- The design avoids redundancy by not embedding scope hierarchies in each variable,
  instead relying on structural containment to establish relationships.
