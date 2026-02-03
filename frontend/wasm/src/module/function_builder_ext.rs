use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use std::path::Path;

use cranelift_entity::{EntityRef as _, SecondaryMap};
use log::warn;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_debuginfo::DebugInfoOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    BlockRef, Builder, Context, EntityRef, FxHashMap, FxHashSet, Ident, Listener, ListenerType, Op,
    OpBuilder, OperationRef, ProgramPoint, RegionRef, Signature, SmallVec, SourceSpan, Type,
    ValueRef,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, FunctionRef},
    interner::Symbol,
    traits::{BranchOpInterface, Terminator},
};

use crate::{
    module::debug_info::{FunctionDebugInfo, LocationScheduleEntry},
    ssa::{SSABuilder, SideEffects, Variable},
};

/// Tracking variables and blocks for SSA construction.
pub struct FunctionBuilderContext {
    ssa: SSABuilder,
    status: FxHashMap<BlockRef, BlockStatus>,
    types: SecondaryMap<Variable, Type>,
}

impl FunctionBuilderContext {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            ssa: SSABuilder::new(context),
            status: Default::default(),
            types: SecondaryMap::with_default(Type::Unknown),
        }
    }

    fn is_empty(&self) -> bool {
        self.ssa.is_empty() && self.status.is_empty() && self.types.is_empty()
    }

    fn clear(&mut self) {
        self.ssa.clear();
        self.status.clear();
        self.types.clear();
    }

    /// Returns `true` if and only if no instructions have been added and the block is empty.
    fn is_pristine(&mut self, block: &BlockRef) -> bool {
        self.status.entry(*block).or_default() == &BlockStatus::Empty
    }

    /// Returns `true` if and the block has been filled.
    fn is_filled(&mut self, block: &BlockRef) -> bool {
        self.status.entry(*block).or_default() == &BlockStatus::Filled
    }
}

#[derive(Clone, Default, Eq, PartialEq, Debug)]
enum BlockStatus {
    /// No instructions have been added.
    #[default]
    Empty,
    /// Some instructions have been added, but no terminator.
    Partial,
    /// A terminator has been added; no further instructions may be added.
    Filled,
}

pub struct SSABuilderListener {
    builder: Rc<RefCell<FunctionBuilderContext>>,
}

impl SSABuilderListener {
    pub const fn new(builder: Rc<RefCell<FunctionBuilderContext>>) -> Self {
        Self { builder }
    }
}

impl Listener for SSABuilderListener {
    fn kind(&self) -> ListenerType {
        ListenerType::Builder
    }

    fn notify_operation_inserted(&self, op: OperationRef, prev: ProgramPoint) {
        let borrow = op.borrow();
        let op = borrow.as_ref().as_operation();
        let mut builder = self.builder.borrow_mut();

        let block = prev.block().expect("invalid program point");
        if builder.is_pristine(&block) {
            builder.status.insert(block, BlockStatus::Partial);
        } else {
            let is_filled = builder.is_filled(&block);
            debug_assert!(!is_filled, "you cannot add an instruction to a block already filled");
        }

        if op.implements::<dyn BranchOpInterface>() {
            let mut unique: FxHashSet<BlockRef> = FxHashSet::default();
            for succ in op.successors().iter() {
                let successor = succ.block.borrow().successor();
                if !unique.insert(successor) {
                    continue;
                }
                builder.ssa.declare_block_predecessor(successor, op.as_operation_ref());
            }
        }

        if op.implements::<dyn Terminator>() {
            builder.status.insert(block, BlockStatus::Filled);
        }
    }

    fn notify_block_inserted(
        &self,
        _block: BlockRef,
        _prev: Option<RegionRef>,
        _ip: Option<BlockRef>,
    ) {
    }
}

