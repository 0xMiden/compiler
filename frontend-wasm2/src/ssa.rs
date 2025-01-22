//! A SSA-building API that handles incomplete CFGs.
//!
//! The algorithm is based upon Braun M., Buchwald S., Hack S., Leißa R., Mallon C.,
//! Zwinkau A. (2013) Simple and Efficient Construction of Static Single Assignment Form.
//! In: Jhala R., De Bosschere K. (eds) Compiler Construction. CC 2013.
//! Lecture Notes in Computer Science, vol 7791. Springer, Berlin, Heidelberg
//!
//! <https://link.springer.com/content/pdf/10.1007/978-3-642-37051-9_6.pdf>
//!
//! Based on Cranelift's Wasm -> CLIF translator v11.0.0

use core::mem;

use midenc_hir::{
    cranelift_entity::{
        entity_impl, packed_option::PackedOption, EntityList, EntitySet, ListPool, SecondaryMap,
    },
    diagnostics::SourceSpan,
    Block, DataFlowGraph, Inst, Value,
};
use midenc_hir_type::Type;

/// Structure containing the data relevant the construction of SSA for a given function.
///
/// The parameter struct `Variable` corresponds to the way variables are represented in the
/// non-SSA language you're translating from.
///
/// The SSA building relies on information about the variables used and defined.
///
/// This SSA building module allows you to def and use variables on the fly while you are
/// constructing the CFG, no need for a separate SSA pass after the CFG is completed.
///
/// A basic block is said _filled_ if all the instruction that it contains have been translated,
/// and it is said _sealed_ if all of its predecessors have been declared. Only filled predecessors
/// can be declared.
#[derive(Default)]
pub struct SSABuilder {
    /// Records for every variable and for every relevant block, the last definition of
    /// the variable in the block.
    variables: SecondaryMap<Variable, SecondaryMap<Block, PackedOption<Value>>>,

    /// Records the position of the basic blocks and the list of values used but not defined in the
    /// block.
    ssa_blocks: SecondaryMap<Block, SSABlockData>,

    /// Call stack for use in the `use_var`/`predecessors_lookup` state machine.
    calls: Vec<Call>,
    /// Result stack for use in the `use_var`/`predecessors_lookup` state machine.
    results: Vec<Value>,

    /// Side effects accumulated in the `use_var`/`predecessors_lookup` state machine.
    side_effects: SideEffects,

    /// Reused storage for cycle-detection.
    visited: EntitySet<Block>,

    /// Storage for pending variable definitions.
    variable_pool: ListPool<Variable>,

    /// Storage for predecessor definitions.
    inst_pool: ListPool<Inst>,
}

/// An opaque reference to a mutable variable.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Variable(u32);
entity_impl!(Variable, "var");

/// Side effects of a `use_var` or a `seal_block` method call.
#[derive(Default)]
pub struct SideEffects {
    /// When a variable is used but has never been defined before (this happens in the case of
    /// unreachable code), a placeholder `iconst` or `fconst` value is added to the right `Block`.
    /// This field signals if it is the case and return the `Block` to which the initialization has
    /// been added.
    pub instructions_added_to_blocks: Vec<Block>,
}

impl SideEffects {
    fn is_empty(&self) -> bool {
        self.instructions_added_to_blocks.is_empty()
    }
}

#[derive(Clone)]
enum Sealed {
    No {
        // List of current Block arguments for which an earlier def has not been found yet.
        undef_variables: EntityList<Variable>,
    },
    Yes,
}

impl Default for Sealed {
    fn default() -> Self {
        Sealed::No {
            undef_variables: EntityList::new(),
        }
    }
}

#[derive(Clone, Default)]
struct SSABlockData {
    // The predecessors of the Block with the block and branch instruction.
    predecessors: EntityList<Inst>,
    // A block is sealed if all of its predecessors have been declared.
    sealed: Sealed,
    // If this block is sealed and it has exactly one predecessor, this is that predecessor.
    single_predecessor: PackedOption<Block>,
}

