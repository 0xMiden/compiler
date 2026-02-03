use midenc_hir::{
    Builder, BuilderExt, DIExpressionAttr, DILocalVariableAttr, Report, SourceSpan, ValueRef,
};

use super::ops::*;

/// Builder trait for creating debug info operations.
///
/// This trait follows the same pattern as other dialect builders
/// (`ArithOpBuilder`, `HirOpBuilder`, etc.) and can be implemented
/// for any type that wraps a `Builder`.
///
/// # Usage
///
/// ```ignore
/// // Emit a debug value tracking where variable 'x' lives:
/// builder.debug_value(ssa_value, variable_attr, span)?;
///
/// // With a custom expression (e.g., value needs a deref):
/// builder.debug_value_with_expr(ssa_value, variable_attr, Some(expr), span)?;
///
/// // Mark a variable as dead:
/// builder.debug_kill(variable_attr, span)?;
/// ```
pub trait DebugInfoOpBuilder<'f, B: ?Sized + Builder> {
    /// Emit a `debuginfo.value` operation that records the current value of a
    /// source-level variable.
    ///
    /// This creates an SSA use of `value`, ensuring that transforms cannot
    /// silently drop the value without updating the debug info.
    fn debug_value(
        &mut self,
        value: ValueRef,
        variable: DILocalVariableAttr,
        span: SourceSpan,
    ) -> Result<DebugValueRef, Report> {
        self.debug_value_with_expr(value, variable, None, span)
    }

    /// Emit a `debuginfo.value` operation with an optional expression that
    /// describes how to recover the source-level value from the IR value.
    ///
    /// The expression encodes the *inverse* of whatever transformation was
    /// applied to the value. For example, if a value was promoted to a stack
    /// allocation (pointer), the expression would contain a `deref` operation
    /// to recover the original value.
    fn debug_value_with_expr(
        &mut self,
        value: ValueRef,
        variable: DILocalVariableAttr,
        expression: Option<DIExpressionAttr>,
        span: SourceSpan,
    ) -> Result<DebugValueRef, Report> {
        let expr = expression.unwrap_or_default();
        let op_builder = self.builder_mut().create::<DebugValue, (_, _, _)>(span);
        op_builder(value, variable, expr)
    }

    /// Emit a `debuginfo.declare` operation that records the storage address
    /// of a source-level variable.
    fn debug_declare(
        &mut self,
        address: ValueRef,
        variable: DILocalVariableAttr,
        span: SourceSpan,
    ) -> Result<DebugDeclareRef, Report> {
        let op_builder = self.builder_mut().create::<DebugDeclare, (_, _)>(span);
        op_builder(address, variable)
    }

    /// Emit a `debuginfo.kill` operation that marks a variable as dead.
    ///
    /// After this point, the debugger should report the variable as unavailable
    /// until the next `debug_value` or `debug_declare` for the same variable.
    fn debug_kill(
        &mut self,
        variable: DILocalVariableAttr,
        span: SourceSpan,
    ) -> Result<DebugKillRef, Report> {
        let op_builder = self.builder_mut().create::<DebugKill, (_,)>(span);
        op_builder(variable)
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

/// Blanket implementation: any `Builder` can use `DebugInfoOpBuilder` directly.
impl<B: ?Sized + Builder> DebugInfoOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
