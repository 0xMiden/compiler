mod context;
mod frame;
mod memory;

use alloc::{format, rc::Rc, string::ToString, vec, vec::Vec};

use midenc_hir2::{
    dialects::builtin::{ComponentId, LocalVariable},
    formatter::DisplayValues,
    smallvec, CallableOpInterface, Context, Immediate, Operation, OperationRef, RegionBranchPoint,
    RegionRef, Report, SmallVec, SourceSpan, Spanned, SymbolPath, Type, Value as _, ValueRange,
    ValueRef,
};
use midenc_session::diagnostics::{InFlightDiagnosticBuilder, Severity};

use self::{context::ExecutionContext, frame::CallFrame};
use crate::{value::MaterializedValue, *};

pub struct HirEvaluator {
    /// The context in which all IR objects are allocated
    context: Rc<Context>,
    /// The execution context stack
    contexts: Vec<ExecutionContext>,
    /// The stack of call frames maintained during execution, for use in constructing stack traces.
    call_stack: Vec<CallFrame>,
    /// An unspecified set of bit flags that can be manipulated by an operation when transferring
    /// control to a successor operation. The semantics of the bits are dictated entirely by the
    /// ops in question.
    ///
    /// It is expected that a protocol is adhered to in regards to setting/clearing condition flags.
    /// Namely, the operation that sets condition flags should expect that the receiving operation
    /// will clear them. Furthermore, if an operation observes a condition it doesn't understand,
    /// it must assert.
    condition: u8,
    /// The last operation that set/reset the condition flags
    condition_set_by: Option<OperationRef>,
    /// The current operation being executed
    ip: Option<OperationRef>,
}

