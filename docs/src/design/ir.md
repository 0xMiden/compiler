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
    * [Entity Lists](#entity-lists)
  * [Traversal](#traversal)
  * [Program Points](#program-points)
  * [Defining Dialects](#defining-dialects)
    * [Dialect Registration](#dialect-registration)
    * [Dialect Hooks](#dialect-hooks)
    * [Defining Operations](#defining-operations)
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

A _dialect_ is a logical grouping of operations, attributes, and associated analyses and transformations. It forms the basis on which the IR is modularized and extended.

There are currently a limited set of dialects, comprising the set of operations we currently have defined lowerings for, or can be converted to another dialect that does:

* `builtin`, which provides the `World`, `Component`, `Module`, `Function`, `GlobalVariable`, `Segment`, and `Ret` operations.
* `test`, used for testing within the compiler without external dependencies.
* `ub`, i.e. _undefined behavior_, which provides the `Poison` (and associated attribute type), and `Unreachable` operations.
* `arith`, i.e. _arithmetic_, which provides all of the mathematical operations we currently support lowerings for. This dialect also provides the `Constant` operation for all supported numeric types.
* `cf`, i.e. _control flow_, which provides all of the unstructured control flow or control flow-adjacent operations, i.e. `Br`, `CondBr`, `Switch`, and `Select`. The latter is not strictly speaking a control flow operation, but is semantically similar. This dialect is largely converted to the `scf` dialect before lowering, with the exception of `Select`, and limited support for `CondBr` (to handle a specific edge case of the control flow lifting transformation).
* `scf`, i.e. _structured control flow_, which provides structured equivalents of all the control flow we support, i.e. `If`, `While` (for both while/do and do/while loops), and `IndexSwitch` (essentially equivalent to `cf.switch`, but in structured form). The `Yield` and `Condition` operations are defined in this dialect to represent control flow within (or out of) a child region of one of the previous three ops.
* `hir` (likely to be renamed to `masm` or `vm` in the near future), which is currently used to represent the set of operations unique to the Miden VM, or correspond to compiler intrinsics implemented in Miden Assembly.

See [_defining dialects_](#defining-dialects) for more information on what dialects are responsible for in HIR.

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

All IR entities, e.g. values, operations, blocks, regions - are allocated via an arena allocator provided by the [`Context`](#context), along with any custom metadata relevant for the specific entity type. A custom smart pointer type, called `RawEntityRef`, is then used to reference those entities, while simultaneously providing access to the metadata, and enforcing Rust's aliasing rules via dynamic borrow checking.

This approach is used due to the graph-like structure of the IR itself - the ergonomics we can provide this way far outweigh the cost of dynamic borrow checking. However, it does place the burden on the programmer to carefully manage the lifetime of borrowed entities, so as to avoid aliasing conflicts. To make such issues easy to troubleshoot and fix, the `RawEntityRef` type will track both the source location of a borrow, and the location where a conflicting borrow occurs, and print this information as part of the runtime error that occurs (when compiled with the `debug_refcell` feature enabled).

There are two main "types" of `RawEntityRef` metadata:

* `()`, aliased as `UnsafeEntityRef`
* `IntrusiveLink`, aliased as `UnsafeIntrusiveEntityRef`.

In both cases, the type is aliased to reflect the underlying entity type being referenced, e.g. `BlockRef` is an `UnsafeIntrusiveEntityRef<Block>`, and `ValueRef` is an `UnsafeEntityRef<dyn Value>`.

The latter of the two is the most important, as it is used for all entity types which have a parent entity in which they are tracked by means of an intrusive doubly-linked list (called an [_entity list_](#entity-lists)), e.g. operations, blocks, regions, operands, etc. The key thing that the `UnsafeIntrusiveEntityRef` type provides, is access to the parent entity, if linked to one, and access to the previous and next sibling of the containing entity list, if present - without needing to borrow the underlying entity. This is critical for traversing the IR without needing to borrow each entity being traversed, unless one wants to explicitly visit it. Entities allocated into an `UnsafeIntrusiveEntityRef` must also implement the `EntityListItem` trait, which provides various callbacks that are invoked when inserting/removing/transferring them between the entity list of the parent entity type.

The `UnsafeEntityRef` type, in contrast, is used for entities that are not tracked in an entity list, but in [_entity storage_](#entity-storage), i.e. a small, resizeable vector of references which are stored as part in the parent entity. Examples include block arguments, operation results, and operation successors. Entities allocated into an `UnsafeEntityRef` and stored in `EntityStorage`, must also implement the `StoreableEntity` trait, which provides a similar set of callbacks to `EntityListItem` for managing the lifecycle of an entity as it is inserted, removed, etc.

#### Entity Storage

This refers to the `EntityStorage<T>` type, which abstracts over the storage of IR entities in a small, resizeable vector attached to a parent entity.

The `EntityStorage` type provides the following:

* Lifecycle management of stored entities, via the `StoreableEntity` trait
* Grouping of entities within storage, with relative indexing, support for insertion, removal, iteration, etc. This is used, for example, to use a single `EntityStorage` for all operands of an operation, while grouping operands semantically (e.g. group 0 are operands of the op itself, group 1 through N are operand groups for each successor of the operation).
* Conveniences for indexing, slicing, iterating, etc.

##### StoreableEntity

The `StoreableEntity` trait is used to ensure that, as entities are inserted/removed/transferred between instances of `EntityStorage`, that metadata about the relationship between the stored entity, and the parent, is updated accordingly.

For example, this is used to ensure that when an `OpOperand` is inserted into the operand storage of an operation, that it is added to the use list of the referenced `Value`. Conversely, when that same operand is removed from the operand storage, the use of the referenced `Value` is removed.

##### ValueRange

The `ValueRange` type is intended to provide a uniform, efficient API over slices/ranges of value-like types, in the form of `ValueRef`. It can be directly constructed from any `EntityRange` or `EntityStorage` reference of `BlockArgumentRef`, `OpOperandRef`, or `OpResultRef` type; as well as raw slices of any of those types in addition to `ValueRef`.

It supports borrowed and owned variants, iteration, indexing, and conversions.

In general, you should prefer to use `ValueRange` when working with collections of value-like types, unless you have a specific reason otherwise.

#### Entity Lists

An entity list is simply a doubly-linked, intrusive list, owned by some entity, in which references to some specific child entity type are stored.

Examples include:

* The list of regions belonging to an operation
* The list of blocks belonging to a region
* The list of operations belonging to a block
* The list of operands using a block argument/op result
* The list of symbol users referencing a symbol

In conjunction with the list itself, there are a set of traits which facilitate automatically maintaining the relationship between parent and child entity as items are inserted, removed, or transferred between parent lists:

* `EntityParent<Child>`, implemented by any entity type which has some child entity of type `Child`. This provides us with the ability to map a parent/child relationship to the offset of the intrusive linked list in the parent entity, so that we can construct a reference to it. Entities can be the parent of multiple other entity types.
* `EntityWithParent<Parent = T>`, implemented by the child entity which has some parent type `T`, this provides the inverse of `EntityParent`, i.e. the ability for the entity list infrastructure to resolve the parent type of a child entity it stores, and given a reference to the parent entity, get the relevant intrusive list for that child. Entities with a parent may only have a single parent entity type at this time.
* `EntityListItem`, implemented by any entity type which can be stored in an entity list. This trait provides the set of callbacks that are invoked by the entity list infrastructure when modifying the list (inserting, removing, and transferring items). This trait is public, but the entity list infra actually uses a different trait, called `EntityListTraits`, which is specialized based on whether the list items implement just `EntityListItem`, or both `EntityListItem` and `EntityWithParent`. The specialization for the latter ensures that the parent/child relationship is updated appropriately by the entity list itself, rather than requiring `EntityListItem` implementations to do so.

We use intrusive linked lists for storing sets of entities that may be arbitrarily large, and where the O(1) insertion, removal, splicing and splitting makes up for the less cache-friendly iteration performance. Given a reference to an entity, we can always construct a cursor to that element of the list, and traverse the list from there, or modify the list there - this is a much more frequent operation than iterating these lists.

### Traversal

Before we can discuss the various ways of traversing the IR, we need to clarify what aspect of the IR we're interested in traversing:

* The data flow graph, i.e. nodes are operations, edges are formed by operands referencing values.
* The control flow graph, i.e. nodes are either operations, blocks, or regions; and edges are formed by operations which transfer control (to the next op, to a specific set of successor blocks or regions).
* The call graph, i.e. nodes are symbols which implement `CallableOpInterface`, and edges are formed by operations which implement `CallOpInterface`.

There are also a few traversal primitives which are commonly used:

* Any `UnsafeIntrusiveEntityRef` for a type which implements `EntityListItem`, provides `next` and `prev` methods which allow navigating to the next/previous sibling item in the list, without borrowing the entity itself. In some cases, being siblings in an entity list does not mean that the items are near each other, e.g. the only thing shared in common between uses of a `Symbol`, is the symbol they refer to, but their order in the symbol use-list has no semantic meaning. In others, being siblings mean that the items are actually located next to each other in that order, e.g. operations in a block.
* Similarly, any `UnsafeIntrusiveEntityRef` for a type which implements `EntityWithParent`, provides a `parent` method which allow navigating to the parent entity from the child, without borrowing either of them.
* All child entities "owned" by a parent entity, are stored in either a [_entity list_](#entity-lists) or [_entity storage_](#entity-storage) attached to that entity.

These three primitives provide the core means by which one can navigate the relevant graph in any direction.

Another thing to be aware of, is that relationships between entities where there may be multiple edges between the same two entities, are typically represented using a special node type. For example:

* `OpOperand` represents a use of a `Value` by an operation. In order to maintain the use-def graph of values, each value type, e.g. `BlockArgument`, has its own entity list for `OpOperand`s. What is stored in the relevant entity storage of the operation then, are `OpOperandRef`s. So while operands are intuitively something we think of as an intrinsic part of an operation, they are actually their own IR entity, which is then stored by reference both in the operation, and in the use-list of the value they reference.
* `BlockOperand` represents a "use" of a `Block` by an operation as a successor. This type is responsible for forming the edges of the CFG, and so much like `OpOperand`, the `Block` type has an entity list for `BlockOperand`s, effectively the set of that block's predecessors; while the operation has entity storage for `BlockOperandRefs` (or more precisely, `SuccessorInfo`, of which `BlockOperandRef` is one part).
* `SymbolUse` represents a use of a `Symbol` by an operation. This underpins the maintenance of the call graph. Unlike operands, symbol usage is not tracked as a fundamental part of every operation, i.e. there is no dedicated `symbols` field of the `Operation` type which provides the entity storage for `SymbolUseRef`s, nor is there a field which defines the entity list. Instead, the symbol use list of an op that implements `Symbol`, must be defined as part of the concrete operation type. Similarly, any concrete operation type that can use/reference a `Symbol` op, must determine for itself how it will store that use. For this reason, symbol maintenance is a bit less ergonomic than other entity types.

We now can explore the different means by which the IR can be traversed:

1. Using the raw traversal primitives described above.
2. The `Graph` trait
3. The `Walk` and `RawWalk` traits
4. `CallOpInterface` and `CallableOpInterface` (specifically for traversing the call graph)

#### The `Graph` trait

The `Graph` trait is an abstraction for directed graphs, and is intended for use when writing generic graph algorithms which can be reused across multiple graph types. For example, the implementation of pre-order and post-order visitors is implemented over any `Graph`.

This trait is currently implemented for `Region`, `Block`/`BlockRef`, and `DomTreeNode`, as so far they are the only types we've needed to visit using this abstraction, primarily when performing pre-order/post-order visits of the CFG and/or dominance tree.

#### The `Walk` and `RawWalk` traits

The `Walk` trait defines how to walk all children of a given type, which are contained within the type for which `Walk` is being implemented. For example:

* `Walk<Operation> for Region` defines how to traverse a region to visit all of the operations it contains, recursively.
* `Walk<Region> for Operation` defines how to traverse an operation to visit all of the regions it contains, recursively.

The difference between `Walk` and `RawWalk`, is that `Walk` requires borrowed references to the types it is implemented for, while `RawWalk` relies on the traversal primitives we introduced at the start of this section, to avoid borrowing any of the entities being traversed, with the sole exception being to access child entity lists long enough to get a reference to the head of the list. If we are ever mutating the IR as we visit it, then we use `RawWalk`, otherwise `Walk` tends to be more ergonomic.

The `Walk` and `RawWalk` traits provide both pre- and post-order traversals, which dictates in what order the visit callback is invoked. You can further dictate the direction in which the children are visited, e.g. are operations of a block visited forward (top-down), or backward (bottom-up)? Lastly, if you wish to be able to break out of a traversal early, the traits provide variants of all functions which allow the visit callback to return a `WalkResult` that dictates whether to continue the traversal, skip the children of the current node, or abort the traversal with an error.

#### `CallOpInterface` and `CallableOpInterface`

These two interfaces provide the means for navigating the call graph, which is not explicitly maintained as its own data structure, but is rather implied by the connections between symbols and symbol uses which implement these interfaces, in a program.

One is generally interested in the call graph for one of a couple reasons:

1. Determine if a function/callable is used
2. Visit all callers of a function/callable
3. Visit the call graph reachable from a given call site as part of an analysis
4. Identify cycles in the call graph

For 1 and 2, one can simply use the `Symbol` use-list: an empty use-list means the symbol is unused. For non-empty use-lists, one can visit every use, determine if that use is by a `CallOpInterface`, and take some action based on that.

For 2 and 3, the mechanism is essentially identical:

1. Assume that you are starting from a call site, i.e. an operation that implements `CallOpInterface`. Your first step is generally going to be to determine if the callable is a `SymbolPath`, or a `ValueRef` (i.e. an indirect call), using the `get_callable_for_callee` interface method.
2. If the callable is a `ValueRef`, you can try to trace that value back to an operation that materialized it from a `Symbol` (if that was the case), so as to make your analysis more precise; but in general there can be situations in which it is not possible to do so. What this means for your analysis depends on what that analysis is.
3. If the callable is a `SymbolPath`, then we need to try and resolve it. This can be done using the `resolve` or `resolve_in_symbol_table` interface methods. If successful, you will get a `SymbolRef` which represents the callable `Symbol`. If the symbol could not be resolved, `None` is returned, and you can traverse that edge of the call graph no further.
4. Once you've obtained the `SymbolRef` of the callable, you can borrow it, and then cast the `&dyn Symbol` reference to a `&dyn CallableOpInterface` reference using `symbol.as_symbol_operation().as_trait::<dyn CallableOpInterface>()`.
5. With that reference, you call the `get_callable_region` interface method. If it returns `None`, then the callable represents a declaration, and so it is not possible to traverse the call graph further. If it returns a `RegionRef`, then you proceed by traversing all of the operations in that region, looking for more call sites to visit.

### Program Points

A _program point_ essentially represents a cursor in the CFG of a program. Specifically, program points are defined as a position before, or after, a block or operation. The type that represents this in the IR is `ProgramPoint`, which can be constructed from just about any type of block or operation reference.

Program points are used in a few ways:

* To specify where a block or operation should be inserted
* To specify at what point a block should be split into two
* To specify at what point a block should be merged into another
* To anchor data flow analysis state, e.g. the state before and after an operation, or the state on entry and exit from a block.

Currently, we distinguish the points representing "before" a block (i.e. at the start of the block), and "after" a block (i.e. at the end of the block), from the first and last operations in the block, respectively. Thus, even though a point referring to the start of the block, and a point referring to "before" the first operation in that block, effectively refer to the same place, we currently treat them as distinct locations. This may change in the future, but for now, it is something to be aware of.

The `ProgramPoint` type can be reified as a literal cursor into the operation list of a block, and then used to perform some action relative to that cursor.

The key thing to understand about program points has to do with the relationship between before/after (or start/end) and what location that actually refers to. The gist, is that a program point, when materialized as a cursor into an operation list, will always have the cursor positioned such that if you inserted a new operation at that point, it would be placed where you expect it to be - i.e. if "before" an operation, the insertion will place the new item immediately preceding the operation referenced by the program point. This is of particular importance if inserting multiple operations using the same point, as the order in which operations will be inserted depends on whether the position is before or after the point. For example, inserting multiple items "before" an operation, will have them appear in that same order in the containing block. However, inserting multiple items "after" an operation, will have them appear in reverse order they were inserted (i.e. the last to be inserted will appear first in the block relative to the others).


### Defining Dialects

Defining a new dialect is as simple as defining a struct type which implements the `Dialect` trait. For example:

```rust
use midenc_hir::{Dialect, DialectInfo};

#[derive(Debug)]
pub struct MyDialect {
    info: DialectInfo,
}

impl Dialect for MyDialect {
    fn info(&self) -> &DialectInfo {
        &self.info
    }
}
```

One last thing remains before the dialect is ready to be used, and that is [_dialect registration_](#dialect-registration).


#### Dialect Registration

Dialect registration is the means by which a dialect and its operations are registered with the [`Context`](#context), such that operations of that dialect can be built.

First, you must define the `DialectRegistration` implementation. To extend our example from above:

```rust
impl DialectRegistration for MyDialect {
    const NAMESPACE: &'static str = "my";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<FooOp>();
    }
}
```

This provides all of the information needed by the `Context` to register our dialect, i.e. what namespace the dialect uses, and what operations are registered with the dialect.

The next step is to actually register the dialect with a specific `Context`. In general, this is automatically handled for you, i.e. whenever an operation of your dialect is being built, a call to `context.get_or_register_dialect::<MyDialect>()` is made, so as to get a reference to the dialect. If the dialect has not yet been registered, a fresh instance will be constructed, all registered [_dialect hooks_](#dialect-hooks) will be invoked, and the initialized dialect registered with the context, before returning a reference to the registered instance.

#### Dialect Hooks

In some cases, it is necessary/desirable to extend a dialect with types/behaviors that we do not want (or cannot make) dependencies of the dialect itself. For example, extending the set of traits/interfaces implemented by operations of some dialect.

The mechanism by which this is done, is in the form of _dialect hooks_, functions which are invoked when a dialect is being registered, before the main dialect registration callbacks (e.g. `register_operations`) are invoked. Hooks are provided a reference to the raw `DialectInfo`, which can be modified as if the hook is part of the `DialectRegistration` itself.

Of particular use, is the `DialectInfo::register_operation_trait` method, which can be used to register a trait (or interface) to an operation before it is registered by the dialect. These "late-bound" traits are then added to the set of traits/interfaces defined as part of the operation itself, when the operation is registered with `DialectInfo::register_operation`.

We currently use dialect hooks for:

* Attaching the `midenc_codegen_masm::Lowering` trait to all operations for which we have defined its lowering to Miden Assembly.
* Attaching the `midenc_hir_eval::Eval` trait to all operations for which we have defined evaluation semantics, for use with the HIR evaluator.

#### Defining Operations

Defining operations involves a non-trivial amount of boilerplate if done by hand, so we have defined the `#[operation]` proc-macro attribute which takes care of all the boilerplate associated with defining a new operation.

As a result, defining an operation looks like this (using an example from `midenc_dialect_arith`):

```rust
use midenc_hir::{derive::operation, effects::*, traits::*, *};

use crate::ArithDialect;

/// Two's complement sum
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Add {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Add {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

// MemoryEffectOpInterface is an alias for EffectOpInterface<MemoryEffect>
impl EffectOpInterface<MemoryEffect> for Add {
    fn has_no_effect(&self) -> bool {
        true
    }

    fn effects(
        &self,
    ) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![])
    }
}
```

To summarize:

* `dialect` specifies the dialect to which the operation will be registered
* `traits` specifies the set of derivable traits for this operation.
* `implements` specifies the set of traits/interfaces which will be manually implemented for this operation. If any of the listed traits/interfaces are _not_ implemented, a compiler error will be emitted.
* The fields of the `Add` struct represent various properties of the operation. Their meaning depends on what (if any) attributes they are decorated with:
  * The `#[operand]` attribute represents an expected operand of this operation, and the field type represents the type constraint to apply to it.
  * The `#[result]` attribute represents a result produced by this operation, and the field type represents the type constraint to apply to it.
  * The `#[attr]` attribute represents a required attribute of this operation. If the `#[default]` attribute is present, it is treated as an optional attribute.
  * If a field has no attributes, or only `#[default]`, it is defined as part of the concrete operation struct, and is considered an internal detail of the op.
  * All other fields are not actually stored as part of the concrete operation type, but as part of the underlying `Operation` struct, and methods will be generated in an `impl Add` block that provide access to those fields in all the ways you'd expect.
  * The `#[operand]`, `#[attr]`, and undecorated fields are all expected, in that order, as arguments to the op builder when constructing an instance of this op.

There are a variety of field attributes and options for them that are not shown here. For now, the best reference for these is looking at the set of current dialects for an operation that is similar to what you want to define. You can also look at the implementation of the `#[operation]` proc-macro in `midenc_hir_macros` as the authoritative source on what is supported and how it affects the output. In the future we will provide more comprehensive documentation for it.


### Builders

Constructing the IR is generally done via implementations of the `Builder` trait, which includes implementations of the `Rewriter` trait, as the former is a super-trait of the latter. Typically, this means the `OpBuilder` type.

Aside from a variety of useful APIs e.g. creating blocks, setting the insertion point of the builder, etc., most commonly you will be constructing specific operations, which is done in one of two ways:

* The lowest level primitive, actually provided by the `BuilderExt` trait due to the need to keep `Builder` object-safe, is its `create<T, Args>` method. This method produces an implementation of `BuildableOp` for the specified operation type (i.e. the `T` type parameter), and signature (i.e. the `Args` type parameter, which must be a tuple type). The desired operation is then constructed by applying the necessary arguments to the `BuildableOp` as a function (because `BuildableOp` is an implementation of the `FnOnce` closure type).
* Much more commonly however, this boilerplate will be abstracted away for you by dialect-specific extensions of the `Builder` trait, e.g. the `BuiltinOpBuilder` trait extends all `Builder` implementations with methods for constructing any of the `builtin` dialect operations, such as the `ret` method, which constructs the `builtin.return` operation. All of the dialects used by the compiler define such a trait, all that is required is to bring it into scope in order to construct the specific operations you want.

> [!NOTE]
> All of the boilerplate for constructing an operation from a `Builder` is generated for you when defining an operation type with the `#[operation]` proc-macro attribute. A key piece of this underlying infrastructure, is the `OperationBuilder` type, which is used to construct the `Operation` that underlies any concrete `Op` implementation. The `OperationBuilder` is also where new operations are verified and inserted into the underlying `Builder` during construction.

If the builder has a valid insertion point set, then either of the above methods will also insert the constructed operation at that point.

The primary difference between `Rewriter` and `Builder`, is that the `Rewriter` API surface is comprised primarily of methods that modify the IR in some way, e.g. moving things around, splitting and merging blocks, erasing operations, replacing ops with values, values with values, etc. Because `Builder` is a super-trait, it can be used both to construct new IR, and to rewrite existing IR.

#### Validation

A key aspect of the IR design, is to enable as much boilerplate as possible to be generated from the description of an operation, especially when it comes to verifying the validity of an operation using the type constraints of it's operands and results, and the traits/interfaces it implements.

Operations are currently validated when constructed by the `OperationBuilder` that underpins the `BuildableOp` implementation generated by the `#[operation]` proc-macro attribute. There are some known issues with doing it here, so we are likely to revisit this at some point, but for now this is the first place where verification occurs. It may also be triggered manually at any point by calling the `Operation::verify` method.

There are two traits which underpin the verification infrastructure:

* `Verify<Trait>` which represents the verification of `Trait` against `Self`, which is an operation type. Typically, all traits with associated verification rules, implement this trait for all `T: Op + Trait`.
* `Verifier<Trait>` which is a trait used to facilitate the generation of verification boilerplate specialized against a specific concrete operation type, without having to be aware of what operation traits/interfaces have associated `Verify` implementations. Instead, no-op verifiers are elided by the Rust compiler using `const { }` blocks. The method by which this is done is highly reliant on Rust's specialization infrastructure and type-level trickery.

In the future, we will likely move verification out of the `OperationBuilder`, into a dedicated pass that is run as part of a pass pipeline, and invoked on each operation via `Operation::verify`. This will enable more robust verification than is currently possible (as operations are not yet inserted in the IR at the point verification is applied currently).

### Effects

Side effects are an important consideration during analysis and transformation of operations in the IR, particularly when evaluating whether operations can be reordered, re-materialized, spilled and reloaded, etc.

The infrastructure underpinning the management and querying of effects is built on the following pieces:

* The `Effect` trait, which is a marker trait for types which represent an effect, e.g. `MemoryEffect` which represents effects on memory, such as reading and writing, allocation and freeing.
* The `EffectOpInterface<T: Effect>` interface, which is implemented for any operation for which the effect `T` is specified, and provides a number of useful methods for querying effects of a specific operation instance.
* The `Resource` trait, which represents a type of resource to which an effect can apply. In many cases, one will use `DefaultResource`, which is a catch-all resource that implies a global effect. However, it is also possible to scope effects to a specific resource, such as a specific address range in memory. This could permit operations with disjoint effects to be reordered relative to one another, when that would otherwise not be allowed if the effects were global.
* The `EffectInstance` type, which provides metadata about a specific effect, any attributes that apply to it, the resource affected, etc.

It should be noted that the choice to implement `EffectOpInterface` for an operation is _not_ based on whether the operation _has_ the effect; but rather, it is based on whether the behavior of the operation with respect to that effect is _specified_ or not.

For example, most operations will have a known effect (or lack thereof) on memory, e.g. `arith.add` will never have a memory effect, while `hir.load` by definition will read from memory. In some cases, whether an operation will have such an effect is not a property of the operation itself, but rather operations that may be nested in one of its child regions, e.g. `scf.if` has no memory effects in and of itself, but one of its regions might contain an operation which does, such as an `hir.store` in the "then" region. In this case, it does not make sense for `scf.if` to implement `EffectOpInterface<MemoryEffect>`, because memory effects are not specified for `scf.if`, but are instead derived from its regions.

When `EffectOpInterface` is not implemented for some operation, then one must treat the operation as conservatively as possible in regards to the specific effect. For example, `scf.call` does not implement this interface for `MemoryEffect`, because whether the call has any memory effects depends on the function being called. As a result, one must assume that the `scf.call` could have any possible memory effect, unless you are able to prove otherwise using inter-procedural analysis.

#### Memory Effects

The infrastructure described above can be used to represent any manner of side effect. However, the compiler is currently only largely concerned with effects on memory. For this, there are a few more specific pieces:

* The `MemoryEffectOpInterface` trait alias, which is just an alias for `EffectOpInterface<MemoryEffect>`.
* The `MemoryEffect` type, which represents the set of memory effects we care about.
* The `HasRecursiveMemoryEffects` trait, which should be implemented on any operation whose regions may contain operations that have memory effects.
* The `Operation::is_memory_effect_free` method, which returns a boolean indicating whether the operation is known not to have any memory effects.

In most places, we're largely concerned with whether an operation is known to be memory effect free, thus allowing us to move that operation around freely. We have not started doing more sophisticated effect analysis and optimizations based on such analysis.
