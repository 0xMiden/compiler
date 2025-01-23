use std::{cell::RefCell, rc::Rc};

use midenc_dialect_hir::{FunctionBuilder, InstBuilderBase};
use midenc_hir::{
    cranelift_entity::{EntitySet, SecondaryMap},
    diagnostics::SourceSpan,
    DefaultInstBuilder,
};
use midenc_hir2::{
    dialects::builtin::Function, BlockArgumentRef, BlockRef, FxHashMap, Ident, Listener, Op,
    OpBuilder, OperationRef, ProgramPoint, RegionRef, Signature, ValueRef,
};
use midenc_hir_type::Type;

use crate::ssa::{SSABuilder, SideEffects, Variable};

/// Tracking variables and blocks for SSA construction.
pub struct FunctionBuilderContext {
    ssa: SSABuilder,
    status: FxHashMap<BlockRef, BlockStatus>,
    types: SecondaryMap<Variable, Type>,
}

impl FunctionBuilderContext {
    pub fn new() -> Self {
        Self {
            ssa: SSABuilder::default(),
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
}

#[derive(Clone, Default, Eq, PartialEq)]
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

impl Listener for SSABuilderListener {
    fn kind(&self) -> midenc_hir2::ListenerType {
        todo!()
    }

    fn notify_operation_inserted(&self, op: OperationRef, prev: ProgramPoint) {
        // TODO: implement

        // let builder = self.builder.borrow_mut();
        // // We only insert the Block in the layout when an instruction is added to it
        // builder.ensure_inserted_block();
        // let opcode = data.opcode();
        // // let inst = self.builder.data_flow_graph_mut().insert_inst(self.ip, data, ty, span);
        //
        // match self.builder.inner.data_flow_graph().insts[inst].data.inner() {
        //     Instruction::Br(Br { successor, .. }) => {
        //         // If the user has supplied jump arguments we must adapt the arguments of
        //         // the destination block
        //         builder.func_ctx.ssa.declare_block_predecessor(successor.destination, inst);
        //     }
        //
        //     Instruction::CondBr(CondBr {
        //         then_dest,
        //         else_dest,
        //         ..
        //     }) => {
        //         builder.func_ctx.ssa.declare_block_predecessor(then_dest.destination, inst);
        //         if then_dest.destination != else_dest.destination {
        //             builder.func_ctx.ssa.declare_block_predecessor(else_dest.destination, inst);
        //         }
        //     }
        //     Instruction::Switch(Switch {
        //         op: _,
        //         arg: _,
        //         ref arms,
        //         default: default_successor,
        //     }) => {
        //         // Unlike all other jumps/branches, arms are
        //         // capable of having the same successor appear
        //         // multiple times, so we must deduplicate.
        //         let mut unique = EntitySet::<Block>::new();
        //         let blocks = arms
        //             .iter()
        //             .map(|arm| arm.successor.destination)
        //             .chain([default_successor.destination]);
        //         for block in blocks {
        //             if !unique.insert(block) {
        //                 continue;
        //             }
        //             builder.func_ctx.ssa.declare_block_predecessor(block, inst);
        //         }
        //     }
        //     inst => debug_assert!(!inst.opcode().is_branch()),
        // }
        //
        // if opcode.is_terminator() {
        //     builder.fill_current_block()
        // }
    }

    fn notify_block_inserted(
        &self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<BlockRef>,
    ) {
    }
}

// TODO: implement in SSABuilderListener

// This implementation is richer than `InsertBuilder` because we use the data of the
// instruction being inserted to add related info to the DFG and the SSA building system,
// and perform debug sanity checks.
// fn build(self, data: Instruction, ty: Type, span: SourceSpan) -> (Inst, &'a mut DataFlowGraph) {
//     // We only insert the Block in the layout when an instruction is added to it
//     self.builder.ensure_inserted_block();
//     let opcode = data.opcode();
//     let inst = self.builder.data_flow_graph_mut().insert_inst(self.ip, data, ty, span);
//
//     match self.builder.inner.data_flow_graph().insts[inst].data.inner() {
//         Instruction::Br(Br { successor, .. }) => {
//             // If the user has supplied jump arguments we must adapt the arguments of
//             // the destination block
//             self.builder.func_ctx.ssa.declare_block_predecessor(successor.destination, inst);
//         }
//
//         Instruction::CondBr(CondBr {
//             then_dest,
//             else_dest,
//             ..
//         }) => {
//             self.builder.func_ctx.ssa.declare_block_predecessor(then_dest.destination, inst);
//             if then_dest.destination != else_dest.destination {
//                 self.builder
//                     .func_ctx
//                     .ssa
//                     .declare_block_predecessor(else_dest.destination, inst);
//             }
//         }
//         Instruction::Switch(Switch {
//             op: _,
//             arg: _,
//             ref arms,
//             default: default_successor,
//         }) => {
//             // Unlike all other jumps/branches, arms are
//             // capable of having the same successor appear
//             // multiple times, so we must deduplicate.
//             let mut unique = EntitySet::<Block>::new();
//             let blocks = arms
//                 .iter()
//                 .map(|arm| arm.successor.destination)
//                 .chain([default_successor.destination]);
//             for block in blocks {
//                 if !unique.insert(block) {
//                     continue;
//                 }
//                 self.builder.func_ctx.ssa.declare_block_predecessor(block, inst);
//             }
//         }
//         inst => debug_assert!(!inst.opcode().is_branch()),
//     }
//
//     if opcode.is_terminator() {
//         self.builder.fill_current_block()
//     }
//     (inst, self.builder.data_flow_graph_mut())
// }

/// A wrapper around Miden's `FunctionBuilder` and `SSABuilder` which provides
/// additional API for dealing with variables and SSA construction.
pub struct FunctionBuilderExt<'c, L: Listener = SSABuilderListener> {
    inner: FunctionBuilder<'c, L>,
    func_ctx: Rc<RefCell<FunctionBuilderContext>>,
}

impl<'c> FunctionBuilderExt<'c> {
    pub fn new(func: &'c mut Function, func_ctx: Rc<RefCell<FunctionBuilderContext>>) -> Self {
        debug_assert!(func_ctx.borrow().is_empty());

        let context = func.as_operation().context_rc();
        let ssa_builder_listener = SSABuilderListener {
            builder: func_ctx.clone(),
        };
        let op_builder = OpBuilder::new(context).with_listener(ssa_builder_listener);
        let inner = FunctionBuilder::new(func, op_builder);
        Self { inner, func_ctx }
    }