impl HirEvaluator {
    /// Construct a new evaluator for `context`
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            contexts: vec![ExecutionContext::default()],
            call_stack: Default::default(),
            condition: 0,
            condition_set_by: None,
            ip: None,
        }
    }

    /// Reset the evaluator state to start the next evaluation with a clean slate.
    pub fn reset(&mut self) {
        self.contexts.truncate(1);
        self.current_context_mut().reset();
        self.call_stack.clear();
        self.condition = 0;
        self.condition_set_by = None;
        self.ip = None;
    }

    /// The current frame of the call stack
    fn current_frame(&self) -> &CallFrame {
        self.call_stack.last().expect("cannot read current call frame")
    }

    /// The current frame of the call stack
    fn current_frame_mut(&mut self) -> &mut CallFrame {
        self.call_stack.last_mut().expect("cannot read current call frame")
    }

    /// The current execution context (i.e. memory, registers)
    pub fn current_context(&self) -> &ExecutionContext {
        self.contexts.last().unwrap()
    }

    /// The current execution context (i.e. memory, registers)
    pub fn current_context_mut(&mut self) -> &mut ExecutionContext {
        self.contexts.last_mut().unwrap()
    }

    /// Enter a fresh execution context (i.e. memory, registers), with an optional identifier
    pub fn enter_context(&mut self, id: Option<ComponentId>) {
        self.contexts.push(id.map(ExecutionContext::new).unwrap_or_default());
    }

    /// Exit the current execution context, and return the previous context on the context stack.
    pub fn exit_context(&mut self) {
        assert!(self.contexts.len() > 1, "cannot exit the root context");
        self.contexts.pop().expect("attempted to exit a context that doesn't exist");
    }

    /// Evaluate `op` with `args`, returning the results, if any, produced by it.
    ///
    /// This will fail with an error if any of the following occur:
    ///
    /// * The number and type of arguments does not match the operands expected by `op`
    /// * `op` implements `Initialize` and initialization fails
    /// * An error occurs while evaluating `op`
    pub fn eval<I>(&mut self, op: &Operation, args: I) -> Result<SmallVec<[Value; 1]>, Report>
    where
        I: IntoIterator<Item = Value>,
    {
        // Handle evaluation of callable symbols specially
        if let Some(callable) = op.as_trait::<dyn CallableOpInterface>() {
            return self.eval_callable(callable, args);
        }

        self.reset();

        let args = args.into_iter().collect::<SmallVec<[_; 8]>>();

        // Check arity
        if op.num_operands() != args.len() {
            return Err(self.report(
                "invalid evaluation",
                op.span(),
                format!("expected {} arguments, but {} were given", op.num_operands(), args.len()),
            ));
        }

        // Check types
        for (expected, given) in op.operands().iter().zip(args.iter()) {
            let given_ty = given.ty();
            let expected = expected.borrow();
            let expected_ty = expected.ty();
            if expected_ty != given_ty {
                return Err(self
                    .error("invalid evaluation")
                    .with_primary_label(
                        op.span(),
                        format!("argument type mismatch: expected {expected_ty}, got {given_ty}",),
                    )
                    .with_secondary_label(
                        expected.span(),
                        "argument provided for this operand is invalid",
                    )
                    .into_report());
            }
        }

        // If the root operation implements [Initialize], perform initialization now.
        if let Some(initializable) = op.as_trait::<dyn Initialize>() {
            initializable.initialize(self)?;
        }

        self.ip = Some(op.as_operation_ref());

        // NOTE: This is a bit of a hack, because `op` may not actually be callable
        let mut frame = CallFrame::new(op.as_operation_ref());

        // Initialize operand registers with the given arguments
        for (param, arg) in ValueRange::<2>::from(op.operands().all()).into_iter().zip(args) {
            frame.set_value(param, arg);
        }

        self.call_stack.push(frame);

        // Start evaluation
        self.eval_op_and_gather_results(op)
    }

    /// Evaluate `callable` with `args`, returning the results, if any, produced by it.
    ///
    /// This will fail with an error if any of the following occur:
    ///
    /// * The number and type of arguments does not match the callable signature.
    /// * `callable` implements `Initialize` and initialization fails
    /// * `callable` is a declaration
    /// * An error occurs while evaluating the callable region
    pub fn eval_callable<I>(
        &mut self,
        callable: &dyn CallableOpInterface,
        args: I,
    ) -> Result<SmallVec<[Value; 1]>, Report>
    where
        I: IntoIterator<Item = Value>,
    {
        self.reset();

        // Verify the callable symbol is defined, not just declared
        let Some(callable_region) = callable.get_callable_region() else {
            return Err(self.report(
                "invalid entrypoint",
                callable.as_operation().span(),
                "symbol declarations are not valid callee targets",
            ));
        };

        let signature = callable.signature();
        let args = args.into_iter().collect::<SmallVec<[_; 8]>>();

        // Check arity
        if signature.arity() != args.len() {
            return Err(self.report(
                "invalid call",
                callable.as_operation().span(),
                format!("expected {} arguments, but {} were given", signature.arity(), args.len()),
            ));
        }

        // Check types
        for (index, (expected, given)) in signature.params().iter().zip(args.iter()).enumerate() {
            let given_ty = given.ty();
            if expected.ty != given_ty {
                let arg = callable_region.borrow().entry().arguments()[index];
                return Err(self
                    .error("invalid call")
                    .with_primary_label(
                        callable.as_operation().span(),
                        format!(
                            "argument type mismatch: expected {}, got {given_ty}",
                            &expected.ty
                        ),
                    )
                    .with_secondary_label(
                        arg.span(),
                        "argument provided for this parameter is invalid",
                    )
                    .into_report());
            }
        }

        // If the callable also implements [Initialize], perform initialization now.
        if let Some(initializable) = callable.as_operation().as_trait::<dyn Initialize>() {
            initializable.initialize(self)?;
        }

        // Initialize the call stack
        let mut frame = CallFrame::new(callable.as_operation().as_operation_ref());

        // Initialize registers with the callee arguments
        let region = callable_region.borrow();
        for (param, arg) in region.entry().argument_values().zip(args) {
            frame.set_value(param, arg);
        }

        self.call_stack.push(frame);

        // Evaluate the callable region
        self.eval_region(callable.as_operation(), callable_region)
    }

    /// Evaluate `symbol` in `symbol_table` with `args`, returning the results, if any, it produces.
    ///
    /// This will fail with an error if any of the following occur:
    ///
    /// * `symbol_table` does not implement `SymbolTable`
    /// * `symbol_table` implements `Initialize` and initialization fails
    /// * `symbol` is not resolvable via `op`'s symbol table
    /// * `symbol` does not implement `CallableOpInterface`
    /// * `symbol` is only a declaration
    /// * The number and type of arguments does not match the symbol parameter list
    /// * An error occurs while evaluating the given symbol
    pub fn call<I>(
        &mut self,
        symbol_table: &Operation,
        symbol: &SymbolPath,
        args: I,
    ) -> Result<SmallVec<[Value; 1]>, Report>
    where
        I: IntoIterator<Item = Value>,
    {
        let Some(symbol_table) = symbol_table.as_symbol_table() else {
            return Err(self.report(
                "expected op to be a symbol table",
                symbol_table.span(),
                "this op does not implement the SymbolTable trait",
            ));
        };

        // Resolve the symbol
        let symbol_manager = symbol_table.symbol_manager();
        let Some(symbol) = symbol_manager.lookup_symbol_ref(symbol) else {
            return Err(self.report(
                "invalid entrypoint",
                symbol_table.as_symbol_table_operation().span(),
                format!("could not resolve '{symbol}' in this op"),
            ));
        };

        let op = symbol.borrow();

        // Verify the symbol is callable
        let Some(callable) = op.as_trait::<dyn CallableOpInterface>() else {
            return Err(self.report(
                "invalid entrypoint",
                op.span(),
                "this symbol does not implement CallableOpInterface",
            ));
        };

        // Verify the callable symbol is defined, not just declared
        let Some(callable_region) = callable.get_callable_region() else {
            return Err(self.report(
                "invalid entrypoint",
                op.span(),
                "symbol declarations are not valid callee targets",
            ));
        };

        let signature = callable.signature();
        let args = args.into_iter().collect::<SmallVec<[_; 8]>>();

        // Check arity
        if signature.arity() != args.len() {
            return Err(self.report(
                "invalid call",
                op.span(),
                format!("expected {} arguments, but {} were given", signature.arity(), args.len()),
            ));
        }

        // Check types
        for (index, (expected, given)) in signature.params().iter().zip(args.iter()).enumerate() {
            let given_ty = given.ty();
            if expected.ty != given_ty {
                let arg = callable_region.borrow().entry().arguments()[index];
                return Err(self
                    .error("invalid call")
                    .with_primary_label(
                        op.span(),
                        format!(
                            "argument type mismatch: expected {}, got {given_ty}",
                            &expected.ty
                        ),
                    )
                    .with_secondary_label(
                        arg.span(),
                        "argument provided for this parameter is invalid",
                    )
                    .into_report());
            }
        }

        // If the root operation implements [Initialize], perform initialization now.
        if let Some(initializable) =
            symbol_table.as_symbol_table_operation().as_trait::<dyn Initialize>()
        {
            initializable.initialize(self)?;
        }

        // Initialize the call stack
        let mut frame = CallFrame::new(symbol);

        // Initialize registers with the callee arguments
        let region = callable_region.borrow();
        for (param, arg) in region.entry().argument_values().zip(args) {
            frame.set_value(param, arg);
        }

        self.call_stack.push(frame);

        // Evaluate the callable region
        self.eval_region(&op, callable_region)
    }

    /// Read a value of type `ty` from `addr`
    ///
    /// Returns an error if `addr` is invalid, `ty` is not a valid immediate type, or the specified
    /// type could not be read from `addr` (either the encoding is invalid, or the read would be
    /// out of bounds).
    pub fn read_memory(&self, addr: u32, ty: &Type) -> Result<Value, Report> {
        self.current_context().read_memory(addr, ty, self.current_span())
    }

    /// Write `value` to `addr` in heap memory.
    ///
    /// Returns an error if `addr` is invalid, or `value` could not be written to `addr` (either the
    /// value is poison, or the write would go out of bounds).
    pub fn write_memory(&mut self, addr: u32, value: impl Into<Value>) -> Result<(), Report> {
        let at = self.current_span();
        self.current_context_mut().write_memory(addr, value, at)
    }

    /// Read the value of the given local variable in the current symbol, if present.
    ///
    /// See [CallFrame::read_local] for details on correct usage.
    ///
    /// # Panics
    ///
    /// This function will panic if the call stack is empty.
    pub fn read_local(&self, local: &LocalVariable) -> Result<Value, Report> {
        self.call_stack
            .last()
            .expect("cannot read local variables outside of a function")
            .read_local(local, self.current_span(), &self.context)
    }

    /// Write `value` to `local`.
    ///
    /// See [CallFrame::write_local] for details on correct usage.
    ///
    /// # Panics
    ///
    /// This function will panic if the call stack is empty.
    pub fn write_local(
        &mut self,
        local: &LocalVariable,
        value: impl Into<Value>,
    ) -> Result<(), Report> {
        let span = self.current_span();
        self.call_stack
            .last_mut()
            .expect("cannot write local variables outside of a function")
            .write_local(local, value.into(), span, &self.context)
    }

    /// Read the concrete value assigned to `value` at the current program point
    pub fn get_value(&self, value: &ValueRef) -> Result<Value, Report> {
        self.current_frame().get_value(value, self.current_span())
    }

    /// Read the concrete value assigned to `value` at the current program point, and verify that
    /// its usage is valid, i.e. not poison.
    pub fn use_value(&self, value: &ValueRef) -> Result<Immediate, Report> {
        self.current_frame().use_value(value, self.current_span())
    }

    /// Set the concrete value assigned to `id`
    pub fn set_value(&mut self, id: ValueRef, value: impl Into<Value>) {
        self.current_frame_mut().set_value(id, value);
    }

    /// Start building an error diagnostic
    pub fn error(&self, message: impl ToString) -> InFlightDiagnosticBuilder<'_> {
        self.context
            .session()
            .diagnostics
            .diagnostic(Severity::Error)
            .with_message(message)
    }

    /// Construct a [Report] from an error diagnostic consisting of a simple message and label.
    pub fn report(&self, message: impl ToString, _at: SourceSpan, label: impl ToString) -> Report {
        panic!("{}: {}", message.to_string(), label.to_string())
        /*
        self.context
            .session()
            .diagnostics
            .diagnostic(Severity::Error)
            .with_message(message)
            .with_primary_label(at, label)
            .into_report()
             */
    }

    pub fn current_span(&self) -> SourceSpan {
        self.ip.map(|ip| ip.span()).unwrap_or_default()
    }
}

