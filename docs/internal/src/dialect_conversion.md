# Dialect Conversion

Status: proposed

This document specifies a dialect conversion framework for HIR. The design is
inspired by MLIR's dialect conversion framework, but is shaped around HIR's Rust
implementation: typed operation structs, runtime operation trait metadata,
arena-backed intrusive IR entities, the existing `Rewriter` API, and the pass
manager.

The immediate use case is making code generation stricter and more extensible:
frontends and higher-level compiler components should be free to introduce new
dialects, while code generation should require the IR to be legalized to a known
target set before lowering to Miden Assembly.

## Goals

- Define a target legality model that can mark whole dialects, individual
  operations, dynamically legal subsets of operations, and interface-based sets
  of operations as legal.
- Support explicit illegal dialects and operations, so high-level dialects can
  be rejected unless conversion patterns legalize them.
- Support transitive legalization. If a pattern legalizes `A` to `B`, and
  another pattern legalizes `B` to `C`, then a target that accepts `C` can
  legalize `A` without an `A` to `C` pattern.
- Express op conversion as rewrite patterns that use the same mental model as
  existing HIR patterns, but with conversion-specific support for remapped
  operands, type conversion, and legalization diagnostics.
- Preserve HIR invariants during conversion: SSA dominance, type-correct
  operands and results, valid block successor operands, symbol table integrity,
  and verifier success after the pass.
- Integrate with MASM lowering so unsupported high-level operations fail before
  codegen, with clear diagnostics.

## Non-Goals

- Do not implement MLIR's rollback mode. Conversion patterns must do all
  fallible matching before mutation. If mutation starts and later conversion
  fails, the pass reports a compiler error instead of restoring old IR.
- Do not require dialect authors to use a TableGen-like declaration language.
  Registration should use normal Rust traits, derive macros, and inventory-style
  providers consistent with the rest of HIR.
- Do not replace canonicalization. Canonicalization remains best-effort
  simplification. Dialect conversion is target-driven legalization.
- Do not make MASM lowering itself responsible for discovering conversion paths.
  MASM lowering should receive already-legal IR.

## Background

MLIR dialect conversion is built around four separable pieces:

- A conversion target that defines legal, dynamically legal, illegal, and
  unknown operations.
- Rewrite patterns that legalize illegal operations.
- An operation legalization graph derived from pattern root operations and
  generated operations.
- An optional type converter that maps source types to target types and inserts
  materializations when converted and unconverted IR must interact.

HIR already has much of the underlying machinery:

- Dialects expose registered operations through `DialectInfo`.
- Operations carry an `OperationName` with dialect, opcode, properties, and
  runtime trait metadata.
- Dialect registration hooks can late-bind traits to operations, as MASM
  lowering already does for `dyn HirLowering`.
- Rewrite patterns already have roots, benefits, bounded recursion flags, and
  generated-op metadata.
- `Rewriter` already supports operation replacement, erasure, moving blocks and
  operations, and use replacement.
- `builtin.unrealized_conversion_cast` already exists and is a natural
  materialization operation for temporary type bridges.

The proposed framework extends those mechanisms instead of introducing a
parallel IR transformation system.

## Crate Layout

Core conversion APIs should live in `midenc-hir`:

```text
hir/src/conversion.rs
hir/src/conversion/target.rs
hir/src/conversion/pattern.rs
hir/src/conversion/pattern_set.rs
hir/src/conversion/rewriter.rs
hir/src/conversion/type_converter.rs
hir/src/conversion/driver.rs
hir/src/conversion/diagnostics.rs
```

Reusable pass adapters or test-only plumbing may live in `midenc-hir-transform`,
beside canonicalization and other reusable transforms:

```text
hir-transform/src/dialect_conversion.rs
```

Concrete target conversion passes should live with their target:

```text
codegen/masm/src/legalization.rs
```

This keeps the reusable mechanics in HIR and keeps target ownership with the
backend that knows the legal dialect subset. The production MASM pass should be
defined in `midenc-codegen-masm`, because it depends on MASM-specific policy and
on `dyn HirLowering`, which is backend-owned lowering metadata.

## Conversion Target

`ConversionTarget` defines whether an operation instance is acceptable for a
given conversion.