/// A wrapper around Miden's `FunctionBuilder` and `SSABuilder` which provides
/// additional API for dealing with variables and SSA construction.
pub struct FunctionBuilderExt<'c, B: ?Sized + Builder> {
    inner: FunctionBuilder<'c, B>,
    func_ctx: Rc<RefCell<FunctionBuilderContext>>,
    debug_info: Option<Rc<RefCell<FunctionDebugInfo>>>,
    param_values: Vec<(Variable, ValueRef)>,
    param_dbg_emitted: bool,
}

impl<'c> FunctionBuilderExt<'c, OpBuilder<SSABuilderListener>> {
    pub fn new(func: FunctionRef, builder: &'c mut OpBuilder<SSABuilderListener>) -> Self {
        let func_ctx = builder.listener().map(|l| l.builder.clone()).unwrap();
        debug_assert!(func_ctx.borrow().is_empty());

        let inner = FunctionBuilder::new(func, builder);

        Self {
            inner,
            func_ctx,
            debug_info: None,
            param_values: Vec::new(),
            param_dbg_emitted: false,
        }
    }
}

impl<B: ?Sized + Builder> FunctionBuilderExt<'_, B> {
    const DI_COMPILE_UNIT_ATTR: &'static str = "di.compile_unit";
    const DI_SUBPROGRAM_ATTR: &'static str = "di.subprogram";

    pub fn set_debug_metadata(&mut self, info: Rc<RefCell<FunctionDebugInfo>>) {
        self.debug_info = Some(info);
        self.param_dbg_emitted = false;
        self.refresh_function_debug_attrs();
    }

    fn emit_dbg_value_for_var(&mut self, var: Variable, value: ValueRef, span: SourceSpan) {
        let Some(info) = self.debug_info.as_ref() else {
            return;
        };
        let idx = var.index();
        let (attr_opt, expr_opt) = {
            let info = info.borrow();
            let local_info = info.locals.get(idx).and_then(|l| l.as_ref());
            match local_info {
                Some(l) => (Some(l.attr.clone()), l.expression.clone()),
                None => (None, None),
            }
        };
        let Some(mut attr) = attr_opt else {
            return;
        };

        if let Some((file_symbol, _directory, line, column)) = self.span_to_location(span) {
            attr.file = file_symbol;
            if line != 0 {
                attr.line = line;
            }
            attr.column = column;
        }

        if let Err(err) =
            DebugInfoOpBuilder::builder_mut(self).debug_value_with_expr(value, attr, expr_opt, span)
        {
            warn!("failed to emit dbg.value for local {idx}: {err:?}");
        }
    }

    pub fn def_var_with_dbg(&mut self, var: Variable, val: ValueRef, span: SourceSpan) {
        self.def_var(var, val);
        self.emit_dbg_value_for_var(var, val, span);
    }

    pub fn register_parameter(&mut self, var: Variable, value: ValueRef) {
        self.param_values.push((var, value));
    }

    pub fn record_debug_span(&mut self, span: SourceSpan) {
        if span == SourceSpan::UNKNOWN {
            return;
        }
        let Some(info_rc) = self.debug_info.as_ref() else {
            return;
        };

        if let Some((file_symbol, directory_symbol, line, column)) = self.span_to_location(span) {
            {
                let mut info = info_rc.borrow_mut();
                info.compile_unit.file = file_symbol;
                info.compile_unit.directory = directory_symbol;
                info.subprogram.file = file_symbol;
                info.subprogram.line = line;
                info.subprogram.column = column;
                info.function_span.get_or_insert(span);
            }
            self.refresh_function_debug_attrs();
            self.emit_parameter_dbg_if_needed(span);
        }
    }

    pub fn apply_location_schedule(&mut self, offset: u64, span: SourceSpan) {
        let Some(info_rc) = self.debug_info.as_ref() else {
            return;
        };

        let updates = {
            let mut info = info_rc.borrow_mut();
            let mut pending = Vec::new();
            while info.next_location_event < info.location_schedule.len() {
                let entry = &info.location_schedule[info.next_location_event];
                if entry.offset > offset {
                    break;
                }
                pending.push(entry.clone());
                info.next_location_event += 1;
            }
            pending
        };

        for entry in updates {
            self.emit_scheduled_dbg_value(entry, span);
        }
    }

    fn emit_scheduled_dbg_value(&mut self, entry: LocationScheduleEntry, span: SourceSpan) {
        let var = Variable::new(entry.var_index);
        let Ok(value) = self.try_use_var(var) else {
            return;
        };

        // Create expression from the scheduled location
        let expression = {
            let ops = vec![entry.storage.to_expression_op()];
            Some(midenc_hir::DIExpressionAttr::with_ops(ops))
        };

        let Some(info) = self.debug_info.as_ref() else {
            return;
        };
        let idx = entry.var_index;
        let attr_opt = {
            let info = info.borrow();
            info.local_attr(idx).cloned()
        };
        let Some(mut attr) = attr_opt else {
            return;
        };

        if let Some((file_symbol, _directory, line, column)) = self.span_to_location(span) {
            attr.file = file_symbol;
            if line != 0 {
                attr.line = line;
            }
            attr.column = column;
        }

        if let Err(err) =
            DebugInfoOpBuilder::builder_mut(self).debug_value_with_expr(value, attr, expression, span)
        {
            warn!("failed to emit scheduled dbg.value for local {idx}: {err:?}");
        }
    }

    pub fn name(&self) -> Ident {
        *self.inner.func.borrow().name()
    }

    pub fn signature(&self) -> EntityRef<'_, Signature> {
        EntityRef::map(self.inner.func.borrow(), |f| f.signature())
    }

    #[inline]
    pub fn current_block(&self) -> BlockRef {
        self.inner.current_block()
    }

    /// Create a new `Block` in the function preserving the current insertion point and declare it
    /// in the SSA context.
    pub fn create_block(&mut self) -> BlockRef {
        // save the current insertion point
        let old_ip = *self.inner.builder().insertion_point();
        let region = self.inner.body_region();
        let block = self.inner.builder_mut().create_block(region, None, &[]);
        // restore the insertion point to the previous block
        self.inner.builder_mut().set_insertion_point(old_ip);
        self.func_ctx.borrow_mut().ssa.declare_block(block);
        block
    }

    /// Create a `Block` with the given parameters.
    pub fn create_block_with_params(
        &mut self,
        params: impl IntoIterator<Item = Type>,
        span: SourceSpan,
    ) -> BlockRef {
        let block = self.create_block();
        for ty in params {
            self.inner.append_block_param(block, ty, span);
        }
        block
    }

    pub fn create_detached_block(&mut self) -> BlockRef {
        self.inner.builder().context().create_block()
    }

    /// Append parameters to the given `Block` corresponding to the function
    /// return values. This can be used to set up the block parameters for a
    /// function exit block.
    pub fn append_block_params_for_function_returns(&mut self, block: BlockRef) {
        // These parameters count as "user" parameters here because they aren't
        // inserted by the SSABuilder.
        debug_assert!(
            self.is_pristine(&block),
            "You can't add block parameters after adding any instruction"
        );

        let results = SmallVec::<[_; 2]>::from_iter(self.signature().results().iter().cloned());
        for argtyp in results {
            self.inner.append_block_param(block, argtyp.ty.clone(), SourceSpan::default());
        }
    }

    /// After the call to this function, new instructions will be inserted into the designated
    /// block, in the order they are declared. You must declare the types of the Block arguments
    /// you will use here.
    ///
    /// When inserting the terminator instruction (which doesn't have a fallthrough to its immediate
    /// successor), the block will be declared filled and it will not be possible to append
    /// instructions to it.
    pub fn switch_to_block(&mut self, block: BlockRef) {
        // First we check that the previous block has been filled.
        let is_unreachable = self.is_unreachable();
        debug_assert!(
            is_unreachable
                || self.is_pristine(&self.inner.current_block())
                || self.is_filled(&self.inner.current_block()),
            "you have to fill your block before switching"
        );
        // We cannot switch to a filled block
        debug_assert!(
            !self.is_filled(&block),
            "you cannot switch to a block which is already filled"
        );
        // Then we change the cursor position.
        self.inner.switch_to_block(block);
    }

    /// Declares that all the predecessors of this block are known.
    ///
    /// Function to call with `block` as soon as the last branch instruction to `block` has been
    /// created. Forgetting to call this method on every block will cause inconsistencies in the
    /// produced functions.
    pub fn seal_block(&mut self, block: BlockRef) {
        let side_effects = self.func_ctx.borrow_mut().ssa.seal_block(block);
        self.handle_ssa_side_effects(side_effects);
    }

    fn handle_ssa_side_effects(&mut self, side_effects: SideEffects) {
        for modified_block in side_effects.instructions_added_to_blocks {
            if self.is_pristine(&modified_block) {
                self.func_ctx.borrow_mut().status.insert(modified_block, BlockStatus::Partial);
            }
        }
    }

    /// Make sure that the current block is inserted in the layout.
    pub fn ensure_inserted_block(&mut self) {
        let block = self.inner.current_block();
        if self.is_pristine(&block) {
            self.func_ctx.borrow_mut().status.insert(block, BlockStatus::Partial);
        } else {
            debug_assert!(
                !self.is_filled(&block),
                "you cannot add an instruction to a block already filled"
            );
        }
    }

    /// Declare that translation of the current function is complete.
    ///
    /// This resets the state of the `FunctionBuilderContext` in preparation to
    /// be used for another function.
    pub fn finalize(self) {
        // Check that all the `Block`s are filled and sealed.
        #[cfg(debug_assertions)]
        {
            let keys: Vec<BlockRef> = self.func_ctx.borrow().status.keys().cloned().collect();
            for block in keys {
                if !self.is_pristine(&block) {
                    assert!(
                        self.func_ctx.borrow().ssa.is_sealed(block),
                        "FunctionBuilderExt finalized, but block {block} is not sealed",
                    );
                    assert!(
                        self.is_filled(&block),
                        "FunctionBuilderExt finalized, but block {block} is not filled",
                    );
                }
            }
        }

        // Clear the state (but preserve the allocated buffers) in preparation
        // for translation another function.
        self.func_ctx.borrow_mut().clear();
    }

    #[inline]
    pub fn variable_type(&self, var: Variable) -> Type {
        self.func_ctx.borrow().types[var].clone()
    }

    /// Declares the type of a variable, so that it can be used later (by calling
    /// [`FunctionBuilderExt::use_var`]). This function will return an error if the variable
    /// has been previously declared.
    pub fn try_declare_var(&mut self, var: Variable, ty: Type) -> Result<(), DeclareVariableError> {
        if self.func_ctx.borrow().types[var] != Type::Unknown {
            return Err(DeclareVariableError::DeclaredMultipleTimes(var));
        }
        self.func_ctx.borrow_mut().types[var] = ty;
        Ok(())
    }

    /// In order to use a variable (by calling [`FunctionBuilderExt::use_var`]), you need
    /// to first declare its type with this method.
    pub fn declare_var(&mut self, var: Variable, ty: Type) {
        self.try_declare_var(var, ty)
            .unwrap_or_else(|_| panic!("the variable {var:?} has been declared multiple times"))
    }

    /// Returns the Miden IR necessary to use a previously defined user
    /// variable, returning an error if this is not possible.
    pub fn try_use_var(&mut self, var: Variable) -> Result<ValueRef, UseVariableError> {
        // Assert that we're about to add instructions to this block using the definition of the
        // given variable. ssa.use_var is the only part of this crate which can add block parameters
        // behind the caller's back. If we disallow calling append_block_param as soon as use_var is
        // called, then we enforce a strict separation between user parameters and SSA parameters.
        self.ensure_inserted_block();

        let (val, side_effects) = {
            let ty = self
                .func_ctx
                .borrow()
                .types
                .get(var)
                .cloned()
                .ok_or(UseVariableError::UsedBeforeDeclared(var))?;
            debug_assert_ne!(
                ty,
                Type::Unknown,
                "variable {var:?} is used but its type has not been declared"
            );
            let current_block = self.inner.current_block();
            self.func_ctx.borrow_mut().ssa.use_var(var, ty, current_block)
        };
        self.handle_ssa_side_effects(side_effects);
        Ok(val)
    }

    /// Returns the Miden IR value corresponding to the utilization at the current program
    /// position of a previously defined user variable.
    pub fn use_var(&mut self, var: Variable) -> ValueRef {
        self.try_use_var(var).unwrap_or_else(|_| {
            panic!("variable {var:?} is used but its type has not been declared")
        })
    }

    /// Registers a new definition of a user variable. This function will return
    /// an error if the value supplied does not match the type the variable was
    /// declared to have.
    pub fn try_def_var(&mut self, var: Variable, val: ValueRef) -> Result<(), DefVariableError> {
        let mut func_ctx = self.func_ctx.borrow_mut();
        let var_ty = func_ctx.types.get(var).ok_or(DefVariableError::DefinedBeforeDeclared(var))?;
        if var_ty != val.borrow().ty() {
            return Err(DefVariableError::TypeMismatch(var, val));
        }
        func_ctx.ssa.def_var(var, val, self.inner.current_block());
        Ok(())
    }

    /// Register a new definition of a user variable. The type of the value must be
    /// the same as the type registered for the variable.
    pub fn def_var(&mut self, var: Variable, val: ValueRef) {
        self.try_def_var(var, val).unwrap_or_else(|error| match error {
            DefVariableError::TypeMismatch(var, val) => {
                assert_eq!(
                    &self.func_ctx.borrow().types[var],
                    val.borrow().ty(),
                    "declared type of variable {var:?} doesn't match type of value {val}"
                );
            }
            DefVariableError::DefinedBeforeDeclared(var) => {
                panic!("variable {var:?} is used but its type has not been declared");
            }
        })
    }

    /// Returns `true` if and only if no instructions have been added since the last call to
    /// `switch_to_block`.
    fn is_pristine(&self, block: &BlockRef) -> bool {
        self.func_ctx.borrow_mut().is_pristine(block)
    }

    /// Returns `true` if and only if a terminator instruction has been inserted since the
    /// last call to `switch_to_block`.
    fn is_filled(&self, block: &BlockRef) -> bool {
        self.func_ctx.borrow_mut().is_filled(block)
    }

    /// Returns `true` if and only if the current `Block` is sealed and has no predecessors
    /// declared.
    ///
    /// The entry block of a function is never unreachable.
    pub fn is_unreachable(&self) -> bool {
        let is_entry = self.inner.current_block() == self.inner.entry_block();
        let func_ctx = self.func_ctx.borrow();
        let is_sealed = func_ctx.ssa.is_sealed(self.inner.current_block());
        let has_no_predecessors = !func_ctx.ssa.has_any_predecessors(self.inner.current_block());
        !is_entry && is_sealed && has_no_predecessors
    }

    /// Changes the destination of a jump instruction after creation.
    ///
    /// **Note:** You are responsible for maintaining the coherence with the arguments of
    /// other jump instructions.
    ///
    /// NOTE: Panics if `branch_inst` is not a branch instruction.
    pub fn change_jump_destination(
        &mut self,
        mut branch_inst: OperationRef,
        old_block: BlockRef,
        new_block: BlockRef,
    ) {
        self.func_ctx.borrow_mut().ssa.remove_block_predecessor(old_block, branch_inst);
        let mut borrow_mut = branch_inst.borrow_mut();
        let Some(inst_branch) = borrow_mut.as_trait_mut::<dyn BranchOpInterface>() else {
            panic!("expected branch instruction, got {branch_inst:?}");
        };
        inst_branch.change_branch_destination(old_block, new_block);
        self.func_ctx.borrow_mut().ssa.declare_block_predecessor(new_block, branch_inst);
    }

    fn refresh_function_debug_attrs(&mut self) {
        let Some(info) = self.debug_info.as_ref() else {
            return;
        };
        let info = info.borrow();
        let mut func = self.inner.func.borrow_mut();
        let op = func.as_operation_mut();
        op.set_intrinsic_attribute(Self::DI_COMPILE_UNIT_ATTR, Some(info.compile_unit.clone()));
        op.set_intrinsic_attribute(Self::DI_SUBPROGRAM_ATTR, Some(info.subprogram.clone()));
    }

    fn emit_parameter_dbg_if_needed(&mut self, span: SourceSpan) {
        if self.param_dbg_emitted {
            return;
        }
        self.param_dbg_emitted = true;
        let params: Vec<_> = self.param_values.to_vec();
        for (var, value) in params {
            let skip_due_to_schedule = if let Some(info_rc) = self.debug_info.as_ref() {
                let info = info_rc.borrow();
                info.locals
                    .get(var.index())
                    .and_then(|entry| entry.as_ref())
                    .is_some_and(|entry| !entry.locations.is_empty())
            } else {
                false
            };
            if skip_due_to_schedule {
                continue;
            }
            self.emit_dbg_value_for_var(var, value, span);
        }
    }

    fn span_to_location(
        &self,
        span: SourceSpan,
    ) -> Option<(Symbol, Option<Symbol>, u32, Option<u32>)> {
        if span == SourceSpan::UNKNOWN {
            return None;
        }

        let context = self.inner.builder().context();
        let session = context.session();
        let source_file = session.source_manager.get(span.source_id()).ok()?;
        let uri = source_file.uri().as_str();
        let path = Path::new(uri);
        let file_symbol = Symbol::intern(uri);
        let directory_symbol = path.parent().and_then(|parent| parent.to_str()).map(Symbol::intern);
        let location = source_file.location(span);
        let line = location.line.to_u32();
        let column = location.column.to_u32();
        Some((file_symbol, directory_symbol, line, Some(column)))
    }
}

