# Data layout

This document describes how we map data/memory accesses from the byte-addressable address space asssumed by Rust and most (if not virtually all) other languages, to the element-addressable address space of the Miden VM.

The details of this are abstracted away by HIR - so if you are working with Miden from Rust, or some other language that lowers to Miden Assembly via the Miden compiler's intermediate representation (HIR), it is essentially transparent.

However, if you need to integrate handwritten Miden Assembly with, for example, Rust code that has been compiled by `midenc`, you will want to be aware of some of these details, as they are an intrinsic part of the _Application Binary Interface (ABI)_ of the compiled code.

For most of this document, we'll be using Rust as the source language, and refer to specific details of how it is lowered into HIR via WebAssembly, and how data is laid out in memory from the perspective of Rust/Wasm, and then ultimately mapped to Miden. In general, these details are going to be very similar in other languages, particularly if going through the WebAssembly frontend, but once something is lowered into HIR, the way types are handled is shared across all languages.

## Byte-addressable memory and type layout

TODO: Describe how Rust lays out common types in memory, and some of the constraints one needs to be aware of when writing low-level code to access/manipulate those types.

## Element-addressable memory and type layout

TODO: Describe the specific methodology used to map the HIR type system to Miden's element-addressable memory.

## WebAssembly

TODO: This section will describe details of the core Wasm type system that are relevant here, particularly our reliance on a hack that uses the `float32` type to represent field elements efficiently in Rust/Wasm.

## Canonical ABI

TODO: This section will describe relevant aspects of the Canonical ABI type system that developers using the Wasm frontend should be aware of.