```rust,ignore
pub struct ConversionTarget {
    context: Rc<Context>,
    unknown_op_policy: UnknownOpPolicy,
    dialect_actions: FxHashMap<Symbol, LegalityRule>,
    op_actions: BTreeMap<OperationName, LegalityRule>,
    interface_actions: Vec<InterfaceLegalityRule>,
    recursive_legality: BTreeMap<OperationName, RecursiveLegalityRule>,
}

pub enum Legality {
    Legal,
    Illegal,
    Unknown,
    DynamicLegal,
    DynamicIllegal,
}

pub enum StaticLegality {
    Legal,
    Illegal,
    Unknown,
    Dynamic,
}

pub enum UnknownOpPolicy {
    Legal,
    Illegal,
    Dynamic(Rc<dyn Fn(&Operation) -> DynamicLegalityResult>),
}

pub enum DynamicLegalityResult {
    Legal,
    Illegal { reason: Option<Report> },
}
```

`LegalityRule` stores the static action and, for dynamic legality, a callback:

```rust,ignore
pub enum LegalityRule {
    Legal,
    Illegal,
    Dynamic(Rc<dyn Fn(&Operation) -> DynamicLegalityResult>),
}
```

The target API should include:

```rust,ignore
impl ConversionTarget {
    pub fn new(context: Rc<Context>) -> Self;

    pub fn set_unknown_op_policy(&mut self, policy: UnknownOpPolicy) -> &mut Self;

    pub fn add_legal_dialect<D: DialectRegistration>(&mut self) -> &mut Self;
    pub fn add_illegal_dialect<D: DialectRegistration>(&mut self) -> &mut Self;
    pub fn add_dynamically_legal_dialect<D, F>(&mut self, callback: F) -> &mut Self
    where
        D: DialectRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static;
    pub fn add_dynamically_legal_dialect_if_op_interface<D, Trait>(&mut self) -> &mut Self
    where
        D: DialectRegistration,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static;

    pub fn add_legal_op<Op: OpRegistration>(&mut self) -> &mut Self;
    pub fn add_illegal_op<Op: OpRegistration>(&mut self) -> &mut Self;
    pub fn add_dynamically_legal_op<Op, F>(&mut self, callback: F) -> &mut Self
    where
        Op: OpRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static;

    pub fn add_legal_op_interface<Trait>(&mut self) -> &mut Self
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static;
    pub fn add_dynamically_legal_op_interface<Trait, F>(&mut self, callback: F) -> &mut Self
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static;

    pub fn mark_op_recursively_legal<Op: OpRegistration>(&mut self) -> &mut Self;
    pub fn mark_op_recursively_legal_if<Op, F>(&mut self, callback: F) -> &mut Self
    where
        Op: OpRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static;

    pub fn legality(&self, op: &Operation) -> Legality;
    pub fn is_legal(&self, op: &Operation) -> bool;
    pub fn is_recursively_legal(&self, op: &Operation) -> bool;
}
```

Legality precedence:

1. Operation-specific rules.
2. Dialect-specific rules.
3. Interface rules.
4. Unknown operation policy.

Operation-specific rules override dialect rules. Dialect rules override broad
interface rules. An explicit illegal operation inside an otherwise legal dialect
must still be converted, and an explicit illegal dialect must not be bypassed by
a broad interface rule.

Targets that support only part of a dialect should express that as dynamic
legality on the dialect or operation, using operation metadata as needed. For
example, a MASM target can mark a dialect dynamically legal only for operations
whose metadata says they implement `dyn HirLowering`, plus any target-specific
type or shape restrictions. Broad `add_legal_op_interface::<dyn HirLowering>()`
is useful for tests and exploratory targets, but final codegen targets should
prefer explicit dialect/op policy so they do not accidentally accept an excluded
dialect.

Dynamic legality is checked on operation instances, not just names. This allows
rules such as:

- `arith.add` is legal only for MASM-supported integer widths.
- A dialect is legal only for operations that implement `dyn HirLowering`.
- `cf.cond_br` is legal only in the limited form still accepted by MASM
  lowering.

### Recursive Legality

If an operation is recursively legal, all nested operations are considered legal
for the purposes of the current conversion. This is useful for operations that
encapsulate a region with separate semantics, or for staged conversion when the
parent operation owns later lowering.

Recursive legality is only valid if the operation itself is legal or dynamically
legal for the target. The driver should assert this at target construction time
when possible, and diagnose it during conversion otherwise.

## Conversion Modes

The framework should support three modes, though MASM should use full
conversion.

```rust,ignore
pub enum ConversionMode {
    Full,
    Partial,
    Analysis,
}
```

Full conversion succeeds only if every operation under the selected roots is
legal after conversion. Unknown operations are legal only if the target says so.