impl SSABuilder {
    /// Clears a `SSABuilder` from all its data, letting it in a pristine state without
    /// deallocating memory.
    pub fn clear(&mut self) {
        self.variables.clear();
        self.ssa_blocks.clear();
        self.variable_pool.clear();
        self.inst_pool.clear();
        debug_assert!(self.calls.is_empty());
        debug_assert!(self.results.is_empty());
        debug_assert!(self.side_effects.is_empty());
    }

    /// Tests whether an `SSABuilder` is in a cleared state.
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
            && self.ssa_blocks.is_empty()
            && self.calls.is_empty()
            && self.results.is_empty()
            && self.side_effects.is_empty()
    }
}

/// States for the `use_var`/`predecessors_lookup` state machine.
enum Call {
    UseVar(Inst),
    FinishPredecessorsLookup(Value, Block),
}

/// The following methods are the API of the SSA builder. Here is how it should be used when
/// translating to Miden IR:
///
/// - for each basic block, create a corresponding data for SSA construction with `declare_block`;
///
/// - while traversing a basic block and translating instruction, use `def_var` and `use_var` to
///   record definitions and uses of variables, these methods will give you the corresponding SSA
///   values;
///
/// - when all the instructions in a basic block have translated, the block is said _filled_ and
///   only then you can add it as a predecessor to other blocks with `declare_block_predecessor`;
///
/// - when you have constructed all the predecessor to a basic block, call `seal_block` on it with
///   the `Function` that you are building.
///
/// This API will give you the correct SSA values to use as arguments of your instructions,
/// as well as modify the jump instruction and `Block` parameters to account for the SSA
/// Phi functions.
impl SSABuilder {
    /// Declares a new definition of a variable in a given basic block.
    pub fn def_var(&mut self, var: Variable, val: Value, block: Block) {
        self.variables[var][block] = PackedOption::from(val);
    }

    /// Declares a use of a variable in a given basic block. Returns the SSA value corresponding
    /// to the current SSA definition of this variable and a list of newly created Blocks
    ///
    /// If the variable has never been defined in this blocks or recursively in its predecessors,
    /// this method will silently create an initializer. You are
    /// responsible for making sure that you initialize your variables.
    pub fn use_var(
        &mut self,
        dfg: &mut DataFlowGraph,
        var: Variable,
        ty: Type,
        block: Block,
    ) -> (Value, SideEffects) {
        debug_assert!(self.calls.is_empty());
        debug_assert!(self.results.is_empty());
        debug_assert!(self.side_effects.is_empty());

        // Prepare the 'calls' and 'results' stacks for the state machine.
        self.use_var_nonlocal(dfg, var, ty.clone(), block);
        let value = self.run_state_machine(dfg, var, ty);

        let side_effects = mem::take(&mut self.side_effects);
        (value, side_effects)
    }

    /// Resolve the minimal SSA Value of `var` in `block` by traversing predecessors.
    ///
    /// This function sets up state for `run_state_machine()` but does not execute it.
    fn use_var_nonlocal(
        &mut self,
        dfg: &mut DataFlowGraph,
        var: Variable,
        ty: Type,
        mut block: Block,
    ) {
        // First, try Local Value Numbering (Algorithm 1 in the paper).
        // If the variable already has a known Value in this block, use that.
        if let Some(val) = self.variables[var][block].expand() {
            self.results.push(val);
            return;
        }

        // Otherwise, use Global Value Numbering (Algorithm 2 in the paper).
        // This resolves the Value with respect to its predecessors.
        // Find the most recent definition of `var`, and the block the definition comes from.
        let (val, from) = self.find_var(dfg, var, ty, block);

        // The `from` block returned from `find_var` is guaranteed to be on the path we follow by
        // traversing only single-predecessor edges. It might be equal to `block` if there is no
        // such path, but in that case `find_var` ensures that the variable is defined in this block
        // by a new block parameter. It also might be somewhere in a cycle, but even then this loop
        // will terminate the first time it encounters that block, rather than continuing around the
        // cycle forever.
        //
        // Why is it okay to copy the definition to all intervening blocks? For the initial block,
        // this may not be the final definition of this variable within this block, but if we've
        // gotten here then we know there is no earlier definition in the block already.
        //
        // For the remaining blocks: Recall that a block is only allowed to be set as a predecessor
        // after all its instructions have already been filled in, so when we follow a predecessor
        // edge to a block, we know there will never be any more local variable definitions added to
        // that block. We also know that `find_var` didn't find a definition for this variable in
        // any of the blocks before `from`.
        //
        // So in either case there is no definition in these blocks yet and we can blindly set one.
        let var_defs = &mut self.variables[var];
        while block != from {
            debug_assert!(var_defs[block].is_none());
            var_defs[block] = PackedOption::from(val);
            block = self.ssa_blocks[block].single_predecessor.unwrap();
        }
    }

