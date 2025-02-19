# Calling conventions

This document describes the various calling conventions recognized/handled by the compiler,
including a specification for the interaction with the IR type system.

There are four calling conventions represented in the compiler:

- `C` aka `SystemV`, which corresponds to the C ABI commonly used for C foreign-function interfaces (FFI).
  We specifically use the System V ABI because it is well understood, documented, and straightforward.
- `Fast`, this convention allows the compiler to follow either the `C` calling convention, or modify it
  as it sees fit on a function-by-function basis. This convention provides no guarantees about how a
  callee will expect arguments to be passed, so should not be used for functions which are expected to
  have a stable, predictable interface. This is a good choice for local functions, or functions which are
  only used within an executable/library and are not part of the public interface.
- `Kernel`, this is a special calling convention that is used when defining kernel modules in the IR.
  Functions which are part of the kernel's public API are required to use this convention, and it is not
  possible to call a function via `syscall` if the callee is not defined with this convention. Because of
  the semantics of `syscall`, this convention is highly restrictive. In particular, it is not permitted to
  pass pointer arguments, or aggregates containing pointers, as `syscall` involves a context switch, and
  thus memory in the caller is not accessible to the callee, and vice versa.
- `Contract`, this is a special calling convention that is used when defining smart contract functions, i.e.
  functions that can be `call`'d. The compiler will not permit you to `call` a function if the callee is not
  defined with this convention, and functions with this convention cannot be called via `exec`. Like `syscall`,
  the `call` instruction involves a context switch, however, unlike the `Kernel` convention, the `Contract`
  convention is allowed to have types in its signature that are/contain pointers, with certain caveats around
  those pointers.

All four conventions above are based on the System V C ABI, tailored to the Miden VM. The only exception is
`Fast`, which may modify the ABI arbitrarily as it sees fit, and makes no guarantees about what modifications,
if any, it will make.

## Data representation

The following is a description of how the IR type system is represented in the `C` calling convention. Later,
a description of how the other conventions extend/restrict/modify this representation will be provided.

### Scalars

General type | C Type | IR Type | `sizeof` | Alignment (bytes) | Miden Type
-|-|-|-|-|-
 Integer | `_Bool`/`bool` | `I1` | 1 | 1 | u32
 Integer | `char`, `signed char` | `I8` | 1 | 1 | i32[^1]
 Integer | `unsigned char` | `U8` | 1 | 1 | u32
 Integer | `short` / `signed short` | `I16` | 2 | 2 | i32[^1]
 Integer | `unsigned short` | `U16` | 2 | 2 | u32
 Integer | `int` / `signed int` / `enum` | `I32` | 4 | 4 | i32[^1][^8]
 Integer | `unsigned int` | `U32` | 4 | 4 | u32
 Integer | `long` / `signed long` | `I32` | 4 | 4 | i32[^1]
 Integer | `unsigned long` / `size_t` | `U32` | 4 | 4 | u32
 Integer | `long long` / `signed long long` | `I64` | 8 | 8 | i64[^2]
 Integer | `unsigned long long` | `U64` | 8 | 8 | u64[^3]
 Pointer | *`any-type *`* / *`any-type (*)()`* | `Ptr(_)` | 4 | 4 | u32[^6][^7]
 Floating point | `float` | `F32` | 4 | 4 | u32[^4]
 Floating point | `double` | `F64` | 8 | 8 | u64[^4]
 Floating point | `long double` | 16 | 16 | (none)[^5]

[^1]: i32 is not a native Miden type, but is implemented using compiler intrinsics on top of the native u32 type

[^2]: i64 is not a native Miden type, but is implemented using compiler intrinsics on top of the stdlib u64 type

[^3]: u64 is not a native Miden type, but is implemented in software using two 32-bit limbs (i.e. a pair of field elements)

[^4]: floating-point types are not currently supported, but will be implemented using compiler intrinsics

[^5]: `long double` values correspond to 128-bit IEEE-754 quad-precision binary128 values. These are not currently
supported, and we have no plans to support them in the near term. Should we ever provide such support, we will do
so using compiler intrinsics.

[^6]: A null pointer (for all types) always has the value zero.

[^7]: Miden's linear memory is word-addressable, not byte-addressable. The `Ptr` type has an `AddressSpace` parameter,
that by default is set to the byte-addressable address space. The compiler translates values of `Ptr` type that are in
this address space, into the Miden-native, word-addressable address space during codegen of load/store operations. See
the section on the memory model below for more details.

[^8]: An `enum` is `i32` if all members of the enumeration can be represented by an `int`/`unsigned int`, otherwise it
uses i64.