Partial conversion legalizes every operation explicitly marked illegal and every
operation that has an available legalizing path, but can leave unknown
operations in place if they are not explicitly illegal. This is useful for
incremental compiler development and optional dialect lowering.

Analysis conversion computes what would be legalizable without mutating IR. It
should report the set of illegal, legal, and legalizable operations. This is a
debugging and planning aid; it is not required by MASM lowering.

## Conversion Patterns

Conversion patterns are rewrite patterns with target-aware semantics.

```rust,ignore
pub trait ConversionPattern: Pattern {
    fn type_converter(&self) -> Option<&TypeConverter> {
        None
    }

    fn match_and_rewrite(
        &self,
        op: OperationRef,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}
```

`ConvertedOperands` contains the current replacement values for each original
operand. When a pattern has a type converter, these operands have already been
converted or materialized to the types requested by that converter.

```rust,ignore
pub struct ConvertedOperands<'a> {
    groups: &'a [SmallVec<[ValueRef; 2]>],
}
```

Most typed patterns should use an adapter:

```rust,ignore
pub trait OpConversionPattern<T: Op>: ConversionPattern {
    fn match_and_rewrite_typed(
        &self,
        op: UnsafeIntrusiveEntityRef<T>,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}
```

`Ok(false)` means the pattern did not match and no mutation occurred. Patterns
should report actionable non-match reasons through a conversion equivalent of
`notify_match_failure`, so diagnostics can explain why each candidate was
rejected. `Err(report)` remains reserved for internal compiler errors or
malformed IR, not ordinary match failure.

Pattern metadata must declare generated operations:

```rust,ignore
let mut info = PatternInfo::new(...);
info.with_generated_ops([arith::Add::name(), scf::If::name()]);
```

HIR's existing `PatternInfo` already has `generated_ops` storage; the conversion
work should add a small builder/helper API for populating it. The legalization
graph depends on this declaration. If a pattern creates an operation not listed
in `generated_ops`, debug builds should report an internal compiler error.
Release builds should still legalize the created operation recursively, but
diagnostics should make the missing metadata obvious.

### Pattern Mutation Contract

Because v1 has no rollback, conversion patterns must follow a strict contract:

- Do all fallible matching before mutating IR.
- Once mutation starts, any later failure is a compiler error, not a normal
  pattern miss.
- Return `Ok(false)` only before mutation.
- Use `ConversionPatternRewriter` for all mutation so created, modified, and
  replaced operations are tracked.

This mirrors how most HIR rewrites should already be written, but conversion
must enforce it more tightly because failed legalization after mutation leaves
the IR in an intermediate state.

## Pattern Registration

Canonicalization patterns are attached to operations. Dialect conversion needs a
target/source-oriented provider mechanism, because conversion patterns are often
owned by the destination dialect or backend rather than the source operation.

```rust,ignore
pub trait ConversionPatternProvider {
    fn name(&self) -> &'static str;
    fn source_dialect(&self) -> Option<Symbol>;
    fn target_dialects(&self) -> &'static [&'static str];
    fn populate(&self, context: Rc<Context>, patterns: &mut ConversionPatternSet);
}
```

Providers should be registered through `inventory`, like dialects and passes.

The conversion pass gathers all providers linked into the current binary. The
legalization graph then filters patterns to those useful for the requested
target, so registering more providers does not force their use.

Manual population should also be supported for tests and specialized pipelines:

```rust,ignore
let mut patterns = ConversionPatternSet::new(context.clone());
populate_cf_to_scf_patterns(context.clone(), &mut patterns);
apply_full_conversion(root, target, patterns)?;
```

## Legalization Graph

The driver builds a legalization graph from pattern metadata. The precise graph
nodes are operation names, because final legality is queried on concrete
operation instances. A pattern contributes an edge from each possible root
operation to every operation it may generate.

A root operation is legalizable if:

- It is already legal according to the target, or
- There exists at least one pattern for that root such that every generated op
  is legal or legalizable.

HIR patterns may be rooted on a concrete operation, a trait, or `Any`.
Operation-rooted patterns give the graph the most precision. Trait-rooted
patterns should be expanded to registered operations that implement the trait
when the context has enough metadata; otherwise they must be treated like
conservative any-op patterns.

