# Canonical ABI vs Miden ABI incompatibility

> [!NOTE]
> This document describes an issue that arises when trying to map the ad-hoc calling convention/ABI
> used by various Miden Assembly procedures, such as those comprising the transaction kernel, and
> the "canonical" ABI(s) representable in Rust. It proposes a solution to this problem in the form
> of _adapter functions_, where the details of a given adapter are one of a closed set of known
> ABI _transformation strategies_.

## Summary

The gist of the problem is that in Miden, the size and number of procedure results is only constrained
by the maximum addressable operand stack depth. In most programming languages, particularly those in
which interop is typically performed using some variant of the C ABI (commonly the one described
in the System V specification), the number of results is almost always limited to a single result,
and the size of the result type is almost always limited to the size of a single machine word, in
some cases two. On these platforms, procedure results of greater arity or size are typically handled
by reserving space in the caller's stack frame, and implicitly prepending the parameter list of the
callee with an extra parameter: a pointer to the memory allocated for the return value. The callee
will directly write the return value via this pointer, instead of returning a value in a register.

In the case of Rust, this means that attempting to represent a procedure that returns multiple values,
or returns a larger-than-machine-word type, such as `Word`, will trigger the implicit transformation
described above, as this is allowed by the standard Rust calling conventions. Since various Miden
procedures that are part of the standard library and the transaction kernel are affected by this,
the question becomes "how do we define bindings for these procedures in Rust?".

The solution is to have the compiler emit glue code that closes the gap between the two ABIs. It
does so by generating adapter functions, which wrap functions that have an ABI unrepresentable in
Rust, and orchestrate lifting/lowering arguments and results between the adapter and the "real"
function.

When type signatures are available for all Miden Assembly procedures, we can completely automate
this process. For now, we will require a manually curated list of known procedures, their signatures,
and the strategy used to "adapt" those procedures for binding in Rust.

## Background

After analyzing all of the functions in the transaction kernel API, the most common cause of a mismatch
between Miden and Rust ABIs, is due to implicit "sret" parameters, i.e. the transformation mentioned
above which inserts an implicit pointer to the caller's stack frame for the callee to write the return
value to, rather than doing so in a register (or in our case, on the operand stack). This seems to
happen for any type that is larger than 8 bytes (i64).

