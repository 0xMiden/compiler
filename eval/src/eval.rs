use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
};

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_hir2::{
    dialects::builtin, AttributeValue, Felt, Immediate, Op, OperationRef, Overflow,
    RegionBranchPoint, RegionBranchTerminatorOpInterface, Report, SmallVec, SourceSpan, Spanned,
    SuccessorInfo, Type, Value as _, ValueRange,
};
use midenc_session::diagnostics::Severity;

use crate::*;

/// This trait is intended to be implemented by any [midenc_hir2::Op] that we wish to be able to
/// evaluate via the [HirEvaluator].
pub trait Eval {
    /// Evaluate this operation, using the provided evaluator for any side effects/results, etc.
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report>;
}

/// This trait is intended to be implemented by any [midenc_hir2::Op] that has associated one-time
/// initialization that it needs to perform prior to starting evaluation.
///
/// Initialization is only performed when calling [HirEvaluator::eval] or one of its variants, on
/// an operation that implements this trait. If evaluation starts in an ancestor or descendant
/// operation, initialization is not performed unless explicitly implemented by the op at which
/// evaluation started.
pub trait Initialize {
    /// Peform initialization, using the provided evaluator for any side effects.
    fn initialize(&self, evaluator: &mut HirEvaluator) -> Result<(), Report>;
}

/// Represents the action to take as the result of evaluating a given operation
#[derive(Default, Debug)]
pub enum ControlFlowEffect {
    /// The current operation has no control effects
    #[default]
    None,
    /// Execution should trap at the current instruction
    Trap { span: SourceSpan, reason: String },
    /// Control is returning from the operation enclosing the current region
    ///
    /// It is expected that the operation being returned from implements CallableOpInterface, and
    /// that the returning operation implements ReturnLike.
    Return(Option<Value>),
    /// Control is transferring unconditionally to another block in the current region
    Jump(SuccessorInfo),
    /// Control is transferring unconditionally to another region in the enclosing operation, or
    /// returning from the enclosing operation itself.
    Yield {
        successor: RegionBranchPoint,
        arguments: ValueRange<'static, 4>,
    },
    /// Control should transfer to `callee`, with `arguments`, and return to the current operation
    /// if control exits normally from the callee.
    Call {
        callee: OperationRef,
        arguments: ValueRange<'static, 4>,
    },
}

impl Initialize for builtin::Component {
    fn initialize(&self, _evaluator: &mut HirEvaluator) -> Result<(), Report> {
        todo!("visit all global variables in all modules of this component")
    }
}
impl Initialize for builtin::Module {
    fn initialize(&self, _evaluator: &mut HirEvaluator) -> Result<(), Report> {
        todo!("visit all global variables in this module")
    }
}

impl Eval for builtin::Ret {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        // For now we only support single-return
        assert!(self.num_operands() < 2, "multi-return functions are not yet supported");

        if self.has_operands() {
            let value = evaluator.get_value(&self.values()[0].borrow().as_value_ref())?;
            Ok(ControlFlowEffect::Return(Some(value)))
        } else {
            Ok(ControlFlowEffect::Return(None))
        }
    }
}

impl Eval for builtin::RetImm {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        Ok(ControlFlowEffect::Return(Some((*self.value()).into())))
    }
}

impl Eval for ub::Unreachable {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        Ok(ControlFlowEffect::Trap {
            span: self.span(),
            reason: "control reached an unreachable program point".to_string(),
        })
    }
}

impl Eval for ub::Poison {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let value = match self.value().as_immediate() {
            Ok(imm) => Value::poison(self.span(), imm),
            Err(ty) => {
                return Err(self
                    .as_operation()
                    .context()
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message("invalid poison")
                    .with_primary_label(self.span(), format!("invalid poison type: {ty}"))
                    .into_report());
            }
        };
        evaluator.set_value(self.result().as_value_ref(), value);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for cf::Br {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        Ok(ControlFlowEffect::Jump(self.successors()[0]))
    }
}

impl Eval for cf::CondBr {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let condition = evaluator.get_value(&self.condition().as_value_ref())?;
        match condition {
            Value::Immediate(Immediate::I1(condition)) => {
                let successor = if condition {
                    self.successors()[0]
                } else {
                    self.successors()[1]
                };
                Ok(ControlFlowEffect::Jump(successor))
            }
            Value::Immediate(_) => {
                panic!("invalid immediate type for cf.cond_br condition: {condition:?}")
            }
            Value::Poison { .. } => {
                panic!("invalid use of poison value for cf.cond_br condition: {condition:?}")
            }
        }
    }
}

impl Eval for cf::Switch {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let selector = evaluator.get_value(&self.selector().as_value_ref())?;
        match selector {
            Value::Immediate(Immediate::U32(selector)) => {
                let successor = self
                    .cases()
                    .iter()
                    .find(|succ| succ.key().is_some_and(|k| *k == selector))
                    .map(|succ| *succ.info())
                    .unwrap_or_else(|| self.successors()[0]);
                Ok(ControlFlowEffect::Jump(successor))
            }
            Value::Immediate(_) => {
                panic!("invalid immediate type for cf.switch selector: {selector:?}")
            }
            Value::Poison { .. } => {
                panic!("invalid use of poison value for cf.switch selector: {selector:?}")
            }
        }
    }
}

impl Eval for cf::Select {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let condition = evaluator.get_value(&self.cond().as_value_ref())?;
        match condition {
            Value::Immediate(Immediate::I1(condition)) => {
                let result = if condition {
                    evaluator.get_value(&self.first().as_value_ref()).unwrap()
                } else {
                    evaluator.get_value(&self.second().as_value_ref()).unwrap()
                };
                evaluator.set_value(self.result().as_value_ref(), result);
                Ok(ControlFlowEffect::None)
            }
            Value::Immediate(_) => {
                panic!("invalid immediate type for cf.select condition: {condition:?}")
            }
            Value::Poison { .. } => {
                panic!("invalid use of poison value for cf.select condition: {condition:?}")
            }
        }
    }
}