impl<'f, B: ?Sized + Builder> ArithOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

impl<'f, B: ?Sized + Builder> ControlFlowOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

impl<'f, B: ?Sized + Builder> UndefinedBehaviorOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

impl<'f, B: ?Sized + Builder> BuiltinOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

impl<'f, B: ?Sized + Builder> DebugInfoOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

impl<'f, B: ?Sized + Builder> HirOpBuilder<'f, B> for FunctionBuilderExt<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        self.inner.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self.inner.builder_mut()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
/// An error encountered when calling [`FunctionBuilderExt::try_use_var`].
pub enum UseVariableError {
    #[error("variable {0} is used before the declaration")]
    UsedBeforeDeclared(Variable),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, thiserror::Error)]
/// An error encountered when calling [`FunctionBuilderExt::try_declare_var`].
pub enum DeclareVariableError {
    #[error("variable {0} is already declared")]
    DeclaredMultipleTimes(Variable),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, thiserror::Error)]
/// An error encountered when defining the initial value of a variable.
pub enum DefVariableError {
    #[error(
        "the types of variable {0} and value {1} are not the same. The `Value` supplied to \
         `def_var` must be of the same type as the variable was declared to be of in \
         `declare_var`."
    )]
    TypeMismatch(Variable, ValueRef),
    #[error(
        "the value of variable {0} was defined (in call `def_val`) before it was declared (in \
         call `declare_var`)"
    )]
    DefinedBeforeDeclared(Variable),
}