impl HirEvaluator {
    /// This function implements the core interpreter loop.
    ///
    /// Fundamentally, the way it works is by evaluating `region` of `op` starting in its entry
    /// block. Each operation in the block is evaluated using the `Eval` implementation for that
    /// op, and the control flow effect produced by evaluation is used to drive the core loop:
    ///
    /// * For primitive operations that produce no control flow effects, evaluation proceeds to
    ///   the next operation in the block, unless a trap or error occurred to stop evaluation.
    /// * For unstructured control flow branches, evaluation proceeds to the first operation in
    ///   the given destination block, and any provided successor arguments are written to the
    ///   registers of the current frame using the block parameter value ids.
    /// * For return-like operations, the call frame for the containing callable operation is
    ///   popped from the call stack, and one of two paths is taken:
    ///   1. If the top of the call stack has been reached, evaluation terminates and the given
    ///      return values are returned as the result of `eval_region` itself.
    ///   2. Otherwise, control is transferred to the next operation after the caller op, by
    ///      updating the next region, block, and op, and continuing the loop there. The return
    ///      values are stored in the registers of the caller's frame as the results of the call.
    /// * For yield-like operations:
    ///   1. If the yield is returning to the parent operation, then much like return-like ops,
    ///      control is tranferred to the next operation after the parent op, and the yielded values
    ///      are stored in the registers of the current frame as the results of the parent op.
    ///   2. If the yield is entering another region, then the yielded values are written to the
    ///      registers of the current frame using the value ids of the destination region's entry
    ///      block arguments. Control is transferred by updating the next region of the interpreter
    ///      loop, and resuming that loop, which will then start evaluating operations in the
    ///      entry block of that region.
    /// * Lastly, call operations push a new frame on the call stack, and its registers are
    ///   initialized with the provided arguments, under the entry block arguments of the callable
    ///   region. Control is transferred by updating the next region of the interpreter loop, and
    ///   resuming that loop, which will then start evaluating operations in the entry block of
    ///   the callable region.
    fn eval_region(
        &mut self,
        op: &Operation,
        region: RegionRef,
    ) -> Result<SmallVec<[Value; 1]>, Report> {
        log::debug!(target: "eval", "evaluating {} of {}", RegionBranchPoint::Child(region), op);

        self.ip = Some(op.as_operation_ref());

        let mut next_region = Some(region);
        'region: while let Some(mut region) = next_region.take() {
            let mut next_block = region.borrow().entry_block_ref();
            'block: while let Some(mut block) = next_block.take() {
                let mut next_op = block.borrow().body().front().as_pointer();
                'op: while let Some(op) = next_op.take() {
                    next_op = op.next();
                    let op = op.borrow();
                    match self.eval_op(&op)? {
                        ControlFlowEffect::None => continue,
                        ControlFlowEffect::Trap { span, reason } => {
                            return Err(self
                                .error("evaluation failed")
                                .with_primary_label(
                                    op.span(),
                                    "execution trapped due to this operation",
                                )
                                .with_secondary_label(span, reason)
                                .into_report());
                        }
                        ControlFlowEffect::Return(returned) => {
                            // If this is the end of the call stack, we're returning from the
                            // top-level operation
                            let is_final_return = self.call_stack.len() == 1;
                            let frame = self.call_stack.pop().unwrap();

                            // Set up the resumption point if we're resuming execution after the
                            // caller
                            if !is_final_return {
                                // Restore the instruction pointer to the point where control was
                                // transferred to the callee
                                let caller_block = frame.caller_block().unwrap();
                                next_op = frame.return_to();
                                // NOTE: We change `block` here, rather than updating `next_block`,
                                // because we're resuming control with `next_op`, which doesn't
                                // revisit the outer `'block` loop.
                                block = caller_block;
                                next_region = caller_block.parent();
                            }

                            // Verify the results that were returned
                            let callee = frame.callee();
                            let callable = callee.as_trait::<dyn CallableOpInterface>().unwrap();
                            let signature = callable.signature();
                            let call = frame.caller();
                            let results_returned = returned.is_some() as usize;
                            let results_expected = signature.results().len();
                            if let Some(call) = call.as_ref() {
                                assert_eq!(
                                    call.num_results(),
                                    results_expected,
                                    "expected to have caught call/callee signature mismatch \
                                     during verification"
                                );
                            }
                            if results_returned != results_expected {
                                return Err(self
                                    .error("evaluation failed")
                                    .with_primary_label(
                                        callee.span(),
                                        format!(
                                            "callee returned {results_returned} results, but \
                                             {results_expected} were expected"
                                        ),
                                    )
                                    .with_secondary_label(
                                        callee.span(),
                                        "this callable returned incorrect number of results",
                                    )
                                    .into_report());
                            }
                            if let Some(return_ty) = returned.as_ref().map(|v| v.ty()) {
                                let expected_ty = &signature.results[0].ty;
                                if &return_ty != expected_ty {
                                    return Err(self
                                        .error("evaluation failed")
                                        .with_primary_label(
                                            callee.span(),
                                            format!(
                                                "callee returned result type that does not match \
                                                 its signature, got {return_ty} but signature \
                                                 requires {expected_ty}"
                                            ),
                                        )
                                        .with_secondary_label(
                                            callee.span(),
                                            "this callable returned a value that does not match \
                                             its signature",
                                        )
                                        .into_report());
                                }
                            }

                            if is_final_return {
                                // We're done executing the top-level operation, return to the
                                // evaluator directly to terminate.
                                return Ok(SmallVec::from_iter(returned));
                            } else if let Some(value) = returned {
                                // Make sure we bind the result values of the call op before
                                // resuming execution after the call
                                let call = call.unwrap();
                                let result = call.results()[0] as ValueRef;
                                self.set_value(result, value);
                            }

                            // Return to after the call
                            continue 'op;
                        }
                        ControlFlowEffect::Jump(successor) => {
                            let dest = successor.successor();

                            // Check that arguments match successor block signature
                            let arguments = successor.successor_operands();
                            let block = dest.borrow();
                            if block.num_arguments() != arguments.len() {
                                return Err(self
                                    .error("evaluation failed")
                                    .with_primary_label(
                                        op.span(),
                                        format!(
                                            "attempted to branch to {dest} with {} arguments, but \
                                             {} were expected",
                                            block.num_arguments(),
                                            arguments.len()
                                        ),
                                    )
                                    .into_report());
                            }
                            for (param, arg) in block.arguments().iter().zip(arguments.iter()) {
                                let expected = param.borrow();
                                let expected_ty = expected.ty();
                                let given = arg.borrow();
                                let given_ty = given.ty();
                                if expected_ty != given_ty {
                                    return Err(self
                                        .error("evaluation failed")
                                        .with_primary_label(
                                            op.span(),
                                            format!(
                                                "attempted to branch to {dest} with mismatched \
                                                 argument types"
                                            ),
                                        )
                                        .with_secondary_label(
                                            expected.span(),
                                            format!("expected {expected_ty}, got {given_ty}"),
                                        )
                                        .into_report());
                                }
                                let value = self.get_value(&arg)?;
                                self.set_value(*param as ValueRef, value);
                            }

                            // Jump
                            next_block = Some(dest);
                            continue 'block;
                        }
                        ControlFlowEffect::Yield {
                            successor: RegionBranchPoint::Parent,
                            arguments,
                        } => {
                            // We're returning to the parent operation from a child region
                            //
                            // Check that arguments match parent op results
                            let parent = region.parent().unwrap();
                            let parent_op = parent.borrow();
                            if parent_op.num_results() != arguments.len() {
                                return Err(self
                                    .error("evaluation failed")
                                    .with_primary_label(
                                        op.span(),
                                        format!(
                                            "attempted to yield to parent with {} arguments, but \
                                             {} were expected",
                                            parent_op.num_results(),
                                            arguments.len()
                                        ),
                                    )
                                    .with_secondary_label(
                                        parent_op.span(),
                                        "this is the parent operation",
                                    )
                                    .into_report());
                            }
                            log::debug!(target: "eval", "  <= {}",
                                DisplayValues::new(parent_op.results().iter().zip(arguments.iter()).map(|(result, arg)| {
                                MaterializedValue {
                                    id: *result as ValueRef,
                                    value: self.get_value(&arg).unwrap(),
                                }
                            })));
                            for (result, arg) in parent_op.results().iter().zip(arguments) {
                                let expected = result.borrow();
                                let expected_ty = expected.ty();
                                let given = arg.borrow();
                                let given_ty = given.ty();
                                if expected_ty != given_ty {
                                    return Err(self
                                        .error("evaluation failed")
                                        .with_primary_label(
                                            op.span(),
                                            "attempted to yield to parent with mismatched \
                                             argument types",
                                        )
                                        .with_secondary_label(
                                            expected.span(),
                                            format!("expected {expected_ty}, got {given_ty}"),
                                        )
                                        .into_report());
                                }

                                let value = self.get_value(&arg)?;
                                self.set_value(*result as ValueRef, value);
                            }

                            // Yield
                            next_op = parent.next();
                            next_block = parent.parent();
                            if let Some(parent_region) = next_block.and_then(|block| block.parent())
                            {
                                region = parent_region;
                                next_region = Some(parent_region);
                            } else {
                                next_region = None;
                            }

                            // If we're yielding from a standalone op, terminate evaluation
                            if next_op.is_none() {
                                break 'region;
                            }
                            continue 'op;
                        }
                        ControlFlowEffect::Yield {
                            successor: RegionBranchPoint::Child(successor),
                            arguments,
                        } => {
                            // Check that arguments match successor region entry block signature
                            let successor_region = successor.borrow();
                            let dest = successor_region.entry();
                            if dest.num_arguments() != arguments.len() {
                                return Err(self
                                    .error("evaluation failed")
                                    .with_primary_label(
                                        op.span(),
                                        format!(
                                            "attempted to yield to {dest} with {} arguments, but \
                                             {} were expected",
                                            dest.num_arguments(),
                                            arguments.len()
                                        ),
                                    )
                                    .into_report());
                            }
                            for (param, arg) in dest.arguments().iter().zip(arguments.iter()) {
                                let expected = param.borrow();
                                let expected_ty = expected.ty();
                                let given = arg.borrow();
                                let given_ty = given.ty();
                                if expected_ty != given_ty {
                                    return Err(self
                                        .error("evaluation failed")
                                        .with_primary_label(
                                            op.span(),
                                            format!(
                                                "attempted to yield to {dest} with mismatched \
                                                 argument types"
                                            ),
                                        )
                                        .with_secondary_label(
                                            expected.span(),
                                            format!("expected {expected_ty}, got {given_ty}"),
                                        )
                                        .into_report());
                                }

                                let value = self.get_value(&arg)?;
                                self.set_value(*param as ValueRef, value);
                            }

                            // Yield
                            next_region = Some(successor);
                            continue 'region;
                        }
                        ControlFlowEffect::Call { callee, arguments } => {
                            let callable_region = self.prepare_call(&op, callee, arguments)?;
                            // Yield control to the callee
                            next_region = Some(callable_region);
                            continue 'region;
                        }
                    }
                }

