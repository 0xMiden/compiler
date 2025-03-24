# High-Level Intermediate Representation (HIR)

This document describes the concepts, usage, and overall structure of the intermediate representation used by `midenc` and its various components.

* [Core Concepts](#core-concepts)
  * [Dialects](#dialects)
  * [Operations](#operations)
  * [Regions](#regions)
  * [Blocks](#blocks)
  * [Values](#values)
    * [Operands](#operands)
    * [Immediates](#immediates)
  * [Attributes](#attributes)
  * [Traits](#traits)
  * [Interfaces](#interfaces)
  * [Symbols](#symbols)
  * [Symbol Tables](#symbol-tables)
  * [Successors and Predecessors](#successors-and-predecessors)
* [High-Level Structure](#high-level-structure)
* [Pass Infrastructure](#pass-infrastructure)
  * [Analysis](#analysis)
  * [Pattern Rewrites](#pattern-rewrites)
  * [Canonicalization](#canonicalization)
    * [Folding](#folding)
* [Implementation Details](#implementation-details)
  * [Session](#session)
  * [Context](#context)
  * [Entity References](#entity-references)
    * [Entity Storage](#entity-storage)
      * [StoreableEntity](#storeableentity)
      * [ValueRange](#valuerange)
    * [Entity Lists](#entity-storage)
  * [Traversal](#traversal)
  * [Program Points](#program-points)
  * [Defining Dialects](#defining-dialects)
    * [Dialect Registration]
    * [Dialect Hooks]
    * [Defining Operations]
  * [Builders](#builders)
    * [Validation](#validation)
  * [Effects](#effects)

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

* [Dialects](#dialects)
* [Operations](#operations)
* [Regions](#regions)
* [Blocks](#blocks)
* [Values](#values)
* [Traits](#traits)
* [Interfaces](#interfaces)

### Dialects

TODO

### Operations

An _operation_ represents a computation. Inputs to that computation are in the form of _operands_, and outputs of the computation are in the form of _results_. In practice, an operation may also have _effects_, such as reading/writing from memory, which also represent input/output of the operation, but not explicitly represented in an operation's operands and results.

Operations can contain zero or more regions. An operation with no regions is also called a _primitive_ operation; while an operation with one or more regions is called a _structured_ operation. An example of the former is the `hir.call` operation, i.e. the function call instruction. An example of the latter is `scf.if`, which represents a structured conditional control flow operation, consisting of two regions, a "then" region, and an "else" region.

Operations can implement any number of [_traits_](#traits) and [_interfaces_](#interfaces), so as to allow various pieces of IR infrastructure to operate over them generically based on those implementations. For example, the `arith.add` operation implements the `BinaryOp` and `Commutative` traits; the `scf.if` operation implements the `HasRecursiveSideEffects` trait, and the `RegionBranchOpInterface` interface.

Operations that represent unstructured control flow may also have _successors_, i.e. the set of blocks which they transfer control to. Edges in the control flow graph are formed by "block operands" that act as the value type of a successor. Block operands are tracked in the use list of their associated blocks, allowing one to traverse up the CFG from successors to predecessors.

Operations may also have associated [_attributes_](#attributes). Attributes represent metadata attached to an operation. Attributes are not guaranteed to be preserved during rewrites, except in certain specific cases.

### Regions

A _region_ encapsulates a control-flow graph (CFG) of one or more [_basic blocks_](#blocks). In HIR, the contents of a region are almost always in _single-static assignment_ (SSA) form, meaning that values may only be defined once, definitions must  _dominate_ uses, and operations in the CFG described by the region are executed one-by-one, from the entry block of the region, until control exits the region (e.g. via `builtin.ret` or some other terminator instruction).

The order of operations in the region closely corresponds to their scheduling order, though the code generator may reschedule operations when it is safe - and more efficient - to do so.

Operations in a region may introduce nested regions. For example, the body of a function consists of a single region, and it might contain an `scf.if` operation that defines two nested regions, one for the true branch, and one for the false branch. Nested regions may access any [_values_](#values) in
an ancestor region, so long as those values dominate the operation that introduced the nested region. The exception to this are operations which are _isolated from above_. The regions of such an operation are not permitted to reference anything defined in an outer scope, except via
[_symbols_](#symbols). For example, _functions_ are an operation which is isolated from above.

The purpose of regions, is to allow for hierarchical/structured control flow operations. Without them, representing structured control flow in the IR is difficult and error-prone, due to the semantics of SSA CFGs, particularly with regards to analyses like dominance and loops. It is also an important part of what makes [_operations_](#operations) such a powerful abstraction, as it provides a way to generically represent the concept of something like a function body, without needing to special-case them.

A region must always consist of at least one block (the entry block), but not all regions allow multiple blocks. When multiple blocks are present, it implies the presence of unstructured control
flow, as the only way to transfer control between blocks is by using unstructured control flow operations, such as `cf.br`, `cf.cond_br`, or `cf.switch`. Structured control flow operations such as `scf.if`, introduce nested regions consisting of only a single block, as all control flow within a structured control flow op, must itself be structured. The specific rules for a region depend on the semantics of the containing operation.

### Blocks

A _block_, or _basic block_, is a set of one or more [_operations_](#operations) in which there is no control flow, except via the block _terminator_, i.e. the last operation in the block, which is responsible for transferring control to another block, exiting the current region (e.g. returning from a function body), or terminating program execution in some way (e.g. `ub.unreachable`).

Blocks belong to [_regions_](#regions), and if a block has no parent region, it is considered _orphaned_.

A block may declare _block arguments_, the only other way to introduce [_values_](#values) into the IR, aside from operation results. Predecessors of a block must ensure that they provide inputs for all block arguments when transfering control to the block.

Blocks which are reachable as successors of some control flow operation, are said to be _used_ by that operation. These uses are represented in the form of the `BlockOperand` type, which specifies what block is used, what operation is the user, and the index of the successor in the operation's [_successor storage_](#entity-storage). The `BlockOperand` is linked into the [_use-list_](#entity-lists) of the referenced `Block`, and a `BlockOperandRef` is stored as part of the successor information in the using operation's successor storage. This is the means by which the control flow graph is traversed - you can navigate to predecessors of a block by visiting all of its "users", and you navigate to successors of a block by visiting all successors of the block terminator operation.

### Values

A _value_ represents terms in a program, temporaries created to store data as it flows through the program. In HIR, which is in SSA form, values are immutable - once created they cannot be changed nor destroyed. This property of values allows them to be reused, rather than recomputed, when the operation that produced them contains no side-effects, i.e. invoking the operation with the same inputs must produce the same outputs. This forms the basis of one of the ways in which SSA IRs can optimize programs.

> [!NOTE]
> One way in which you can form an intuition for values in an SSA IR, is by thinking of them as registers in a virtual machine with no limit to the number of machine registers. This corresponds well to the fact that most values in an IR, are of a type which corresponds to something that can fit in a typical machine register (e.g. 32-bit or 64-bit values, sometimes larger).
>
> Values which cannot be held in actual machine registers, are usually managed in the form of heap or stack-allocated memory, with various operations used to allocate, copy/move, or extract smaller values from them. While not strictly required by the SSA representation, this is almost always effectively enforced by the instruction set, which will only consist of instructions whose operands and results are of a type that can be held in machine registers.

Value _definitions_ (aka "defs") can be introduced in two ways:

1. Block argument lists, i.e. the `BlockArgument` value kind. In general, block arguments are used as a more intuitive and ergonomic representation of SSA _phi nodes_, joining multiple definitions of a single value together at control flow join points. Block arguments are also used to represent _region arguments_, which correspond to the set of values that will be forward to that region by the parent operation (or from a sibling region). These arguments are defined as block arguments of the region's entry block. A prime example of this, is the `Function` op. The parameters expressed by the function signature are reflected in the entry block argument list of the function body region.
2. Operation results, i.e. the `OpResult` value kind. This is the primary way in which values are introduced.

Both value kinds above implement the `Value` trait, which provides the set of metadata and behaviors that are common across all value kinds. In general, you will almost always be working with values in terms of this trait, rather than the concrete type.

Values have _uses_ corresponding to usage as an operand of some operation. This is represented via the `OpOperand` type, which encodes the use of a specific value (i.e. its _user_, or owning operation; what value is used; its index in operand storage). The `OpOperand` is linked into the [_use list_](#entity-lists) of the value, and the `OpOperandRef` is stored in the [_entity storage_](#entity-storage) of the using operation. This allows navigating from an operation to all of the values it uses, as well from a value to all of its users. This makes replacing all uses of a value extremely efficient.

As always, all _uses_ of a value must be dominated by its definition. The IR is invalid if this rule is ever violated.

#### Operands

An _operand_ is a [_value_](#values) used as an argument to an operation.

Beyond the semantics of any given operation, operand ordering is only significant in so far as it is used as the order in which those items are expected to appear on the operand stack once lowered to Miden Assembly. The earlier an operand appears in the list of operands for an operation, the
closer to the top of the operand stack it will appear.

Similarly, the ordering of operand results also correlates to the operand stack order after lowering. Specifically, the earlier a result appears in the result list, the closer to the top of the operand stack it will appear after the operation executes.

#### Immediates

Immediates are a built-in [_attribute_](#attributes) type, which we use to represent constants that are able to be used as "immediate" operands of machine instructions (e.g. a literal memory address, or integer value).

The `Immediate` type  provides a number of useful APIs for interacting with an immediate value, such as bitcasts, conversions, and common queries, e.g. "is this a signed integer".

It should be noted, that this type is a convenience, it is entirely possible to represent the same information using other types, e.g. `u32` rather than `Immediate::U32`, and the IR makes no assumptions about what type is used for constants in general. We do, however, assume this type is used for constants in specific dialects of the IR, e.g. `hir`.

### Attributes

Attributes represent named metadata attached to an _operation_.

Attributes can be used in two primary ways:

* A name without a value, i.e. a "marker" attribute. In this case, the presence of the attribute is significant, e.g. `#[inline]`.
* A name with a value, i.e. a "key-value" attribute. This is the more common usage, e.g. `#[overflow = wrapping]`.

Any type that implements the `AttributeValue` trait can be used as the value of a key/value-style attribute. This trait is implemented by default for all integral types, as well as a handful of IR types which have been used as attributes. There are also a few generic built-in attribute types that you may be interested in:

* `ArrayAttr`, which can represent an array/vector-like collection of attribute values, e.g. `#[indices = [1, 2, 3]]`.
* `SetAttr`, which represents a set-like collection of attribute values. The primary difference between this and `ArrayAttr` is that the values are guaranteed to be unique.
* `DictAttr`, which represents a map-like collection of attribute values.

It should be noted that there is no guarantee that attributes are preserved by transformations, i.e. if an operation is erased/replaced, attributes _may_ be lost in the process. As such, you must not assume that they will be preserved, unless made an intrinsic part of the operation definition.

### Traits

A _trait_ defines some property of an operation. This allows operations to be operated over generically based on those properties, e.g. in an analysis or rewrite, without having to handle the concrete operation type explicitly.

Operations can always be cast to their implementing traits, as well as queried for if they implement a given trait. The set of traits attached to an operation can either be declared as part of the operation itself, or be attached to the operation at [dialect registration](#dialect-registration) time via [dialect hooks](#dialect-hooks).

There are a number of predefined traits, found in `midenc_hir::traits`, e.g.:

* `IsolatedFromAbove`, a marker trait that indicates that regions of the operation it is attached to cannot reference items from any parents, except via [_symbols_](#symbols).
* `Terminator`, a marker trait for operations which are valid block terminators
* `ReturnLike`, a trait that describes behavior shared by instructions that exit from an enclosing region, "returning" the results of executing that region. The most notable of these is `builtin.ret`, but `scf.yield` used by the structured control flow ops is also return-like in nature.
* `ConstantLike`, a marker trait for operations that produce a constant value
* `Commutative`, a marker trait for binary operations that exhibit commutativity, i.e. the order of the operands can be swapped without changing semantics.

### Interfaces

An _interface_, in contrast to a [_trait_](#traits), represents not only that an operation exhibits some property, but also provides a set of specialized APIs for working with them.

Some key examples:

* `EffectOpInterface`, operations whose side effects, or lack thereof, are well-specified. `MemoryEffectOpInterface` is a specialization of this interface specifically for operations with memory effects (e.g. read/write, alloc/free). This interface allows querying what effects an operation has, what resource the effect applies to (if known), or whether an operation affects a specific resource, and by what effect(s).
* `CallableOpInterface`, operations which are "callable", i.e. can be targets of a call-like operation. This allows querying information about the callable, such as its signature, whether it is a declaration or definition, etc.
* `CallOpInterface`, operations which can call a callable operation. This interface provides information about the call, and its callee.
* `SelectOpInterface`, operations which represent a selection between two values based on a boolean condition. This interface allows operating on all select-like operations without knowing what dialect they are from.
* `BranchOpInterface`, operations which implement an unstructured control flow branch from one block to one or more other blocks. This interface provides a generic means of accessing successors, successor operands, etc.
* `RegionBranchOpInterface`, operations which implement structured control flow from themselves (the parent), to one of their regions (the children). Much like `BranchOpInterface`, this interface provides a generic means of querying which regions are successors on entry, which regions are successors of their siblings, whether a region is "repetitive", i.e. loops, and more.
* `RegionBranchTerminatorOpInterface`, operations which represent control flow from some region of a `RegionBranchOpInterface` op, either to the parent op (e.g. returning/yielding), or to another region of that op (e.g. branching/yielding). Such operations are always children of a `RegionBranchOpInterface`, and conversely, the regions of a `RegionBranchOpInterface` must always terminate with an op that implements this interface.

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

### Successors and Predecessors

The concept of _predecessor_ and _successor_ corresponds to a parent/child relationship between nodes in a control-flow graph (CFG), where edges in the graph are directed, and describe the order in which control flows through the program. If a node $A$ transfers control to a node $B$ after it is finished executing, then $A$ is a _predecessor_ of $B$, and $B$ is a _successor_ of $A$.

Successors and predecessors can be looked at from a few similar, but unique, perspectives:

#### Relating blocks

We're generally interested in successors/predecessors as they relate to blocks in the CFG. This is of primary interest in dominance and loop analyses, as the operations belonging to a block inherit the interesting properties of those analyses from their parent block.

In abstract, the predecessor of a block is the operation which transfers control to that block. When considering what blocks are predecessors of the current block, we're deriving that by mapping each predecessor operation to its parent block.

We are often interested in specific edges of the CFG, and because it is possible for a predecessor operation to have multiple edges to the same successor block, it is insufficient to refer to these edges by predecessor op and target block alone, instead we also need to know the successor index in the predecessor op.

Unique edges in the CFG are represented in the form of the `BlockOperand` type, which provides not only references to the predecessor operation and the successor block, but also the index of the successor in the predecessor's successor storage.

#### Relating operations

This perspective is less common, but useful to be aware of.

Operations in a basic block are, generally, assumed to execute in order, top to bottom. Thus, the predecessor/successor terminology can also refer to the relationship between two consecutive operations in a basic block, i.e. if $A$ immediately precedes $B$ in a block, then $A$ is the predecessor of $B$, and $B$ is the successor of $A$.

We do not generally refer to this relationship in the compiler, except in perhaps one or two places, so as to avoid confusion due to the overloaded terminology.

#### Relating regions

Another important place in which the predecessor/successor terminology applies, is in the relationship between a parent operation and its regions, specifically when the parent implements `RegionBranchOpInterface`.

In this dynamic, the relationship exists between two points, which we represent via the `RegionBranchPoint` type, where the two points can be either the parent op itself, or any of its child regions. In practice, this produces three types of edges:

1. From the parent op itself, to any of its child regions, i.e. "entering" the op and that specific region). In this case, the predecessor is the parent operation, and the successor is the child region (or more precisely, the entry block of that region).
2. From one of the child regions to one of its siblings, i.e. "yielding" to the sibling region. In this case, the predecessor is the terminator operation of the origin region, and the successor is the entry block of the sibling tregion.
3. From a child regions to the parent operation, i.e. "returning" from the op. In this case, the predecessor is the terminator operation of the child region, and the successor is the operation immediately succeeding the parent operation (not the parent operation itself).

This relationship is important to understand when working with `RegionBranchOpInterface` and `RegionBranchTerminatorOpInterface` operations.

#### Relating call and callable

The last place where the predecessor/successor terminology is used, is in regards to inter-procedural analysis of call operations and their callees.

In this situation, predecessors of a callable are the set of call sites which refer to it; while successors of a callable are the operations immediately succeeding the call site where control will resume when returning from the callable region.

We care about this when performing inter-procedural analyses, as it dictates how the data flow analysis state is propagated from caller to callee, and back to the caller again.

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

See the [Rewrites](rewrites.md) document for more information on rewrite passes in general, including the current set of transformation passes that build on the pattern rewrite infrastructure.

Pattern rewrites are essentially transformation passes which are scheduled on a specific operation type, or any operation that implements some trait or interface; recognizes some pattern about the operation which we desire to rewrite/transform in some way, and then attempts to perform that rewrite.

Pattern rewrites are applied using the `GreedyPatternRewriteDriver`, which coordinates the application of rewrites, reschedules operations affected by a rewrite to determine if the newly rewritten IR is now amenable to further rewrites, and attempts to fold operations and materialize constants, and if so configured, apply region simplification.

These are the core means by which transformation of the IR is performed.

### Canonicalization

Canonicalization is a form of [_pattern rewrite_](#pattern-rewrites) that applies to a specific operation type that has a _canonical_ form, recognizes whether the operation is in that form, and if not, transforms it so that it is.

What constitutes the _canonical form_ of an operation, depends on the operation itself:

* In some cases, this might be ensuring that if an operation has a constant operand, that it is always in the same position - thus making pattern recognition at higher levels easier, as they only need to attempt to match a single pattern.
* In the case of control flow, the canonical form is often the simplest possible form that preserves the semantics.
* Some operations can be simplified based on known constant operands, or reduced to a constant themselves. This process is called [_constant folding_](#folding), and is an implicit canonicalization of all operations which support folding, via the `Foldable` trait.

#### Folding

Constant-folding is the process by which an operation is simplified, replaced with a simpler/less-expensive operation, or reduced to a constant value - when some or all of its operands are known constant values.

The obvious example of this, is something like `v3 = arith.add v1, v2`, where both `v1` and `v2` are known to be constant values. This addition can be performed at compile-time, and the entire `arith.add` replaced with `arith.constant`, potentially enabling further folds of any operation using `v3`.

What about when only some of the operands are constant? That depends on the operation in question. For example, something like `v4 = cf.select v1, v2, v3`, where `v1` is known to be the constant value `true`, would allow the entire `cf.select` to be erased, and all uses of `v4` replaced with `v2`. However, if only `v2` was constant, the attempt to fold the `cf.select` would fail, as no change can be made.

A fold has three outcomes:

* Success, i.e. the operation was able to be folded away; it can be erased and all uses of its results replaced with the fold outputs
* In-place, the operation was able to be simplified, but not folded away/replaced. In this case, there are no fold outputs, the original operation is simply updated.
* Failure, i.e. the operation could not be folded or simplified in any way

Operation folding can be done manually, but is largely handled via the [_canonicalization_](#canonicalization) pass, which combines folding with other pattern rewrites, as well as region simplification.

## Implementation Details

The following sections get into certain low-level implementation details of the IR, which are important to be aware of when working with it. They are not ordered in any particular way, but are here for future reference.

You should always refer to the documentation associated with the types mentioned here when working with them; however, this section is intended to provide an intro to the concepts and design decisions involved, so that you have the necessary context to understand how these things fit together and are used.

### Session

The `Session` type, provided by the `midenc-session` crate, represents all of the configuration for the current compilation _session_, i.e. invocation.

A session begins by providing the compiler driver with some inputs, user-configurable flags/options, and intrumentation handler. A session ends when those inputs have been compiled to some output, and the driver exits.

### Context

The `Context` type, provided by the `midenc-hir` crate, encapsulates the current [_session_](#session), and provides all of the IR-specific storage and state required during compilation.

In particular, a `Context` maintains the set of registered dialects, their hooks, the allocator for all IR entities produced with that context, and the uniquer for allocated value and block identifiers. All IR entities which are allocated using the `Context`, are referenced using [_entity references_](#entity-references).

The `Context` itself is not commonly used directly, except in rare cases - primarily only when extending the context with dialect hooks, and when allocating values, operands, and blocks by hand.

Every operation has access to the context which created it, making it easy to always access the context when needed.

> [!WARNING]
> You _must_ ensure that the `Context` outlives any reference to an IR entity which is allocated with it. For this reason, we typically instantiate the `Context` at the same time as the `Session`, near the driver entrypoint, and either pass it by reference, or clone a reference-counted pointer to it; only dropping the original as the compiler is exiting.

### Entity References

TODO

#### Entity Storage

TODO

##### StoreableEntity

##### ValueRange

#### Entity Lists

TODO

### Traversal

TODO

### Program Points

TODO

### Defining Dialects

TODO

#### Dialect Registration

TODO

#### Dialect Hooks

TODO

#### Defining Operations

TODO

### Builders

TODO

#### Validation

TODO

### Effects

TODO
