use alloc::{format, vec};

use midenc_hir::{
    AddressSpace, BlockRef, Context, EntityRef, Felt, FxHashMap, Immediate, Operation,
    OperationRef, Report, SmallVec, SourceSpan, SymbolPath, ValueId, ValueRef,
    dialects::builtin::{self, attributes::LocalVariable},
};
use midenc_session::diagnostics::{Diagnostic, Severity, WrapErr, miette};

use super::memory::{self, MemoryAddress};
use crate::Value;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("evaluation failed: concrete use of poison value {value}")]
#[diagnostic()]
pub struct InvalidPoisonUseError {
    pub value: ValueId,
    #[label(primary)]
    pub at: SourceSpan,
    #[label("poison originally produced here")]
    pub origin: SourceSpan,
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("evaluation failed: {value} is undefined")]
#[diagnostic()]
pub struct UndefinedValueError {
    pub value: ValueId,
    #[label()]
    pub at: SourceSpan,
}

/// Information about the current symbol being executed
pub struct CallFrame {
    /// The callee corresponding to this frame
    callee: OperationRef,
    /// The operation that called `callee`, if called by an operation, not the evaluator itself
    caller: Option<OperationRef>,
    /// Virtual registers used to map SSA values to their runtime value
    registers: FxHashMap<ValueRef, Value>,
    /// Function-local memory reserved as scratch space for local variables
    locals: SmallVec<[Felt; 8]>,
    /// The offset of each local variable in `locals`, in elements, indexed by local
    local_offsets: SmallVec<[usize; 8]>,
}

impl CallFrame {
    pub fn new(callee: OperationRef) -> Self {
        let callee_op = callee.borrow();
        let (locals, local_offsets) = match callee_op.downcast_ref::<builtin::Function>() {
            Some(function) => {
                // Locals are addressed by element offset, so the buffer holds the elements of the
                // whole frame: a local wider than one element occupies several of them.
                let frame_elements =
                    function.locals().iter().map(|ty| ty.size_in_felts()).sum::<usize>();
                let capacity = frame_elements;
                let mut buf = SmallVec::with_capacity(capacity);
                buf.resize(capacity, Felt::ZERO);
                (buf, function.local_offsets().collect())
            }
            None => Default::default(),
        };

        Self {
            callee,
            caller: None,
            registers: Default::default(),
            locals,
            local_offsets,
        }
    }

    pub fn with_caller(mut self, caller: OperationRef) -> Self {
        self.caller = Some(caller);
        self
    }