                return Err(self.report(
                    "evaluation failed",
                    block.grandparent().unwrap().span(),
                    format!(
                        "execution reached end of {block}, but no terminating control flow \
                         effects were emitted"
                    ),
                ));
            }
        }

        self.ip = Some(op.as_operation_ref());

        // Obtain any results this operation produced
        let mut results = SmallVec::with_capacity(op.num_results());
        let current_frame = self.current_frame();
        for result in ValueRange::<2>::from(op.results().all()) {
            if let Some(value) = current_frame.try_get_value(&result) {
                results.push(value);
                continue;
            }
            return Err(self
                .error("evaluation invariant violated")
                .with_primary_label(
                    self.current_span(),
                    format!("{result} was not properly set in the evaluator state"),
                )
                .with_help(format!(
                    "the implementation of Eval for '{}' is not updating the register state for \
                     its results",
                    op.name()
                ))
                .into_report());
        }

        Ok(results)
    }

    /// Evaluate `op` and verify it's results (if applicable).
    ///
    /// This is intended to be called from `eval_region`. See `eval_op_and_gather_results` if you
    /// want something to evaluate a single operation and handle its control flow effects at the
    /// same time.
    fn eval_op(&mut self, op: &Operation) -> Result<ControlFlowEffect, Report> {
        self.ip = Some(op.as_operation_ref());

        // Ensure the op is evaluatable
        let Some(evaluatable) = op.as_trait::<dyn Eval>() else {
            return Err(self.report(
                "evaluation failed",
                self.current_span(),
                format!("'{}' does not implement Eval", op.name()),
            ));
        };

        log::debug!(target: "eval", "evaluating: {} {}", op.name(), DisplayValues::new(ValueRange::<2>::from(op.operands().all()).into_iter().map(|v| MaterializedValue {
            id: v,
            value: self.get_value(&v).unwrap(),
        })));

        // Evaluate it
        let effect = evaluatable.eval(self)?;

        // Do not check results if control flow effect does not support results
        match effect {
            effect @ (ControlFlowEffect::Jump(_)
            | ControlFlowEffect::Trap { .. }
            | ControlFlowEffect::Call { .. }
            | ControlFlowEffect::Yield {
                successor: RegionBranchPoint::Parent,
                ..
            }) => return Ok(effect),
            ControlFlowEffect::Yield {
                successor,
                ref arguments,
                ..
            } => {
                log::debug!(target: "eval", "  => {successor} {}",
                    DisplayValues::new(arguments.iter().map(|v| {
                    MaterializedValue {
                        id: v,
                        value: self.get_value(&v).unwrap(),
                    }
                })));
                return Ok(effect);
            }
            ControlFlowEffect::Return(returning) => {
                match returning {
                    Some(value) => log::debug!(target: "eval", "  <= {value}"),
                    None => log::debug!(target: "eval", "  <= ()"),
                }
                return Ok(effect);
            }
            ControlFlowEffect::None => (),
        }

        // Obtain any results this operation produced
        let current_frame = self.current_frame();
        log::debug!(target: "eval", "  <= {}", DisplayValues::new(ValueRange::<2>::from(op.results().all()).into_iter().map(|v| {
            MaterializedValue {
                id: v,
                value: self.get_value(&v).unwrap(),
            }
        })));
        for result in ValueRange::<2>::from(op.results().all()) {
            if current_frame.is_defined(&result) {
                continue;
            }
            return Err(self
                .error("evaluation invariant violated")
                .with_primary_label(
                    self.current_span(),
                    format!("{result} was not properly set in the evaluator state"),
                )
                .with_help(format!(
                    "the implementation of Eval for '{}' is not updating the register state for \
                     its results",
                    op.name()
                ))
                .into_report());
        }

        Ok(effect)
    }

    /// This helper is designed to evaluate a single operation and return any results it produces.
    ///
    /// For control flow operations that enter another block or region, no results are gathered.
    /// For control flow operations that return from a region, the successor arguments are gathered
    /// as results.
    ///
    /// If the op being evaluated represents a call to a callable operation, the callable will be
    /// evaluated, and the results gathered for the call will be determined by the control flow
    /// effect produced by the call.
    fn eval_op_and_gather_results(
        &mut self,
        op: &Operation,
    ) -> Result<SmallVec<[Value; 1]>, Report> {
        match self.eval_op(op)? {
            ControlFlowEffect::None => {
                // Obtain any results this operation produced
                let current_frame = self.current_frame();
                Ok(ValueRange::<2>::from(op.results().all())
                    .into_iter()
                    .map(|result| current_frame.get_value_unchecked(&result))
                    .collect())
            }
            ControlFlowEffect::Jump(_) => Ok(smallvec![]),
            ControlFlowEffect::Trap { span, reason } => Err(self
                .error("evaluation failed")
                .with_primary_label(op.span(), "execution trapped due to this operation")
                .with_secondary_label(span, reason)
                .into_report()),
            ControlFlowEffect::Return(value) => Ok(SmallVec::from_iter(value)),
            ControlFlowEffect::Call { callee, arguments } => {
                let callable_region = self.prepare_call(op, callee, arguments)?;
                return self.eval_region(&callee.borrow(), callable_region);
            }
            ControlFlowEffect::Yield {
                successor,
                arguments,
            } => match successor {
                RegionBranchPoint::Parent => {
                    let current_frame = self.current_frame();
                    Ok(arguments
                        .into_iter()
                        .map(|result| current_frame.get_value_unchecked(&result))
                        .collect())
                }
                RegionBranchPoint::Child(region) => self.eval_region(op, region),
            },
        }
    }

    /// Validate a call to `callee` with `arguments`, and prepare the evaluator for execution of
    /// the callable region.
    ///
    /// If successful, returns the callable region to evaluate, otherwise returns `Err`.
    fn prepare_call(
        &mut self,
        caller: &Operation,
        callee: OperationRef,
        arguments: ValueRange<'static, 4>,
    ) -> Result<RegionRef, Report> {
        let callee_op = callee.borrow();
        let Some(callable) = callee_op.as_trait::<dyn CallableOpInterface>() else {
            return Err(self
                .error("evaluation failed")
                .with_primary_label(caller.span(), "invalid callee")
                .with_secondary_label(
                    callee_op.span(),
                    "this operation does not implement CallableOpInterface",
                )
                .into_report());
        };

        let Some(callable_region) = callable.get_callable_region() else {
            return Err(self
                .error("evaluation failed")
                .with_primary_label(caller.span(), "invalid callee")
                .with_secondary_label(
                    callee_op.span(),
                    "there is no definition for this callable, only this declaration",
                )
                .into_report());
        };

        let signature = callable.signature();
        if arguments.len() != signature.arity() {
            return Err(self
                .error("evaluation failed")
                .with_primary_label(caller.span(), "invalid number of arguments for callee")
                .with_secondary_label(
                    callee_op.span(),
                    format!(
                        "this callable expected {} arguments, got {}",
                        signature.arity(),
                        arguments.len()
                    ),
                )
                .into_report());
        }

        let mut frame = CallFrame::new(callee).with_caller(caller.as_operation_ref());

        for (index, (param, arg)) in signature.params().iter().zip(arguments).enumerate() {
            let argument = arg.borrow();
            let expected_ty = &param.ty;
            let given_ty = argument.ty();
            let callable_region = callable_region.borrow();
            let param_value = callable_region.entry().arguments()[index];
            if given_ty != expected_ty {
                return Err(self
                    .error("evaluation failed")
                    .with_primary_label(caller.span(), "invalid argument for callee")
                    .with_secondary_label(
                        callee_op.span(),
                        "argument types do not match this signature",
                    )
                    .with_secondary_label(
                        param_value.span(),
                        format!("expected value of type {expected_ty}, got {given_ty}"),
                    )
                    .into_report());
            } else {
                let value = self.get_value(&arg)?;
                frame.set_value(param_value as ValueRef, value);
            }
        }

        // Push new call frame
        self.call_stack.push(frame);

        Ok(callable_region)
    }
}