impl Eval for scf::If {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let condition = evaluator.use_value(&self.condition().as_value_ref())?;
        match condition {
            Immediate::I1(condition) => {
                let successor = if condition {
                    self.then_body().as_region_ref()
                } else {
                    self.else_body().as_region_ref()
                };
                Ok(ControlFlowEffect::Yield {
                    successor: RegionBranchPoint::Child(successor),
                    arguments: ValueRange::Empty,
                })
            }
            _ => {
                panic!("invalid immediate type for scf.if condition: {condition:?}")
            }
        }
    }
}

impl Eval for scf::While {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let arguments = ValueRange::<4>::from(self.inits()).into_owned();

        Ok(ControlFlowEffect::Yield {
            successor: RegionBranchPoint::Child(self.before().as_region_ref()),
            arguments,
        })
    }
}

impl Eval for scf::IndexSwitch {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let selector = evaluator.use_value(&self.selector().as_value_ref())?;
        match selector {
            Immediate::U32(selector) => {
                let successor = self
                    .get_case_index_for_selector(selector)
                    .map(|index| self.get_case_region(index))
                    .unwrap_or_else(|| self.default_region().as_region_ref());
                Ok(ControlFlowEffect::Yield {
                    successor: RegionBranchPoint::Child(successor),
                    arguments: ValueRange::Empty,
                })
            }
            _ => {
                panic!("invalid immediate type for scf.index_switch selector: {selector:?}")
            }
        }
    }
}

impl Eval for scf::Yield {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let arguments = ValueRange::<4>::from(self.yielded()).into_owned();

        // The following uses compiler infrastructure to determine where to yield to without
        // hardcoding the list of known parent operations here and how to select the successor
        // region, since that's already been done in the compiler. If this turns out to be a big
        // perf bottleneck, we can implement something more efficient.
        let this = self.as_operation().as_trait::<dyn RegionBranchTerminatorOpInterface>().unwrap();
        let mut operands = SmallVec::<[_; 4]>::with_capacity(self.yielded().len());
        for yielded in self.yielded().iter() {
            match evaluator.get_value(&yielded.borrow().as_value_ref())? {
                Value::Immediate(value) | Value::Poison { value, .. } => {
                    operands.push(Some(Box::new(value) as Box<dyn AttributeValue>))
                }
            }
        }

        // Because all of the operands are known constants, this should always select a single
        // successor region
        let succs = this.get_successor_regions(&operands);
        assert_eq!(succs.len(), 1);
        let successor = succs[0].successor();

        Ok(ControlFlowEffect::Yield {
            successor,
            arguments,
        })
    }
}

impl Eval for scf::Condition {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let condition = evaluator.get_value(&self.condition().as_value_ref())?;
        match condition {
            Value::Immediate(Immediate::I1(condition)) => {
                let parent_op = self.parent_op().unwrap();
                let parent_op = parent_op.borrow();
                let while_op = parent_op.downcast_ref::<scf::While>().unwrap();
                let arguments = ValueRange::<4>::from(self.forwarded());
                if condition {
                    Ok(ControlFlowEffect::Yield {
                        successor: RegionBranchPoint::Child(while_op.after().as_region_ref()),
                        arguments: arguments.into_owned(),
                    })
                } else {
                    Ok(ControlFlowEffect::Yield {
                        successor: RegionBranchPoint::Parent,
                        arguments: arguments.into_owned(),
                    })
                }
            }
            Value::Immediate(_) => {
                panic!("invalid immediate type for scf.condition flag: {condition:?}")
            }
            Value::Poison { .. } => {
                panic!("invalid use of poison value for scf.condition flag: {condition:?}")
            }
        }
    }
}

impl Eval for hir::Assert {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let input = self.value().as_value_ref();
        match evaluator.use_value(&input)? {
            Immediate::I1(condition) => {
                if condition {
                    Ok(ControlFlowEffect::None)
                } else {
                    Ok(ControlFlowEffect::Trap {
                        span: self.span(),
                        reason: format!("assertion failed with code {}", self.code()),
                    })
                }
            }
            imm => Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected boolean value, got value of type {}: {imm}", imm.ty()),
            )),
        }
    }
}

impl Eval for hir::Assertz {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let input = self.value().as_value_ref();
        match evaluator.use_value(&input)? {
            Immediate::I1(condition) => {
                if condition {
                    Ok(ControlFlowEffect::Trap {
                        span: self.span(),
                        reason: format!("assertion failed with code {}", self.code()),
                    })
                } else {
                    Ok(ControlFlowEffect::None)
                }
            }
            imm => Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected boolean value, got value of type {}: {imm}", imm.ty()),
            )),
        }
    }
}

impl Eval for hir::AssertEq {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.lhs().as_value_ref();
        let rhs = self.rhs().as_value_ref();
        let lhs_value = evaluator.use_value(&lhs)?;
        let rhs_value = evaluator.use_value(&rhs)?;
        if lhs_value != rhs_value {
            Ok(ControlFlowEffect::Trap {
                span: self.span(),
                reason: format!("assertion failed: {lhs_value} != {rhs_value}"),
            })
        } else {
            Ok(ControlFlowEffect::None)
        }
    }
}

impl Eval for hir::PtrToInt {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let input = evaluator.get_value(&self.operand().as_value_ref())?;
        assert_eq!(input.ty(), Type::U32);
        evaluator.set_value(self.result().as_value_ref(), input);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::IntToPtr {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let input = evaluator.get_value(&self.operand().as_value_ref())?;
        match input {
            Value::Poison { origin, value, .. } => {
                if let Some(ptr) = value.as_u32() {
                    evaluator.set_value(
                        self.result().as_value_ref(),
                        Value::poison(origin, Immediate::U32(ptr)),
                    );
                    return Ok(ControlFlowEffect::None);
                }
            }
            Value::Immediate(value) => {
                if let Some(ptr) = value.as_u32() {
                    evaluator.set_value(
                        self.result().as_value_ref(),
                        Value::Immediate(Immediate::U32(ptr)),
                    );
                    return Ok(ControlFlowEffect::None);
                }
            }
        }

        Err(evaluator.report(
            "evaluation failed",
            self.span(),
            format!("invalid value for int-to-ptr cast (from {}): {}", input.ty(), input),
        ))
    }
}

impl Eval for hir::Cast {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        todo!()
    }
}

