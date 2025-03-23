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

TODO

## Folding

TODO

## Canonicalization

TODO

## Sparse Conditional Constant Propagation

TODO

## Control Flow Lifting

TODO

## Control Flow Sinking

TODO

## Spills

TODO
