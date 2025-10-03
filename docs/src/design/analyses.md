# Analyses

This document provides an overview of some of the current analysis passes the compiler uses when lowering from the frontend to Miden Assembly. This is not guaranteed to be comprehensive, but mostly meant as a high-level reference to what analyses exist and why they are being used.

All analysis passes, at the time of writing, are maintained in the `midenc-hir-analysis` crate, with the exception of two, which are part of the core `midenc-hir` crate.

* [Dominance](#dominance)
* [Loop Forest](#loop-forest)
* [Dead Code](#dead-code)
* [Sparse Constant Propagation](#sparse-constant-propagation)
* [Liveness](#liveness)
* [Spills](#spills)


## Dominance

Dominance analysis is responsible for computing three data structures commonly used in a compiler operating over a control flow graph in SSA form:

* [Dominance tree](#dominance-tree)
* [Post-dominance tree](#post-dominance-tree)
* [Iterated dominance frontier](#iterated-dominance-frontier)

What does dominance refer to in this context? Quite simply, it refers to the relationship between program points in a control flow graph. If all paths to a specific program point $B$, flow through a preceding program point $A$, then it can be said that $A$ _dominates_ $B$. Further, since any program point trivially dominates itself; when $A$ dominates $B$, and $A \neq B$, then $A$ is said to _properly dominate_ $B$. This distinction is occasionally important in the use of the computed dominance analysis.

We're particularly interested in dominance as it pertains to uses and definitions of values of a program in SSA form. SSA, or _single-static assignment_ form, requires that programs adhere to the following properties:

* Values, once defined, are immutable - hence "single-static assignment". To "reassign" a value, you must introduce a new definition representing the changed value. This doesn't account for mutating heap-allocated types where the "value" in SSA is the pointer to the data in memory, it is strictly in reference to what is encoded as an SSA value.
* Uses of a value must always be _properly dominated_ by the definition of that value, i.e. there cannot be any path through the control flow graph that reaches a use of a value, not preceded by its definition.

> [!NOTE]
> Values correspond to registers in an abstract machine with infinitely many of them. Thus the type of a value must be something that has a defined lowering to whatever targets you wish to support. In practice, this is fixed-width integers up to 128 bits, single- or double-precision floating point numbers, pointers, and structs that can be represented as a very small set of scalar values. The set of allowed types of SSA values is not strictly speaking a property of SSA, but it is almost always the case that the types do not include values that require memory allocation (except as a pointer).

Dominance analysis is critical for safe transformation of SSA-form programs.

### Dominance tree

A dominance tree represents blocks of the CFG as a tree, where the root of the tree is the entry block, and each block in the tree has a single parent, its _immediate dominator_, and zero or more children for which it is the immediate dominator.

A reverse post-order traversal of the dominance tree is commonly used to visit nodes of the CFG such that each node is only seen once all of its predecessors have been seen, or if control must pass through the node before reaching a non-dominating predecessor (e.g. a loop header must always be passed through before any of its loopback edges).

A dominance tree tells us what blocks control will always pass through to reach any other given block in the CFG.

### Post-dominance tree

A post-dominance tree is the inverse of a dominance tree, i.e. $B$ _post-dominates_ $A$, if all paths to the exit node of the CFG starting at $A$, must go through $B$. Accordingly, the _immediate post-dominator_ of $A$ is the post-dominator of $A$ that doesn't strictly post-dominate any other strict post-dominators of $A$.

A post-dominance tree tells us what blocks control will always pass through to reach the exit.

### Iterated dominance frontier

The dominance frontier of some node $B$, is the set of all nodes $N$, such that $B$ dominates an immediate predecessor of $N$, but $B$ does not strictly dominate $N$. In other words, it is the set of nodes where $B$'s dominance stops.

The _iterated_ dominance frontier of some node $B$, represents computing the dominance frontier $N$ of $B$, and then the dominance frontiers of all nodes in $N$, recursively.

Iterated dominance frontiers are especially useful when one needs to introduce new definitions of a value, and ensure that all uses of the original value reachable from the new definition, are rewritten to use the new definition. Because uses must be dominated by defs, and the new definition may not strictly dominate all uses, but nevertheless be defined along _some_ path to a use, we may be required to introduce new _phi nodes_ (in HIR, block arguments) that join two or more definitions of a value together at join points in the CFG. The iterated dominance frontier of the new definition tells us all of the blocks where we would need to introduce a new block argument, if there are uses of the value in, or dominated by, that block.

We currently use this in our spills analysis/transformation, in order to do the very thing described above for each reload of a spilled value (which represent new definitions of the spilled value). We want to ensure that uses of the original value reachable from the reload, use the reload instead, thus terminating the live range of the spilled value at the point it is spilled.


## Loop Forest

The loop forest represents the set of loops identified in a given CFG, as well as their components:

* Entering blocks (loop predecessor blocks), i.e. non-loop nodes that are predecessors of the loop header. If only one such block exists, it is called the _loop pre-header_.
* Loop _header_, i.e. the block which dominates all other loop nodes.
* _Latch_ nodes, i.e. a loop node that has an edge back to the loop header
* _Exiting_ blocks, i.e. blocks which have a successor outside of the loop, but are inside the loop themselves.
* _Exit_ blocks, i.e. a non-loop block which is the successor of an exiting block.

Each block may only be the header for a single loop, and thus you can identify a loop by the header block.

See [LLVM Loop Terminology (and Canonical Forms)](https://llvm.org/docs/LoopTerminology.html) for a more comprehensive description of how loops are treated analyzed by the compiler, as we base our implementation on LLVM's.

The loop forest can be queried for info about a particular loop, whether a block is part of a loop, and if it is a loop header. The information for a particular loop lets you query what blocks are part of the loop, what their role(s) in the loop are, and the relationship to other loops (i.e. whether the loop is a child of another loop).

We currently do not make heavy use of this, except to attach some loop metadata to nodes during data flow analysis. Since we aim to lift unstructured control flow into structured control flow early during compilation, and this analysis is only defined for unstructured CFGs, it is only pertinent prior to control flow lifting.

## Dead Code

The dead code analysis computes the following information about the IR:

* Whether a block is _executable_, or _live_.
* The set of known predecessors at certain program points (successors of a control flow op, entry points of a callable region, exit points of a call op), and whether those predecessors are executable.

This is a fundamental analysis in our data flow analysis framework, and it coordinates with the _sparse constant propagation_ analysis to refine its results. We build on this to more precisely compute other analyses based on the liveness of a particular program point, or predecessors to that point.

We do not yet perform _dead-code elimination_ based on this analysis, but likely will do so in the future.

## Sparse Constant Propagation

This analysis takes all constant values in a program, and uses that information to attempt determining whether or not uses of those constants that produce new values, may themselves be reduced to constants.

This analysis does not transform the IR, that is done by the _sparse conditional constant propagation_ (SCCP) transform. Instead, it attaches data flow facts to each value derived from an operation that uses a value for which we know a constant value.

This analysis feeds into the [_dead code_](#dead-code) analysis, by determining whether or not specific control flow paths are taken when operands to a control flow op have known constant values, e.g. if a `scf.if` selector is `true`, then the else region is statically dead.

The SCCP transform mentioned above will actually take the results of the two analyses and rewrite the IR to remove statically unreachable paths, and to fold operations which can be reduced to constants.

## Liveness

This analysis computes the liveness, and live range, of every value in the program. This is of crucial importance during code generation, particularly as it relates to management of Miden's operand stack.

Our specific implementation is based on the ideas and algorithms described in [_Register Spilling and Live-Range Splitting for SSA-form Programs_](https://pp.ipd.kit.edu/uploads/publikationen/braun09cc.pdf), by Matthias Braun and Sebastian Hack. Specifically, unlike many traditional liveness analysis implementations, we do not track liveness as a boolean state at each program point, but rather in terms of _next-use distances_. This seemingly small change has some significant benefits: we are able to reason more precisely about how important certain values are at each program point (e.g. should we keep a value in a register/on the operand stack, or can it be spilled to memory to make room for higher priority values); and we are able to quickly assess on entry to a loop, whether the next use of a value occurs within the loop, or after it. This choice of representation of liveness data plays a key role in our [_spills analysis_](#spills).

Our liveness analysis also builds on top of the [_dead code_](#dead-code) and [_sparse constant propagation_](#sparse-constant-propagation) analyses, to avoid considering uses of values along code paths which are statically unreachable/dead. Like those two analyses, it is also implemented on top of our data flow analysis framework.

## Spills

The purpose of the spills analysis is to identify programs where we can statically determine that the number of simultaneously-live values would overflow the addressable Miden operand stack, thus requiring us to spill values to memory in order to access values that are in the overflow table. By doing this ahead of time during compilation, we can make smarter choices about what to spill, and when, such that the operand stack never overflows, and potentially expensive spill/reload operations are not emitted in hot code paths, such as loops.

This analysis is tightly integrated with our [_liveness analysis_](#liveness) implementation, particularly the fact that our liveness information is based on next-use distances. Like liveness, it also builds on top of the [_dead code_](#dead-code) and [_sparse constant propagation_](#sparse-constant-propagation) analyses to avoid considering statically unreachable/dead code paths.

The spills analysis also acts as a register allocator, in that part of how it determines what to spill and when, is by computing the live-in/live-out register sets at each block and operation, along with the set of values in those sets which have been spilled along code paths reaching each program point. We use this information to schedule operands at control flow join points, so that the state of the operand stack is kept consistent on exit from predecessors.