> [!TIP]
> For a complete list of the transaction kernel functions, in WIT format, see
> [miden.wit](https://github.com/0xMiden/compiler/blob/main/tests/rust-apps-wasm/wit-sdk/sdk/wit/miden.wit).

For most transaction kernel functions, the adapter function can be generated automatically using the
pattern recognition and adapter functions described below.

### Prerequisites

* The compiler must know the type signature for any function we wish to apply the adapter strategy to

### Implementation

The compiler will analyze every component import to determine if that import requires an adapter,
as determined by matching against a predefined set of patterns. The adapter generation will take
place in the frontend, as it has access to all of the needed information, and ensures that we do
not have any transformations or analyses that make decisions on the un-adapted procedure.

The following pseudo-code can be used to recognize the various Miden ABI patterns:

```rust
pub enum MidenAbiPattern {
    /// Calling this procedure will require an sret parameter on the Rust side, so
    /// we need to emit an adapter that will lift/lower calls according to that
    /// strategy.
    ReturnViaPointer,
    /// The underlying procedure is fully representable in Rust, and requires no adaptation.
    NoAdapterNeeded,
}

pub struct MidenAbiPatternRecognition {
    pattern: Option<MidenAbiPattern>,
    component_function: ComponentFunctionType,
    wasm_core_func: Signature,
    tx_kernel_function: Signature,
}

pub fn recognize_miden_abi_pattern(
    component_function: &ComponentFunctionType,
    wasm_core_func: &Signature,
    tx_kernel_func: &Signature) -> MidenAbiPatternRecognition {
    if wasm_core_func == tx_kernel_func {
        return MidenAbiPatternRecognition {
            pattern: Some(NoAdapterNeeded),
            component_function,
            wasm_core_function,
            tx_kernel_function,
        };
    } else if component_function.returns[0].byte_size > 8 && wasm_core_func.params.last() == I32 {
        return MidenAbiPatternRecognition {
            pattern: Some(ReturnViaPointer),
            component_function,
            wasm_core_function,
            tx_kernel_function,
        };
    } else {
        return MidenAbiPatternRecognition {
            pattern: None,
            component_function,
            wasm_core_function,
            tx_kernel_function,
        };
    }
}
```

The following pseudo-code can then be used to generate the adapter function:

```rust
pub fn generate_adapter(recognition: MidenAbiPatternRecognition) {
    match recognition.pattern {
        Some(pattern) => generate_adapter(
            pattern,
            recognition.component_function,
            recognition.wasm_core_function,
            recognition.tx_kernel_function
        ),
        None => use_manual_adapter(
            recognition.component_function,
            recognition.wasm_core_function,
            recognition.tx_kernel_function
        ),
    }
}

/// Escape hatch for the cases when the compiler can't generate an adapter function automatically
/// and we need to provide the adapter function manually.
pub fn use_manual_adapter(...) {
    // Find and use the manual adapter in the adapter library for the tx_kernel_function
}
```

The manual adapter library is a collection of adapter functions that are used when the compiler
can't generate an adapter function automatically so its expected to be provided. The manual adapter
library is a part of the Miden compiler. It is not anticipated that we will have many, or any, of
these; however in the near term we are going to manually map procedures to their adapter strategies,
as we have not yet automated the pattern recognition step.

### Return-via-pointer adapter

The return value is expected to be returned by storing its flattened representation in a pointer
passed as an argument.

Recognize this Miden ABI pattern by looking at the Wasm component function type. If the return value
is bigger than 64 bits, expect the last argument in the Wasm core(HIR) signature to be `i32` (a pointer).

The adapter function calls the tx kernel function and stores the result in the provided pointer (the
last argument of the Wasm core function).

Here is the pseudo-code for generating the adapter function for the return-via-pointer Miden ABI
pattern:

```rust
let ptr = wasm_core_function.params.last();
let adapter_function = FunctionBuilder::new(wasm_core_function.clone());
let tx_kernel_function_params = wasm_core_function.params.drop_last();
let tx_kernel_func_val = adapter_function.call(tx_kernel_function, tx_kernel_function_params);
adapter_function.store(tx_kernel_func_val, ptr);
adapter_function.build();
```

Here is how the adapter might look like in a pseudo-code for the `add_asset` function:

```
/// Takes an Asset as an argument and returns a new Asset
func wasm_core_add_asset(v0: f64, v1: f64, v2: f64, v3: f64, ptr: i32) {
    v4 = call tx_kernel_add_asset(v0, v1, v2, v3);
    // v4 is a tuple of 4 f64 values
    store v4 in ptr;
}
```

### No-op adapter

No adapter is needed. The Wasm core function type is the same as the tx kernel ad-hoc signature.

This Miden ABI pattern is selected if no other Miden ABI pattern is applicable and the wasm core function signature is the same as the tx kernel ad-hoc signature.

For example, the `get_id` function falls under this Miden ABI pattern and its calls will be translated to the tx kernel function calls without any modifications.


## Transaction kernel functions that require manual adapter functions

### `get_assets`

`get_assets:func() -> list<core-asset>` in the `note` interface is the only function that requires attention.
In Canonical ABI, any function that returns a dynamic list of items needs to allocate memory in the caller's
module due to the shared-nothing nature of the Wasm component model. For this case, a `realloc` function
is passed as a part of lift/lower Canonical ABI options for the caller to allocate memory in the caller's
module.

Here are the signatures of the `get_assets` function in the WIT, core Wasm, and the tx kernel ad-hoc ABI:
Comment from the `miden-base`

```
#! Writes the assets of the currently executing note into memory starting at the specified address.
#!
#! Inputs: [dest_ptr]
#! Outputs: [num_assets, dest_ptr]
#!
#! - dest_ptr is the memory address to write the assets.
#! - num_assets is the number of assets in the currently executing note.
```

Wasm component function type:
`get-assets: func() -> list<core-asset>;`

Wasm core signature:
`wasm_core_get_assets(i32) -> ()`

If we add a new `get_assets_count: func() -> u32;` function to the tx kernel and add the assets count
parameter to the `get_assets` function (`get_assets: func(assets_count: u32) -> list<core-asset>;`)
we should have everything we need to manually write the adapter function for the `get_assets`
function.

The list is expected to be returned by storing the pointer to its first item in a `ptr` pointer
passed as an argument and item count at `ptr + 4 bytes` address (`ptr` points to two pointers).

We could try to recognize this Miden ABI pattern by looking at the Wasm component function type. If
the return value is a list, expect the last argument in the Wasm core(HIR) signature to be `i32`
(a pointer). The problem is recognizing the list count parameter in the Wasm core(HIR) signature.

The adapter function calls allocates `asset_count * item_size` memory via the `realloc` call and
passes the pointer to the newly allocated memory to the tx kernel function.

Here is how the adapter function might look like in a pseudo-code for the `get_assets` function:

```rust
func wasm_core_get_assets(asset_count: u32, ptr_ptr: i32) {
    mem_size = asset_count * item_size;
    ptr = realloc(mem_size);
    (actual_asset_count, ptr) = call tx_kernel_get_assets(ptr);
    assert(actual_asset_count == asset_count);
    store ptr in ptr_ptr;
    store account_count in ptr_ptr + 4;
}
```

> [!NOTE]
> Since the `get_assets` tx kernel function in the current form can trash the provided memory if
> the actual assets count differs from the returned by `get_assets_count`, we can introduce the
> asset count parameter to the `get_assets` tx kernel function and check that it the same as the
> actual assets count written to memory.


## The example of some functions signatures

### `add_asset` (return-via-pointer Miden ABI pattern)

Comment from the `miden-base`

```
#! Add the specified asset to the vault.
#!
#! Panics:
#! - If the asset is not valid.
#! - If the total value of two fungible assets is greater than or equal to 2^63.
#! - If the vault already contains the same non-fungible asset.
#!
#! Stack: [ASSET]
#! Output: [ASSET']
#!
#! - ASSET' final asset in the account vault is defined as follows:
#!   - If ASSET is a non-fungible asset, then ASSET' is the same as ASSET.
#!   - If ASSET is a fungible asset, then ASSET' is the total fungible asset in the account vault
#!     after ASSET was added to it.
```

Wasm component function type:
`add-asset(core-asset) -> core-asset`

Wasm core signature:
`wasm_core_add_asset(f64, f64, f64, f64, i32) -> ()`
The last `i32` is a pointer to a returned value (`word`)

Tx kernel ad-hoc signature:
`tx_kernel_add_asset(felt, felt, felt, felt) -> (felt, felt, felt, felt)`


### `get_id` (no-adapter-needed Miden ABI pattern)

Comment from the `miden-base`
```
#! Returns the account id.
#!
#! Stack: []
#! Output: [acct_id]
#!
#! - acct_id is the account id.
```

Wasm component function type:
`get-id() -> account-id`

Wasm core signature:
`wasm_core_get_id() -> f64`

Tx kernel ad-hoc signature:
`tx_kernel_get_id() -> felt`