Dynamic legality is instance-specific, so graph construction cannot prove a
dynamic operation name is always legal. It should still treat dynamically legal
operation names as conditional terminal nodes when deciding whether a path might
exist, then require the concrete generated operation instance to pass dynamic
legality after the rewrite. This prevents the graph from pruning useful paths
just because the terminal legality depends on operation attributes or types.

Algorithm sketch:

```text
for each pattern:
    if pattern.root is Any:
        add to any-op patterns
        continue

    roots = expand operation root or trait root to operation names
    if roots cannot be expanded:
        add to any-op patterns
        continue

    for root in roots:
        if target says root is statically legal or conditionally legal:
            skip pattern for this root
            continue

        invalid[root].insert(pattern)
        for generated in pattern.generated_ops:
            parents[generated].insert(root)
        worklist.insert(pattern)

if any-op patterns exist:
    conservatively keep all rooted patterns
else:
    while worklist not empty:
        pattern = worklist.pop()
        if any generated op is statically illegal or unknown and not legalizable:
            continue
        if any generated op is conditionally legal:
            keep pattern, but require runtime legality check after rewrite
        legalizer_patterns[root].push(pattern)
        invalid[root].remove(pattern)
        for parent in parents[root]:
            worklist.extend(invalid[parent])

for each legalizer pattern list:
    compute legalization depth recursively
    sort by smaller depth, then higher PatternBenefit
```

This matches the behavior we want from MLIR: an `A -> B` pattern becomes useful
when `B` is itself legalizable, not only when `B` is directly legal.

Patterns with `PatternKind::Trait` and `PatternKind::Any` are useful but reduce
static pruning precision when they cannot be expanded to concrete root
operations. They should be allowed, but target-specific conversion libraries
should prefer operation-specific roots whenever possible.

## Conversion Driver

The core driver is target-driven, not greedy. It walks the IR and legalizes
operations that are not legal for the target.

```rust,ignore
pub fn apply_full_conversion(
    root: OperationRef,
    target: ConversionTarget,
    patterns: ConversionPatternSet,
    config: ConversionConfig,
) -> Result<ConversionResult, Report>;
```

`ConversionConfig`:

```rust,ignore
pub struct ConversionConfig {
    pub mode: ConversionMode,
    pub reconcile_unrealized_casts: bool,
    pub max_iterations: Option<NonZeroU32>,
    pub listener: Option<Rc<dyn ConversionListener>>,
    pub verify_after_conversion: bool,
}
```

Driver behavior:

1. Freeze pattern set and build the legalization graph.
2. Walk each root in preorder.
3. If an op is recursively legal, skip its nested regions.
4. If an op is legal and needs no type conversion, continue.
5. If an op is illegal or dynamically illegal, try conversion patterns in
   computed legalization order.
6. Before invoking a pattern, build converted operands from the current value
   mapping and the pattern's type converter.
7. After a pattern succeeds, recursively legalize created and modified
   operations.
8. If no pattern applies, emit a diagnostic explaining the failed target and any
   known candidate patterns.
9. Reconcile redundant `builtin.unrealized_conversion_cast` operations if
   configured.
10. Run verification if configured.

The driver should use preorder traversal, because parent operations often own
regions whose signatures or semantics must be converted before nested operations
can be interpreted correctly.

## Conversion Rewriter

`ConversionPatternRewriter` wraps the existing `Rewriter` and records
conversion-specific state.

```rust,ignore
pub struct ConversionPatternRewriter {
    inner: PatternRewriter<ConversionListenerAdapter>,
    value_mapping: ValueMapping,
    created_ops: SmallSet<OperationRef, 8>,
    modified_ops: SmallSet<OperationRef, 8>,
    replaced_ops: SmallSet<OperationRef, 8>,
    current_type_converter: Option<Rc<TypeConverter>>,
}
```

Required APIs:

```rust,ignore
impl ConversionPatternRewriter {
    pub fn get_remapped_values(&self, value: ValueRef) -> SmallVec<[ValueRef; 2]>;

    pub fn notify_match_failure(
        &mut self,
        op: OperationRef,
        reason: impl Into<Report>,
    );

    pub fn replace_op(
        &mut self,
        op: OperationRef,
        replacement_values: &[ValueRef],
    ) -> Result<(), Report>;

    pub fn replace_op_with_new_op<T, Args>(
        &mut self,
        op: OperationRef,
        span: SourceSpan,
        args: Args,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, Report>
    where
        T: BuildableOp<Args>;

    pub fn erase_op(&mut self, op: OperationRef) -> Result<(), Report>;

    pub fn convert_region_types(
        &mut self,
        region: RegionRef,
        converter: &TypeConverter,
        entry: Option<SignatureConversion>,
    ) -> Result<(), Report>;

    pub fn apply_signature_conversion(
        &mut self,
        block: BlockRef,
        converter: &TypeConverter,
        signature: SignatureConversion,
    ) -> Result<BlockRef, Report>;
}
```

