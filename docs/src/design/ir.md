# High-Level Intermediate Representation (HIR)

This document describes the concepts, usage, and overall structure of the intermediate
representation used by `midenc`.

## Introduction

TODO

## Concepts

### Components

A _component_ is a named entity that encapsulates one or more [_interfaces_](#interfaces), and comes
in two forms:

* An _executable_ component, which has a statically-defined entrypoint, a function which initializes
and executes a program encapsulated by the component.
* A _library_ component, which exports one or more interfaces, and can be used as a dependency of
other components.

We also commonly refer to executable components as _programs_, and library components as _libraries_,
which also correspond to the equivalent concepts in Miden Assembly. However, components are a more
general abstraction over programs and libraries, where the distinction is mostly one of intended
use and/or convention.

Components can have zero or more dependencies, which are expressed in the form of interfaces that
they require instances of at runtime. Thus any component that provides the interface can be used
to satisfy the dependency.

A component _instance_ refers to a component that has had all of its dependencies resolved
concretely, and is thus fully-defined.

A component _definition_ specifies four things:

1. The name of the component
2. The interfaces it imports
3. The interfaces it exports
4. The [_modules_](#modules) which implement the exported interfaces concretely

### Interfaces

An _interface_ is a named entity that describes one or more [_functions_](#functions) that it
exports. Conceptually, an _interface_ loosely corresponds to the notion of a module, in that both
a module and an interface define a namespace, in which one or more functions are exported.

However, an _interface_, unlike a module, is abstract, and does not have any internal structure.
It is more like a trait, in that it abstractly represents a set of named behaviors implemented by
some [component](#components).

### Modules

A module is primarily two things:

1. A container for one or more functions belonging to a common namespace
2. A concrete implementation of one or more [interfaces](#interfaces)

Functions within a module may be exported, and so a module always has an implicit interface
consisting of all of its exported functions. Functions which are _not_ exported, are only visible
within the module, and do not form a part of the implicit interface of a module.

Module names are used to name the implicit interface of the module. Thus, within a component, both
imported interfaces, and the implicit interfaces of all modules it defines, can be used to resolve
function references in those modules.

A module defines a symbol table, whose entries are the functions defined in that module.

### Functions

A function is a special type of [_operation_](#operations). It is special in the following ways:

* A function has a [_symbol_](#symbols-and-symbol-tables), and thus declares an entry in the nearest
containing [_symbol table_](#symbols-and-symbol-tables).
* A function is _isolated from above_, i.e. the contents of the function cannot escape the function,
nor reference things outside the function, except via symbol table references. Thus entities such as
[_values_](#values) and [_blocks_](#blocks) are function-scoped, if not more narrowly scoped, in the
case of operations with nested [_regions_](#regions).

A function has an arbitrary set of parameters and results, corresponding to its type signature. A
function also has the notion of an _application binary interface_ (ABI), which drives how code is
generated for both caller and callee. For example, a function may have a specific calling convention
as a whole, and specific parameters/results may have type-specific  semantics declared, such as
whether to zero- or sign-extend the value if the input is of a smaller range.

A function always consists of a single [_region_](#regions), called the _body_, with at least one
[_block_](#blocks), which is called the _entry block_. The block parameters of the entry block
always correspond to the function parameters, i.e. the arity and type of the block parameters must
match the function signature.

Additionally, a function has an additional constraint on its body, which is that all blocks in the
region must end with one of a restricted set of _terminator_ operations: any branch operation, which
transfers control between blocks of the region; the `unreachable` operation, which will result in
aborting the program if executed; or the `return` operation, which must return the same arity and
type of values declared in the function signature.

### Global Variables

A global variable is a second special type of [_operation_](#operations), after
[_functions_](#functions):

* A global variable has a [_symbol_](#symbols-and-symbol-tables), and declares an entry in the
nearest containing [_symbol table_](#symbols-and-symbol-tables).
* The _initializer_ of a global variable is, like function bodies, _isolated from above_.

A global variable may have an _initializer_, a single region/single block body which is implicitly
executed to initialize the value of the global variable. The initializer must be statically
evaluatable, i.e. a "constant" expression. In most cases, this will simply be a constant value, but
some limited forms of constant expressions are permitted.

### Symbols and Symbol Tables

A _symbol_ is simply a named entity, e.g. a function `foo` is a symbol whose value is `foo`.
On their own, symbols aren't particularly useful. This is where the concept of a _symbol table_
becomes important.

A _symbol table_ is a collection of uniqued symbols belonging to the same namespace, i.e. every
symbol has a single entry in the symbol table, regardless of entity type. Thus, it is not permitted
to have both a function and a global variable with the same name, in the same symbol table. If such
a thing needed to be allowed, perhaps because the namespace for functions and global variables are
separate, then you would use a per-entity symbol table.

For our purposes, a _module_ defines a symbol table, and both functions and global variables share
that table. We do not currently use symbol tables for anything else.

### Operations

An _operation_ is the most important entity in HIR, and the most abstract. In the _Regionalized
Value State Dependence Graph_ paper, the entire representation described there consists of various
types of operations. In HIR, we do not go quite that abstract, however we do take a fair amount of
inspiration from that paper, as well as from MLIR.

Operations consist of the following pieces:

* Zero or more [_regions_](#regions) and their constituent [_blocks_](#blocks)
* Zero or more [_operands_](#operands), i.e. arguments or inputs
* Zero or more _results_, or outputs, one of the two ways that [_values_](#values) can be introduced
* Zero or more [_successors_](#successors), in the case of operations which transfer control to
another block in the same region.
* Zero or more [_attributes_](#attributes), the semantics of which depend on the operation.
* Zero or more [_traits_](#traits), implemented by the operation.

An operation always belongs to a _block_ when in use.

As you can see, this is a highly flexible concept. It is capable of representing modules and
functions, as well as primitive instructions. It can represent both structured and unstructured
control-flow. There is very little in terms of an IR that _can't_ be represented using operations.

However, in our case, we use operations for five specific concepts:

* Functions (the first of two special ops)
* Global Variables (the second of two special ops)
* Structured Control Flow (if/then, do/while and for loops)
* Unstructured Control Flow (br, cond_br, switch, ret)
* Primitive Instructions (i.e. things which correspond to the target ISA, e.g. `add`, `call`, etc.)

For the most part, the fact that functions and global variables are implemented using operations
is not particularly important. Instead, most operations you will interact with are of the other
three varieties. While we've broken them up into three categories, for the most part, they aren't
actually significantly different. The primary difference is that the unstructured control-flow ops
are valid _terminators_ for blocks, in multi-block regions, while the structured control-flow ops
are not, and only a few special cases of primitive ops are also valid terminators (namely the
`ret` and `unreachable` ops). For most primitive and structured control-flow ops, their behavior
appears very similar: they take some operands, perform some action, and possibly return some
results.

### Regions

A _region_ encapsulates a control-flow graph (CFG) of one or more [_basic blocks_](#blocks). In HIR,
the contents of a region are in _single-static assignment_ (SSA) form, meaning that values may only
be defined once, definitions must [_dominate_](#dominance-relation) uses, and operations in the CFG
described by the region are executed one-by-one, from the entry block of the region, until control
exits the region (e.g. via `ret` or some other terminator instruction).

The order of operations in the region closely corresponds to their scheduling order, though the
code generator may reschedule operations when it is safe - and more efficient - to do so.

Operations in a region may introduce nested regions. For example, the body of a function consists
of a single region, and it might contain an `if` operation that defines two nested regions, one for
the true branch, and one for the false branch. Nested regions may access any [_values_](#values) in
an ancestor region, so long as those values dominate the operation that introduced the nested region.
The exception to this are operations which are _isolated from above_. The regions of such an
operation are not permitted to reference anything defined in an outer scope, except via
[_symbols_](#symbols-and-symbol-tables). For example, [_functions_](#functions) are an operation
which is isolated from above.

The purpose of regions, is to allow for hierarchical/structured control flow operations. Without
them, representing structured control flow in the IR is difficult and error-prone, due to the
semantics of SSA CFGs, particularly with regards to analyses like dominance and loops. It is also
an important part of what makes [_operations_](#operations) such a powerful abstraction, as it
provides a way to generically represent the concept of something like a function body, without
needing to special-case them.

A region must always consist of at least one block (the entry block), but not all regions allow
multiple blocks. When multiple blocks are present, it implies the presence of unstructured control
flow, as the only way to transfer control between blocks is by using unstructured control flow
operations, such as `br`, `cond_br`, or `switch`. Structured control flow operations such as `if`,
introduce nested regions consisting of only a single block, as all control flow within a structured
control flow op, must itself be structured. The specific rules for a region depend on the semantics
of the containing operation.

### Blocks

A _block_, or _basic block_, is a set of one or more [_operations_](#operations) in which there is
no control flow, except via the block _terminator_, i.e. the last operation in the block, which is
responsible for transferring control to another block, exiting the current region (e.g. returning
from a function body), or terminating program execution in some way (e.g. `unreachable`).

A block may declare _block parameters_, the only other way to introduce [_values_](#values) into
the IR, aside from operation results. Predecessors of a block must ensure that they provide
arguments for all block parameters when transfering control to the block.

Blocks always belong to a [_region_](#regions). The first block in a region is called the _entry
block_, and is special in that its block parameters (if any) correspond to whatever arguments
the region accepts. For example, the body of a function is a region, and the entry block in that
region must have a parameter list that exactly matches the arity and type of the parameters
declared in the function signature. In this way, the function parameters are materialized as
SSA values in the IR.

### Values

A _value_ represents terms in a program, temporaries created to store data as it flows through the
program. In HIR, which is in SSA form, values are immutable - once created they cannot be changed
nor destroyed. This property of values allows them to be reused, rather than recomputed, when the
operation that produced them contains no side-effects, i.e. invoking the operation with the same
inputs must produce the same outputs. This forms the basis of one of the ways in which SSA IRs can
optimize programs.

> [!NOTE]
> One way in which you can form an intuition for values in an SSA IR, is by thinking of them as
> registers in a virtual machine with no limit to the number of machine registers. This corresponds
> well to the fact that most values in an IR, are of a type which corresponds to something that can
> fit in a typical machine register (e.g. 32-bit or 64-bit values, sometimes larger).
>
> Values which cannot be held in actual machine registers, are usually managed in the form of heap
> or stack-allocated memory, with various operations used to allocate, copy/move, or extract smaller
> values from them. While not strictly required by the SSA representation, this is almost always
> effectively enforced by the instruction set, which will only consist of instructions whose
> operands and results are of a type that can be held in machine registers.

Value _definitions_ (aka "defs") can be introduced in two ways:

1. Block parameters. Most notably, the entry block for function bodies materializes the function
parameters as values via block parameters. Block parameters are also used at places in the CFG
where two definitions for a single value are joined together. For example, if the value assigned to
a variable in the source language is assigned conditionally, then in the IR, there will be a block
with a parameter corresponding to the value of that variable after it is assigned. All uses after
that point, would refer to that block parameter, rather than the value from a specific branch.
Similarly, loop-carried variables, such as an iteration count, are typically manifested as block
parameters of the block corresponding to the loop header.
2. Operation results. The most common way in which values are introduced.

Values have _uses_ corresponding to operands or successor arguments (special operands which are used
to satisfy successor block parameters). As a result, values also have _users_, corresponding to the
specific operation and operand forming a _use.

All _uses_ of a value must be [_dominated_](#dominance-relation) by its _definition_. The IR is
invalid if this rule is ever violated.

### Operands

An _operand_ is a [_value_](#values) or [_immediate_](#immediates) used as an argument to an
operation.

Beyond the semantics of any given operation, operand ordering is only significant in so far as it
is used as the order in which those items are expected to appear on the operand stack once lowered
to Miden Assembly. The earlier an operand appears in the list of operands for an operation, the
closer to the top of the operand stack it will appear.

Similarly, the ordering of operand results also correlates to the operand stack order after
lowering. Specifically, the earlier a result appears in the result list, the closer to the top of
the operand stack it will appear after the operation executes.

### Immediates

An _immediate_ is a literal value, typically of integral type, used as an operand. Not all
operations support immediates, but those that do, will typically use them to attempt to perform
optimizations only possible when there is static information available about the operands. For
example, multiplying any number by 2, will always produce an even number, so a sequence such as
`mul.2 is_odd` can be folded to `false` at compile-time, allowing further optimizations to occur.

Immediates are separate from _constants_, in that immediates _are_ constants, but specifically
constants which are valid operand values.

### Attributes

An _attribute_ is (typically optional) metadata attached to an IR entity. In HIR, attributes can be
attached to functions, global variables, and operations.

Attributes are stored as a set of arbitrary key-value data, where values can be one of four types:

* `unit`, Attributes of this value type are usually "marker" attributes, i.e. they convey their
information simply by being present.
* `bool`, Attributes of this value type are somewhat similar to those of `unit` type, but by
carrying a boolean value, they can be used to convey both positive and negative meaning. For
example, you might want to support explicit inlining with `#[inline(true)]`, and prevent any form
of inlining with `#[inline(false)]`. Here, `unit` would be insufficient to describe both options
under a single attribute.
* `int`, Attributes of this value type are used to convey numeric metadata. For example, inliner
thresholds, or some other kind of per-operation limits.
* `string`, Attributes of this value type are useed to convey arbitrary values. Most commonly
you might see this type with things that are enum-like, e.g. `#[cc(fast)]` to specify a particular
calling convention for a function.

Some attributes are "first-class", in that they are defined as part of an operation. For example,
the calling convention of a function is an intrinsic attribute of a function, and feels like a
native part of the `Function` API - rather than having to look up the attribute, and cast the value
to a more natural Rust type, you can simply call `function.calling_convention()`.

Attributes are not heavily used at this time, but are expected to serve more purposes in the future
as we increase the amount of information frontends need to convey to the compiler backend.

### Traits

A _trait_ defines some behavior that can be implemented by an operation. This allows operations
to operated over generically in an analysis or rewrite, rather than having to handle every possible
concrete operation type. This makes passes less fragile to changes in the IR in general, and allows
the IR to be extended without having to update every single place where operations are handled.

An operation can be cast to a specific trait that it implements, and trait instances can be
downcast to the concrete operation type if known.

There are a handful of built-in traits, used to convey certain semantic information about the
operations they are attached to, and in particular, are used to validate those operations, for
example:

* `IsolatedFromAbove`, a marker trait that indicates that regions of the operation it is attached to
cannot reference items from any parents, except via [_symbols_](#symbols-and-symbol-tables).
* `Terminator`, a marker trait for operations which are valid block terminators
* `ReturnLike`, a trait that describes behavior shared by instructions that exit from an enclosing
region, "returning" the results of executing that region. The most notable of these is `ret`, but
`yield` used by the structured control flow ops is also return-like in nature.
* `BranchOp`, a trait that describes behavior shared by all unstructured control-flow branch
instructions, e.g. `br`, `cond_br`, and `switch`.
* `ConstantLike`, a marker trait for operations that produce a constant value
* `Commutative`, a marker trait for binary operations that exhibit commutativity, i.e. the order of
the operands can be swapped without changing semantics.

There are others as well, responsible for aiding in type checking, decorating operations with the
types of side effects they do (or do not) exhibit, and more.

### Successors and Predecessors

The concept of _predecessor_ and _successor_ corresponds to a parent/child relationship in a
control-flow graph (CFG), where edges in the graph are directed, and describe the order in which
control flows through the program. If a node $A$ transfers control to a node $B$ after it is
finished executing, then $A$ is a _predecessor_ of $B$, and $B$ is a _successor_ of $A$.

Successors and predecessors can be looked at from two similar, but slightly different, perspectives:

1. In terms of operations. In an SSA CFG, operations in a basic block are executed in order, and
thus the successor of an operation in the block, is the next operation to be executed in that block,
with the predecessor being the inverse of that relationship. At basic block boundaries, the
successor(s) of the _terminator_ operation, are the set of operations to which control can be
transferred. Likewise, the predecessor(s) of the first operation in a block, are the set of
terminators which can transfer control to the containing block. This is the most precise, but is not
quite as intuitive as the alternative.
2. In terms of blocks. The successor(s) of a basic block, are the set of blocks to which control may
be transferred when exiting the block. Likewise, the precessor(s) of a block, are the set of blocks
which can transfer control to it. We are most frequently dealing with the concept of successors and
predecessors in terms of blocks, as it allows us to focus on the interesting parts of the CFG. For
example, the dominator tree and loop analyses, are constructed in terms of a block-oriented CFG,
since we can trivially derive dominance and loop information for individual ops from their
containing blocks.

Typically, you will see successors as a pair of `(block_id, &[value_id])`, i.e. the block to which
control is transferred, and the set of values being passed as block arguments. On the other hand,
predecessors are most often a pair of `(block_id, terminator_op_id)`, i.e. the block from which
control originates, and the specific operation responsible.

### Dominance Relation

In an SSA IR, the concept of _dominance_ is of critical importance. Dominance is a property of the
relationship between two or more entities and their respective program points. For example, between
the use of a value as an operand for an operation, and the definition of that value; or between a
basic block and its successors. The dominance property is anti-symmetric, i.e. if $A$ dominates $B$,
then $B$ cannot dominate $A$, unless $A = B$. Put simply:

> Given a control-flow graph $G$, and a node $A \in G$, then $\forall B \in G$, $A dom B$ if all
> paths to $B$ from the root of $G$, pass through $A$.
>
> Furthermore, $A$ _strictly_ dominates $B$, if $A \neq B$.

An example of why dominance is an important property of a program, can be seen when considering the
meaning of a program like so (written in pseudocode):

```
if (...) {
  var a = 1;
}

foo(a)
```

Here, the definition of `a` does not dominate its usage in the call to `foo`. If the conditional
branch is ever false, `a` is never defined, nor initialized - so what should happen when we reach
the call to `foo`?

In practice, of course, such a program is rarely possible to expresss in a high-level language,
however in a low-level CFG, it is possible to reference values which are defined somewhere in the
graph, but in such a way that is not _legal_ according to the "definitions must dominate uses"
rule of SSA CFGs. The dominance property is what we use to validate the correctness of the IR, as
well as evaluate the range of valid transformations that can be applied to the IR. For example, we
might determine that it is valid to move an expression into a specific `if/then` branch, because
it is only used in that branch - the dominance property is how we determine that there are paths
through the program in which the result of the expression is unused, as well as what program points
represent the nearest point to one of its uses that still dominates _all_ of the uses.

There is another useful notion of dominance, called _post-dominance_, which can be described much
like the regular notion of dominance, except in terms of paths to the exit of the CFG, rather than
paths from the entry:

> Given a control-flow graph $G$, and a node $A \in $G$, then $\forall B \in G$, $A pdom B$ if all
> paths through $B$ that exit the CFG, must flow through $A$ first.
>
> Furthermore, $A$ _strictly_ post-dominates $B$ if $A \neq B$.

The notion of post-dominance is important in determining the applicability of certain transformations,
in particular with loops.


## Structure

The hierarchy of HIR looks like so:

        Component <- Imports <- Interface
            |
            v
         Exports
            |
            v
        Interface
            |
            v
          Module ---------
            |              |
            v              v
        Function/Op     Global Variable
            |
            v
       -- Region
      |     |
      |     v
      |   Block -> Value <--
      |     |        |      |
      |     |        v      |
      |     |      Operand  |
      |     v        |      |
       > Operation <-       |
            |               |
            v               |
          Result -----------

In short:

* A _component_ imports dependencies in the form of _interfaces_
* A _component_ exports at least one _interface_.
* An _interface_ is concretely implemented with a _module_.
* A _module_ contains _function_ and _global variable_ definitions, and imports them from the set
of interfaces available to the component.
* A _function_, as a type of _operation_, consists of a body _region_.
* A _region_ consists of at least one _block_, that may define _values_ in the form of block
parameters.
* A _block_ consists of _operations_, whose results introduce new _values_, and whose operands are
_values_ introduced either as block parameters or the results of previous operations.
* An _operation_ may contain nested _regions_, with associated blocks and operations.

### Passes

A _pass_ is effectively a fancy function with a specific signature and some constraints on its
semantics. There are three primary types of passes: [_analysis_](#analyses),
[_rewrite_](#rewrites), and [_conversion_](#conversions). These three pass types have different
signatures and semantics, but play symbiotic roles in the compilation pipeline.

There are two abstractions over all passes, used and extended by the three types described above:

* The `Pass` trait, which provides an abstraction suitable for describing any type of compiler pass
used by `midenc`. It primarily exists to allow writing pass-generic helpers.
* The `PassInfo` trait, which exists to provide a common interface for pass metadata, such as the
name, description, and command-line flag prefix. All passes must implement this trait.

#### Analyses

Analysis of the IR is expressed in the form of a specialized pass, an `Analysis`, and an
`AnalysisManager`, which is responsible for computing analyses on demand, caching them, and
invalidating the relevant parts of the cache when the IR changes. Analyses are expressed in terms
of a specific entity, such as `Function`, and are cached based on the unique identity of that
entity.

An _analysis_ is responsible for computing some fact about the given IR entity it is given. Facts
typically include things such as: computing dominance, identifying loops and their various component
parts, reachability, liveness, identifying unused (i.e. dead) code, and much more.

To do this, analyses are given an immutable reference to the IR in question; a reference to the
current `AnalysisManager`, so that the results of other analyses can be consulted; and a reference
to the current compilation `Session` for access to configuration relevant to the analysis.

Analysis results are computed as an instance of `Self`. This provides structure to the analysis
results, and provides a place to implement helpful functionality for querying the results.

A well-written analysis should be based purely off the inputs given to `Analysis::analyze`, and
ideally be based on some formalism, so that properties of the analysis can be verified. Most of
the analyses in HIR today, are based on the formalisms underpinning _dataflow analysis_, e.g.
semi-join lattices.

#### Rewrites

_Rewrites_ are a type of pass which mutate the IR entity to which they are applied. They can be
chained (via the `chain` method on the `RewritePass` trait) to form rewrite pipelines, called a
`RewriteSet`. A `RewriteSet` manages executing each rewrite in the set, and coordinating with the
`AnalysisManager` between rewrites.

Rewrites are given a mutable reference to the IR they should apply to; the current `AnalysisManager`
so that analyes can be consulted (or computed) to facilitate the rewrite; and the current
compilation `Session`, for configuration.

Rewrites must leave the IR in a valid state. Rewrites are also responsible for indicating, via the
`AnalysisManager`, which analyses can be preserved after applying the rewrite. A rewrite that makes
no changes, should mark all analyses preserved, to avoid recomputing the analysis results the next
time they are requested. If an analysis is not explicitly preserved by a rewrite, it will be
invalidated by the containing `RewriteSet`.

Rewrite passes written for a `Function`, can be adapted for application to a `Module`, using the
`ModuleRewriteAdapter`. This makes writing rewrites for use in the main compiler pipeline, as simple
as defining it for a `Function`, and then using the `ModuleRewriteAdapter` to add the rewrite to the
pipeline.

A well-written rewrite pass should only use data available in the IR itself, or analyses in the
provided `AnalysisManager`, to drive application of the rewrite. Additionally, rewrites should
focus on a single transformation, and rely on chaining rewrites to orchestrate more complex
transformations composed of multiple stages. Lastly, the rewrite should ideally have a logical
proof of its safety, or failing that, a basis in some formalism that can be suitably analyzed and/or
tested. If a rewrite cannot be described in such a way, it will be very difficult to provide
guarantees about the code produced by the transformation. This makes it hard to be confident
in the rewrite (and by extension, the compiler), and impossible to verify.

#### Conversions

_Conversions_ are a type of pass which converts between intermediate representations, or more
abstractly, between _dialects_.

The concept of a dialect is focused primarily on semantics, and less so on concrete representations.
For example, source and target dialects might share the same underlying IR, with the target dialect
having a more restricted set of _legal_ operations, possibly with stricter semantics.

This brings us to the concept of _legalization_, i.e. converting all _illegal_ operations in the IR
into _legal_ equivalents. Each dialect defines the set of operations which it considers legal. The
concept of legality is mostly important when the same underlying IR is used across multiple dialects,
as is the case with HIR.

There are two types of conversion passes in HIR: _dialect conversion_, and _translation_:

* _Dialect conversion_ is used at various points in the compilation pipeline to simplify the IR
for later passes. For example, Miden Assembly has no way to represent multi-way branches, such
as implemented by `switch`. At a certain point, we switch to a dialect where `switch` is illegal,
so that further passes can be written as if `switch` doesn't exist. Yet another dialect later in
the pipeline, makes all unstructured control flow illegal, in preparation for translation to
Miden Assembly, which has no unstructured control flow operators.
* _Translation_ refers to a conversion from one IR to a completely different one. Currently, the
only translation we have, is the one responsible for translating from HIR to Miden Assembly. In
the future, we hope to also implement frontends as translations _to_ HIR, but that is not currently
the case. Translations are currently implemented as simple passes that take in some IR as input,
and produce _whatever_ as output.

Dialect conversions are implemented in the form of generic _conversion infrastructure_. A dialect
conversion is described as a set of _conversion patterns_, which define what to do when a specific
operation is seen in the input IR; and the set of operations which are legal in the target dialect.
The conversion driver is responsible for visiting the IR, repetitively applying any matched
conversion patterns until a fixpoint is reached, i.e. no more patterns are matched, or a conversion
pattern fails to apply successfully. If any illegal operations are found which do not have
corresponding conversion patterns, then a legalization error is raised, and the conversion overall
fails.

## Usage

Let's get into the gritty details of how the compiler works, particularly in relation to HIR.

The entry point for compilation, generally speaking, is the _driver_. The driver is what is
responsible for collecting compiler inputs, including configuration, from the user, instantiating a
`Session`, and then invoking the compiler frontend with those items.

The first stage of compilation is _parsing_, in which each input is converted to HIR by an
appropriate frontend. For example, Wasm modules are loaded and translated to HIR using the Wasm
frontend. The purpose of this stage is to get all inputs into HIR form, for subsequent stages. The
only exception to this are MASM sources, which are assembled to MAST directly, and then set aside
until later in the pipeline when we go to assemble the final artifact.

The second stage of compilation is _semantic analysis_. This is the point where we validate the
HIR we have so far, and ensure that there are no obvious issues that will cause compilation to
fail unexpectedly later on. In some cases, this stage is skipped, as we have already validated the
IR in the frontend.

The third stage of compilation is _linking_. This is where we gather together all of the inputs,
as well as any compiler options that tell us what libraries to link against, and where to search
for them, and then ensure that there are no missing inputs, undefined symbols, or incompatible type
signatures. The output of this stage is a well-formed [_component_](#components).

The fourth stage of compilation is _rewriting_, in which all of the rewrite passes we wish to
apply to the IR, are applied. You could also think of this stage as a combination of optimization
and preparing for codegen.

The fifth stage of compilation is _codegen_, where HIR is translated to Miden Assembly.

The final stage of compilation is _assembly_, where the Miden Assembly we produced, along with
any other Miden Assembly libraries, are assembled to MAST, and then packaged in the Miden package
format.

The `Session` object contains all of the important compiler configuration and exists for the
duration of a compiler invocation. In addition to this, there is a `Context` object, which is used
to allocate IR entities contained within a single `Module`. The context of each `Module` is further
subdivided by `Function`, in the form of `FunctionContext`, from which all function-local IR
entities are allocated. This ensures that each `Function` gets its own set of values, blocks, and
regions, while sharing `Module`-wide entities, such as constants and symbols.

Operations are allocated from the nearest `Context`, and use that context to allocate and access
IR entities used by the operation.

The `Context` object is pervasive, it is needed just about anywhere that IR entities are used, to
allow accessing data associated with those entities. Most entity references are integer ids, which
uniquely identify the entity, but provide no access to them without going through the `Context`.

This is a bit awkward, but is easier to work with in Rust. The alternative is to rely on
dynamically-checked interior mutability inside reference-counted allocations, e.g. `Rc<RefCell<T>>`.
Not only is this similarly pervasive in terms of how APIs are structured, but it loses some of
the performance benefits of allocating IR objects close together on the heap.

The following is some example code in Rust, demonstrating how it might look to create a component
in HIR and work with it.

```rust
use midenc_hir::*;

/// Defining Ops

/// `Add` is a binary integral arithmetic operator, i.e. `+`
///
/// It consists of two operands, which must be of the same type, and produces a result that
/// is also of that type.
///
/// It supports various types of overflow semantics, see `Overflow`.
pub struct Add {
    op: Operation,
}
impl Add {
    pub fn build(overflow: Overflow, lhs: Operand, rhs: Operand, span: SourceSpan, context: &mut Context) -> OpId {
        let ctrl_ty = context.value_type(lhs);
        let mut builder = OpBuilder::<Self>::new(context);
        // Set the source span for this op
        builder.with_span(span);
        // We specify the concrete operand values
        builder.with_operands([lhs, rhs]);
        // We specify results in terms of types, and the builder will materialize values
        // for the instruction results based on those types.
        builder.with_results([ctrl_ty]);
        // We must also specify operation attributes, e.g. overflow behavior
        //
        // Attribute values must implement `AttributeValue`, a trait which represents the encoding
        // and decoding of a type as an attribute value.
        builder.with_attribute("overflow", overflow);
        // Add op traits that this type implements
        builder.with_trait::<Commutative>();
        builder.with_trait::<SameTypeOperands>();
        builder.with_trait::<Canonicalize>();
        // Instantiate the op
        //
        // NOTE: In order to use `OpBuilder`, `Self` must implement `Default` if it has any other
        // fields than the underlying `Operation`. This is because the `OpBuilder` will construct
        // a default instance of the type when allocating it, and according to Rust rules, any
        // subsequent reference to the type requires that all fields were properly initialized.
        builder.build()
    }

    /// The `OpOperand` type abstracts over information about a given operand, such as whether it
    /// is a value or immediate, and its type. It also contains the link for the operand in the
    /// original `Value`'s use list.
    pub fn lhs(&self) -> &OpOperand {
        &self.op.operands[0]
    }

    pub fn rhs(&self) -> &OpOperand {
        &self.op.operands[1]
    }

    /// The `OpResult` type abstracts over information about a given result, such as whether it
    /// is a value or constant, and its type.
    pub fn result(&self) -> &OpResult {
        &self.op.results[0]
    }

    /// Attributes of the op can be reified from the `AttributeSet` of the underlying `Operation`,
    /// using `get_attribute`, which will use the `AttributeValue` implementation for the type to
    /// reify it from the raw attribute data.
    pub fn overflow(&self) -> Overflow {
        self.op.get_attribute("overflow").unwrap_or_default()
    }
}
/// All ops must implement this trait, but most implementations will look like this
impl Op for Add {
    type Id = OpId;

    fn id(&self) -> Self::Id { self.op.key }
    fn name(&self) -> &'static str { "add" }
    fn as_operation(&self) -> &Operation { &self.op}
    fn as_operation_mut(&mut self) -> &mut Operation { &mut self.op}
}
/// Marker trait used to indicate an op exhibits the commutativity property
impl Commutative for Add {}
/// Marker trait used to indicate that operands of this op should all be the same type
impl SameTypeOperands for Add {}
/// Canonicalization is optional, but encouraged when an operation has a canonical form.
///
/// This is applied after transformations which introduce or modify an `Add` op, and ensures
/// that it is in canonical form.
///
/// Canonicalizations ensure that pattern-based rewrites can be expressed in terms of the
/// canonical form, rather than needing to account for all possible variations.
impl Canonicalize for Add {
    fn canonicalize(&mut self) {
        // If `add` is given an immediate operand, always place it on the right-hand side
        if self.lhs().is_immediate() {
            if self.rhs().is_immediate() {
                return;
            }
            self.op.operands.swap(0, 1);
        }
    }
}
/// Ops can optionally implement [PrettyPrint] and [PrettyParser], to allow for a less verbose
/// textual representation. If not implemented, the op will be printed using the generic format
/// driven by the underlying `Operation`.
impl formatter::PrettyPrint for Add {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        let opcode = match self.overflow() {
            Overflow::Unchecked => const_text("add"),
            Overflow::Checked => const_text("add.checked"),
            Overflow::Wrapping => const_text("add.wrapping"),
            Overflow::Overflowing => const_text("add.overflowing"),
        };
        opcode + const_text(" ") + display(self.lhs()) + const_text(", ") + display(self.rhs())
    }
}
impl parsing::PrettyParser for Add {
    fn parse(s: &str, context: &mut Context) -> Result<Self, Report> {
        todo!("not shown here")
    }
}

/// Constructing Components

// An interface can consist of functions, global variables, and potentially types in the future
let mut std = Interface::new("std");
std.insert("math::u64/add_checked", FunctionType::new([Type::U64, Type::U64], [Type::U64]));

let test = Interface::new("test");
test.insert("run", FunctionType::new([], [Type::U32]));

// A component is instantiated empty
let mut component = Component::new("test");

// You must then declare the interfaces that it imports and exports
component.import(std.clone());
component.export(test.clone());

// Then, to actually define a component instance, you must define modules which implement
// the interfaces exported by the component
let mut module = Module::new("test");
let mut run = module.define("run", FunctionType::new([], [Type::U32]));
//... build 'run' function

// And add them to the component, like shown here. Here, `module` is added to the component as an
// implementation of the `test` interface
component.implement(test, module);

// Modules can also be added to a component without using them to implement an interface,
// in which case they are only accessible from other modules in the same component, e.g.:
let foo = Module::new("foo");
// Here, the 'foo' module will only be accessible from the 'test' module.
component.add(foo);

// Lastly, during compilation, imports are resolved by linking against the components which
// implement them. The linker step identifies the concrete paths of each item that provides
// an imported symbol, and rewrites the generic interface path with the concrete path of the
// item it was resolved to.

/// Visiting IR

// Visiting the CFG

let entry = function.body().entry();
let mut worklist = VecDeque::from_iter([entry]);

while let Some(next) = worklist.pop_front() {
    // Ignore blocks we've already visited
    if !visited.insert(next) {
        continue;
    }
    let terminator = context.block(next).last().unwrap();
    // Visit all successors after this block, if the terminator branches to another block
    if let Some(branch) = terminator.downcast_ref::<Branch>() {
        worklist.extend(branch.successors().iter().map(|succ| succ.block));
    }
    // Visit the operations in the block bottom-up
    let mut current_op = terminator;
    while let Some(op) = current_op.prev() {
        current_op = op;
    }
}

// Visiting uses of a value

let op = function.body().entry().first().unwrap();
let result = op.results().first().unwrap();
assert!(result.is_used());
for user in result.uses() {
    dbg!(user);
}

// Applying a rewrite pattern to every match in a function body

struct FoldAdd;
impl RewritePattern for FoldAdd {
    fn matches(&self, op: &dyn Op) -> bool {
        op.is::<Add>()
    }

    fn apply(&mut self, op: &mut dyn Op) -> RewritePatternResult {
        let add_op = op.downcast_mut::<Add>().unwrap();
        if let Some(lhs) = add_op.lhs().as_immediate() {
            if let Some(rhs) = add_op.rhs().as_immediate() {
                let result = lhs + rhs;
                return Ok(RewriteAction::ReplaceAllUsesWith(add_op.result().value, result));
            }
        }
        Ok(RewriteAction::None)
    }
}
```