    /// Find the most recent definition of this variable, returning both the definition and the
    /// block in which it was found. If we can't find a definition that's provably the right one for
    /// all paths to the current block, then append a block parameter to some block and use that as
    /// the definition. Either way, also arrange that the definition will be on the `results` stack
    /// when `run_state_machine` is done processing the current step.
    ///
    /// If a block has exactly one predecessor, and the block is sealed so we know its predecessors
    /// will never change, then its definition for this variable is the same as the definition from
    /// that one predecessor. In this case it's easy to see that no block parameter is necessary,
    /// but we need to look at the predecessor to see if a block parameter might be needed there.
    /// That holds transitively across any chain of sealed blocks with exactly one predecessor each.
    ///
    /// This runs into a problem, though, if such a chain has a cycle: Blindly following a cyclic
    /// chain that never defines this variable would lead to an infinite loop in the compiler. It
    /// doesn't really matter what code we generate in that case. Since each block in the cycle has
    /// exactly one predecessor, there's no way to enter the cycle from the function's entry block;
    /// and since all blocks in the cycle are sealed, the entire cycle is permanently dead code. But
    /// we still have to prevent the possibility of an infinite loop.
    ///
    /// To break cycles, we can pick any block within the cycle as the one where we'll add a block
    /// parameter. It's convenient to pick the block at which we entered the cycle, because that's
    /// the first place where we can detect that we just followed a cycle. Adding a block parameter
    /// gives us a definition we can reuse throughout the rest of the cycle.
    fn find_var(
        &mut self,
        dfg: &mut DataFlowGraph,
        var: Variable,
        ty: Type,
        mut block: Block,
    ) -> (Value, Block) {
        // Try to find an existing definition along single-predecessor edges first.
        self.visited.clear();
        let var_defs = &mut self.variables[var];
        while let Some(pred) = self.ssa_blocks[block].single_predecessor.expand() {
            if !self.visited.insert(block) {
                break;
            }
            block = pred;
            if let Some(val) = var_defs[block].expand() {
                self.results.push(val);
                return (val, block);
            }
        }

        // We've promised to return the most recent block where `var` was defined, but we didn't
        // find a usable definition. So create one.
        let val = dfg.append_block_param(block, ty, SourceSpan::default());
        var_defs[block] = PackedOption::from(val);

        // Now every predecessor needs to pass its definition of this variable to the newly added
        // block parameter. To do that we have to "recursively" call `use_var`, but there are two
        // problems with doing that. First, we need to keep a fixed bound on stack depth, so we
        // can't actually recurse; instead we defer to `run_state_machine`. Second, if we don't
        // know all our predecessors yet, we have to defer this work until the block gets sealed.
        match &mut self.ssa_blocks[block].sealed {
            // Once all the `calls` added here complete, this leaves either `val` or an equivalent
            // definition on the `results` stack.
            Sealed::Yes => self.begin_predecessors_lookup(val, block),
            Sealed::No { undef_variables } => {
                undef_variables.push(var, &mut self.variable_pool);
                self.results.push(val);
            }
        }
        (val, block)
    }

    /// Declares a new basic block to construct corresponding data for SSA construction.
    /// No predecessors are declared here and the block is not sealed.
    /// Predecessors have to be added with `declare_block_predecessor`.
    pub fn declare_block(&mut self, block: Block) {
        // Ensure the block exists so seal_one_block will see it even if no predecessors or
        // variables get declared for this block. But don't assign anything to it:
        // SecondaryMap automatically sets all blocks to `default()`.
        let _ = &mut self.ssa_blocks[block];
    }