> [!NOTE]
> The compiler does not support scalars larger than one word (128 bits) at this time. As a result, anything that is
> larger than that must be allocated in linear memory, or in an automatic allocation (function-local memory), and passed
> around by reference.

The native scalar type for the Miden VM is a "field element", specifically a 64-bit value representing an integer
in the "Goldilocks" field, i.e. `0..(2^64-2^32+1)`. A number of instructions in the VM operate on field elements directly.
However, the native integral/pointer type, i.e. a "machine word", is actually `u32`. This is because a field element
can fully represent 32-bit integers, but not the full 64-bit integer range. Values of `u32` type are valid field element
values, and can be used anywhere that a field element is expected (barring other constraints).

Miden also has the notion of a "word", not to be confused with a "machine word" (by which we mean the native integral
type used to represent pointers), which corresponds to a set of 4 field elements. Words are commonly used in Miden,
particularly to represent hashes, and a number of VM instructions operate on word-sized operands. As an aside, 128-bit
integer values are represented using a word, or two 64-bit limbs (each limb consisting of two 32-bit limbs).

All integral types mentioned above, barring field elements, use two's complement encoding. Unsigned integral types
make use of the sign bit to change the value range (i.e. 0..2^32-1, rather than -2^31..2^31-1), but the encoding follows
two's complement rules.

The Miden VM only has native support for field elements, words, and `u32`; all other types are implemented in software
using intrinsics.

### Aggregates and unions

Structures and unions assume the alignment of their most strictly aligned component. Each member is assigned to the
lowest available offset with the appropriate alignment. The size of any object is always a multiple of the object's alignment.
An array uses the same alignment as its elements. Structure and union objects can require padding to meet size and alignment
constraints. The contents of any padding is undefined.

### Memory model

Interacting with memory in Miden is quite similar to WebAssembly in some ways:

* The address space is linear, with addresses starting at zero, and ranging up to 2^32-1
* There is no memory protection per se, you either have full read/write access, or no access to a specific memory context
* How memory is used is completely up to the program being executed

This is where it begins to differ though, and takes on qualities unique to Miden (in part, or whole):

* Certain regions of the address space are "reserved" for special uses, improper use of those regions may result in
undefined behavior.
* Miden has different types of function call instructions: `call` vs `syscall` vs `exec`. The first two
perform a context switch when transferring control to the callee, and the callee has no access to the
caller's memory (and the caller has no access to the callee's memory). As a result, references to memory
cannot be passed from caller to callee in arguments, nor can they be returned from the callee to the caller.
* Most significant of all though, is that Miden does not have byte-addressable memory, it is instead word-addressable,
i.e. every address refers to a full word.
* It is not possible to load a specific field element from a word in memory, unless it happens to be the first element
of the word. Instead, one must load the full word, and drop the elements you don't need.

This presents some complications, particularly:

* Most languages assume a byte-oriented memory model, which is not trivially mapped to a word-oriented model
* Simple things, such as taking the address of a field in a struct, and then dereferencing it, cannot be directly
represented in Miden using native pointer arithmetic and `load` instruction. Operations like this must be translated
into instruction sequences that load whole words from memory, extract the data needed, and discard the unused bits.
This makes the choice of where in memory to store something much more important than byte-addressable memory, as
loads of values which are not aligned to element or word boundaries can be quite inefficient in some cases.

The compiler solves this by providing a byte-addressable IR, and internally translating operations in the IR to the equivalent
sequence of Miden instructions needed to emulate that operation. This translation is done during code generation, and uses
the following semantics to determine how a particular operation gets lowered:

* A byte-addressable pointer can be emulated in Miden's word-addressable environment using three pieces of information:
  - The address of the word containing the first byte of the value, this is a "native" Miden address value
  - The index of the field element within that word containing the first byte of the value
  - The offset (in bytes) from the start of the 4 byte chunk represented by the selected element, corresponding
    to the first byte of the value. Since the chunk is represented as a u32 value, the offset is relative to the
    most-significant bit (i.e. the byte with the lowest address is found in bits 55-63, since Miden integers are little-endian)