The conversion rewriter must own all IR mutation during conversion. It should
use the existing rewriter listener hooks to track created, modified, replaced,
and erased operations, but it should not expose a mutable inner `Rewriter` that
patterns can use to bypass value mapping or generated-op tracking. If a generic
rewriter API is needed inside conversion, it should be re-exposed through
conversion-aware wrappers.

### Value Mapping

The rewriter maps each original value to zero, one, or many replacement values:

```rust,ignore
pub struct ValueMapping {
    replacements: FxHashMap<ValueRef, SmallVec<[ValueRef; 2]>>,
}
```

Replacement with zero values is valid only when all live uses are also removed
or remapped. This is required for zero-sized type lowering and dead value
elimination, but should be diagnosed aggressively when stale uses remain.

When an operation is replaced, the rewriter updates:

- Uses that are already in converted IR.
- The conversion value mapping.
- Source materializations for users that remain unconverted and still expect
  original types.

## Type Conversion

Type conversion is optional per pattern. Patterns that do not need type changes
can leave the type converter unset and receive the latest remapped values
without materializations.

```rust,ignore
pub struct TypeConverter {
    conversions: Vec<TypeConversionFn>,
    value_conversions: Vec<ValueConversionFn>,
    source_materializations: Vec<MaterializationFn>,
    target_materializations: Vec<MaterializationFn>,
}

pub enum TypeConversion {
    One(Type),
    Many(SmallVec<[Type; 2]>),
    Drop,
}
```

The converter should support both context-free type conversion and
context-aware value conversion:

```rust,ignore
pub fn convert_type(&self, ty: &Type) -> Option<SmallVec<[Type; 2]>>;
pub fn convert_value(&self, value: ValueRef) -> Option<SmallVec<[Type; 2]>>;
pub fn is_legal_type(&self, ty: &Type) -> bool;
```

The first production implementation should focus on 1:1 type conversion. The
API should leave room for 1:N and drop conversions, but those are not required
for the initial MASM legalization path unless a concrete lowering needs them.

1:N conversion becomes necessary when one source SSA value must be represented
by multiple target SSA values, such as aggregate flattening, ABI lowering of
records/tuples/results into scalar components, fat-pointer lowering, or
multi-limb value lowering. Drop conversion is the 1:0 case, such as removing a
zero-sized marker value. Until such a use case is implemented, the driver may
reject `Many` and `Drop` conversions that require boundary materialization.

### Materialization

Materializations bridge converted and unconverted IR.

- Target materialization converts a value to the type expected by a conversion
  pattern.
- Source materialization converts a replacement value back to the original type
  for users that have not been converted.

Materialization callbacks can build real operations. If no callback succeeds,
the default is to build `builtin.unrealized_conversion_cast` only for 1:1
materializations. The current builtin cast is unary and single-result; 1:N or
drop materializations require either target-specific real operations or a future
variadic/multi-result cast operation. If neither exists, conversion must fail
with a type materialization diagnostic rather than silently fabricating an
invalid cast.

`builtin.unrealized_conversion_cast` is an intermediate bridge, not a legal
final operation for production full conversion. The conversion driver may create
it temporarily to keep IR type-correct while adjacent operations are being
converted. After conversion, those casts must be realized or reconciled before
the final target legality check.

Reconciliation should:

- Erase identity casts.
- Collapse cast chains when source and final destination types match.
- Replace uses with the original value when possible.
- Fail if a cast remains in a full conversion target that does not explicitly
  permit temporary casts.

For MASM codegen, unreconciled unrealized casts must be illegal. Same VM stack
shape is not enough to prove semantic compatibility, because it can erase
value-range or ABI facts such as `felt` versus `u32`. If a conversion needs such
a bridge, it must be realized as a real checked cast or eliminated before
`ToMasmComponent`.

## Region and Block Signature Conversion

Block arguments require explicit handling because they participate in CFG edges.

```rust,ignore
pub struct SignatureConversion {
    converted_types: SmallVec<[Type; 4]>,
    mappings: Vec<InputMapping>,
}

pub enum InputMapping {
    Keep {
        old_index: usize,
        new_index: usize,
        new_count: usize,
    },
    Replace {
        old_index: usize,
        values: SmallVec<[ValueRef; 2]>,
    },
    Drop {
        old_index: usize,
    },
}
```