    // pub fn data_flow_graph(&self) -> &DataFlowGraph {
    //     todo!()
    //     // self.inner.data_flow_graph()
    // }

    // pub fn data_flow_graph_mut(&mut self) -> &mut DataFlowGraph {
    //     // self.inner.data_flow_graph_mut()
    //     todo!()
    // }

    pub fn name(&self) -> &Ident {
        self.inner.func.name()
    }

    pub fn signature(&self) -> &Signature {
        self.inner.func.signature()
    }

    pub fn ins<'short>(&'short mut self) -> FuncInstBuilderExt<'short, 'c> {
        // pub fn ins<'short>(&'short mut self) -> DefaultInstBuilder<'short, L> {
        // let block = self.inner.current_block();
        // self.inner.ins()
        FuncInstBuilderExt::new(self)
    }

    // TODO: remove
    #[inline]
    pub fn current_block(&self) -> BlockRef {
        self.inner.current_block()
    }

    // pub fn inst_results(&self, inst: OperationRef) -> &[ValueRef] {
    //     inst.borrow().results()
    // }

    pub fn create_block(&mut self) -> BlockRef {
        let block = self.inner.create_block();
        todo!("declare block");
        // self.func_ctx.borrow_mut().ssa.declare_block(block);
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

    /// Append parameters to the given `Block` corresponding to the function
    /// return values. This can be used to set up the block parameters for a
    /// function exit block.
    pub fn append_block_params_for_function_returns(&mut self, block: BlockRef) {
        todo!()
        // // These parameters count as "user" parameters here because they aren't
        // // inserted by the SSABuilder.
        // debug_assert!(
        //     self.is_pristine(block),
        //     "You can't add block parameters after adding any instruction"
        // );
        //
        // #[allow(clippy::unnecessary_to_owned)]
        // for argtyp in self.signature().results().to_vec() {
        //     self.inner.append_block_param(block, argtyp.ty.clone(), SourceSpan::default());
        // }
    }

    /// After the call to this function, new instructions will be inserted into the designated
    /// block, in the order they are declared. You must declare the types of the Block arguments
    /// you will use here.
    ///
    /// When inserting the terminator instruction (which doesn't have a fallthrough to its immediate
    /// successor), the block will be declared filled and it will not be possible to append
    /// instructions to it.
    pub fn switch_to_block(&mut self, block: BlockRef) {
        todo!()
        // // First we check that the previous block has been filled.
        // debug_assert!(
        //     self.is_unreachable()
        //         || self.is_pristine(self.inner.current_block())
        //         || self.is_filled(self.inner.current_block()),
        //     "you have to fill your block before switching"
        // );
        // // We cannot switch to a filled block
        // debug_assert!(
        //     !self.is_filled(block),
        //     "you cannot switch to a block which is already filled"
        // );
        // // Then we change the cursor position.
        // self.inner.switch_to_block(block);
    }

    /// Retrieves all the parameters for a `Block` currently inferred from the jump instructions
    /// inserted that target it and the SSA construction.
    pub fn block_params(&self, block: BlockRef) -> &[BlockArgumentRef] {
        // block.borrow().arguments()
        todo!()
    }

    /// Declares that all the predecessors of this block are known.
    ///
    /// Function to call with `block` as soon as the last branch instruction to `block` has been
    /// created. Forgetting to call this method on every block will cause inconsistencies in the
    /// produced functions.
    pub fn seal_block(&mut self, block: BlockRef) {
        todo!();
        // let side_effects = self
        //     .func_ctx
        //     .borrow_mut()
        //     .ssa
        //     .seal_block(block, self.inner.data_flow_graph_mut());
        // self.handle_ssa_side_effects(side_effects);
    }

    /// A Block is 'filled' when a terminator instruction is present.
    fn fill_current_block(&mut self) {
        todo!()
        // self.func_ctx.borrow_mut().status[&self.inner.current_block()] = BlockStatus::Filled;
    }

    fn handle_ssa_side_effects(&mut self, side_effects: SideEffects) {
        todo!()
        // for modified_block in side_effects.instructions_added_to_blocks {
        //     if self.is_pristine(modified_block) {
        //         self.func_ctx.status[modified_block] = BlockStatus::Partial;
        //     }
        // }
    }

    /// Make sure that the current block is inserted in the layout.
    pub fn ensure_inserted_block(&mut self) {
        todo!()
        // let block = self.inner.current_block();
        // if self.is_pristine(block) {
        //     self.func_ctx.status[block] = BlockStatus::Partial;
        // } else {
        //     debug_assert!(
        //         !self.is_filled(block),
        //         "you cannot add an instruction to a block already filled"
        //     );
        // }
    }

    /// Declare that translation of the current function is complete.
    ///
    /// This resets the state of the `FunctionBuilderContext` in preparation to
    /// be used for another function.
    pub fn finalize(self) {
        todo!()
        // // Check that all the `Block`s are filled and sealed.
        // #[cfg(debug_assertions)]
        // {
        //     for block in self.func_ctx.status.keys() {
        //         if !self.is_pristine(block) {
        //             assert!(
        //                 self.func_ctx.ssa.is_sealed(block),
        //                 "FunctionBuilderExt finalized, but block {} is not sealed",
        //                 block,
        //             );
        //             assert!(
        //                 self.is_filled(block),
        //                 "FunctionBuilderExt finalized, but block {} is not filled",
        //                 block,
        //             );
        //         }
        //     }
        // }
        //
        // // Clear the state (but preserve the allocated buffers) in preparation
        // // for translation another function.
        // self.func_ctx.clear();
    }

    #[inline]
    pub fn variable_type(&self, var: Variable) -> &Type {
        todo!()
        // &self.func_ctx.types[var]
    }

    /// Declares the type of a variable, so that it can be used later (by calling
    /// [`FunctionBuilderExt::use_var`]). This function will return an error if the variable
    /// has been previously declared.
    pub fn try_declare_var(&mut self, var: Variable, ty: Type) -> Result<(), DeclareVariableError> {
        todo!()
        // if self.func_ctx.types[var] != Type::Unknown {
        //     return Err(DeclareVariableError::DeclaredMultipleTimes(var));
        // }
        // self.func_ctx.types[var] = ty;
        // Ok(())
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
        todo!()
        // // Assert that we're about to add instructions to this block using the definition of the
        // // given variable. ssa.use_var is the only part of this crate which can add block parameters
        // // behind the caller's back. If we disallow calling append_block_param as soon as use_var is
        // // called, then we enforce a strict separation between user parameters and SSA parameters.
        // self.ensure_inserted_block();
        //
        // let (val, side_effects) = {
        //     let ty = self
        //         .func_ctx
        //         .types
        //         .get(var)
        //         .cloned()
        //         .ok_or(UseVariableError::UsedBeforeDeclared(var))?;
        //     debug_assert_ne!(
        //         ty,
        //         Type::Unknown,
        //         "variable {:?} is used but its type has not been declared",
        //         var
        //     );
        //     let current_block = self.inner.current_block();
        //     self.func_ctx
        //         .ssa
        //         .use_var(self.inner.data_flow_graph_mut(), var, ty, current_block)
        // };
        // self.handle_ssa_side_effects(side_effects);
        // Ok(val)
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
        todo!()
        // let var_ty = self
        //     .func_ctx
        //     .types
        //     .get(var)
        //     .ok_or(DefVariableError::DefinedBeforeDeclared(var))?;
        // if var_ty != self.data_flow_graph().value_type(val) {
        //     return Err(DefVariableError::TypeMismatch(var, val));
        // }
        //
        // self.func_ctx.ssa.def_var(var, val, self.inner.current_block());
        // Ok(())
    }

    /// Register a new definition of a user variable. The type of the value must be
    /// the same as the type registered for the variable.
    pub fn def_var(&mut self, var: Variable, val: ValueRef) {
        todo!()
        // self.try_def_var(var, val).unwrap_or_else(|error| match error {
        //     DefVariableError::TypeMismatch(var, val) => {
        //         assert_eq!(
        //             &self.func_ctx.types[var],
        //             self.data_flow_graph().value_type(val),
        //             "declared type of variable {:?} doesn't match type of value {}",
        //             var,
        //             val
        //         );
        //     }
        //     DefVariableError::DefinedBeforeDeclared(var) => {
        //         panic!("variable {:?} is used but its type has not been declared", var);
        //     }
        // })
    }

    /// Returns `true` if and only if no instructions have been added since the last call to
    /// `switch_to_block`.
    fn is_pristine(&self, block: &BlockRef) -> bool {
        self.func_ctx.borrow().status[block] == BlockStatus::Empty
    }

    /// Returns `true` if and only if a terminator instruction has been inserted since the
    /// last call to `switch_to_block`.
    fn is_filled(&self, block: &BlockRef) -> bool {
        self.func_ctx.borrow_mut().status[block] == BlockStatus::Filled
    }

    /// Returns `true` if and only if the current `Block` is sealed and has no predecessors
    /// declared.
    ///
    /// The entry block of a function is never unreachable.
    pub fn is_unreachable(&self) -> bool {
        todo!()
        // let is_entry = self.inner.current_block() == self.data_flow_graph().entry_block();
        // !is_entry
        //     && self.func_ctx.ssa.is_sealed(self.inner.current_block())
        //     && !self.func_ctx.ssa.has_any_predecessors(self.inner.current_block())
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
        todo!()
        // self.func_ctx.ssa.remove_block_predecessor(old_block, inst);
        // match &mut *self.data_flow_graph_mut().insts[inst].data {
        //     Instruction::Br(Br {
        //         ref mut successor, ..
        //     }) if successor.destination == old_block => {
        //         successor.destination = new_block;
        //     }
        //     Instruction::CondBr(CondBr {
        //         ref mut then_dest,
        //         ref mut else_dest,
        //         ..
        //     }) => {
        //         if then_dest.destination == old_block {
        //             then_dest.destination = new_block;
        //         } else if else_dest.destination == old_block {
        //             else_dest.destination = new_block;
        //         }
        //     }
        //     Instruction::Switch(Switch {
        //         op: _,
        //         arg: _,
        //         ref mut arms,
        //         ref mut default,
        //     }) => {
        //         for arm in arms.iter_mut() {
        //             if arm.successor.destination == old_block {
        //                 arm.successor.destination = new_block;
        //             }
        //         }
        //         if default.destination == old_block {
        //             default.destination = new_block;
        //         }
        //     }
        //     _ => panic!("{} must be a branch instruction", inst),
        // }
        // self.func_ctx.ssa.declare_block_predecessor(new_block, inst);
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

pub struct FuncInstBuilderExt<'a, 'b> {
    builder: &'b mut FunctionBuilderExt<'a>,
}
impl<'a, 'b> FuncInstBuilderExt<'a, 'b> {
    fn new(builder: &'b mut FunctionBuilderExt<'a>) -> Self {
        // assert!(builder.data_flow_graph().is_block_linked(block));
        Self { builder }
    }
}

impl<'a> InstBuilderBase<'a> for FuncInstBuilderExt<'a, '_> {
    type L = SSABuilderListener;

    fn builder(&self) -> &OpBuilder<Self::L> {
        self.builder.inner.builder() // as &OpBuilder<L>
    }

    fn builder_mut(&mut self) -> &mut OpBuilder<Self::L> {
        self.builder.inner.builder_mut()
    }

    // fn builder_parts(
    //     &mut self,
    // ) -> (&mut midenc_hir2::dialects::builtin::Function, &mut OpBuilder<Self::L>) {
    //     (self.builder.inner.func, self.builder.inner.builder_mut())
    // }
}