    pub fn caller(&self) -> Option<EntityRef<'_, Operation>> {
        self.caller.as_ref().map(|caller| caller.borrow())
    }

    pub fn return_to(&self) -> Option<OperationRef> {
        self.caller.as_ref().and_then(|caller| caller.next())
    }

    pub fn caller_block(&self) -> Option<BlockRef> {
        self.caller.as_ref().and_then(|caller| caller.parent())
    }

    pub fn callee(&self) -> EntityRef<'_, Operation> {
        self.callee.borrow()
    }

    pub fn symbol_path(&self) -> Option<SymbolPath> {
        self.callee.borrow().as_symbol().map(|symbol| symbol.path())
    }

    pub fn is_defined(&self, value: &ValueRef) -> bool {
        self.registers.contains_key(value)
    }

    pub fn try_get_value(&self, value: &ValueRef) -> Option<Value> {
        self.registers.get(value).copied()
    }

    pub fn get_value(&self, value: &ValueRef, at: SourceSpan) -> Result<Value, Report> {
        self.registers.get(value).copied().ok_or_else(|| {
            Report::new(UndefinedValueError {
                value: value.borrow().id(),
                at,
            })
        })
    }

    #[inline(always)]
    #[track_caller]
    pub fn get_value_unchecked(&self, value: &ValueRef) -> Value {
        self.registers[value]
    }

    pub fn use_value(&self, value: &ValueRef, at: SourceSpan) -> Result<Immediate, Report> {
        match self.get_value(value, at)? {
            Value::Poison { origin, .. } => Err(Report::new(InvalidPoisonUseError {
                value: value.borrow().id(),
                at,
                origin,
            })),
            Value::Immediate(imm) => Ok(imm),
        }
    }

    pub fn set_value(&mut self, id: ValueRef, value: impl Into<Value>) {
        self.registers.insert(id, value.into());
    }

    /// The offset of `local` in this frame's local memory, in elements.
    ///
    /// # Panics
    ///
    /// Panics if `local` is not a local of the callee of this frame.
    fn local_offset(&self, local: &LocalVariable) -> usize {
        self.local_offsets[local.as_usize()]
    }

    fn checked_local_address(
        &self,
        local: &LocalVariable,
        span: SourceSpan,
        context: &Context,
    ) -> Result<MemoryAddress, Report> {
        let offset = self.local_offset(local);
        let size = local.ty().size_in_felts();
        if offset >= self.locals.len() || (offset + size) > self.locals.len() {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid access to local variable")
                .with_primary_label(
                    span,
                    format!(
                        "attempted to access {size} elements from offset {offset}, but only {} \
                         are allocated",
                        self.locals.len(),
                    ),
                )
                .into_report());
        }
        Ok(MemoryAddress::new(offset as u32, AddressSpace::Element))
    }

    /// Read the value of the given local variable from its element-addressed buffer.
    pub fn read_local(
        &self,
        local: &LocalVariable,
        span: SourceSpan,
        context: &Context,
    ) -> Result<Value, Report> {
        let addr = self.checked_local_address(local, span, context)?;
        memory::read_value(addr, &local.ty(), &self.locals).wrap_err("invalid memory read")
    }

    /// Write a value of the local's declared type, rejecting mismatches before mutation.
    pub fn write_local(
        &mut self,
        local: &LocalVariable,
        value: Value,
        span: SourceSpan,
        context: &Context,
    ) -> Result<(), Report> {
        let ty = local.ty();
        if value.ty() != ty {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid write to local variable")
                .with_primary_label(
                    span,
                    format!("expected value of type {ty}, got {}", value.ty()),
                )
                .into_report());
        }
        let addr = self.checked_local_address(local, span, context)?;
        memory::write_value(addr, value, &mut self.locals).wrap_err("invalid memory write")
    }
}

impl core::fmt::Debug for CallFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CallFrame")
            .field_with("callee", |f| match self.symbol_path() {
                Some(path) => write!(f, "{path}"),
                None => f.write_str("<anonymous>"),
            })
            .field_with("caller", |f| match self.caller {
                Some(caller) => write!(f, "{}", caller.borrow()),
                None => f.write_str("<not available>"),
            })
            .field_with("registers", |f| {
                let mut builder = f.debug_map();
                for (k, v) in self.registers.iter() {
                    builder.key(k).value_with(|f| write!(f, "{v}")).finish()?;
                }
                builder.finish()
            })
            .field_with("locals", |f| write!(f, "{:?}", self.locals))
            .finish()
    }
}

impl core::fmt::Display for CallFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.symbol_path() {
            Some(path) => write!(f, "{path}"),
            None => f.write_str("<anonymous>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::{Type, testing::Test};

    use super::*;

    #[test]
    fn local_write_rejects_a_value_of_the_wrong_type() {
        let mut test = Test::named("local_write_type");
        test.with_function("local_write_type", &[], &[]);
        let local = test.function().borrow_mut().alloc_local(Type::U8);
        let mut frame = CallFrame::new(test.function().as_operation_ref());
        let result = frame.write_local(
            &local,
            Value::Immediate(Immediate::U128(1)),
            SourceSpan::UNKNOWN,
            &test.context_rc(),
        );
        assert!(result.is_err());
    }
}