For simple 1:1 type changes, the implementation may update block argument types
in place if all uses can be safely remapped.

When future 1:N or drop argument conversion support is added, the
implementation should create a replacement block with the converted signature,
move operations into it, replace block uses, and erase the old block once all
argument uses have been replaced. This mirrors MLIR's conservative approach and
fits HIR's existing block move APIs.

Successor operands must be rewritten with the same signature conversion. This
requires using `BranchOpInterface` for unstructured branches and
`RegionBranchOpInterface`/`RegionBranchTerminatorOpInterface` for structured
region control flow.

The conversion driver must not assume that generic branch helpers can handle
changed argument counts. For each terminator or region branch operation whose
successor signature changes, conversion must either use an operation-specific
rewrite that rebuilds the successor operand list, or call a trait method that is
explicitly capable of changing successor operand arity. The driver should run
independent structural checks after signature conversion:

- Every successor operand list has the same arity as the destination block
  arguments after conversion.
- Forwarded and produced successor operands still match the operation's
  interface contract.
- Region-return operands match the parent operation result or continuation
  signature.

Function-like operations need dialect-specific conversion patterns because the
function signature attribute, entry block arguments, symbol uses, and call sites
must agree. The framework should provide utilities, but not assume every
callable has the same signature representation.

Function-like conversion utilities should cover:

- Updating the function type/signature attribute.
- Applying entry block argument conversion.
- Rewriting direct call operands and result uses.
- Updating declarations and external ABI metadata.
- Preserving or updating symbol table entries and callee symbol references.
- Diagnosing unresolved indirect-call or function-pointer cases that the target
  does not support.

## MASM Legalization Target

MASM lowering should run a full conversion before `ToMasmComponent`.

The target should be stricter than "these dialects are allowed". It should
accept only operations that are supported by MASM lowering and satisfy any
dynamic type/shape restrictions.

Recommended policy:

```rust,ignore
pub fn populate_masm_conversion_target(target: &mut ConversionTarget) {
    target.add_dynamically_legal_dialect::<arith::ArithDialect>(is_masm_lowerable);
    target.add_dynamically_legal_dialect::<builtin::BuiltinDialect>(is_masm_lowerable);
    target.add_dynamically_legal_dialect::<hir::HirDialect>(is_masm_lowerable);
    target.add_dynamically_legal_dialect::<scf::ScfDialect>(is_masm_lowerable);

    target.add_dynamically_legal_dialect::<cf::ControlFlowDialect>(|op| {
        legal_if(op.implements::<dyn HirLowering>() && is_intentionally_supported_cf_form(op))
    });

    target.add_illegal_op::<builtin::UnrealizedConversionCast>();
    target.add_illegal_dialect::<wasm::WasmDialect>();
}

fn is_masm_lowerable(op: &Operation) -> DynamicLegalityResult {
    legal_if(op.implements::<dyn HirLowering>() && satisfies_masm_dynamic_constraints(op))
}

fn legal_if(condition: bool) -> DynamicLegalityResult {
    if condition {
        DynamicLegalityResult::Legal
    } else {
        DynamicLegalityResult::Illegal { reason: None }
    }
}
```

The exact dialect and operation lists should be explicit. `dyn HirLowering`
guards codegen support, but only as a predicate inside MASM's dialect/op
legality rules. It should not be installed as a blanket interface legality rule
for the final MASM target, because that would allow excluded dialects to reach
codegen just because they happen to have lowering code registered.

`cf` should become illegal once control-flow lifting has run, except for any
temporary edge cases that MASM lowering still intentionally supports. Those edge
cases should be operation-specific dynamic legality rules, not blanket dialect
legality.

`builtin.unrealized_conversion_cast` should be illegal for the final MASM
target. Existing lowering support for it should be treated as a defensive
diagnostic path while the legalizer is being introduced, not as evidence that
unrealized casts are acceptable after full conversion.

## Diagnostics

Conversion errors must identify both the illegal operation and the missing path.

Diagnostics should include:

- Operation name and source span.
- Whether the operation is explicitly illegal, dynamically illegal, or unknown.
- The reason returned by dynamic legality callbacks, when present.
- Candidate patterns considered for the operation.
- For each candidate that failed before mutation, the match failure reason when
  available.
