# Debug Info Format Specification

This document describes the `.debug_info` custom section format used in MASP (Miden Assembly Package) files. This section contains source-level debug information that enables debuggers to map between Miden VM execution state and the original source code.

## Overview

The debug info section is stored as a custom section in the MASP package with the section ID `debug_info`. It is designed to be:

- **Compact**: Uses index-based references and string deduplication
- **Self-contained**: All information needed for debugging is in this section
- **Extensible**: Version field allows for future format evolution

## Section Structure

The `.debug_info` section contains the following logical subsections:

```
┌─────────────────────────────────────────┐
│           Debug Info Header             │
│  - version (u8)                         │
├─────────────────────────────────────────┤
│           .debug_str                    │
│  - String table (deduplicated)          │
├─────────────────────────────────────────┤
│           .debug_types                  │
│  - Type definitions                     │
├─────────────────────────────────────────┤
│           .debug_files                  │
│  - Source file information              │
├─────────────────────────────────────────┤
│           .debug_functions              │
│  - Function metadata                    │
│  - Variables (nested)                   │
│  - Inlined calls (nested)               │
└─────────────────────────────────────────┘
```

## Format Version

Current version: **1**

The version byte is the first field in the section and indicates the format version. Readers should reject sections with unsupported versions.

---

## .debug_str - String Table

The string table contains all strings used in the debug info, deduplicated to save space. Other sections reference strings by their index into this table.

### Contents

- File paths
- Function names
- Variable names
- Type names
- Linkage/mangled names

### Example Output

```
.debug_str contents:
  [   0] "/Users/user/project/src/lib.rs"
  [   1] "my_function"
  [   2] "x"
  [   3] "result"
```

---

## .debug_types - Type Information

The type table contains definitions for all types referenced by variables and functions. Types can reference other types by index, allowing for complex type hierarchies.

### Type Kinds

| Tag | Kind | Description |
|-----|------|-------------|
| 0 | Primitive | Built-in scalar types |
| 1 | Pointer | Pointer to another type |
| 2 | Array | Fixed or dynamic array |
| 3 | Struct | Composite type with fields |
| 4 | Function | Function signature |
| 5 | Unknown | Opaque/unknown type |

### Primitive Types

| Value | Type | Size (bytes) | Size (felts) |
|-------|------|--------------|--------------|
| 0 | void | 0 | 0 |
| 1 | bool | 1 | 1 |
| 2 | i8 | 1 | 1 |
| 3 | u8 | 1 | 1 |
| 4 | i16 | 2 | 1 |
| 5 | u16 | 2 | 1 |
| 6 | i32 | 4 | 1 |
| 7 | u32 | 4 | 1 |
| 8 | i64 | 8 | 2 |
| 9 | u64 | 8 | 2 |
| 10 | i128 | 16 | 4 |
| 11 | u128 | 16 | 4 |
| 12 | f32 | 4 | 2 |
| 13 | f64 | 8 | 2 |
| 14 | felt | 8 | 1 |
| 15 | word | 32 | 4 |

### Example Output

```
.debug_types contents:
  [   0] PRIMITIVE: i32 (size: 4 bytes, 1 felts)
  [   1] PRIMITIVE: felt (size: 8 bytes, 1 felts)
  [   2] POINTER -> i32
  [   3] ARRAY [felt; 4]
  [   4] STRUCT Point (size: 16 bytes)
            +   0: x : felt
            +   8: y : felt
```

---

## .debug_files - Source File Information

The file table contains information about source files referenced by functions and variables.

### Fields

| Field | Type | Description |
|-------|------|-------------|
| path_idx | u32 | Index into string table for file path |
| directory_idx | Option\<u32\> | Optional index for directory path |
| checksum | Option\<[u8; 32]\> | Optional SHA-256 checksum for verification |

### Example Output

```
.debug_files contents:
  [   0] /Users/user/project/src/lib.rs
  [   1] /rustc/abc123.../library/core/src/panicking.rs
  [   2] unknown
```

---

## .debug_functions - Function Information

The function table contains debug metadata for each function in the compiled program.

### Fields

| Field | Type | Description |
|-------|------|-------------|
| name_idx | u32 | Index into string table for function name |
| linkage_name_idx | Option\<u32\> | Optional mangled/linkage name |
| file_idx | u32 | Index into file table |
| line | u32 | Line number where function is defined |
| column | u32 | Column number |
| type_idx | Option\<u32\> | Optional function type (index into type table) |
| mast_root | Option\<[u8; 32]\> | MAST root digest linking to compiled code |
| variables | Vec | Local variables and parameters |
| inlined_calls | Vec | Inlined function call sites |

### Variables

Each function contains a list of variables (parameters and locals):

| Field | Type | Description |
|-------|------|-------------|
| name_idx | u32 | Index into string table |
| type_idx | u32 | Index into type table |
| arg_index | u32 | 1-based parameter index (0 = local variable) |
| line | u32 | Declaration line |
| column | u32 | Declaration column |
| scope_depth | u32 | Lexical scope depth (0 = function scope) |

### Inlined Calls

For tracking inlined function calls:

| Field | Type | Description |
|-------|------|-------------|
| callee_idx | u32 | Index into function table for inlined function |
| file_idx | u32 | Call site file |
| line | u32 | Call site line |
| column | u32 | Call site column |

### Example Output