    /// Declares a new predecessor for a `Block` and record the branch instruction
    /// of the predecessor that leads to it.
    ///
    /// The precedent `Block` must be filled before added as predecessor.
    /// Note that you must provide no jump arguments to the branch
    /// instruction when you create it since `SSABuilder` will fill them for you.
    ///
    /// Callers are expected to avoid adding the same predecessor more than once in the case
    /// of a jump table.
    pub fn declare_block_predecessor(&mut self, block: Block, inst: Inst) {
        debug_assert!(!self.is_sealed(block), "you cannot add a predecessor to a sealed block");
        debug_assert!(
            self.ssa_blocks[block]
                .predecessors
                .as_slice(&self.inst_pool)
                .iter()
                .all(|&branch| branch != inst),
            "you have declared the same predecessor twice!"
        );
        self.ssa_blocks[block].predecessors.push(inst, &mut self.inst_pool);
    }

    /// Remove a previously declared Block predecessor by giving a reference to the jump
    /// instruction. Returns the basic block containing the instruction.
    ///
    /// Note: use only when you know what you are doing, this might break the SSA building problem
    pub fn remove_block_predecessor(&mut self, block: Block, inst: Inst) {
        debug_assert!(!self.is_sealed(block));
        let data = &mut self.ssa_blocks[block];
        let pred = data
            .predecessors
            .as_slice(&self.inst_pool)
            .iter()
            .position(|&branch| branch == inst)
            .expect("the predecessor you are trying to remove is not declared");
        data.predecessors.swap_remove(pred, &mut self.inst_pool);
    }

    /// Completes the global value numbering for a `Block`, all of its predecessors having been
    /// already sealed.
    ///
    /// This method modifies the function's `Layout` by adding arguments to the `Block`s to
    /// take into account the Phi function placed by the SSA algorithm.
    ///
    /// Returns the list of newly created blocks for critical edge splitting.
    pub fn seal_block(&mut self, block: Block, dfg: &mut DataFlowGraph) -> SideEffects {
        debug_assert!(
            !self.is_sealed(block),
            "Attempting to seal {} which is already sealed.",
            block
        );
        self.seal_one_block(block, dfg);
        mem::take(&mut self.side_effects)
    }

    /// Helper function for `seal_block`
    fn seal_one_block(&mut self, block: Block, dfg: &mut DataFlowGraph) {
        // For each undef var we look up values in the predecessors and create a block parameter
        // only if necessary.
        let mut undef_variables =
            match mem::replace(&mut self.ssa_blocks[block].sealed, Sealed::Yes) {
                Sealed::No { undef_variables } => undef_variables,
                Sealed::Yes => return,
            };
        let ssa_params = undef_variables.len(&self.variable_pool);

        let predecessors = self.predecessors(block);
        if predecessors.len() == 1 {
            let pred = dfg.insts[predecessors[0]].block;
            self.ssa_blocks[block].single_predecessor = PackedOption::from(pred);
        }

        // Note that begin_predecessors_lookup requires visiting these variables in the same order
        // that they were defined by find_var, because it appends arguments to the jump instructions
        // in all the predecessor blocks one variable at a time.
        for idx in 0..ssa_params {
            let var = undef_variables.get(idx, &self.variable_pool).unwrap();

            // We need the temporary Value that was assigned to this Variable. If that Value shows
            // up as a result from any of our predecessors, then it never got assigned on the loop
            // through that block. We get the value from the next block param, where it was first
            // allocated in find_var.
            let block_params = dfg.block_params(block);

            // On each iteration through this loop, there are (ssa_params - idx) undefined variables
            // left to process. Previous iterations through the loop may have removed earlier block
            // parameters, but the last (ssa_params - idx) block parameters always correspond to the
            // remaining undefined variables. So index from the end of the current block params.
            let val = block_params[block_params.len() - (ssa_params - idx)];

            debug_assert!(self.calls.is_empty());
            debug_assert!(self.results.is_empty());
            // self.side_effects may be non-empty here so that callers can
            // accumulate side effects over multiple calls.
            self.begin_predecessors_lookup(val, block);
            self.run_state_machine(dfg, var, dfg.value_type(val).clone());
        }

        undef_variables.clear(&mut self.variable_pool);
    }