- For graph-level rejection, the generated operation that blocks legalization.
- For type conversion failures, the original type and the converter/pattern that
  requested conversion.

Dynamic legality callbacks and conversion patterns should be able to report
structured failure reasons. A boolean-only predicate is acceptable as a
convenience adapter, but the core driver should preserve richer reasons so
failed full conversion does not degrade into "pattern did not match" without
actionable context.

Example:

```text
error: failed to legalize operation `foo.matmul`
  reason: target `masm` marks dialect `foo` illegal
  candidate patterns:
    foo.matmul-to-linalg.matmul rejected: generated op `linalg.matmul` has no path to target
    foo.matmul-to-hir-loop rejected: operand #2 type `tensor<4x4xfelt>` cannot be converted
```

The debug log should mirror the conversion tree:

```text
legalizing foo.matmul
  pattern foo.matmul-to-hir-loop
    created scf.for
    created arith.add
    legalizing scf.for: legal
    legalizing arith.add: legal
  success
```

## Safety and Invariants

After successful full conversion:

- Every operation under the converted root is legal or nested under a
  recursively legal operation.
- No operation explicitly marked illegal remains.
- All operation operands have values with the expected types.
- All block successor operands match destination block argument lists.
- All value replacements dominate their uses.
- No unreconciled `builtin.unrealized_conversion_cast` remains in production
  full conversion.
- The standard verifier succeeds.

Debug builds should add additional checks:

- A successful pattern must replace, erase, or mark the root operation modified
  in place.
- Created operations must be listed in the pattern's `generated_ops`.
- Patterns must not return `Ok(false)` after mutation begins.
- `replace_op` replacement arity must match converted result mapping.
- Signature conversions must account for every original block argument.

## Testing Strategy

Unit tests in `midenc-hir`:

- Static legal dialect, op, interface, illegal override, unknown policy.
- Dynamic legality for specific operation instances.
- Recursive legality skips nested illegal operations.
- Legalization graph resolves `A -> B -> C`.
- Legalization graph rejects `A -> B` when `B` has no legal path.
- Any-op pattern behavior is conservative but usable.
- Generated-op metadata checking catches undeclared created ops in debug tests.

Conversion driver tests:

- Full conversion succeeds when all illegal ops are replaced.
- Full conversion fails on unknown/illegal ops with no path.
- Partial conversion leaves unknown ops but converts explicit illegal ops.
- Analysis conversion reports legalizable and non-legalizable ops without
  mutation.
- Pattern ordering prefers shorter legalization depth before pattern benefit.

Type conversion tests:

- 1:1 type conversion remaps operands.
- 1:N and drop conversions are rejected cleanly when they would require
  unsupported default materialization.
- Target materialization is inserted for converted operands.
- Source materialization is inserted for unconverted users.
- Unrealized conversion casts are reconciled.
- Future 1:N/drop support replaces operation results with multiple values and
  fails if stale live uses remain.

Region/signature tests:

- Entry block argument type conversion.
- Non-entry block argument conversion.
- Successor operand rewriting for `cf.br` and `cf.cond_br`.
- Region branch operation conversion for `scf.if`/`scf.while` where needed.

MASM integration tests:

- A high-level test dialect op fails before codegen without a conversion path.
- A high-level test dialect op legalizes transitively to MASM-supported ops.
- Unsupported `wasm` ops fail before codegen unless converted.
- Any unreconciled unrealized cast fails before codegen.

## Implementation Plan

### Phase 1: Core Legality Model

- Add `hir::conversion` module skeleton and public re-exports.
- Implement `ConversionTarget`, `LegalityRule`, `Legality`, and
  `UnknownOpPolicy`.
- Add tests for dialect/op/interface/dynamic legality and override precedence.
- Add recursive legality bookkeeping and tests.

Deliverable: legality can be queried for operation instances without running
rewrites.

### Phase 2: Conversion Pattern Infrastructure

- Add `ConversionPattern`, `OpConversionPattern<T>`,
  `ConversionPatternSet`, and `FrozenConversionPatternSet`.
- Reuse or extend existing `PatternInfo` for generated-op metadata.
- Normalize root inspection around `Pattern::kind()`/`PatternInfo::root_trait`;
  fix any existing helper mismatch before relying on trait-rooted conversion
  patterns.
- Add inventory-based `ConversionPatternProvider`.
- Add tests for provider population and frozen pattern indexing.

Deliverable: conversion patterns can be registered and grouped by root
operation.

### Phase 3: Legalization Graph