impl Eval for hir::Bitcast {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let input = evaluator.get_value(&self.operand().as_value_ref())?;
        let result = self.result();
        let output_ty = result.ty();
        let output = input.map_ty(output_ty).map_err(|err| {
            evaluator.report("evaluation failed", self.span(), format!("invalid bitcast: {err}"))
        })?;
        evaluator.set_value(result.as_value_ref(), output);

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::Exec {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let Some(symbol_table) = self.as_operation().nearest_symbol_table() else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                "cannot evaluate function calls without a symbol table in scope",
            ));
        };

        let symbol_table = symbol_table.borrow();
        let symbol_table = symbol_table.as_symbol_table().unwrap();
        let symbol_path = &self.callee().path;
        let Some(symbol) = symbol_table.resolve(symbol_path) else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("unable to resolve callee '{symbol_path}'"),
            ));
        };

        let arguments = ValueRange::<4>::from(self.arguments()).into_owned();

        Ok(ControlFlowEffect::Call {
            callee: symbol.borrow().as_operation_ref(),
            arguments,
        })
    }
}

impl Eval for hir::Store {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let addr = self.addr();
        let addr_value = evaluator.use_value(&addr.as_value_ref())?;
        let Immediate::U32(addr_value) = addr_value else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected pointer to be a u32 immediate, got {}", addr_value.ty()),
            ));
        };

        let value = evaluator.get_value(&self.value().as_value_ref())?;
        let value_ty = value.ty();
        let pointer_ty = addr.ty();
        let expected_ty = pointer_ty
            .pointee()
            .expect("expected pointer type to have been verified already");
        if &value_ty != expected_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid store: value type is {value_ty}, but pointee type is {expected_ty}"
                ),
            ));
        }

        evaluator.write_memory(addr_value, value)?;

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::StoreLocal {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let local = self.local();
        let value = evaluator.get_value(&self.value().as_value_ref())?;
        let value_ty = value.ty();
        let local_ty = local.ty();
        if value_ty != local_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid store to local variable: value type is {value_ty}, but local type is \
                     {local_ty}"
                ),
            ));
        }

        evaluator.write_local(local, value)?;

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::Load {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let addr = self.addr();
        let addr_value = evaluator.use_value(&addr.as_value_ref())?;
        let Immediate::U32(addr_value) = addr_value else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected pointer to be a u32 immediate, got {}", addr_value.ty()),
            ));
        };

        let result = self.result();
        let ty = result.ty();
        let pointer_ty = addr.ty();
        let expected_ty = pointer_ty
            .pointee()
            .expect("expected pointer type to have been verified already");
        if ty != expected_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("invalid load: value type is {ty}, but pointee type is {expected_ty}"),
            ));
        }

        let loaded = evaluator.read_memory(addr_value, ty)?;

        evaluator.set_value(result.as_value_ref(), loaded);

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::LoadLocal {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let local = self.local();
        let result = self.result();
        let ty = result.ty();
        let local_ty = local.ty();
        if ty != &local_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid load from local variable: value type is {ty}, but local type is \
                     {local_ty}"
                ),
            ));
        }

        let loaded = evaluator.read_local(local)?;

        evaluator.set_value(result.as_value_ref(), loaded);

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::MemGrow {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let pages = evaluator.use_value(&self.pages().as_value_ref())?;
        let Immediate::U32(pages) = pages else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected u32 input, got {}: {pages}", pages.ty()),
            ));
        };

        let current_size = {
            let current_context = evaluator.current_context_mut();
            let current_size = current_context.memory_size();
            current_context.memory_grow(pages as usize);
            current_size as u32
        };
        evaluator.set_value(self.result().as_value_ref(), Immediate::U32(current_size));
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::MemSize {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let current_size = evaluator.current_context().memory_size() as u32;

        evaluator.set_value(self.result().as_value_ref(), Immediate::U32(current_size));

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::MemSet {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let addr = self.addr();
        let addr_value = evaluator.use_value(&addr.as_value_ref())?;
        let Immediate::U32(addr_value) = addr_value else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected pointer to be a u32 immediate, got {}", addr_value.ty()),
            ));
        };

        let count = evaluator.use_value(&self.count().as_value_ref())?;
        let Immediate::U32(count) = count else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected count to be a u32 immediate, got {}", count.ty()),
            ));
        };

        let value = evaluator.use_value(&self.value().as_value_ref())?;

        // Verify that element type matches pointee type
        let value_ty = value.ty();
        let pointer_ty = addr.ty();
        let expected_ty = pointer_ty
            .pointee()
            .expect("expected pointer type to have been verified already");
        if &value_ty != expected_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid memset: element type is {value_ty}, but pointee type is {expected_ty}"
                ),
            ));
        }

        // Perform memset
        for offset in 0..count {
            let addr = addr_value + offset;
            evaluator.write_memory(addr, value)?;
        }

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for hir::MemCpy {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let source = self.source();
        let source_value = evaluator.use_value(&source.as_value_ref())?;
        let Immediate::U32(source_value) = source_value else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected source pointer to be a u32 immediate, got {}", source_value.ty()),
            ));
        };

        let dest = self.destination();
        let dest_value = evaluator.use_value(&dest.as_value_ref())?;
        let Immediate::U32(dest_value) = dest_value else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "expected destination pointer to be a u32 immediate, got {}",
                    dest_value.ty()
                ),
            ));
        };

        let count = evaluator.use_value(&self.count().as_value_ref())?;
        let Immediate::U32(count) = count else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("expected count to be a u32 immediate, got {}", count.ty()),
            ));
        };

        // Verify that source and destination pointer types match
        let source_ty = source.ty();
        let dest_ty = dest.ty();
        if source_ty != dest_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid memcpy: source and destination types do not match: {source_ty} vs \
                     {dest_ty}"
                ),
            ));
        }

        // Perform memcpy
        for offset in 0..count {
            let src = source_value + offset;
            let dst = dest_value + offset;
            let value = evaluator.read_memory(src, &source_ty)?;
            evaluator.write_memory(dst, value)?;
        }

        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Constant {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        evaluator.set_value(self.result().as_value_ref(), *self.value());
        Ok(ControlFlowEffect::None)
    }
}

