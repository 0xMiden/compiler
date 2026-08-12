# i1307 — PoC: schema for user-defined note types (WIT + Wasm)

Implements the design in discussion [#1294](https://github.com/0xMiden/compiler/discussions/1294) (issue #1307).
Branch: `i1307-note-type-schema` (on `next`). All new crates are `publish = false` (PoC grade).

## Settled decisions

- Full design in scope: schema emission, runtime API, bindings macro, codec component. Codec last.
- `wasmtime` enters the workspace (feature-gated, consumer side only).
- Replicate the `PackageSections` refactor from the WIP `wit-in-package` branch on this branch (do not base on it).
- Examples: p2id = no-codec demo; new `dex-note` + `dex-note-codec` pair = custom-type + codec demo.
- Migration: only the shared p2id path in `tests/integration-network/src/mockchain/support/helpers.rs`. Other hand-encoded sites stay for a follow-up.
- Type surface (PoC): named-field structs → schema; unit structs → no schema section; tuple structs → compile error; `Vec` fields → "not supported yet" error; nested custom types need `#[export_type]` defined before the `#[note]` struct; doc comments are carried into the WIT; embed the whole `core-types` interface (no subset pruning).

## Name registry (fixed up front)

| Thing | Name |
|---|---|
| Wasm custom section (schema) | `rodata,miden_note_schema` |
| Wasm static (schema) | `__MIDEN_NOTE_STORAGE_SCHEMA_BYTES` |
| `.masp` section id (schema) | `note_storage_schema` (via `SectionId::custom`) |
| `.masp` section id (codec) | `note_codec` (via `SectionId::custom`) |
| Schema WIT package | `<ns>:<name>-schema@<version>`, interface `note-storage`, root alias `type storage = <root>;` |
| Codec world | `package miden:note-codec@1.0.0`, `world note-codec`, `type felt = u64` |
| Reader crate | `sdk/note-schema` → `miden-note-schema` |
| Shared codegen (internal) | `sdk/note-schema/codegen` → `miden-note-schema-codegen` |
| Bindings macro crate | `sdk/note-bindings` → `miden-note-bindings` (pure proc-macro) |
| Author codec crates | `sdk/note-codec` → `miden-note-codec` (lib) + `sdk/note-codec/macros` → `miden-note-codec-macros` |
| Codec-crate pointer | `[package.metadata] note-codec-crate = "…"` in the note's `miden-project.toml` (`MetadataSet` is free-form; no upstream `miden-project` change) |

The normative felt layout rule (document it in `miden-note-schema` rustdoc): layout is structural
over the WIT type tree; the record `miden:base/core-types.felt` is the 1-felt bedrock; `u64` = 2
felts (lo u32, hi u32); `u32`/`u8`/`bool` = 1 range-checked felt; `option<T>` = 1 tag felt +
payload; `variant` = 1 tag felt (declaration ordinal) + case payload; records concatenate fields in
declaration order. This is exactly `miden-field-repr`'s documented layout, so `word` (4),
`account-id` (2), etc. need no special cases — they bottom out at `felt` structurally. Codecs never
change layout; they only bind string parse/display/validate to a WIT fqn.

---

## Phase 0 — `PackageSections` plumbing refactor

Goal: one struct carried through the pipeline instead of a per-payload `Option<Vec<u8>>` field, so
Phase 1 (and the wit-in-package rebase later) only add a field.

1. `sdk/wasm-metadata/src/lib.rs`:
   - Add consts `WASM_ACCOUNT_COMPONENT_METADATA_CUSTOM_SECTION_NAME = "rodata,miden_account"`
     (replace the two hardcoded uses), `WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME`,
     `PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID`, `PACKAGE_NOTE_CODEC_SECTION_ID`.
   - Add `pub struct PackageSections { pub account_component_metadata: Option<Vec<u8>>, pub note_storage_schema: Option<Vec<u8>> }`
     (mirror the shape on `origin/wit-in-package`, without its `component_wit` field).
2. Thread `PackageSections` through, replacing `account_component_metadata_bytes`:
   - `frontend/wasm/src/module/module_env.rs` (`ParsedModule`), `frontend/wasm/src/component/translator.rs:176-192`,
     `frontend/wasm/src/lib.rs` (`FrontendOutput`), `midenc-compile/src/pipeline/frontends/wasm.rs:470-487`,
     `midenc-compile/src/pipeline/artifacts.rs` (`MidenComponent`, `CodegenOutput`),
     `midenc-compile/src/pipeline/backend.rs` (`LoweredTarget`),
     `midenc-compile/src/pipeline/assembly.rs::post_process_package` and its callers
     (`backend.rs:828`, `frontends/hir.rs:392`, `frontends/wasm.rs:590`, `seed.rs:437`, `seed.rs:604`).
3. No behavior change. `cargo make test` must stay green with no expectation updates.

## Phase 1 — Schema emission into the `.masp`

### 1a. Macro side (`sdk/base-macros`)

1. Registry doc support (`src/types.rs`, `src/export_type.rs`): add `docs: Vec<String>` to
   `ExportedTypeDef`, `ExportedField`, `ExportedVariant`; capture `#[doc]` attrs at registration.
   The component-macro consumer ignores the new fields.
2. New module `src/note_schema.rs`:
   - Input: the `#[note]` struct item (+ doc attrs), the export-type registry, package identity.
   - Package identity: reuse the same source `build_note_script_wit` uses for the main WIT package
     name (crate-name/manifest based) and append `-schema`; version from the same source. Must not
     fail on fixtures without `miden-project.toml`.
   - Map field types with the existing `map_type_to_type_ref`; wrap its rejections in a
     note-specific diagnostic ("`Vec` is not supported in note storage schemas yet", etc.).
     Resolve custom types through the registry (transitive closure; unregistered → error that names
     `#[export_type]` and the ordering rule, like `ensure_custom_type_defined`).
   - Render the multi-package WIT document. Settled layout (spike-verified, 2026-08-05 — the
     all-braced form in the discussion sketch does NOT parse in wit-parser 0.247; the main package
     must be first and in header form):
     `package <ns>:<name>-schema@<ver>;` header, then `interface note-storage { use …; records…; type storage = <root>; }`
     at top level, then `package miden:base@1.0.0 { interface core-types { … } }` as a braced
     block at the end — body extracted verbatim from `SDK_WIT_SOURCE` (textual block extraction;
     unit-test it). `WitBuilder` needs a `package_block` (braced package) helper; doc comments
     emitted as `///` lines. Field/record names kebab-cased with the existing helpers.
   - Emit bytes: UTF-8 text, padded to a 16-byte multiple with NULs (mirror ACM padding; readers
     trim trailing NULs).
   - Emit the static: `#[unsafe(link_section = "rodata,miden_note_schema")] pub static __MIDEN_NOTE_STORAGE_SCHEMA_BYTES: [u8; N]`
     (fixed name → duplicate-symbol link error enforces one storage schema per crate).
3. Hook into `expand_note_struct` (`src/note.rs:100`): named-field arm emits the schema static;
   unit arm emits nothing; unnamed/tuple arm becomes a compile error ("note storage schema needs
   named fields").
4. Unit tests (`sdk/base-macros/tests/`, plus module tests using
   `reset_export_type_registry_for_tests`):
   - Golden expansion for a p2id-shaped struct and for a struct with a nested `#[export_type]`
     record and an enum.
   - Parse the emitted document with `wit_parser` (`wit-bindgen-core` re-exports it; wit-component
     0.247 is already a dev-dep) and assert it resolves; assert interface/alias/root are found.
   - Error cases: tuple struct, `Vec` field, unregistered custom type.

### 1b. Compiler side

1. `frontend/wasm/src/module/module_env.rs`: new `Payload::CustomSection` arm for
   `WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME` → store raw bytes (validate UTF-8 after NUL trim;
   full WIT validation stays in tests/consumers for the PoC).
2. `translator.rs`: collect across nested modules, error on >1 (mirror the ACM logic) → the new
   `PackageSections.note_storage_schema` field.
3. `midenc-compile/src/pipeline/assembly.rs`: `attach_note_storage_schema` — push
   `Section::new(SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID)?, bytes)` whenever
   present. Custom sections are digest-exempt in `miden-mast-package` 0.25.8 by construction —
   nothing to do for digests.

### 1c. Tests

- New `tests/integration/src/end_to_end/examples/note_schema_metadata.rs` (mirror
  `counter_metadata.rs`): build `examples/p2id-note`, read the section, expect-test the WIT text,
  and round-trip it through `wit_parser::Resolve`.
- Same golden for the `swapp-note` fixture (6 fields: `Word`, `Felt`, `AccountId`, …) — the richest
  existing storage struct.
- Update package-size expectations (`basic_wallet_package_sizes.rs` and friends) with
  `UPDATE_EXPECT=1` — every named-field note package now carries the section.

## Phase 2 — Runtime API: `miden-note-schema`

New std host crate `sdk/note-schema` (workspace member, `publish = false`).
Deps: `miden-mast-package`, `wit-parser` (promote 0.247 to a workspace dep), `miden-protocol`,
`miden-field`, `miden-field-repr` (native mode — reuse `FeltReader`/`FeltWriter` so the layout
rules live in one place).

1. Schema model + reader:
   - `NoteStorageSchema::from_package(&Package)` — find section, trim NULs, parse UTF-8 WIT,
     resolve, locate interface `note-storage` and alias `storage`, build an internal model
     (ordered fields; type tree of records/variants/options/primitives/leaf fqns).
     `from_wit_text(&str)` for tests. Surface validation errors are actionable.
   - Layout interpreter over the model per the normative rule above (widths, walk order).
2. Codecs:
   - `trait ConsumerTypeCodec { parse(&self,&str)->Result<Vec<Felt>,_>; display(&self,&[Felt])->String; validate(&self,&[Felt])->Result<(),_> }`.
   - `CodecRegistry` keyed by WIT fqn. `Default` = standard leaf codecs over protocol parsers:
     `felt` (decimal/hex), `word` (hex / 4-felt), `account-id` (`AccountId::parse` bech32|hex →
     `[prefix, suffix]` per the WIT record order), `asset-amount` (decimal u64). Keep the set small.
3. Build direction: `schema.builder()` — `set(name, &str)` (kebab accepted, snake normalized;
   dotted paths reach nested-record leaves — the structural UX), leaf routing: codec fqn if
   registered, else primitive parse, else error naming the fqn; `build()` does completeness +
   range checks → `NoteStorage`.
4. Decode direction: `schema.decode(&NoteStorage)` → named value tree; `Display` uses the registry
   when an fqn is registered, else structural rendering.
5. Tests: unit tests on `from_wit_text` (layout widths, builder round-trips, error paths);
   integration test: build p2id `.masp`, `builder().set("target-account-id", bech32).build()`
   equals the hand-built `[prefix, suffix]` storage; decode round-trip displays the bech32 back.

## Phase 3 — Typed bindings: `miden-note-bindings`

1. `sdk/note-schema/codegen` (`miden-note-schema-codegen`, internal lib): schema model → Rust
   tokens for the host-profile types — one generator shared by Phases 3 and 4:
   - standard leaves → `miden_protocol::account::AccountId`, `miden_field::Word`,
     `miden_field::Felt`; primitives verbatim; custom records/variants → generated
     structs/enums with `#[derive(ToFeltRepr, FromFeltRepr)]` (native) + a WIT-fqn const per type.
   - protocol-leaf encode/decode helpers follow the WIT record order (`account-id` →
     `[prefix, suffix]`).
2. `sdk/note-bindings` (pure proc-macro crate):
   - `from_project!("../dex-note")` — resolve against `CARGO_MANIFEST_DIR`; find the freshest
     `.masp` across `<dir>/target/miden/<profile>/` (mirror the `fpi.rs:1671`/`:1704` candidate
     logic); missing artifact → compile error naming `cargo miden build`.
   - `from_package!("path/to.masp")` — exact path. Both read the section and delegate to the shared
     codegen, then add the consumer surface per the design: `to_note_storage`,
     `from_note_storage`, `from_str_values(&BTreeMap<_,_>, &CodecRegistry)`, `validate_with`,
     `display_with`; when the schema has no custom types, drop the `codecs` parameters.
   - Hidden `from_wit_text!` entry for golden expansion tests.
3. Tests: expansion goldens over schema WIT strings (p2id-shaped, custom-type-shaped); one
   end-to-end test that builds `examples/p2id-note` and then compiles + runs a small temp consumer
   crate (process-spawned `cargo`, pattern like `tools/cargo-miden/tests`).

## Phase 4 — Codec component

### 4a. World + author-side crates

1. `sdk/note-codec` (`miden-note-codec`, lib): ships `wit/note-codec.wit` (the world exactly as in
   the discussion, `type felt = u64` with the doc comment about why it is not the core-types
   record); `trait AuthorTypeCodec { fn parse(&str)->Result<Self,String>; fn display(&self)->String; fn validate(&self)->Result<(),String> }`;
   boundary glue: u64↔Felt via canonical u64 with canonicality checks (reject ≥ p at the boundary).
2. `sdk/note-codec/macros` (`miden-note-codec-macros`, re-exported from the lib):
   - `from_project!` / `from_package!` — same artifact resolution as Phase 3, but generate only the
     host-profile types + record the schema (incl. root) in a process-global registry for
     `export_codecs!`.
   - `#[note_codec]` — marks an `AuthorTypeCodec` impl; registers the type's fqn.
   - `export_codecs!()` — wit-bindgen `generate!` for the `note-codec` world plus the `export!`
     glue: `supported-types` from the marked set; `parse`/`display`/`validate` dispatch by fqn
     through the marked impls and their felt-repr impls. Component glue gated
     `cfg(target_family = "wasm")` so the crate still builds and unit-tests natively.
3. Componentization target: primary = `wasm32-unknown-unknown` cdylib +
   `wit_component::ComponentEncoder` (no WASI imports — clean sandbox, jco-friendly); fallback if
   friction = `wasm32-wasip2` (rustc emits a component directly, consumer then needs a WASI
   context). Spike this first inside Phase 4.

### 4b. `cargo-miden` orchestration

In `tools/cargo-miden/src/commands/build.rs` after `compile_to_memory`, before `write_masp_file`:
read the note's `miden-project.toml` `[package.metadata] note-codec-crate` (via the
`miden-project` crate's `MetadataSet`); when set: `cargo build --release` for the codec crate at
the componentization target, componentize, and `package.sections.push(Section::new(SectionId::custom("note_codec")?, bytes))`.
Omitted key → no section (structural fallback).

### 4c. Consumer adapter

`miden-note-schema`, feature `codec-component` (off by default): `CodecRegistry::load_from_package`
— find the `note_codec` section, instantiate with `wasmtime` (component model), wrap in an adapter
implementing `ConsumerTypeCodec` per fqn reported by `supported-types`. `wasmtime` is a
feature-gated dependency of this crate only.

### 4d. Example pair + end-to-end

1. `examples/dex-note` (guest, standalone project like p2id): `#[export_type] #[derive(FromFeltRepr)] struct LimitPrice { numerator: u64, denominator: u64 }`,
   `#[note] struct DexNote { target: AccountId, price: LimitPrice }`, trivial `#[note_script]`
   against `basic-wallet` (p2id-like), doc comments on everything (they surface in the WIT).
   `miden-project.toml` gets the `note-codec-crate` pointer.
2. `examples/dex-note-codec` (host, plain cargo, no wasm target config): depends only on
   `miden-note-codec`; `from_project!("../dex-note")`; `#[note_codec] impl AuthorTypeCodec for LimitPrice`
   (parse "3/2" and "1.5" forms, display, validate denominator ≠ 0); `export_codecs!()`.
3. Tests:
   - `tools/cargo-miden/tests/dex_note_codec_build.rs`: `cargo miden build` on dex-note → package
     has both `note_storage_schema` and `note_codec` sections.
   - `tests/integration-network` mockchain test: consume a dex note whose storage was built from
     strings (`"target"` = bech32 natively, `"price"` = `"1.5"` through the component), and decode
     an incoming note back to `"1.5"` via `display`.
   - p2id no-codec demo + migration: mockchain test building p2id storage via the schema string
     builder; switch the shared p2id path in `support/helpers.rs` (`to_core_felts` call in
     `build_asset_transfer_tx`) to the schema builder.

## Cross-cutting finish work

- `sdk/sdk/CHANGELOG.md`: entry for the `#[note]` schema emission + new `.masp` section (the
  published macro crates change even though the new crates are `publish = false`); new PoC
  restrictions (tuple structs, `Vec` fields) called out. No MIGRATION entry (additive; the
  restrictions break no released code — verify no external breakage claim beyond the repo).
- Full sweep per repo rules: `cargo make test-all`, `cargo make clippy`, `cargo make format-rust`;
  `UPDATE_EXPECT=1` only for the intended package-size/golden updates.

## Risks / early spikes

1. ~~**Multi-package WIT text** in wit-parser 0.247~~ — RESOLVED by spike: header-form main
   package first + braced dependency package after works; all-braced does not. JS-tooling (jco)
   parse compatibility is untested and out of PoC scope (Rust consumers only).
2. **Componentization target** (4a.3) — spike at Phase 4 start.
3. **Package identity in `expand_note_struct`** — must degrade gracefully for fixtures without
   `miden-project.toml` (fall back to crate name/version, same as the note-script WIT).
4. **Link-section padding** — ACM pads to 16 bytes; keep the padding, trim NULs on every reader.
5. **`from_project!` freshness** — newest-mtime across profiles is the settled design rule
   (schema is profile-invariant); reuse the fpi.rs candidate-dir logic rather than reinventing.
6. **Process-global registries in proc macros** — one schema registry per crate build is the same
   trade the `#[export_type]` registry already makes; keep the reset-for-tests hook pattern.

## Suggested execution order

Phase 0 → 1 → 2 → 3 → 4, each landable and testable on its own. Phases 0–1 touch the compiler
pipeline and macros; 2–3 are pure new host crates; 4 touches `cargo-miden` + examples. Codex-sized
work packets: each phase is one packet, with Phase 4 split into (world+author crates), (cargo-miden
+ example), (consumer + e2e).
