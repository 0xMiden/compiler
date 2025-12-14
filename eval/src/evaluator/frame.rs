#![expect(unused_assignments)]

use alloc::{format, vec};

use midenc_hir::{
    BlockRef, Context, EntityRef, Felt, FxHashMap, Immediate, Operation, OperationRef, Report,
    SmallVec, SourceSpan, SymbolPath, ValueId, ValueRef,
    dialects::builtin::{self, LocalVariable},
    formatter::DisplayHex,
};
use midenc_session::diagnostics::{Diagnostic, Severity, WrapErr, miette};

use super::memory;
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
    locals: SmallVec<[u8; 64]>,
}

impl CallFrame {
    pub fn new(callee: OperationRef) -> Self {
        let callee_op = callee.borrow();
        let locals = match callee_op.downcast_ref::<builtin::Function>() {
            Some(function) => {
                let capacity = function.num_locals() * core::mem::size_of::<Felt>();
                let mut buf = SmallVec::with_capacity(capacity);
                buf.resize(capacity, 0);
                buf
            }
            None => Default::default(),
        };

        Self {
            callee,
            caller: None,
            registers: Default::default(),
            locals,
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

    /// Read the value of the given local variable
    ///
    /// Returns an error if `local` is invalid, or a value of the defined type could not be read
    /// from it (e.g. the encoding is not valid for the type).
    pub fn read_local(
        &self,
        local: &LocalVariable,
        span: SourceSpan,
        context: &Context,
    ) -> Result<Value, Report> {
        let offset = local.absolute_offset() * core::mem::size_of::<Felt>();
        let ty = local.ty();
        let size = ty.size_in_bytes();
        if offset >= self.locals.len() || (offset + size) >= self.locals.len() {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid read of local variable")
                .with_primary_label(
                    span,
                    format!(
                        "attempted to read value of size {size} from offset {offset}, but only {} \
                         are allocated",
                        self.locals.len()
                    ),
                )
                .into_report());
        }

        memory::read_value(offset, &ty, &self.locals).wrap_err("invalid memory read")
    }

    /// Write `value` to `local`.
    ///
    /// Returns an error if `local` is invalid, or `value` could not be written to `local` (e.g.
    /// the write would go out of bounds, or `value` is not a valid instance of the type associated
    /// with `local`).
    pub fn write_local(
        &mut self,
        local: &LocalVariable,
        value: Value,
        span: SourceSpan,
        context: &Context,
    ) -> Result<(), Report> {
        let offset = local.absolute_offset() * core::mem::size_of::<Felt>();
        let ty = local.ty();
        let size = ty.size_in_bytes();
        if offset >= self.locals.len() || (offset + size) >= self.locals.len() {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("invalid write of local variable")
                .with_primary_label(
                    span,
                    format!(
                        "attempted to write value of size {size} to offset {offset}, but only {} \
                         are allocated",
                        self.locals.len()
                    ),
                )
                .into_report());
        }

        memory::write_value(offset, value, &mut self.locals);

        Ok(())
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
            .field_with("locals", |f| write!(f, "{:0x}", DisplayHex::new(&self.locals)))
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