macro_rules! binop {
    ($op:ident, $evaluator:ident, $operator:ident) => {{
        binop!($op, $evaluator, $operator, $operator)
    }};

    ($op:ident, $evaluator:ident, $operator:ident, $felt_operator:ident) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::I8(y)) => Immediate::I8(x.$operator(y)),
            (Immediate::U8(x), Immediate::U8(y)) => Immediate::U8(x.$operator(y)),
            (Immediate::I16(x), Immediate::I16(y)) => Immediate::I16(x.$operator(y)),
            (Immediate::U16(x), Immediate::U16(y)) => Immediate::U16(x.$operator(y)),
            (Immediate::I32(x), Immediate::I32(y)) => Immediate::I32(x.$operator(y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32(x.$operator(y)),
            (Immediate::I64(x), Immediate::I64(y)) => Immediate::I64(x.$operator(y)),
            (Immediate::U64(x), Immediate::U64(y)) => Immediate::U64(x.$operator(y)),
            (Immediate::I128(x), Immediate::I128(y)) => Immediate::I128(x.$operator(y)),
            (Immediate::U128(x), Immediate::U128(y)) => Immediate::U128(x.$operator(y)),
            (Immediate::Felt(x), Immediate::Felt(y)) => Immediate::Felt(x.$felt_operator(y)),
            _ => unreachable!(),
        }
    }};
}

macro_rules! binop_checked {
    ($op:ident, $evaluator:ident, $operator:ident, $felt_operator:ident) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::I8(y)) => x.$operator(y).map(Immediate::I8),
            (Immediate::U8(x), Immediate::U8(y)) => x.$operator(y).map(Immediate::U8),
            (Immediate::I16(x), Immediate::I16(y)) => x.$operator(y).map(Immediate::I16),
            (Immediate::U16(x), Immediate::U16(y)) => x.$operator(y).map(Immediate::U16),
            (Immediate::I32(x), Immediate::I32(y)) => x.$operator(y).map(Immediate::I32),
            (Immediate::U32(x), Immediate::U32(y)) => x.$operator(y).map(Immediate::U32),
            (Immediate::I64(x), Immediate::I64(y)) => x.$operator(y).map(Immediate::I64),
            (Immediate::U64(x), Immediate::U64(y)) => x.$operator(y).map(Immediate::U64),
            (Immediate::I128(x), Immediate::I128(y)) => x.$operator(y).map(Immediate::I128),
            (Immediate::U128(x), Immediate::U128(y)) => x.$operator(y).map(Immediate::U128),
            (Immediate::Felt(x), Immediate::Felt(y)) => Some(Immediate::Felt(x.$felt_operator(y))),
            _ => unreachable!(),
        }
    }};
}

macro_rules! binop_overflowing {
    ($op:ident, $evaluator:ident, $operator:ident, $felt_operator:ident) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::I8(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::I8(value), flag)
            }
            (Immediate::U8(x), Immediate::U8(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::U8(value), flag)
            }
            (Immediate::I16(x), Immediate::I16(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::I16(value), flag)
            }
            (Immediate::U16(x), Immediate::U16(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::U16(value), flag)
            }
            (Immediate::I32(x), Immediate::I32(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::I32(value), flag)
            }
            (Immediate::U32(x), Immediate::U32(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::U32(value), flag)
            }
            (Immediate::I64(x), Immediate::I64(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::I64(value), flag)
            }
            (Immediate::U64(x), Immediate::U64(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::U64(value), flag)
            }
            (Immediate::I128(x), Immediate::I128(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::I128(value), flag)
            }
            (Immediate::U128(x), Immediate::U128(y)) => {
                let (value, flag) = x.$operator(y);
                (Immediate::U128(value), flag)
            }
            (Immediate::Felt(x), Immediate::Felt(y)) => {
                let value = x.$felt_operator(y);
                (Immediate::Felt(value), false)
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! logical_binop {
    ($op:ident, $evaluator:ident, $operator:ident) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I1(x), Immediate::I1(y)) => x.$operator(y),
            _ => {
                return Err($evaluator.report(
                    "evaluation failed",
                    $op.span(),
                    format!("expected boolean operands, got {lhs_ty} and {rhs_ty}"),
                ));
            }
        }
    }};
}

trait InvalidFeltOperation: Sized {
    fn invalid_binary_felt_op(self, _other: Self) -> Self {
        panic!("unsupported felt operator")
    }

    fn invalid_unary_felt_op(self) -> Self {
        panic!("unsupported felt operator")
    }
}
impl InvalidFeltOperation for Felt {}

impl Eval for arith::Add {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Add;

