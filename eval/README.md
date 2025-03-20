# Miden IR Interpreter

This crate defines a lightweight interpreter for arbitrary HIR operations, primarily intended for use in verification and tests.

It is implemented for all of the builtin dialects, but by implementing the `Eval` trait for operations in your dialect, they can be evaluated by the interpreter as well.

The primary interface is the `HirEvaluator` struct, which is provided to implementations of `Eval::eval`, and contains a number of primitive helpers that one can use to interact with the current state of the evaluator, e.g. read/write memory or local variables, get/set the concrete values associated with SSA registers, and more.

The evaluator works as a sort of coroutine evaluator, i.e. the core interpreter loop is managed by `HirEvaluator`, but the actual details of each operation are delegated to the `Eval` implemntation for that type. Operations interact with the evaluator for control flow by returning a `ControlFlowEffect` when returning from their `eval` implementation. The evaluator will use this to effect control transfers or other effects (e.g. traps).

For example, a `Call`-like operation would return `ControlFlowEffect::Call`, and let the evaluator perform the actual control flow, but the `Call`-like op itself would handle the validation and details unique to its implementation. For example, a call that needs to switch into a new interpreter context would call `HirEvaluator::enter_context` before returning from its `eval` implementation.