- Implement graph construction from target and frozen pattern set.
- Compute legalizable roots and pattern legalization depth.
- Apply cost model: shorter legalization depth, then higher pattern benefit.
- Add graph-only tests for direct, transitive, missing, and cyclic paths.

Deliverable: the framework can decide which patterns can lead to the target
before mutating IR.

### Phase 4: Basic Conversion Driver Without Type Conversion

- Implement `ConversionPatternRewriter` as a wrapper around existing
  `PatternRewriter`.
- Track created, modified, replaced, and erased operations.
- Implement full conversion for same-type replacements.
- Enforce no-rollback mutation contract in debug builds.
- Add driver tests with a small test dialect converting `test_a` to `test_b` to
  a target-legal op.

Deliverable: full conversion works for op-to-op and op-to-sequence rewrites
where value types do not change.

### Phase 5: Type Converter and Materialization

- Add `TypeConverter`, conversion callbacks, value mapping, and converted
  operand adaptors.
- Implement target/source materialization with
  `builtin.unrealized_conversion_cast` fallback for 1:1 conversions only.
- Implement cast reconciliation.
- Add tests for 1:1 conversions, rejected unsupported 1:N/drop
  materializations, and failed type conversions.

Deliverable: conversion patterns can safely consume operands and produce
results with 1:1 changed types. 1:N/drop support is reserved for a later phase
unless a concrete MASM legalization use case requires it.

### Phase 6: Region and Signature Conversion

- Implement `SignatureConversion`.
- Implement block signature conversion for 1:1 argument type changes.
- Add utilities for rewriting branch successor operands through
  `BranchOpInterface`.
- Add hooks/utilities for region branch operations where required.
- Add function-like conversion helpers, but keep callable dialect semantics in
  dialect-specific patterns.

Deliverable: conversions can change block argument and region signatures.

### Phase 7: Reusable Driver Integration

- Add helper APIs:

```rust,ignore
apply_full_conversion(root, target, patterns, config)
apply_partial_conversion(root, target, patterns, config)
analyze_conversion(root, target, patterns, config)
```

- Add optional reusable pass adapter support in `midenc-hir-transform` only for
  callers that provide a concrete `ConversionTarget` and pattern population
  function directly.
- Do not require a global target-name registry for production use in v1.
- Wire verifier execution after successful conversion.

Deliverable: conversion is usable from concrete passes, pipelines, and tests
without baking target policy into the generic infrastructure.

### Phase 8: MASM Legalization

- Add MASM conversion target construction in `codegen/masm`.
- Add a MASM-owned `LegalizeForMasm` pass in `midenc-codegen-masm`.
- Populate MASM-specific dynamic legality rules.
- Add `LegalizeForMasm` immediately before MASM lowering in the compiler
  pipeline. It should be the final legalization gate before `ToMasmComponent`,
  after higher-level canonicalization and dialect-lowering passes have run.
- Convert existing ad hoc pre-codegen legality assumptions into target rules.
- Replace lowering panics/assertions for unsupported IR with legalization
  diagnostics where possible.
- Add integration tests for successful and failing legalization.

Deliverable: MASM lowering receives legal IR or a conversion diagnostic.

### Phase 9: Developer Experience

- Improve diagnostics with conversion trace output.
- Add debug logging target, likely `dialect-conversion`.
- Document how dialect authors register conversion providers.
- Add examples using a test dialect and a realistic `cf`/`scf` or `wasm`/HIR
  conversion.

Deliverable: new dialect authors can define and debug conversions without
reading the driver implementation.

## Review Points

- Full conversion should be the first production mode. Partial and analysis
  modes are useful enough to include in the API, but can land after the full
  conversion path if implementation needs to be staged.
- Interface-based predicates should be supported, but MASM should use them
  inside explicit dialect/op policy rather than blanket interface legality.
- 1:N/drop type conversion is not required for v1 MASM legalization unless a
  concrete lowering needs it. The API may reserve space for it, but default
  materialization is 1:1 until a variadic/multi-result bridge exists.
- `builtin.unrealized_conversion_cast` is temporary conversion scaffolding and
  should be illegal after production full conversion.
- The MASM legalization pass is owned by `midenc-codegen-masm`; generic
  dialect conversion infrastructure should not depend on MASM target policy.
- The no-rollback contract is accepted for v1. If a future conversion use case
  needs speculative mutation, it should be designed as a separate transaction
  layer rather than retrofitted into the initial driver.