        let result = match self.overflow() {
            Overflow::Unchecked => binop!(self, evaluator, add),
            Overflow::Checked => {
                let result = binop_checked!(self, evaluator, checked_add, add);
                let Some(result) = result else {
                    return Err(evaluator.report(
                        "evaluation failed",
                        self.span(),
                        "arithmetic overflow",
                    ));
                };
                result
            }
            Overflow::Wrapping => binop!(self, evaluator, wrapping_add, add),
            Overflow::Overflowing => unreachable!(),
        };

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::AddOverflowing {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Add;

        let (result, overflowed) = binop_overflowing!(self, evaluator, overflowing_add, add);
        evaluator.set_value(self.result().as_value_ref(), result);
        evaluator.set_value(self.overflowed().as_value_ref(), overflowed);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Sub {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Sub;

        let result = match self.overflow() {
            Overflow::Unchecked => binop!(self, evaluator, sub),
            Overflow::Checked => {
                let result = binop_checked!(self, evaluator, checked_sub, sub);
                let Some(result) = result else {
                    return Err(evaluator.report(
                        "evaluation failed",
                        self.span(),
                        "arithmetic underflow",
                    ));
                };
                result
            }
            Overflow::Wrapping => binop!(self, evaluator, wrapping_sub, sub),
            Overflow::Overflowing => unreachable!(),
        };

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::SubOverflowing {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Sub;

        let (result, overflowed) = binop_overflowing!(self, evaluator, overflowing_sub, sub);
        evaluator.set_value(self.result().as_value_ref(), result);
        evaluator.set_value(self.overflowed().as_value_ref(), overflowed);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Mul {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Mul;

        let result = match self.overflow() {
            Overflow::Unchecked => binop!(self, evaluator, mul),
            Overflow::Checked => {
                let result = binop_checked!(self, evaluator, checked_sub, mul);
                let Some(result) = result else {
                    return Err(evaluator.report(
                        "evaluation failed",
                        self.span(),
                        "arithmetic overflow",
                    ));
                };
                result
            }
            Overflow::Wrapping => binop!(self, evaluator, wrapping_mul, mul),
            Overflow::Overflowing => unreachable!(),
        };

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::MulOverflowing {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Mul;

        let (result, overflowed) = binop_overflowing!(self, evaluator, overflowing_mul, mul);
        evaluator.set_value(self.result().as_value_ref(), result);
        evaluator.set_value(self.overflowed().as_value_ref(), overflowed);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Exp {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        todo!()
    }
}

impl Eval for arith::Div {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Div;

        let result = binop!(self, evaluator, div);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Sdiv {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Div;

        let result = binop_checked!(self, evaluator, checked_div, div);
        match result {
            Some(result) => {
                evaluator.set_value(self.result().as_value_ref(), result);
            }
            None => {
                let divisor = evaluator.get_value(&self.rhs().as_value_ref()).unwrap();
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("division by {divisor}"),
                ));
            }
        }
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Mod {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = binop_checked!(self, evaluator, checked_rem_euclid, invalid_binary_felt_op);
        match result {
            Some(result) => {
                evaluator.set_value(self.result().as_value_ref(), result);
            }
            None => {
                return Err(evaluator.report("evaluation failed", self.span(), "division by zero"));
            }
        }
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Smod {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = binop_checked!(self, evaluator, checked_rem_euclid, invalid_binary_felt_op);
        match result {
            Some(result) => {
                evaluator.set_value(self.result().as_value_ref(), result);
            }
            None => {
                let divisor = evaluator.get_value(&self.rhs().as_value_ref()).unwrap();
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("division by {divisor}"),
                ));
            }
        }
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Divmod {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Div;
        let quotient = binop_checked!(self, evaluator, checked_div_euclid, div);
        let remainder = binop_checked!(self, evaluator, checked_rem_euclid, invalid_binary_felt_op);

        match (quotient, remainder) {
            (Some(quotient), Some(remainder)) => {
                evaluator.set_value(self.quotient().as_value_ref(), quotient);
                evaluator.set_value(self.remainder().as_value_ref(), remainder);
            }
            _ => {
                let divisor = evaluator.get_value(&self.rhs().as_value_ref()).unwrap();
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("division by {divisor}"),
                ));
            }
        }
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Sdivmod {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Div;
        let quotient = binop_checked!(self, evaluator, checked_div_euclid, div);
        let remainder = binop_checked!(self, evaluator, checked_rem_euclid, invalid_binary_felt_op);

        match (quotient, remainder) {
            (Some(quotient), Some(remainder)) => {
                evaluator.set_value(self.quotient().as_value_ref(), quotient);
                evaluator.set_value(self.remainder().as_value_ref(), remainder);
            }
            _ => {
                let divisor = evaluator.get_value(&self.rhs().as_value_ref()).unwrap();
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("division by {divisor}"),
                ));
            }
        }
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::And {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitAnd;
        let result = logical_binop!(self, evaluator, bitand);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Or {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitOr;
        let result = logical_binop!(self, evaluator, bitor);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Xor {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitXor;
        let result = logical_binop!(self, evaluator, bitxor);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Band {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitAnd;
        let result = binop!(self, evaluator, bitand, invalid_binary_felt_op);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Bor {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitOr;
        let result = binop!(self, evaluator, bitor, invalid_binary_felt_op);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Bxor {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::BitXor;
        let result = binop!(self, evaluator, bitxor, invalid_binary_felt_op);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
impl Eval for arith::Shl {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.lhs();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = self.shift();
        let rhs_value = evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        let result = match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::U32(y)) => Immediate::I8(x.wrapping_shl(y)),
            (Immediate::U8(x), Immediate::U32(y)) => Immediate::U8(x.wrapping_shl(y)),
            (Immediate::I16(x), Immediate::U32(y)) => Immediate::I16(x.wrapping_shl(y)),
            (Immediate::U16(x), Immediate::U32(y)) => Immediate::U16(x.wrapping_shl(y)),
            (Immediate::I32(x), Immediate::U32(y)) => Immediate::I32(x.wrapping_shl(y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32(x.wrapping_shl(y)),
            (Immediate::I64(x), Immediate::U32(y)) => Immediate::I64(x.wrapping_shl(y)),
            (Immediate::U64(x), Immediate::U32(y)) => Immediate::U64(x.wrapping_shl(y)),
            (Immediate::I128(x), Immediate::U32(y)) => Immediate::I128(x.wrapping_shl(y)),
            (Immediate::U128(x), Immediate::U32(y)) => Immediate::U128(x.wrapping_shl(y)),
            (_, Immediate::U32(_)) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("type does not support shl: {lhs_ty}"),
                ));
            }
            (..) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("invalid shift, expected u32, got {rhs_ty}"),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Shr {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.lhs();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = self.shift();
        let rhs_value = evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        let result = match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::U32(y)) => Immediate::I8(x.wrapping_shr(y)),
            (Immediate::U8(x), Immediate::U32(y)) => Immediate::U8(x.wrapping_shr(y)),
            (Immediate::I16(x), Immediate::U32(y)) => Immediate::I16(x.wrapping_shr(y)),
            (Immediate::U16(x), Immediate::U32(y)) => Immediate::U16(x.wrapping_shr(y)),
            (Immediate::I32(x), Immediate::U32(y)) => Immediate::I32(x.wrapping_shr(y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32(x.wrapping_shr(y)),
            (Immediate::I64(x), Immediate::U32(y)) => Immediate::I64(x.wrapping_shr(y)),
            (Immediate::U64(x), Immediate::U32(y)) => Immediate::U64(x.wrapping_shr(y)),
            (Immediate::I128(x), Immediate::U32(y)) => Immediate::I128(x.wrapping_shr(y)),
            (Immediate::U128(x), Immediate::U32(y)) => Immediate::U128(x.wrapping_shr(y)),
            (_, Immediate::U32(_)) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("type does not support shr: {lhs_ty}"),
                ));
            }
            (..) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("invalid shift, expected u32, got {rhs_ty}"),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Ashr {
    fn eval(&self, _evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        todo!()
    }
}

impl Eval for arith::Rotl {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.lhs();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = self.shift();
        let rhs_value = evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        let result = match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::U32(y)) => Immediate::I8(x.rotate_left(y)),
            (Immediate::U8(x), Immediate::U32(y)) => Immediate::U8(x.rotate_left(y)),
            (Immediate::I16(x), Immediate::U32(y)) => Immediate::I16(x.rotate_left(y)),
            (Immediate::U16(x), Immediate::U32(y)) => Immediate::U16(x.rotate_left(y)),
            (Immediate::I32(x), Immediate::U32(y)) => Immediate::I32(x.rotate_left(y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32(x.rotate_left(y)),
            (Immediate::I64(x), Immediate::U32(y)) => Immediate::I64(x.rotate_left(y)),
            (Immediate::U64(x), Immediate::U32(y)) => Immediate::U64(x.rotate_left(y)),
            (Immediate::I128(x), Immediate::U32(y)) => Immediate::I128(x.rotate_left(y)),
            (Immediate::U128(x), Immediate::U32(y)) => Immediate::U128(x.rotate_left(y)),
            (_, Immediate::U32(_)) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("type does not support rotl: {lhs_ty}"),
                ));
            }
            (..) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("invalid shift, expected u32, got {rhs_ty}"),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Rotr {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.lhs();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = self.shift();
        let rhs_value = evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        let result = match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::U32(y)) => Immediate::I8(x.rotate_right(y)),
            (Immediate::U8(x), Immediate::U32(y)) => Immediate::U8(x.rotate_right(y)),
            (Immediate::I16(x), Immediate::U32(y)) => Immediate::I16(x.rotate_right(y)),
            (Immediate::U16(x), Immediate::U32(y)) => Immediate::U16(x.rotate_right(y)),
            (Immediate::I32(x), Immediate::U32(y)) => Immediate::I32(x.rotate_right(y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32(x.rotate_right(y)),
            (Immediate::I64(x), Immediate::U32(y)) => Immediate::I64(x.rotate_right(y)),
            (Immediate::U64(x), Immediate::U32(y)) => Immediate::U64(x.rotate_right(y)),
            (Immediate::I128(x), Immediate::U32(y)) => Immediate::I128(x.rotate_right(y)),
            (Immediate::U128(x), Immediate::U32(y)) => Immediate::U128(x.rotate_right(y)),
            (_, Immediate::U32(_)) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("type does not support rotr: {lhs_ty}"),
                ));
            }
            (..) => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("invalid shift, expected u32, got {rhs_ty}"),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

macro_rules! comparison {
    ($op:ident, $evaluator:ident, $operator:ident) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::I8(y)) => x.$operator(&y),
            (Immediate::U8(x), Immediate::U8(y)) => x.$operator(&y),
            (Immediate::I16(x), Immediate::I16(y)) => x.$operator(&y),
            (Immediate::U16(x), Immediate::U16(y)) => x.$operator(&y),
            (Immediate::I32(x), Immediate::I32(y)) => x.$operator(&y),
            (Immediate::U32(x), Immediate::U32(y)) => x.$operator(&y),
            (Immediate::I64(x), Immediate::I64(y)) => x.$operator(&y),
            (Immediate::U64(x), Immediate::U64(y)) => x.$operator(&y),
            (Immediate::I128(x), Immediate::I128(y)) => x.$operator(&y),
            (Immediate::U128(x), Immediate::U128(y)) => x.$operator(&y),
            (Immediate::Felt(x), Immediate::Felt(y)) => x.as_int().$operator(&y.as_int()),
            _ => unreachable!(),
        }
    }};
}

macro_rules! comparison_with {
    ($op:ident, $evaluator:ident, $comparator:path) => {{
        let lhs = $op.lhs();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;
        let rhs = $op.rhs();
        let rhs_value = $evaluator.use_value(&rhs.as_value_ref())?;

        let lhs_ty = lhs.ty();
        let rhs_ty = lhs.ty();
        if lhs_ty != rhs_ty {
            return Err($evaluator.report(
                "evaluation failed",
                $op.span(),
                format!("operand types do not match: {lhs_ty} vs {rhs_ty}"),
            ));
        }

        match (lhs_value, rhs_value) {
            (Immediate::I8(x), Immediate::I8(y)) => Immediate::I8($comparator(x, y)),
            (Immediate::U8(x), Immediate::U8(y)) => Immediate::U8($comparator(x, y)),
            (Immediate::I16(x), Immediate::I16(y)) => Immediate::I16($comparator(x, y)),
            (Immediate::U16(x), Immediate::U16(y)) => Immediate::U16($comparator(x, y)),
            (Immediate::I32(x), Immediate::I32(y)) => Immediate::I32($comparator(x, y)),
            (Immediate::U32(x), Immediate::U32(y)) => Immediate::U32($comparator(x, y)),
            (Immediate::I64(x), Immediate::I64(y)) => Immediate::I64($comparator(x, y)),
            (Immediate::U64(x), Immediate::U64(y)) => Immediate::U64($comparator(x, y)),
            (Immediate::I128(x), Immediate::I128(y)) => Immediate::I128($comparator(x, y)),
            (Immediate::U128(x), Immediate::U128(y)) => Immediate::U128($comparator(x, y)),
            (Immediate::Felt(x), Immediate::Felt(y)) => {
                Immediate::Felt(Felt::new($comparator(x.as_int(), y.as_int())))
            }
            _ => unreachable!(),
        }
    }};
}

impl Eval for arith::Eq {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, eq);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Neq {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, ne);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Gt {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, gt);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Gte {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, ge);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Lt {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, lt);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Lte {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison!(self, evaluator, le);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Min {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison_with!(self, evaluator, core::cmp::min);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Max {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let result = comparison_with!(self, evaluator, core::cmp::max);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Trunc {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let lhs_ty = lhs_value.ty();
        let result = self.result();
        let expected_ty = result.ty();
        if &lhs_ty == expected_ty {
            evaluator.set_value(self.result().as_value_ref(), lhs_value);
            return Ok(ControlFlowEffect::None);
        }

