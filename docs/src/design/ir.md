# High-Level Intermediate Representation (HIR)

This document describes the concepts, usage, and overall structure of the intermediate representation used by `midenc` and its various components.

* [Core Concepts](#core-concepts)
  * [Dialects](#dialects)
  * [Operations](#operations)
  * [Regions](#regions)
  * [Blocks](#blocks)
  * [Values](#values)
    * [Operands](#operands)
    * [Results](#results)
    * [Immediates](#immediates)
  * [Attributes](#attributes)
  * [Traits](#traits)
  * [Interfaces](#interfaces)
  * [Symbols](#symbols)
  * [Symbol Tables](#symbol-tables)
  * [Successors and Predecessors](#successors-and-predecessors)
  * [Dominance](#dominance)
* [High-Level Structure](#high-level-structure)
* [Pass Infrastructure](#pass-infrastructure)
* [Analysis](#analysis)
* [Pattern Rewrites](#pattern-rewrites)
* [Canonicalization](#canonicalization)

## Core Concepts

HIR is directly based on the design and implementation of [MLIR](https://mlir.llvm.org), in many cases, the documentation there can be a useful guide for HIR as well, in terms of concepts, etc. The actual implementation of HIR looks quite a bit different due to it being in Rust, rather than C++.

MLIR, and by extension, HIR, are compiler intermediate representations based on a concept called the _Regionalized Value State Dependence Graph_ (commonly abbreviated as RVSDG), first introduced in [this paper](https://arxiv.org/pdf/1912.05036). The RVSDG representation, unlike other representations (e.g. LLVM IR), is oriented around data flow, rather than control flow, though it can represent both. Nodes in the data flow graph, which we call [_operations_](#operations), represent computations; while edges, which we call [_values_](#values), represent dependencies between computations. Regions represent the hierarchical structure of a program, both at a high level (e.g. the relationship between modules and functions), as well as at a low level (e.g. structured control flow, such as an if-else operation, or while loop. This representation allows for representing programs at a much higher level of abstraction, makes many data flow analyses and optimizations simpler and more effective, and naturally exposes parallelism inherent in the programs it represents. It is well worth reading the RVSDG paper if you are interested in learning more!

More concretely, the above entities relate to each other as follows:

* Operations can contain regions, operands which represent input values, and results which represent output values.
* Regions can contain [_basic blocks_](#blocks)
* Blocks can contain operations, and may introduce values in the form of block arguments. See the [Basic Blocks](#blocks) section for more details.
* Values from the edges of the data flow graph, i.e. operation A depends on B, if B produces a result that A consumes as an operand.

As noted above, [operations](#operations) can represent both high-level and low-level concepts, e.g. both a function definition, and a function call. The semantics of an operation are encoded in the form of a wide variety of [_operation traits_](#traits), e.g. whether it is commutative, or idempotent; as well as a core set of [_operation interfaces_](#interfaces), e.g. there is an interface for side effects, unstructured/structured control flow operations, and more. This allows working with operations generically, i.e. you can write a control flow analysis without needing to handle every single control flow operation explicitly - instead, you can perform the analysis against a single interface (or set of interfaces that relate to each other), and any operation that implements the interface is automatically supported by the analysis.

Operations are organized into [dialects](#dialects). A dialect can be used to represent some set of operations that are used in a specific phase of compilation, but may not be legal in later phases, and vice versa. For example, we have both `cf` (unstructured control flow) and `scf` (structured control flow) dialects. When lowering to Miden Assembly, we require all control flow to be represented using the `scf` dialect, but early in the pipeline, we receive programs with control flow in the `cf` dialect, which is then "lifted" into `scf` before code generation.

See the following sections for more information on the concepts introduced above:

* [Operations](#operations)
* [Regions](#regions)
* [Blocks](#blocks)
* [Values](#values)
* [Dialects](#dialects)
* [Traits](#traits)
* [Interfaces](#interfaces)

### Operations

An _operation_ represents a computation. Inputs to that computation are in the form of _operands_, and outputs of the computation are in the form of _results_. In practice, an operation may also have _effects_, such as reading/writing from memory, which also represent input/output of the operation, but not explicitly represented in an operation's operands and results.

Operations can contain zero or more regions. An operation with no regions is also called a _primitive_ operation; while an operation with one or more regions is called a _structured_ operation. An example of the former is the `hir.call` operation, i.e. the function call instruction. An example of the latter is `scf.if`, which represents a structured conditional control flow operation, consisting of two regions, a "then" region, and an "else" region.

Operations can implement any number of [_traits_](#traits) and [_interfaces_](#interfaces), so as to allow various pieces of IR infrastructure to operate over them generically based on those implementations. For example, the `arith.add` operation implements the `BinaryOp` and `Commutative` traits; the `scf.if` operation implements the `HasRecursiveSideEffects` trait, and the `RegionBranchOpInterface` interface.

Operations that represent unstructured control flow may also have _successors_, i.e. the set of blocks which they transfer control to. Edges in the control flow graph are formed by "block operands" that act as the value type of a successor. Block operands are tracked in the use list of their associated blocks, allowing one to traverse up the CFG from successors to predecessors.

Operations may also have associated [_attributes_](#attributes). Attributes represent metadata attached to an operation. Attributes are not guaranteed to be preserved during rewrites, except in certain specific cases.

### Regions

A _region_ encapsulates a control-flow graph (CFG) of one or more [_basic blocks_](#blocks). In HIR, the contents of a region are almost always in _single-static assignment_ (SSA) form, meaning that values may only be defined once, definitions must [_dominate_](#dominance-relation) uses, and operations in the CFG described by the region are executed one-by-one, from the entry block of the region, until control exits the region (e.g. via `builtin.ret` or some other terminator instruction).

The order of operations in the region closely corresponds to their scheduling order, though the code generator may reschedule operations when it is safe - and more efficient - to do so.

Operations in a region may introduce nested regions. For example, the body of a function consists of a single region, and it might contain an `scf.if` operation that defines two nested regions, one for the true branch, and one for the false branch. Nested regions may access any [_values_](#values) in
an ancestor region, so long as those values dominate the operation that introduced the nested region. The exception to this are operations which are _isolated from above_. The regions of such an operation are not permitted to reference anything defined in an outer scope, except via
[_symbols_](#symbols). For example, _functions_ are an operation which is isolated from above.

The purpose of regions, is to allow for hierarchical/structured control flow operations. Without them, representing structured control flow in the IR is difficult and error-prone, due to the semantics of SSA CFGs, particularly with regards to analyses like dominance and loops. It is also an important part of what makes [_operations_](#operations) such a powerful abstraction, as it provides a way to generically represent the concept of something like a function body, without needing to special-case them.

A region must always consist of at least one block (the entry block), but not all regions allow multiple blocks. When multiple blocks are present, it implies the presence of unstructured control
flow, as the only way to transfer control between blocks is by using unstructured control flow operations, such as `cf.br`, `cf.cond_br`, or `cf.switch`. Structured control flow operations such as `scf.if`, introduce nested regions consisting of only a single block, as all control flow within a structured control flow op, must itself be structured. The specific rules for a region depend on the semantics of the containing operation.

### Blocks

A _block_, or _basic block_, is a set of one or more [_operations_](#operations) in which there is no control flow, except via the block _terminator_, i.e. the last operation in the block, which is responsible for transferring control to another block, exiting the current region (e.g. returning from a function body), or terminating program execution in some way (e.g. `ub.unreachable`).

A block may declare _block parameters_, the only other way to introduce [_values_](#values) into the IR, aside from operation results. Predecessors of a block must ensure that they provide arguments for all block parameters when transfering control to the block.

Blocks always belong to a [_region_](#regions). The first block in a region is called the _entry block_, and is special in that its block parameters (if any) correspond to whatever arguments the region accepts. For example, the body of a function is a region, and the entry block in that region must have a parameter list that exactly matches the arity and type of the parameters declared in the function signature. In this way, the function parameters are materialized as
SSA values in the IR.

### Values

A _value_ represents terms in a program, temporaries created to store data as it flows through the program. In HIR, which is in SSA form, values are immutable - once created they cannot be changed nor destroyed. This property of values allows them to be reused, rather than recomputed, when the operation that produced them contains no side-effects, i.e. invoking the operation with the same inputs must produce the same outputs. This forms the basis of one of the ways in which SSA IRs can optimize programs.

> [!NOTE]
> One way in which you can form an intuition for values in an SSA IR, is by thinking of them as registers in a virtual machine with no limit to the number of machine registers. This corresponds well to the fact that most values in an IR, are of a type which corresponds to something that can fit in a typical machine register (e.g. 32-bit or 64-bit values, sometimes larger).
>
> Values which cannot be held in actual machine registers, are usually managed in the form of heap or stack-allocated memory, with various operations used to allocate, copy/move, or extract smaller values from them. While not strictly required by the SSA representation, this is almost always effectively enforced by the instruction set, which will only consist of instructions whose operands and results are of a type that can be held in machine registers.

Value _definitions_ (aka "defs") can be introduced in two ways:

1. Block parameters. Most notably, the entry block for function bodies materializes the function parameters as values via block parameters. Block parameters are also used at places in the CFG where two definitions for a single value are joined together. For example, if the value assigned to a variable in the source language is assigned conditionally, then in the IR, there will be a block with a parameter corresponding to the value of that variable after it is assigned. All uses after that point, would refer to that block parameter, rather than the value from a specific branch. Similarly, loop-carried variables, such as an iteration count, are typically manifested as block parameters of the block corresponding to the loop header.
2. Operation results. The most common way in which values are introduced.

Values have _uses_ corresponding to operands or successor arguments (special operands which are used to satisfy successor block parameters). As a result, values also have _users_, corresponding to the specific operation and operand forming a _use.

All _uses_ of a value must be [_dominated_](#dominance-relation) by its _definition_. The IR is invalid if this rule is ever violated.

### Operands

An _operand_ is a [_value_](#values) used as an argument to an operation.

Beyond the semantics of any given operation, operand ordering is only significant in so far as it is used as the order in which those items are expected to appear on the operand stack once lowered to Miden Assembly. The earlier an operand appears in the list of operands for an operation, the
closer to the top of the operand stack it will appear.

Similarly, the ordering of operand results also correlates to the operand stack order after lowering. Specifically, the earlier a result appears in the result list, the closer to the top of the operand stack it will appear after the operation executes.

### Immediates

An _immediate_ is a literal value, typically of integral type, used as an operand. Not all operations support immediates, but those that do, will typically use them to attempt to perform optimizations only possible when there is static information available about the operands. For example, multiplying any number by 2, will always produce an even number, so a sequence such as
`mul.2 is_odd` can be folded to `false` at compile-time, allowing further optimizations to occur.

Immediates are separate from _constants_, in that immediates _are_ constants, but specifically constants which are valid operand values.

### Attributes

An _attribute_ is (typically optional) metadata attached to an IR entity. In HIR, attributes can be attached to functions, global variables, and operations.

Attributes are stored as a set of arbitrary key-value data, where values are optional. An attribute with no value acts like a "marker", i.e. it is meaningful just be being present (e.g. `#[inline]` in Rust).

Attribute values must implement the `AttributeValue` trait.

### Traits

A _trait_ defines some property of an operation. This allows operations
to operated over generically based on those properties, in an analysis or rewrite, without having to handle the specific operation type explicitly.

Operations can always be cast to their implementing traits, as well as queried for if they implement a given trait.

There are a number of predefined traits, found in `midenc_hir::traits`, e.g.:

* `IsolatedFromAbove`, a marker trait that indicates that regions of the operation it is attached to cannot reference items from any parents, except via [_symbols_](#symbols).
* `Terminator`, a marker trait for operations which are valid block terminators
* `ReturnLike`, a trait that describes behavior shared by instructions that exit from an enclosing region, "returning" the results of executing that region. The most notable of these is `builtin.ret`, but `scf.yield` used by the structured control flow ops is also return-like in nature.
* `ConstantLike`, a marker trait for operations that produce a constant value
* `Commutative`, a marker trait for binary operations that exhibit commutativity, i.e. the order of the operands can be swapped without changing semantics.

There are others as well, responsible for aiding in type checking, decorating operations with the types of side effects they do (or do not) exhibit, and more.

### Interfaces

TODO

### Successors and Predecessors

The concept of _predecessor_ and _successor_ corresponds to a parent/child relationship in a control-flow graph (CFG), where edges in the graph are directed, and describe the order in which control flows through the program. If a node $A$ transfers control to a node $B$ after it is finished executing, then $A$ is a _predecessor_ of $B$, and $B$ is a _successor_ of $A$.

Successors and predecessors can be looked at from two similar, but slightly different, perspectives:

1. In terms of operations. In an SSA CFG, operations in a basic block are executed in order, and thus the successor of an operation in the block, is the next operation to be executed in that block, with the predecessor being the inverse of that relationship. At basic block boundaries, the successor(s) of the _terminator_ operation, are the set of operations to which control can be transferred. Likewise, the predecessor(s) of the first operation in a block, are the set of terminators which can transfer control to the containing block. This is the most precise, but is not quite as intuitive as the alternative.
2. In terms of blocks. The successor(s) of a basic block, are the set of blocks to which control may be transferred when exiting the block. Likewise, the precessor(s) of a block, are the set of blocks which can transfer control to it. We are most frequently dealing with the concept of successors and predecessors in terms of blocks, as it allows us to focus on the interesting parts of the CFG. For example, the dominator tree and loop analyses, are constructed in terms of a block-oriented CFG, since we can trivially derive dominance and loop information for individual ops from their containing blocks.

Typically, you will see successors as a pair of `(block_id, &[value_id])`, i.e. the block to which control is transferred, and the set of values being passed as block arguments. On the other hand, predecessors are most often a pair of `(block_id, terminator_op_id)`, i.e. the block from which control originates, and the specific operation responsible.

### Dominance Relation

In an SSA IR, the concept of _dominance_ is of critical importance. Dominance is a property of the relationship between two or more entities and their respective program points. For example, between the use of a value as an operand for an operation, and the definition of that value; or between a basic block and its successors. The dominance property is anti-symmetric, i.e. if $A$ dominates $B$, then $B$ cannot dominate $A$, unless $A = B$. Put simply:

> Given a control-flow graph $G$, and a node $A \in G$, then $\forall B \in G$, $A dom B$ if all paths to $B$ from the root of $G$, pass through $A$.
>
> Furthermore, $A$ _strictly_ dominates $B$, if $A \neq B$.

An example of why dominance is an important property of a program, can be seen when considering the meaning of a program like so (written in pseudocode):

```
if (...) {
  var a = 1;
}

foo(a)
```

Here, the definition of `a` does not dominate its usage in the call to `foo`. If the conditional branch is ever false, `a` is never defined, nor initialized - so what should happen when we reach the call to `foo`?

In practice, of course, such a program is rarely possible to expresss in a high-level language, however in a low-level CFG, it is possible to reference values which are defined somewhere in the graph, but in such a way that is not _legal_ according to the "definitions must dominate uses" rule of SSA CFGs. The dominance property is what we use to validate the correctness of the IR, as well as evaluate the range of valid transformations that can be applied to the IR. For example, we might determine that it is valid to move an expression into a specific `if/then` branch, because it is only used in that branch - the dominance property is how we determine that there are paths through the program in which the result of the expression is unused, as well as what program points
represent the nearest point to one of its uses that still dominates _all_ of the uses.

There is another useful notion of dominance, called _post-dominance_, which can be described much like the regular notion of dominance, except in terms of paths to the exit of the CFG, rather than paths from the entry:

> Given a control-flow graph $G$, and a node $A \in $G$, then $\forall B \in G$, $A pdom B$ if all paths through $B$ that exit the CFG, must flow through $A$ first.
>
> Furthermore, $A$ _strictly_ post-dominates $B$ if $A \neq B$.

The notion of post-dominance is important in determining the applicability of certain transformations, in particular with loops.

## High-Level Structure

Beyond the core IR concepts introduced in the previous section, HIR also imposes some hierarchical structure to programs in form of builtin operations that are special-cased in certain respects:

* [Worlds](#worlds)
* [Components](#components)
* [Modules](#modules)
* [Functions](#functions)

In short, when compiling a program, the inputs (source program, dependencies, etc.) are represented in a single _world_ (i.e. everything we know about that program and what is needed to compile it). The input program is then translated into a single top-level _component_ of that world, and any of it's dependendencies are represented in the form of component _declarations_ (in HIR, a declaration - as opposed to a definition - consists of just the metadata about a thing, not its implementation, e.g. a function signature).

A _component_ can contain one or more _modules_, and optionally, one or more _data segments_. Each module can contain any number of _functions_ and _global variables_.

> [!NOTE]
> To understand how these relate to Miden Assembly, and Miden packages, see the [Packaging](packaging.md) document.

The terminology and semantics of worlds and components, are based on the Web Assembly [Component Model](https://component-model.bytecodealliance.org). In particular, the following properties are key to understanding the relationships between these entities:

* Worlds must encode everything needed by a component
* Components represent a shared-nothing boundary, i.e. nothing outside a component can access the resources of that component (e.g. memory). We rely on this property so that we can correctly represent the interaction between Miden _contexts_ (each of which has its own memory, with no way to access the memory of other contexts).
* Component-level exports represent the "interface" of a component, and are required to adhere to the [Canonical ABI](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md).

The following is a rough visual representation of the hierarchy and relationships between these concepts in HIR:

              World
                |
                v
       ---- Component -----------
      |                          |
      v                          |
    Function  (component export) |
                                 |
                 ----------------
                |           |
                v           v
              Module  Data Segment
                |
                |-----------
                v           v
             Function  Global Variable
                |
                v
       ----- Region (a function has a single region, it's "body")
      |         |   (the body region has a single block, it's "entry")
      |         v
      |       Block -> Block Argument (function parameters)
      |         |        |
      |         |        |
      |         |        v
      |         |      Operand
      |         v        |  ^
       ---> Operation <--   |
                |           |
                v           |
              Result -------

A few notes:

* Dependencies between components may only exist between component-level exported functions, i.e. it is not valid to depend on a function defined in a module of another component directly.
* Only component exports use the Canonical ABI, internally they handle lifting/lowering to the "core" ABI of the function which actually implements the behavior being exported.
* Data segments represent data that will be written into the shared memory of a component when the component is initialized. Thus, they must be specified at component level, and may not be shared between components.
* Global variables, representing some region of memory with a specified type, by definition cannot be shared between components, and are only visible within a component. We further restrict their definition to be within a module. Global variables _can_ be shared between modules, however.
* Worlds, components, and modules are single-region, single-block operations, with graph-like region semantics (i.e. their block does not adhere to SSA dominance rules). They all implement the `SymbolTable` trait, and all but World implements the `Symbol` trait.
* Functions are single-region, but that region can contain multiple blocks, and the body region is an SSA CFG region, i.e. it's blocks and operations must adhere to SSA dominance rules. The interaction with a function is determined by its _signature_, which dictates the types of its parameters and results, but these are not represented as operation operands/results, instead the function parameters are encoded as block parameters of its entry block, and function results are materialized at call sites based on the function signature. A validation rule ensures that the return-like operations in the function body return values that match the signature of the containing function.

### Worlds

A _world_ represents all available information about a program and its dependencies, required for compilation. It is unnamed, and so it is not possible to interact between worlds.

Worlds may only contain components (possibly in the future we'll relax this to allow for non-component modules as well, but not today). Each world is a symbol table for the components it contains, facilitating inter-component dependencies.

A world is by definition the root symbol table for everything it contains, i.e. an absolute symbol path is always resolved from the nearest world, or failing that, the nearest operation without a parent.

### Components

A _component_ is a named entity with an _interface_ comprised of it's exported functions. This implicit interface forms a signature that other components can use to provide for link-time virtualization of components, i.e. any component that can fulfill a given interface, can be used to satisfy that interface.

Components may contain modules, as well as data segment definitions which will be visible to all code running within the component boundary.

A component _declaration_, as opposed to a definition, consists strictly of its exported functions, all of which are declarations, not definitions.

A component _instance_ refers to a component that has had all of its dependencies resolved concretely, and is thus fully-defined.

The modules of a component provide the implementation of its exported interface, i.e. top-level component functions typically only handle lifting module exports into the Canonical ABI.

### Modules

A module is primarily two things:

1. A named container for one or more functions belonging to a common namespace.
2. A concrete implementation of the functionality exported from a component.

Functions within a module may be exported. Functions which are _not_ exported, are only visible within that module.

A module defines a symbol table, whose entries are the functions and global variables defined in that module. Relative symbol paths used within the module are always resolved via this symbol table.

### Functions

A function is the highest-level unit of computation represented in HIR, and differs from the other container types (e.g. component, module), in that its body region is an SSA CFG region, i.e. its blocks and operations must adhere to the SSA dominance property.

A function _declaration_ is represented as a function operation whose body region is empty, i.e. has no blocks.

A function has a signature that encodes its parameters and results, as well as the calling convention it expects callers to use when calling it, and any special attributes that apply to it (i.e. whether it is inlineable, whether any of its parameters are special in some way, etc.).

Function parameters are materialized as values in the form of entry block arguments, and always correspond 1:1. Function results are materialized as values only at call sites, not as operation results of the function op.

Blocks in the function body must be terminated with one of two operations:

* `builtin.ret`, which returns from the function to its caller. The set of operands passed to this operation must match the arity and types specified in the containing function's signature.
* `ub.unreachable`, representing some control flow path that should never be reachable at runtime. This is translated to an abort/trap during code generation. This operation is defined in the `ub` dialect as it corresponds to undefined behavior in a program.

### Global Variables

A global variable represents a named, typed region of memory, with a fixed address at runtime.

Global variables may specify an optional _initializer_, which is a region consisting of operations that will be executed in order to initialize the state of the global variable prior to program start. Typically, the initializer should only consist of operations that can be executed at compile-time, not runtime, but because of how Miden memory is initialized, we can actually relax this rule.

### Symbol Tables

A _symbol table_ represents a namespace in which [_symbols_](#symbols) may be defined and resolved.

Operations that represent a symbol table, must implement the `SymbolTable` trait.

Symbol tables may be nested, so long as child symbol table operations are also valid symbols, so that the hierarchy of namespaces can be encoded as a _symbol path_ (see [Symbols](#symbols)).

### Symbols

A _symbol_ is a named operation, e.g. the function `foo` names that function so that it can be referenced and called from other operations.

Symbols are only meaningful in the context of a _symbol table_, i.e. the namespace in which a symbol is registered. Symbols within a symbol table must be unique.

A symbol is reified as a _symbol path_, i.e. `foo/bar` represents a symbol path consisting of two path components, `foo` and `bar`. Resolving that symbol path requires first resolving `foo` in the current symbol table, to an operation that is itself a symbol table, and then resolving `bar` there.

Symbol paths can come in two forms: relative and absolute. Relative paths are resolved as described above, while absolute paths are resolved from the root symbol table, which is either the containing [_world_](#worlds), or the nearest symbol table which has no parent.

Symbols, like the various forms of [_values_](#values), track their uses and definitions, i.e. when you reference a symbol from another operation, that reference is recorded in the use list of the referenced symbol. This allows us to trivially determine if a symbol is used, and visit all of those uses.

## Pass Infrastructure

Compiler passes encode transformations of the IR from frontend to backend. In HIR, you define a pass over a concrete operation type, or over all operations and then filter on some criteria.

The execution of passes is configured and run via a _pass manager_, which you construct and then add passes to, and then run once finalized.

Passes typically make uses of [_analyses_](#analyses) in order to perform their specific transformations. In order to share the computation of analyses between passes, and to correctly know when those analyses can be preserved or recomputed, the pass manager will construct an _analysis manager_, which is then provided to passes during execution, so that they can query it for a specific analysis of the current operation.

Passes can register statistics, which will then be tracked by the pass manager.

The primary way you interact with the pass infrastructure is by:

1. Construct a `PassManager` for whatever root operation type you plan to run the pass pipeline on.
2. Add one or more `Pass` implementations, nesting pass managers as needed in order to control which passes are applied at which level of the operation hierarchy.
3. Run the `PassManager` on the root operation you wish to transform.

In HIR, there are three primary types of passes:

* DIY, i.e. anything goes. What these do is completely up to the pass author.
* Pattern rewrites, which match against an operation by looking for some pattern, and then performing a rewrite of that operation based on that pattern. These are executed by the `GreedyPatternRewriteDriver`, and must adhere to a specific set of rules in order for the driver to be guaranteed to reach fixpoint.
* Canonicalizations, a special case of pattern rewrite which are orchestrated by the `Canonicalizer` rewrite pass.

### Analyses

An _analysis_ is responsible for computing some fact about the given IR entity it is given. Facts include things such as: the dominance tree for an SSA control flow graph; identifying loops and their various component parts such as the header, latches, and exits; reachability; liveness; identifying unused (i.e. dead) code, and much more.

Analyses in the IR can be defined in one of two ways (and sometimes both):

1. As an implementation of the `Analysis` trait. This is necessary for analyses which you wish to query from the `AnalysisManager` in a pass.
2. As an implementation of the `DataFlowAnalysis` trait, or one of its specializations, e.g. `DenseBackwardDataFlowAnalysis`. These are analyses which adhere to the classical data flow analysis rules, i.e. the analysis state represents a join/meet semi-lattice (depending on the type and direction of the analysis), and the transfer function ensures that the state always converges in a single direction.

`Analysis` implementations get the current `AnalysisManager` instance in their `analyze` callback, and can use this to query other analyses that they depend on. It is important that implementations also implement `invalidate` if they should be invalidated based on dependent analyses (and whether those have been invalidated can be accessed via the provided `PreservedAnalyses` state in that callback).

Analyses can be implemented for a specific concrete operation, or any operation.

### Pattern Rewrites

TODO

### Canonicalization

TODO
