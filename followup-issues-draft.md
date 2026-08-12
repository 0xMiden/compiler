# i1307 follow-up issues (draft — not filed)

Source: discussion #1294 design, `review_claude.md` findings deferred from the 2026-08-07 fix
wave, and PoC restrictions. Each section is one proposed issue.

## 1. Schema/codec sections and package identity (digest coverage, envelope, attestation)

Custom `.masp` sections are digest-exempt by construction in `miden-mast-package`, so two
packages with identical identity can carry different `note_storage_schema` layouts and different
executable `note_codec` bytes (review finding 2d). Decide how these sections participate in
package identity: upstream digest coverage (a real `SectionId` + content-digest inclusion), or an
attested hash carried inside digest-covered data. In the same change, give the sections a
versioned envelope (storage-ABI version + owning package identity) — weigh against the #1294
convention that the schema section is plain inspectable WIT text. Belongs with the #1290
multi-component package redesign conversation. Duplicate-section rejection and exact-one readers
already landed on the branch.

## 2. Codec build as a first-class pipeline phase

The nested codec build now runs inside `post_process_package` (gated, session-threaded), but the
cleaner shape is a pipeline phase that produces a validated artifact before assembly, keeping
post-processing deterministic and side-effect-free (review finding 3, oracle suggestion).

## 3. Sealed storage-ABI trait: one source for schema, encode, and decode

A custom type with a manual (non-derived) `FromFeltRepr` impl can decode fields in a different
order than the emitted schema declares — a silent on-chain field swap (review finding 6). Derive
the schema node and the felt encode/decode from one source (sealed trait or equivalent) so the
"schema cannot drift from the code" guarantee covers manual impls too. Interim state on the
branch: expansion-time WIT resolution + the derive-based path; the manual-impl hole is
documented.

## 4. Artifact index for note-project discovery + codec provenance

`from_project!` discovery now prefers the compiler-staged path via env var, but profile
discovery still selects by newest mtime without package identity, and consumer bindings track
only the chosen file (review finding 4 residue). Have the build write a small artifact index
(package id/version/target/profile) that `from_project!` resolves through and tracks. Decide
whether codec crate sources/manifest/lockfile join the package-cache fingerprint legs (relates
to the i1302 fingerprint design).

## 5. String-builder support for nested constructor payloads

The builder rejects `option<record>` and variant-with-record-payload leaves with a clear error
(landed); actually supporting them needs a constructor syntax or structured input (review
finding 8, deferred half).

## 6. Migrate the remaining hand-encoded mockchain storage sites

`support/helpers.rs`'s shared p2id path uses the schema builder; ~19 sites still hand-encode
felts (`to_core_felts`, p2ide 4-felt layout, swapp 13-felt `to_storage_felts`, FPI fixtures).
Migrate them to schema-driven construction; delete `to_core_felts` when the last user goes.

## 7. `list<T>` (Vec) support in note storage schemas

`Vec` fields are rejected today (breaking change, documented in MIGRATION). Design the `list<T>`
schema mapping (felt-repr already defines the len-prefixed layout) end to end: emitter, reader
layout interpreter (variable width), builder/decode UX, bindings codegen.

## 8. Schema polish: subset pruning and JS-side validation

- Embed only the transitively referenced `core-types` subset instead of the whole interface
  (#1294 allows both; whole-interface embed was the PoC call).
- Validate the schema document and codec world against JS tooling (jco) — the wit-parser
  header-form + braced-package layout is unverified there.

## 9. Uniqueness-guard cycle cost (optional micro-optimization)

The `#[note]` schema uniqueness guard is an exported `u8` static and costs +10 cycles per note
execution (+34 bytes MAST) via rodata/advice-map layout shift. Try a zero-sized guard static
(same `export_name` collision semantics, no rodata byte); if it works, apply to the frontend
metadata guard too.