        if lhs_ty.size_in_bits() < expected_ty.size_in_bits() {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid truncation: input type of {lhs_ty} is smaller than the target type \
                     {expected_ty}"
                ),
            ));
        }

        let result = match expected_ty {
            Type::I1 => lhs_value.bitcast_u128().map(|x| Immediate::I1(x % 2 != 0)),
            Type::I8 => lhs_value.bitcast_u128().map(|x| Immediate::I8(x as i8)),
            Type::U8 => lhs_value.bitcast_u128().map(|x| Immediate::U8(x as u8)),
            Type::I16 => lhs_value.bitcast_u128().map(|x| Immediate::I16(x as i16)),
            Type::U16 => lhs_value.bitcast_u128().map(|x| Immediate::U16(x as u16)),
            Type::I32 => lhs_value.bitcast_u128().map(|x| Immediate::I32(x as i32)),
            Type::U32 => lhs_value.bitcast_u128().map(|x| Immediate::U32(x as u32)),
            Type::I64 => lhs_value.bitcast_u128().map(|x| Immediate::I64(x as i64)),
            Type::U64 => lhs_value.bitcast_u128().map(|x| Immediate::U64(x as u64)),
            Type::I128 => lhs_value.bitcast_i128().map(Immediate::I128),
            Type::U128 => lhs_value.bitcast_u128().map(Immediate::U128),
            unsupported_ty => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("invalid truncation: target type of {unsupported_ty} not supported"),
                ));
            }
        }
        .expect("expected infallable cast by this point");

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Zext {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let lhs_ty = lhs_value.ty();
        let result = self.result();
        let expected_ty = result.ty();
        if &lhs_ty == expected_ty {
            evaluator.set_value(self.result().as_value_ref(), lhs_value);
            return Ok(ControlFlowEffect::None);
        }

        if lhs_ty.size_in_bits() > expected_ty.size_in_bits() {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid extension: input type of {lhs_ty} is larger than the target type \
                     {expected_ty}"
                ),
            ));
        }

        let result = match expected_ty {
            Type::U8 => lhs_value.bitcast_u8().map(Immediate::U8),
            Type::U16 => lhs_value.bitcast_u16().map(Immediate::U16),
            Type::U32 => lhs_value.bitcast_u32().map(Immediate::U32),
            Type::U64 => lhs_value.bitcast_u64().map(Immediate::U64),
            Type::U128 => lhs_value.bitcast_u128().map(Immediate::U128),
            unsupported_ty => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!(
                        "invalid zero-extension: target type of {unsupported_ty} not supported"
                    ),
                ));
            }
        }
        .expect("expected infallable cast by this point");

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Sext {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let lhs_ty = lhs_value.ty();
        let result = self.result();
        let expected_ty = result.ty();
        if &lhs_ty == expected_ty {
            evaluator.set_value(self.result().as_value_ref(), lhs_value);
            return Ok(ControlFlowEffect::None);
        }

        if lhs_ty.size_in_bits() > expected_ty.size_in_bits() {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!(
                    "invalid extension: input type of {lhs_ty} is larger than the target type \
                     {expected_ty}"
                ),
            ));
        }

        let result = match expected_ty {
            Type::I8 => lhs_value.bitcast_i8().map(Immediate::I8),
            Type::I16 => lhs_value.bitcast_i16().map(Immediate::I16),
            Type::I32 => lhs_value.bitcast_i32().map(Immediate::I32),
            Type::I64 => lhs_value.bitcast_i64().map(Immediate::I64),
            Type::I128 => lhs_value.bitcast_i128().map(Immediate::I128),
            unsupported_ty => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!(
                        "invalid sign-extension: target type of {unsupported_ty} not supported"
                    ),
                ));
            }
        }
        .expect("expected infallable cast by this point");

        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