```
.debug_functions contents:
  [   0] FUNCTION: my_function
         Location: /Users/user/project/src/lib.rs:10:1
         MAST root: 0xabcd1234...
         Variables (3):
           - x (param #1): i32 @ 10:14
           - y (param #2): i32 @ 10:22
           - result (local): i32 @ 11:9 [scope depth: 1]
         Inlined calls (1):
           - helper_fn inlined at lib.rs:12:5
```

---

## Usage

### Generating Debug Info

Compile with debug info enabled:

```bash
midenc input.wasm --exe --debug full -o output.masp
```

For projects using `trim-paths`, use the `-Z trim-path-prefix` option to preserve absolute paths:

```bash
midenc input.wasm --exe --debug full \
    -Z trim-path-prefix="/path/to/project" \
    -o output.masp
```

### Inspecting Debug Info

Use the `miden-debugdump` tool to inspect debug info in a MASP file:

```bash
# Full dump (includes all sections)
miden-debugdump output.masp

# Summary only
miden-debugdump output.masp --summary

# Specific section from .debug_info
miden-debugdump output.masp --section functions
miden-debugdump output.masp --section variables
miden-debugdump output.masp --section types
miden-debugdump output.masp --section files
miden-debugdump output.masp --section strings

# Show DebugVar decorators from MAST (.debug_loc)
miden-debugdump output.masp --section locations

# Verbose mode (shows additional details like raw decorator list)
miden-debugdump output.masp --section locations --verbose

# Raw indices (for debugging the debug info itself)
miden-debugdump output.masp --raw
```

---

## Design Rationale

### Index-Based References

All cross-references use indices rather than embedding data directly. This:
- Enables string deduplication (file paths, names appear once)
- Reduces section size
- Allows efficient random access

### Separation of Concerns

The section is divided into logical subsections:
- **Strings**: Shared across all other sections
- **Types**: Can be referenced by multiple variables/functions
- **Files**: Shared by multiple functions
- **Functions**: Contains variables and inlined calls inline

### Compatibility with DWARF

The format is inspired by DWARF but simplified for Miden's needs:
- No complex DIE tree structure
- No location expressions (handled by `DebugVar` decorators in MAST)
- No line number tables (locations embedded in functions/variables)

---

## Debug Variable Locations

Debug information in MASP is split between two locations: the `.debug_info` custom section (documented above) and `Decorator::DebugVar` entries embedded in the MAST instruction stream.

### Architecture Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                        MASP Package                              │
├──────────────────────────────────────────────────────────────────┤
│  MAST Forest                                                     │
│  ├── MastNode[]                                                  │
│  │   └── Decorator::DebugVar(DebugVarInfo)  ← Runtime locations  │
│  │       • name: "x"                                             │
│  │       • value_location: Stack(0) / Local(2) / Memory(...)    │
│  │       • source location                                       │
│  └── String table (for names)                                    │
├──────────────────────────────────────────────────────────────────┤
│  .debug_info Section (separate custom section)                   │
│  ├── .debug_str (deduplicated strings)                           │
│  ├── .debug_types (type definitions)                             │
│  ├── .debug_files (source file paths)                            │
│  └── .debug_functions (static metadata, variables, inlined)      │
└──────────────────────────────────────────────────────────────────┘
```

### Why Two Locations?

| Aspect | `Decorator::DebugVar` in MAST | `.debug_info` Section |
|--------|-------------------------------|----------------------|
| **Where stored** | Embedded in instruction stream | Custom section at end of MASP |
| **Purpose** | Runtime value location at specific execution points | Static metadata (types, files, function info) |
| **When used** | During execution, debugger reads variable values | To display type names, source files, etc. |
| **DWARF analog** | Location lists (`.debug_loc`) | `.debug_info` / `.debug_abbrev` |

The `.debug_info` section tells you **what** variables exist (name, type, scope). The `DebugVar` decorators tell you **where** a variable's value is at a specific point during execution.

### DebugVarInfo Structure

Each `Decorator::DebugVar` contains a `DebugVarInfo` with the following fields:

| Field | Type | Description |
|-------|------|-------------|
| name | String | Variable name |
| value_location | DebugVarLocation | Where to find the value |
| type_id | Option\<u32\> | Index into `.debug_types` |
| arg_index | Option\<u32\> | 1-based parameter index (if parameter) |
| location | Option\<FileLineCol\> | Source location of declaration |

### DebugVarLocation Variants

The `value_location` field describes where the variable's value can be found at runtime:

| Variant | Encoding | Description |
|---------|----------|-------------|
| `Stack(u8)` | Tag 0 + u8 | Value is at stack position N (0 = top) |
| `Memory(u32)` | Tag 1 + u32 | Value is at memory word address |
| `Const(u64)` | Tag 2 + u64 | Value is a constant field element |
| `Local(u16)` | Tag 3 + u16 | Value is in local variable slot N |
| `Expression(Vec<u8>)` | Tag 4 + len + bytes | Complex location (DWARF-style expression) |

### Example

For a function like:
```rust
fn add(x: i32, y: i32) -> i32 {
    let sum = x + y;
    sum
}
```

The MAST will contain decorators like:
```
# At function entry
Decorator::DebugVar { name: "x", value_location: Local(0), arg_index: Some(1), ... }
Decorator::DebugVar { name: "y", value_location: Local(1), arg_index: Some(2), ... }

# After computing sum
Decorator::DebugVar { name: "sum", value_location: Stack(0), arg_index: None, ... }
```

A debugger pausing at a specific instruction can read these decorators to know where each variable's value is stored at that moment.

---
