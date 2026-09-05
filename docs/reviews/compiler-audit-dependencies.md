# Dependency audit verification

## Macro dependency features (D06)

The shared `syn` dependency now uses its default features. Consumers request additional syntax or trait support where they use it:

| Consumer | Additional features | Reason |
|---|---|---|
| `midenc-hir-macros` | `full`, `extra-traits` | Parses `ItemStruct`; derives `Debug` for structures containing syntax nodes. Darling also requires both features. |
| `miden-base-macros` | `full` | Parses Rust files, functions, structs, and impl/trait items. Its diagnostic structs do not require syntax-node `Debug` implementations. |
| `midenc-integration-tests` | `full` | The SDK fixture helper parses `ImplItemFn`. |
| `miden-field-repr-derive` | None | Its derive-input, field, and variant parsing compiles with defaults. |

The experiment used isolated manifests pointing directly at the repository's macro library sources, outside the workspace, with a separate target directory. Dependencies were pinned to `syn 2.0.119`, `proc-macro2 1.0.107`, `quote 1.0.47`, and, for HIR, `darling 0.23.0` with `diagnostics` and `heck 0.5.0`. Each variant ran `cargo check --offline -j2`; `cargo metadata --offline --format-version 1` supplied resolved features. The compiler was `rustc 1.100.0-nightly (0dfb098f3 2026-08-31)`.

| Macro crate | Direct additional syn features | Check | Resolved additional syn features | Elapsed seconds |
|---|---|---|---|---:|
| HIR | `full`, `extra-traits` | Pass | `full`, `extra-traits` | 10.241 |
| HIR | `extra-traits` | Pass | `full`, `extra-traits` | 0.303 |
| HIR | `full` | Pass | `full`, `extra-traits` | 0.277 |
| HIR | None | Pass | `full`, `extra-traits` | 0.307 |
| Field representation derive | `full`, `extra-traits` | Pass | `full`, `extra-traits` | 0.512 |
| Field representation derive | `extra-traits` | Pass | `extra-traits` | 1.653 |
| Field representation derive | `full` | Pass | `full` | 1.810 |
| Field representation derive | None | Pass | None | 1.362 |

Runs were sequential and reused the isolated target directory: these elapsed times include check and metadata execution and are **not comparative build benchmarks**. The first run compiled common dependencies; later runs reused them. The useful result is successful independent compilation and the resolved feature sets. Removing direct HIR feature declarations alone produces no graph reduction because `darling_core` activates both features. The narrower field-representation feature set benefits independent builds; workspace feature unification can restore the broader set. No full-workspace package-count or timing improvement is claimed for this change.

HIR case conversion now uses Heck, as the SDK macro and Cargo tooling already do. Generated attribute registration tests check the actual emitted `Symbol::intern` argument for acronym and Unicode names. `HTTPRequest` becomes `http_request`, and `XMLHttpRequest` becomes `xml_http_request`. Unicode names follow Heck's lowercase normalization: `ÉclairValue` becomes `éclair_value`, `ΔeltaValue` becomes `δelta_value`, and `HTTPÉclair` becomes `http_éclair`. This intentionally differs from Inflector 0.11.4, which preserved the uppercase Unicode letters in these examples. Explicit attribute names remain available when a particular registration spelling is required. The two generated-name tests passed against the actual HIR macro source in the isolated crate.

Existing ASCII operation spellings are preserved with explicit `#[operation(name = "...")]`
overrides. An exhaustive comparison of Rust type identifiers and capitalized identifiers
(including macro invocations) using the actual Inflector 0.11.4 and Heck 0.5.0
implementations identified these 14 registered operations:

| Operation types | Preserved registered spelling | Heck default |
|---|---|---|
| `I32Load8S`, `I32Load16S` | `i32_load_8s`, `i32_load_16s` | `i32_load8_s`, `i32_load16_s` |
| `I64Load8S`, `I64Load16S`, `I64Load32S` | `i64_load_8s`, `i64_load_16s`, `i64_load_32s` | `i64_load8_s`, `i64_load16_s`, `i64_load32_s` |
| `Ext2Add`, `Ext2Sub`, `Ext2Mul`, `Ext2Div`, `Ext2Neg`, `Ext2Inv` | `ext_2_add`, `ext_2_sub`, `ext_2_mul`, `ext_2_div`, `ext_2_neg`, `ext_2_inv` | `ext2_add`, `ext2_sub`, `ext2_mul`, `ext2_div`, `ext2_neg`, `ext2_inv` |
| `Ilog2`, `Pow2` | `ilog_2`, `pow_2` | `ilog2`, `pow2` |
| `FriExt2Fold4` | `fri_ext_2_fold_4` | `fri_ext2_fold4` |

Candidate differences were checked against actual operation, attribute, dialect, and
pass declarations. Existing derived attribute and dialect names need no overrides;
macro-generated integer attributes (`I8` through `U128`) also retain their names.
There are no `PassInfo` derive consumers in the repository; passes such as `Local2Reg`
register their names explicitly. Other differing type names belong to patterns,
SDK examples, constraints, or external instruction enums, rather than HIR registrations.
The operation-name regression inspects emitted `OpRegistration::name` for both
Heck's digit handling and an explicit compatibility spelling. Generated builder
methods use their existing type-based definitions and are unaffected by the opcode override.

Reproduce the feature experiment with a temporary package containing `[workspace]`, `[lib] proc-macro = true`, an absolute `lib.path` to the corresponding source, and the pinned dependencies above. Use default syn features plus each table row's additional feature list, then inspect the resolved syn node in Cargo metadata. Run the committed generated-name regressions with `cargo test -p midenc-hir-macros naming_tests`.

## FileCheck production edge (D01)

Moving HIR's test-only FileCheck dependency to dev-dependencies was verified against freshly generated Cargo build-unit graphs for `midenc` using `cargo build -Z unstable-options --unit-graph -p midenc`. The baseline graph contained **362 distinct package IDs and 684 build units**; the changed graph contained **315 package IDs and 620 units**: a reduction of **47 packages and 64 units** for this normal tool build. Counts use the set of `units[].pkg_id` and the length of `units`, respectively. The captured graph files were `/private/tmp/midenc-units.json` and `/private/tmp/midenc-after-d01.json`.

These are resolved build graphs, not measured compile-time savings. Test builds still need FileCheck, and the reduction should not be extrapolated to all workspace targets. The earlier review's marginal edge-removal estimate was 46 packages; the regenerated graph is the result to use for the implemented change.