    /// Given the local SSA Value of a Variable in a Block, perform a recursive lookup on
    /// predecessors to determine if it is redundant with another Value earlier in the CFG.
    ///
    /// If such a Value exists and is redundant, the local Value is replaced by the
    /// corresponding non-local Value. If the original Value was a Block parameter,
    /// the parameter may be removed if redundant. Parameters are placed eagerly by callers
    /// to avoid infinite loops when looking up a Value for a Block that is in a CFG loop.
    ///
    /// Doing this lookup for each Value in each Block preserves SSA form during construction.
    ///
    /// ## Arguments
    ///
    /// `sentinel` is a dummy Block parameter inserted by `use_var_nonlocal()`.
    /// Its purpose is to allow detection of CFG cycles while traversing predecessors.
    fn begin_predecessors_lookup(&mut self, sentinel: Value, dest_block: Block) {
        self.calls.push(Call::FinishPredecessorsLookup(sentinel, dest_block));
        // Iterate over the predecessors.
        self.calls.extend(
            self.ssa_blocks[dest_block]
                .predecessors
                .as_slice(&self.inst_pool)
                .iter()
                .rev()
                .copied()
                .map(Call::UseVar),
        );
    }

    /// Examine the values from the predecessors and compute a result value, creating
    /// block parameters as needed.
    fn finish_predecessors_lookup(
        &mut self,
        dfg: &mut DataFlowGraph,
        sentinel: Value,
        dest_block: Block,
    ) -> Value {
        // Determine how many predecessors are yielding unique, non-temporary Values.
        let num_predecessors = self.predecessors(dest_block).len();
        // When this `Drain` is dropped, these elements will get truncated.
        let results = self.results.drain(self.results.len() - num_predecessors..);
        // Keep the block argument.
        let mut preds = self.ssa_blocks[dest_block].predecessors;
        for (idx, &val) in results.as_slice().iter().enumerate() {
            let pred = preds.get_mut(idx, &mut self.inst_pool).unwrap();
            let branch = *pred;
            assert!(
                dfg.insts[branch].opcode().is_branch(),
                "you have declared a non-branch instruction as a predecessor to a block!"
            );
            dfg.append_branch_destination_argument(branch, dest_block, val);
        }
        sentinel
    }

    /// Returns the list of `Block`s that have been declared as predecessors of the argument.
    fn predecessors(&self, block: Block) -> &[Inst] {
        self.ssa_blocks[block].predecessors.as_slice(&self.inst_pool)
    }

    /// Returns whether the given Block has any predecessor or not.
    pub fn has_any_predecessors(&self, block: Block) -> bool {
        !self.predecessors(block).is_empty()
    }

    /// Returns `true` if and only if `seal_block` has been called on the argument.
    pub fn is_sealed(&self, block: Block) -> bool {
        matches!(self.ssa_blocks[block].sealed, Sealed::Yes)
    }

    /// The main algorithm is naturally recursive: when there's a `use_var` in a
    /// block with no corresponding local defs, it recurses and performs a
    /// `use_var` in each predecessor. To avoid risking running out of callstack
    /// space, we keep an explicit stack and use a small state machine rather
    /// than literal recursion.
    fn run_state_machine(&mut self, func: &mut DataFlowGraph, var: Variable, ty: Type) -> Value {
        // Process the calls scheduled in `self.calls` until it is empty.
        while let Some(call) = self.calls.pop() {
            match call {
                Call::UseVar(branch) => {
                    let block = func.insts[branch].block;
                    self.use_var_nonlocal(func, var, ty.clone(), block);
                }
                Call::FinishPredecessorsLookup(sentinel, dest_block) => {
                    let val = self.finish_predecessors_lookup(func, sentinel, dest_block);
                    self.results.push(val);
                }
            }
        }
        debug_assert_eq!(self.results.len(), 1);
        self.results.pop().unwrap()
    }
}