macro_rules! unaryop {
    ($op:ident, $evaluator:ident, $operator:ident) => {{
        unaryop!($op, $evaluator, $operator, $operator)
    }};

    ($op:ident, $evaluator:ident, $operator:ident, $felt_operator:ident) => {{
        let lhs = $op.operand();
        let lhs_value = $evaluator.use_value(&lhs.as_value_ref())?;

        match lhs_value {
            Immediate::I8(x) => Immediate::I8(x.$operator()),
            Immediate::U8(x) => Immediate::U8(x.$operator()),
            Immediate::I16(x) => Immediate::I16(x.$operator()),
            Immediate::U16(x) => Immediate::U16(x.$operator()),
            Immediate::I32(x) => Immediate::I32(x.$operator()),
            Immediate::U32(x) => Immediate::U32(x.$operator()),
            Immediate::I64(x) => Immediate::I64(x.$operator()),
            Immediate::U64(x) => Immediate::U64(x.$operator()),
            Immediate::I128(x) => Immediate::I128(x.$operator()),
            Immediate::U128(x) => Immediate::U128(x.$operator()),
            Immediate::Felt(x) => Immediate::Felt(x.$felt_operator()),
            _ => unreachable!(),
        }
    }};
}

impl Eval for arith::Incr {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use midenc_hir2::FieldElement;

        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::I8(x.wrapping_add(1)),
            Immediate::U8(x) => Immediate::U8(x.wrapping_add(1)),
            Immediate::I16(x) => Immediate::I16(x.wrapping_add(1)),
            Immediate::U16(x) => Immediate::U16(x.wrapping_add(1)),
            Immediate::I32(x) => Immediate::I32(x.wrapping_add(1)),
            Immediate::U32(x) => Immediate::U32(x.wrapping_add(1)),
            Immediate::I64(x) => Immediate::I64(x.wrapping_add(1)),
            Immediate::U64(x) => Immediate::U64(x.wrapping_add(1)),
            Immediate::I128(x) => Immediate::I128(x.wrapping_add(1)),
            Immediate::U128(x) => Immediate::U128(x.wrapping_add(1)),
            Immediate::Felt(x) => Immediate::Felt(x + Felt::ONE),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("type does not support incr: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Neg {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::I8(-x),
            Immediate::U8(x) => Immediate::U8(!x),
            Immediate::I16(x) => Immediate::I16(-x),
            Immediate::U16(x) => Immediate::U16(!x),
            Immediate::I32(x) => Immediate::I32(-x),
            Immediate::U32(x) => Immediate::U32(!x),
            Immediate::I64(x) => Immediate::I64(-x),
            Immediate::U64(x) => Immediate::U64(!x),
            Immediate::I128(x) => Immediate::I128(-x),
            Immediate::U128(x) => Immediate::U128(!x),
            Immediate::Felt(x) => Immediate::Felt(Felt::new(!x.as_int())),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("negation is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Inv {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use midenc_hir2::FieldElement;

        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::Felt(x) => Immediate::Felt(x.inv()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("modular inverse is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Ilog2 {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.ilog2()),
            Immediate::U8(x) => Immediate::U32(x.ilog2()),
            Immediate::I16(x) => Immediate::U32(x.ilog2()),
            Immediate::U16(x) => Immediate::U32(x.ilog2()),
            Immediate::I32(x) => Immediate::U32(x.ilog2()),
            Immediate::U32(x) => Immediate::U32(x.ilog2()),
            Immediate::I64(x) => Immediate::U32(x.ilog2()),
            Immediate::U64(x) => Immediate::U32(x.ilog2()),
            Immediate::I128(x) => Immediate::U32(x.ilog2()),
            Immediate::U128(x) => Immediate::U32(x.ilog2()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().ilog2()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("ilog2 is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Pow2 {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let Some(power) = lhs_value.as_u32() else {
            return Err(evaluator.report(
                "evaluation failed",
                self.span(),
                format!("invalid power for pow2: {lhs_value} (type is {})", lhs.ty()),
            ));
        };
        let result = 2u32.pow(power);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Not {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I1(x) => !x,
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("expected boolean operand, got {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Bnot {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        use core::ops::Not;

        let result = unaryop!(self, evaluator, not, invalid_unary_felt_op);
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::IsOdd {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => x % 2 != 0,
            Immediate::U8(x) => x % 2 != 0,
            Immediate::I16(x) => x % 2 != 0,
            Immediate::U16(x) => x % 2 != 0,
            Immediate::I32(x) => x % 2 != 0,
            Immediate::U32(x) => x % 2 != 0,
            Immediate::I64(x) => x % 2 != 0,
            Immediate::U64(x) => x % 2 != 0,
            Immediate::I128(x) => x % 2 != 0,
            Immediate::U128(x) => x % 2 != 0,
            Immediate::Felt(x) => x.as_int() % 2 != 0,
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("is_odd is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Popcnt {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.count_ones()),
            Immediate::U8(x) => Immediate::U32(x.count_ones()),
            Immediate::I16(x) => Immediate::U32(x.count_ones()),
            Immediate::U16(x) => Immediate::U32(x.count_ones()),
            Immediate::I32(x) => Immediate::U32(x.count_ones()),
            Immediate::U32(x) => Immediate::U32(x.count_ones()),
            Immediate::I64(x) => Immediate::U32(x.count_ones()),
            Immediate::U64(x) => Immediate::U32(x.count_ones()),
            Immediate::I128(x) => Immediate::U32(x.count_ones()),
            Immediate::U128(x) => Immediate::U32(x.count_ones()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().count_ones()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("popcnt is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Clz {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.leading_zeros()),
            Immediate::U8(x) => Immediate::U32(x.leading_zeros()),
            Immediate::I16(x) => Immediate::U32(x.leading_zeros()),
            Immediate::U16(x) => Immediate::U32(x.leading_zeros()),
            Immediate::I32(x) => Immediate::U32(x.leading_zeros()),
            Immediate::U32(x) => Immediate::U32(x.leading_zeros()),
            Immediate::I64(x) => Immediate::U32(x.leading_zeros()),
            Immediate::U64(x) => Immediate::U32(x.leading_zeros()),
            Immediate::I128(x) => Immediate::U32(x.leading_zeros()),
            Immediate::U128(x) => Immediate::U32(x.leading_zeros()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().leading_zeros()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("clz is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Ctz {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::U8(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::I16(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::U16(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::I32(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::U32(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::I64(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::U64(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::I128(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::U128(x) => Immediate::U32(x.trailing_zeros()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().trailing_zeros()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("ctz is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Clo {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.leading_ones()),
            Immediate::U8(x) => Immediate::U32(x.leading_ones()),
            Immediate::I16(x) => Immediate::U32(x.leading_ones()),
            Immediate::U16(x) => Immediate::U32(x.leading_ones()),
            Immediate::I32(x) => Immediate::U32(x.leading_ones()),
            Immediate::U32(x) => Immediate::U32(x.leading_ones()),
            Immediate::I64(x) => Immediate::U32(x.leading_ones()),
            Immediate::U64(x) => Immediate::U32(x.leading_ones()),
            Immediate::I128(x) => Immediate::U32(x.leading_ones()),
            Immediate::U128(x) => Immediate::U32(x.leading_ones()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().leading_ones()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("clo is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}

impl Eval for arith::Cto {
    fn eval(&self, evaluator: &mut HirEvaluator) -> Result<ControlFlowEffect, Report> {
        let lhs = self.operand();
        let lhs_value = evaluator.use_value(&lhs.as_value_ref())?;

        let result = match lhs_value {
            Immediate::I8(x) => Immediate::U32(x.trailing_ones()),
            Immediate::U8(x) => Immediate::U32(x.trailing_ones()),
            Immediate::I16(x) => Immediate::U32(x.trailing_ones()),
            Immediate::U16(x) => Immediate::U32(x.trailing_ones()),
            Immediate::I32(x) => Immediate::U32(x.trailing_ones()),
            Immediate::U32(x) => Immediate::U32(x.trailing_ones()),
            Immediate::I64(x) => Immediate::U32(x.trailing_ones()),
            Immediate::U64(x) => Immediate::U32(x.trailing_ones()),
            Immediate::I128(x) => Immediate::U32(x.trailing_ones()),
            Immediate::U128(x) => Immediate::U32(x.trailing_ones()),
            Immediate::Felt(x) => Immediate::U32(x.as_int().trailing_ones()),
            _ => {
                return Err(evaluator.report(
                    "evaluation failed",
                    self.span(),
                    format!("cto is not supported for value type: {}", lhs.ty()),
                ));
            }
        };
        evaluator.set_value(self.result().as_value_ref(), result);
        Ok(ControlFlowEffect::None)
    }
}