* This relies on us treating Miden's linear memory as an array of 16-byte chunks of raw memory (each word is 4 field elements,
each element represents a 4-byte chunk). In short, much like translating a virtual memory address to a physical one, we must
translate byte-addressable "virtual" pointers to "real" Miden pointers with enough metadata to be able to extract the data we're
trying to load (or encode the data we're trying to store).

Because we're essentially emulating byte-addressable memory on word-addressable memory, loads/stores can range from simple and
straightforward, to expensive and complicated, depending on the size and alignment of the value type. The process goes as follows:

* If the value type is word-aligned, it can be loaded/stored in as little as a single instruction depending on the size of the type
* Likewise if the value type is element-aligned, and the address is word-aligned
* Element-aligned values require some extra instructions to load a full word and drop the unused elements (or in the case of stores,
loading the full word and replacing the element being stored)
* Loads/stores of types with sub-element alignment depend on the alignment of the pointer itself. Element or word-aligned addresses
are still quite efficient to load/store from, but if the first byte of the value occurs in the middle of an element, then the bytes
of that value must be shifted into place (or unused bytes masked out). If the value crosses an element boundary, then the bytes in
both elements must be isolated and shifted into position such that they can be bitwise-OR'd together to obtain the aligned value on
the operand stack. If a value crosses a word boundary, then elements from both words must be loaded, irrelevant ones discarded, the
relevant bytes isolated and shifted into position so that the resulting operand on the stack is aligned and laid out correctly.
* Stores are further complicated by the need to preserve memory that is not being explicitly written to, so values that do not overwrite
a full word or element, require combining bytes from the operand being stored and what currently resides in memory.

The worst case scenario for an unaligned load or store involves a word-sized type starting somewhere in the last element of the first
word. This will require loading elements from three consecutive words, plus a lot of shuffling bits around to get the final, aligned
word-sized value on the operand stack. Luckily, such operations should be quite rare, as by default all word-sized scalar types are
word-aligned or element-aligned, so an unaligned load or store would require either a packed struct, or a type such as an array of
bytes starting at some arbitrary address. In practice, most loads/stores are likely to be element-aligned, so most overhead from
emulation will come from values which cross an element or word boundary.

## Function calls

This section describes the conventions followed when executing a function call via `exec`, including how arguments are passed on the
operand stack, stack frames, etc. Later, we'll cover the differences when executing calls via `call` or `syscall`.

### Locals and the stack frame

Miden does not have registers in the style of hardware architectures. Instead it has an operand stack, on which an arbitrary number of
operands may be stored, and local variables. In both cases - an operand on the operand stack, or a single local variable - the value
type is nominally a field element, but it is easier to reason about them as untyped element-sized values. The operand stack is used
for function arguments, return values, temporary variables, and scratch space. Local variables are not always used, but are typically
used to hold multiply-used values which you don't want to keep on the operand stack, function-scoped automatic allocations (i.e. `alloca`),
and other such uses.

Miden does not have a stack frame per se. When you call a procedure in Miden Assembly, any local variables declared by that procedure
are allocated space in a reserved region of linear memory in a single consecutive chunk. However, there is no stack or frame pointer,
and because Miden is a Harvard architecture machine, there are no return addresses. Instead, languages (such as C) which have the concept
of a stack frame with implications for the semantics of say, taking the address of a local variable, will need to emit code in function
prologues and epilogues to maintain a shadow stack in Miden's linear memory. If all you need is local variables, you can get away with
leaning on Miden's notion of local variables without implementing a shadow stack.

Because there are no registers, the notion of callee-saved or caller-saved registers does not have a direct equivalent in Miden. However,
in its place, a somewhat equivalent set of rules defines the contract between caller and callee in terms of the state of the operand stack,
those are described below in the section covering the operand stack.

#### The shadow stack

Miden is a [Harvard](https://en.wikipedia.org/wiki/Harvard_architecture) architecture; as such, code and data are not in the same memory
space. More precisely, in Miden, code is only addressable via the hash of the MAST root of that code, which must correspond to code that
has been loaded into the VM. The hash of the MAST root of a function can be used to call that function both directly and indirectly, but
that is the only action you can take with it. Code can not be generated and called on the fly, and it is not stored anywhere that is
accessible to code that is currently executing.

One consequence of this is that there are no return addresses or instruction pointers visible to executing code. The runtime call stack is
managed by the VM itself, and is not exposed to executing code in any way. This means that address-taken local C variables need to be on a
separate stack in linear memory (which we refer to as a "shadow stack"). Not all functions necessarily require a frame in the shadow stack,
as it cannot be used to perform unwinding, so only functions which have locals require a frame.

The Miden VM actually provides some built-in support for stack frames when using Miden Assembly. Procedures which are declared with some
number of locals, will be automatically allocated sufficient space for those locals in a reserved region of linear memory when called. If
you use the `locaddr` instruction to get the actual address of a local, that address can be passed as an argument to callees (within the
constraints of the callee's calling convention).

Languages with more elaborate requirements with regard to the stack will need to implement their own shadow stack, and emit code in function
prologues/epilogues to manage it.

#### The operand stack

The Miden virtual machine is a stack machine, not a register machine. Rather than having a fixed set of registers that are used to
store and manipulate scalar values, the Miden VM has the operand stack, which can hold an arbitrary number of operands (where each
operand is a single field element), of which the first 16 can be directly manipulated using special stack instructions. The operand
stack is, as the name implies, a last-in/first-out data structure.

The following are basic rules all conventions are expected to follow with regard to the operand stack:

1. The state of the operand stack from the point of view of the caller should be preserved, with two exceptions:
  - The callee is expected to consume all of its arguments, and the caller will expect those operands to be gone when control is returned to it
  - If the callee signature declares a return value, the caller expects to see that on top of the stack when control is returned to it
2. No more than 16 elements of the operand stack may be used for passing arguments. If more than that is required to represent all of the arguments,
then one of the following must happen:
  - Spill to stack frame: in this scenario, up to 15 elements of the operand stack are used for arguments, and the remaining element is used to hold
  a pointer to a local variable in the caller's stack frame. That local variable is a struct whose fields are the spilled arguments, appearing in
  the same order as they would be passed. The callee must use the pointer it is given to compute the effective address for each spilled argument
  that it wishes to access.
  - Spill to heap: this is basically identical to the approach above, except the memory is allocated from the global heap, rather than using memory
  associated with the caller's stack frame.
  - Spill to the advice provider: in this scenario, 12 elements of the stack are used for arguments, and the remaining 4 are used to hold a hash
  which refers to the remaining arguments on the advice provider stack. The callee must arrange to fetch the spilled arguments from the advice
  provider using that hash.

#### Function signatures

Miden Abstract Syntax Trees (MASTs) do not have any notion of functions, and as such are not aware of parameters, return values, etc. For
this document, that's not a useful level of abstraction to examine. Even a step higher, Miden Assembly (MASM) has functions (procedures
in MASM parlance), but no function signature, i.e. given a MASM procedure, there is no way to know how many arguments it expects, how
many values it returns, let alone the types of arguments/return values. Instead, we're going to specify calling conventions in terms of
Miden IR, which has a fairly expressive type system more or less equivalent to that of LLVM, and how that translates to Miden primitives.

Functions in Miden IR always have a signature, which specify the following:

* The calling convention required to call the function
* The number and types of the function arguments
* The type of value, if any, returned by the function, and whether it is returned by value or reference

The following table relates IR types to how they are expected to be passed from the caller to the callee, and vice versa:

Type                      | Parameter     | Result   |
--------------------------|---------------|----------|
scalar                    | direct        | direct   |
empty struct or union[^1] | ignored       | ignored  |
scalar struct or union[^2] | direct        | direct   |
other struct or union     | indirect      | indirect |
array                     | indirect      | N/A      |

[^1]: Zero-sized types have no representation in memory, so they are ignored/skipped

[^2]: Any struct or union that recursively (including through nested structs,
unions, and arrays) contains just a single scalar value and is not specified to
have greater than natural alignment.

The compiler will automatically generate code that follows these rules, but if emitting MASM from your own backend, it is necessary to do so manually.
For example, a function whose signature specifies that it returns a non-scalar struct by value, must actually be written such that it expects to receive
a pointer to memory allocated by the caller sufficient to hold the return value, as the first parameter of the function (i.e. the parameter is prepended
to the parameter list). When returning, the function must write the return value to that pointer, rather than returning it on the operand stack. In this
example, the return value is returned indirectly (by reference).

A universal rule is that the arguments are passed in reverse order, i.e. the first argument in the parameter list of a function will be on top of the
operand stack. This is different than many Miden instructions which seemingly use the opposite convention, e.g. `add`, which expects the right-hand
operand on top of the stack, so `a + b` is represented like `push a, push b, add`. If we were to implement `add` as a function, it would instead be
`push b, push a, exec.add`. The rationale behind this is that, in general, the more frequently used arguments appear earlier in the parameter list,
and thus we want those closer to the top of the operand stack to reduce the amount of stack manipulation we need to do.

Arguments/return values are laid out on the operand stack just like they would be as if you had just loaded it from memory, so all arguments are aligned,
but may span multiple operands on the operand stack as necessary based on the size of the type (i.e. a struct type that contains a `u32` and a `i1`
field would require two operands to represent). If the maximum number of operands allowed for the call is reached, any remaining arguments must be
spilled to the caller's stack frame, or to the advice provider. The former is used in the case of `exec`/`dynexec`, while the latter is used for `call`
and `syscall`, as caller memory is not accessible to the callee with those instructions.

While ostensibly 16 elements is the maximum number of operands on the operand stack that can represent function arguments, due to the way `dynexec`/`dyncall`
work, it is actually limited to 12 elements, because at least 4 must be free to hold the hash of the function being indirectly called.
