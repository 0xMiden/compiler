# Debug Info Metadata Pipeline

This note describes how the Miden compiler threads source-level variable
metadata through HIR when compiling Wasm input. The goal is to make every HIR
function carry `DI*` attributes and `debuginfo.*` operations that mirror the
DWARF records present in the Wasm binary, so downstream passes (or tooling
consuming serialized HIR) can reason about user variables.

## The DebugInfo Dialect

Debug variable tracking is implemented as a first-class IR dialect
(`midenc-dialect-debuginfo`, namespace `"debuginfo"`), inspired by
[Mojo's DebugInfo dialect](https://llvm.org/devmtg/2024-04/slides/TechnicalTalks/MojoDebugging.pdf).
Unlike metadata-based approaches (e.g., Flang/FIR), debug operations here are
real IR operations with SSA operands, which means:

- If a transform deletes a value without updating its debug uses, that is a
  hard error — not a silent drop.
- Standard `replace_all_uses_with` automatically propagates value replacements
  to debug uses.
- The IR verifier catches dangling debug references.

### Operations

| Operation | Operands | Purpose |
|-----------|----------|---------|
| `debuginfo.debug_value` | SSA value + `DILocalVariableAttr` + `DIExpressionAttr` | Records the current value of a source variable |
| `debuginfo.debug_declare` | SSA address + `DILocalVariableAttr` | Records the storage location (address) of a variable |
| `debuginfo.debug_kill` | `DILocalVariableAttr` only | Marks a variable as dead at this program point |

### Design Pillars

1. **SSA use-def chains** — debug values participate in standard use-def tracking,
   making it impossible for transforms to silently lose debug info.
2. **Expression trees** — `DIExpressionAttr` describes how to recover source values
   from transformed IR values (encoding the inverse transformation).
3. **Explicit lifetimes** — `debuginfo.debug_kill` provides precise variable death
   points instead of relying on scope-based heuristics.

### Builder API

The `DebugInfoOpBuilder` trait provides a convenient API for emitting debug ops:

```rust
// Track a variable's value:
builder.debug_value(ssa_value, variable_attr, span)?;

// Track with a custom expression (e.g., value needs a dereference):
builder.debug_value_with_expr(ssa_value, variable_attr, Some(expr), span)?;

// Track a variable's storage address:
builder.debug_declare(address_value, variable_attr, span)?;

// Mark a variable as dead:
builder.debug_kill(variable_attr, span)?;
```

The trait has a blanket implementation for all `Builder` types, so any IR builder
can emit debug operations directly.

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
   parameters, and emits `debuginfo.debug_value` operations whenever locals change.
4. **Span-aware updates** – as each wasm operator is translated we store the
   real `SourceSpan`. The first non-unknown span is used to retroactively patch
   the compile unit, subprogram, and parameter variable records with real file,
   line, and column information so the resulting HIR references surfaces from
   the actual user file.

The emitted HIR therefore contains both the SSA instructions and the debug
operations that map values back to the user program.

## HIR Metadata Constructs

The core attribute types live in `hir/src/attributes/debug.rs`:

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
programmatically. The debug operations (`debuginfo.debug_value`,
`debuginfo.debug_declare`, `debuginfo.debug_kill` from
`dialects/debuginfo/src/ops.rs`) consume SSA values plus the metadata
attributes. The `debug_value` operation includes a `DIExpressionAttr` field
that describes the location or computation of the variable's value.

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
  `debug_value` operations after we encounter the first real span (parameters
  have no dedicated wasm operator with source ranges).
- Every wasm operator calls `builder.record_debug_span(span)` prior to emission;
  the first non-unknown span updates the compile unit/subprogram attrs and
  triggers parameter `debug_value` emission so arguments are tied to the correct
  location.
- `def_var_with_dbg` is the canonical entry point for `local.set` and
  `local.tee`. It updates the SSA value and immediately emits a
  `debuginfo.debug_value` with the precise span of the store.
- Decoded `DW_AT_location` ranges are normalized into a per-function schedule.
  As the translator visits each wasm offset we opportunistically emit extra
  `debug_value` operations so source variables track transitions between Wasm
  locals without relying on `debuginfo.debug_declare`.
- When present, `DW_AT_decl_line`/`DW_AT_decl_column` on variables override the
  default span so we keep the original lexical definition sites instead of
  inheriting the statement we first observed during translation.

Locals declared in the wasm prologue receive an initial value but no debug
operation until they are defined in user code. Subsequent writes insert
additional `debug_value` ops so consumers can track value changes over time.

## Example

In the serialized HIR for the test pipeline you now see:

```hir
debuginfo.debug_value v0 #[expression = di.expression(DW_OP_WASM_local 0)]
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
When a variable moves between locations, additional `debug_value` operations are
emitted with updated expressions:

```hir
debuginfo.debug_value v22 #[expression = di.expression(DW_OP_WASM_local 3)]
    #[variable = di.local_variable(name = sum, ...)]
```

Both the attribute and the trailing comment reference the same source location
so downstream tooling can disambiguate the variable regardless of how it parses
HIR.

## Transform Hooks

The `debuginfo::transform` module (`dialects/debuginfo/src/transform.rs`)
provides utilities that make it straightforward for transform authors to
maintain debug info across IR transformations.

### Simple Replacements (Automatic)

When a transform replaces one value with another (e.g., CSE, copy propagation),
the standard `replace_all_uses_with` automatically updates all debug uses:

```text
// Before: debuginfo.debug_value %1 #[variable = x]
// rewriter.replace_all_uses_with(%1, %0)
// After:  debuginfo.debug_value %0 #[variable = x]  -- automatic!
```

### Complex Transforms (SalvageAction)

For transforms that change a value's representation (not just replace it),
the `salvage_debug_info()` function takes a `SalvageAction` describing the
inverse transformation. Available actions:

| Action | Use Case | Expression Update |
|--------|----------|-------------------|
| `Deref { new_value }` | Value promoted to stack allocation | Prepends `DW_OP_deref` |
| `OffsetBy { new_value, offset }` | Frame pointer adjustment | Appends `const(offset), minus` |
| `WithExpression { new_value, ops }` | Arbitrary complex transform | Appends custom expression ops |
| `Constant { value }` | Constant propagation | Emits `debuginfo.debug_kill` (future: constant expression) |
| `Undef` | Value completely removed | Emits `debuginfo.debug_kill` |

Example usage in a transform:

```rust
use midenc_dialect_debuginfo::transform::{salvage_debug_info, SalvageAction};

// Value was promoted to memory:
let ptr = builder.alloca(ty, span)?;
builder.store(old_val, ptr, span)?;
salvage_debug_info(&old_val, &SalvageAction::Deref { new_value: ptr }, &mut builder);
```

### Helper Functions

- `is_debug_info_op(op)` — checks if an operation is a debug info op (useful
  for DCE to skip debug uses when determining liveness)
- `debug_value_users(value)` — collects all `debuginfo.debug_value` ops that
  reference a given value
- `collect_debug_ops(op)` — recursively collects all debug ops within an
  operation's regions

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
  globals, and operand-stack slots), and attach them to `debuginfo.debug_value`
  operations as `DIExpressionAttr`. The system emits additional `debug_value`
  operations whenever a variable's storage changes, with each operation
  containing the appropriate expression. This allows modeling multi-location
  lifetimes where variables move between different storage locations. Support
  for more complex composite expressions (pieces, arithmetic operations, etc.)
  is implemented but not fully utilized from DWARF parsing yet.
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
`debuginfo.debug_value` operations are emitted with the correct `di.local_variable`
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

- Debug variable tracking uses a dedicated `debuginfo` dialect with SSA-based
  operations (`debuginfo.debug_value`, `debuginfo.debug_declare`,
  `debuginfo.debug_kill`), making debug info a first-class IR citizen that
  transforms cannot silently drop.
- HIR exposes DWARF-like metadata via reusable `DI*` attributes including
  `DIExpressionAttr` for location expressions.
- The wasm frontend precomputes function metadata, keeps it mutable during
  translation, and emits `debuginfo.debug_value` operations with location
  expressions for every parameter/variable assignment.
- Transform authors maintain debug info via `salvage_debug_info()` — they only
  describe the inverse of their transformation, and the framework updates all
  affected debug operations automatically.
- Location expressions (DW_OP_WASM_local, etc.) are preserved from DWARF and
  attached to `debug_value` operations, enabling accurate tracking of variables
  as they move between different storage locations.
- The serialized HIR describes user variables with accurate file/line/column
  information and storage locations, providing a foundation for future tooling
  (debugging, diagnostics correlation, or IR-level analysis).
- The design avoids redundancy by not embedding scope hierarchies in each variable,
  instead relying on structural containment to establish relationships.
