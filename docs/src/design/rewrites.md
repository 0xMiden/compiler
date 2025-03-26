# Rewrites

This document provides an overview of some of the current transformation/rewrite passes the compiler uses when lowering from the frontend to Miden Assembly. This is not guaranteed to be comprehensive, but mostly meant as a high-level reference to what rewrites exist and what they acheive.

Most rewrite passes, at the time of writing, are maintained in the `midenc-hir-transform` crate, with the exception of those which are either dialect-specific (i.e. canonicalization, or reliant on dialect-aware interfaces), or part of the core `midenc-hir` crate (i.e. region simplification, folding).

* [Region Simplification](#region-simplification)
* [Folding](#folding)
* [Canonicalization](#canonicalization)
* [Sparse Conditional Constant Propagation](#sparse-conditional-constant-propagation)
* [Unstructured to Structured Control Flow Lifting](#control-flow-lifting)
* [Control Flow Sinking](#control-flow-sinking)
* [Spills](#spills)


## Region Simplification

Region simplification is a region-local transformation which:

* Removes redundant block arguments
* Merges identical blocks
* Removes unused/dead code

This transformation is exposed from the `Region` type, but is considered the responsibility of the `GreedyPatternRewriteDriver` to apply, based on the current compiler configuration.

> [!NOTE]
> Currently, the block merging portion of region simplification is only stubbed out, and is not actually being performed. It will be incorporated in the future.

## Folding

Folding, or _constant folding_ to be more precise, is the process by which an operation is simplified, or even reduced to a constant, when some or all of its operands have known constant values.

Operations which can be folded must implement the `Foldable` trait, and implement the folding logic there. Folds can produce three outcomes, represented via `FoldResult`:

* `Ok`, indicating that the operation was able to be reduced to a (possibly constant) value or set of values.
* `InPlace`, indicating that the operation was able to rewrite itself into a simpler form, but could not be completely folded. This is commonly the case when only a subset of the operands are known-constant.
* `Failed`, indicating that the operation could not be folded or simplified at all.

Folding can be done at any time, but similar to region simplification, is largely delegated to the `GreedyPatternRewriteDriver` to apply as part of canonicalization.

## Canonicalization

Canonicalization refers to rewriting an operation such that it is in its _canonical form_. An operation can have several canonicalization patterns that can be applied, some even stack. It must be the case that these canonicalizations _converge_, i.e. you must never define two separate canonicalizations that could try to rewrite an operation in opposing ways, they must either not overlap, or converge to a fixpoint.

An operation that has at least one defined canonicalization pattern, must implement the `Canonicalizable` trait, and implement the `register_canonicalization_patterns` method to insert those rewrite patterns into the provided `RewriteSet`.

Canonicalization is performed using the `Canonicalizer` pass, provided by the `midenc_hir_transform` crate. Internally, this configures and runs the `GreedyPatternRewriteDriver` to not only apply all canonicalization patterns until fixpoint, but also constant folding and region simplification (depending on configuration).

This is the primary means by which the IR produced by a frontend is simplified and prepared for further transformation by the compiler.

## Sparse Conditional Constant Propagation

This pass applies the results of the _sparse constant propagation_ analysis described in [_Analyses_](analyses.md), by rewriting the IR to materialize constant values, replace operations that were reduced to constants, and of particular note, prune unreachable blocks/regions from the IR based on control flow that was resolved to a specific target or set of targets based on constant operands.

## Control Flow Lifting

This pass is responsible for converting unstructured control flow, represented via the `cf` dialect, into structured equivalents provided by the `scf` dialect.

Because some forms of unstructured control flow cannot be fully converted into structured equivalents, this process is called "lifting" rather than "conversion".

As an example, here's what it looks like to lift an unstructured conditional branch to a `scf.if`:

* Before:
```
^block0(v0: i1, v1: u32, v2: u32):
    cf.cond_br v0, ^block1(v1), ^block2(v2);

^block1(v3: u32):
    cf.br ^block3(v3);

^block2(v4: u32):
    v5 = arith.constant 1 : u32;
    v6 = arith.add v4, v5;
    cf.br ^block3(v6);

^block3(v7: u32):
    builtin.ret v7
```

* After:
```
v8 = scf.if v0 {
    scf.yield v1
} else {
    v5 = arith.constant 1 : u32;
    v6 = arith.add v2, v5;
    scf.yield v6
};
builtin.ret v8
```

The above transformation is the simplest possible example. In practice, the transformation is much more involved, as it must handle control flow that exits from arbitrarily deep nesting (e.g. a `builtin.ret` within the body of a loop, guarded by a conditional branch). The specific details of the transformation in general are described in detail in the module documentation of `midenc_hir_transform::cfg_to_scf`.

This transformation is a prerequisite for the generation of Miden Assembly, which provides only structured control flow primitives.

## Control Flow Sinking

This actually refers to two separate passes, but both are duals of the same goal, which is to move operations closer to their uses, thus reducing the amount of computation that is performed on code paths where the result of that computation is not used.

The `ControlFlowSink` pass is generic, and will perform the transformation described above, so long as the operation has no side effects, is not a block terminator, and has no regions.

The `SinkOperandDefs` pass (which will be renamed in the near future), is designed specifically to move constant-like operations directly before their uses, and materialize copies if necessary so that each user gets its own copy. We do this to counter the effect of the control flow lifting transform and the canonicalizer, which both materialize constants in the entry block of a region. These constants then have overly broad live ranges that introduce a high likelihood of needing to spill values to memory. Furthermore, because the Miden VM is a stack machine, not a register machine, there is very little benefit to sharing constant definitions. Instead, by materializing constants immediately before they are used, we produce much more efficient code (as we do not need to shuffle the operand stack to access constants previously defined), and we significantly reduce the chances that we will need to spill values to memory.

## Spills

The `TransformSpills` pass, implemented in `midenc_dialect_hir`, applies the results of the `Spills` analysis described in [_Analyses_](analyses.md).

It inserts all of the computed spills and reloads, and fixes up the IR to ensure that all uses of a spilled value, use the closest dominating reload or definition of that value.

The resulting IR is guaranteed to keep the maximum operand stack pressure to 16 elements or less.
