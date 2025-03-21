use std::{cell::RefCell, rc::Rc};

use cranelift_entity::SecondaryMap;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
    traits::{BranchOpInterface, Terminator},
    BlockRef, Builder, Context, FxHashMap, FxHashSet, Ident, Listener, ListenerType, OpBuilder,
    OperationRef, ProgramPoint, RegionRef, Signature, ValueRef,
};
use midenc_hir_type::Type;
use midenc_session::diagnostics::SourceSpan;

use crate::ssa::{SSABuilder, SideEffects, Variable};

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
}

impl<'c> FunctionBuilderExt<'c, OpBuilder<SSABuilderListener>> {
    pub fn new(func: &'c mut Function, builder: &'c mut OpBuilder<SSABuilderListener>) -> Self {
        let func_ctx = builder.listener().map(|l| l.builder.clone()).unwrap();
        debug_assert!(func_ctx.borrow().is_empty());

        let inner = FunctionBuilder::new(func, builder);

        Self { inner, func_ctx }
    }
}

impl<B: ?Sized + Builder> FunctionBuilderExt<'_, B> {
    pub fn name(&self) -> Ident {
        *self.inner.func.name()
    }

    pub fn signature(&self) -> &Signature {
        self.inner.func.signature()
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

        #[allow(clippy::unnecessary_to_owned)]
        for argtyp in self.signature().results().to_vec() {
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
                        "FunctionBuilderExt finalized, but block {} is not sealed",
                        block,
                    );
                    assert!(
                        self.is_filled(&block),
                        "FunctionBuilderExt finalized, but block {} is not filled",
                        block,
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
            .unwrap_or_else(|_| panic!("the variable {:?} has been declared multiple times", var))
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
                "variable {:?} is used but its type has not been declared",
                var
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
            panic!("variable {:?} is used but its type has not been declared", var)
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
                    "declared type of variable {:?} doesn't match type of value {}",
                    var,
                    val
                );
            }
            DefVariableError::DefinedBeforeDeclared(var) => {
                panic!("variable {:?} is used but its type has not been declared", var);
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
    pub fn change_jump_destination(
        &mut self,
        inst: OperationRef,
        old_block: BlockRef,
        new_block: BlockRef,
    ) {
        self.func_ctx.borrow_mut().ssa.remove_block_predecessor(old_block, inst);
        self.inner
            .builder()
            .context()
            .change_branch_destination(inst, old_block, new_block);
        self.func_ctx.borrow_mut().ssa.declare_block_predecessor(new_block, inst);
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
