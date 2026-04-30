use midenc_hir::{
    OpPrinter, UnsafeIntrusiveEntityRef,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::debuginfo::attributes::{ExpressionAttr, VariableAttr},
    effects::{
        DebugEffect, DebugEffectOpInterface, EffectOpInterface, MemoryEffect,
        MemoryEffectOpInterface,
    },
    smallvec,
    traits::{AnyType, Transparent},
};

use super::DebugInfoDialect;

pub type DebugValueRef = UnsafeIntrusiveEntityRef<DebugValue>;
pub type DebugDeclareRef = UnsafeIntrusiveEntityRef<DebugDeclare>;
pub type DebugKillRef = UnsafeIntrusiveEntityRef<DebugKill>;

/// Records the current value of a source-level variable.
///
/// This is the core operation of the debuginfo dialect. It creates a first-class SSA use of the
/// value, which means:
///
/// - If a transform deletes the value without updating its debug uses, that's a hard error (not a
///   silent drop like with metadata-based approaches).
/// - Standard MLIR-style use-def tracking automatically enforces this — transforms must call
///   `replace_all_uses_with` or explicitly handle debug uses.
///
/// The `variable` attribute identifies the source variable, and the `expression` attribute
/// describes how to recover the source-level value from the IR value (e.g., "dereference this
/// pointer" if the value was promoted to an alloca).
///
/// # Example
///
/// ```text
/// di.value %0 #[variable = di.local_variable(name = x, ...)]
///             #[expression = di.expression(DW_OP_WASM_local 0)]
/// ```
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugValue {
    #[operand]
    #[effects(DebugEffect(DebugEffect::Read, DebugEffect::Write))]
    value: AnyType,
    #[attr]
    #[effects(DebugEffect(DebugEffect::Write))]
    variable: VariableAttr,
    #[attr]
    expression: ExpressionAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugValue {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}

/// Records the storage location (address) of a source-level variable.
///
/// Unlike [DebugValue] which tracks values, [DebugDeclare] tracks the address where a variable is
/// stored. This is useful for variables that live in memory (e.g., stack allocations) where the
/// address itself doesn't change, but the value at that address may be updated through stores.
///
/// Like `DebugValue`, this creates a real SSA use of the address value, preventing silent drops
/// during transforms.
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugDeclare {
    #[operand]
    #[effects(DebugEffect(DebugEffect::Read))]
    address: AnyType,
    #[attr]
    #[effects(DebugEffect(DebugEffect::Allocate))]
    variable: VariableAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugDeclare {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}

/// Marks a source-level variable as dead at this program point.
///
/// This provides explicit lifetime boundaries for variables, giving the debugger precise
/// information about when a variable is no longer valid. Without this, debuggers must rely on
/// scope-based heuristics which can be inaccurate after optimizations.
///
/// After a `debuginfo.kill`, the debugger should report the variable as "optimized out" or "not
/// available" until the next `di.value` or `di.declare` for the same variable.
///
/// # Example
///
/// ```text
/// di.kill #[variable = di.local_variable(name = x, ...)]
/// ```
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugKill {
    #[attr]
    #[effects(DebugEffect(DebugEffect::Free))]
    variable: VariableAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugKill {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}
