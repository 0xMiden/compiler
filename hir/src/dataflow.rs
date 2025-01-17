use core::ops::{Deref, DerefMut, Index, IndexMut};

use cranelift_entity::{PrimaryMap, SecondaryMap};
use smallvec::SmallVec;

use crate::{
    diagnostics::{SourceSpan, Span, Spanned},
    *,
};

pub struct DataFlowGraph {
    pub entry: Block,
    pub attrs: AttributeSet,
    pub blocks: OrderedArenaMap<Block, BlockData>,
    pub insts: ArenaMap<Inst, InstNode>,
    pub results: SecondaryMap<Inst, ValueList>,
    pub values: PrimaryMap<Value, ValueData>,
    pub value_lists: ValueListPool,
    pub imports: FxHashMap<FunctionIdent, ExternalFunction>,
    pub globals: PrimaryMap<GlobalValue, GlobalValueData>,
    pub locals: PrimaryMap<LocalId, Local>,
    pub constants: ConstantPool,
}
impl Default for DataFlowGraph {
    fn default() -> Self {
        let mut dfg = Self::new_uninit();
        let entry = dfg.blocks.create();
        dfg.entry = entry;
        dfg.blocks.append(entry, BlockData::new(entry));
        dfg
    }
}
impl DataFlowGraph {
    /// Create a new, completely uninitialized DataFlowGraph
    pub fn new_uninit() -> Self {
        Self {
            entry: Block::from_u32(0),
            attrs: AttributeSet::default(),
            blocks: OrderedArenaMap::new(),
            insts: ArenaMap::new(),
            results: SecondaryMap::new(),
            values: PrimaryMap::new(),
            value_lists: ValueListPool::new(),
            imports: Default::default(),
            globals: PrimaryMap::new(),
            locals: PrimaryMap::new(),
            constants: ConstantPool::default(),
        }
    }

    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute<Q>(&self, name: &Q) -> Option<&AttributeValue>
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get(name)
    }

    /// Return true if this function has an attributed named `name`
    pub fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.has(name)
    }

    /// Set the attribute `name` with `value` for this function.
    pub fn set_attribute(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>) {
        self.attrs.insert(name, value);
    }

    /// Remove any attribute with the given name from this function
    pub fn remove_attribute<Q>(&mut self, name: &Q)
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.remove(name);
    }

    /// Returns an [ExternalFunction] given its [FunctionIdent]
    pub fn get_import(&self, id: &FunctionIdent) -> Option<&ExternalFunction> {
        self.imports.get(id)
    }

    /// Look up an [ExternalFunction] given it's module and function name
    pub fn get_import_by_name<M: AsRef<str>, F: AsRef<str>>(
        &self,
        module: M,
        name: F,
    ) -> Option<&ExternalFunction> {
        let id = FunctionIdent {
            module: Ident::with_empty_span(Symbol::intern(module.as_ref())),
            function: Ident::with_empty_span(Symbol::intern(name.as_ref())),
        };
        self.imports.get(&id)
    }

    /// Returns an iterator over the [ExternalFunction]s imported by this function
    pub fn imports<'a, 'b: 'a>(&'b self) -> impl Iterator<Item = &'a ExternalFunction> + 'a {
        self.imports.values()
    }

    /// Imports function `name` from `module`, with `signature`, returning a [FunctionIdent]
    /// corresponding to the import.
    ///
    /// If the function is already imported, and the signature doesn't match, `Err` is returned.
    pub fn import_function(
        &mut self,
        module: Ident,
        name: Ident,
        signature: Signature,
    ) -> Result<FunctionIdent, SymbolConflictError> {
        use hashbrown::hash_map::Entry;

        let id = FunctionIdent {
            module,
            function: name,
        };
        match self.imports.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(ExternalFunction { id, signature });
                Ok(id)
            }
            Entry::Occupied(entry) => {
                if entry.get().signature != signature {
                    Err(SymbolConflictError(id))
                } else {
                    Ok(id)
                }
            }
        }
    }

    /// Create a new global value reference
    pub fn create_global_value(&mut self, data: GlobalValueData) -> GlobalValue {
        self.globals.push(data)
    }

    /// Gets the data associated with the given [GlobalValue]
    pub fn global_value(&self, gv: GlobalValue) -> &GlobalValueData {
        &self.globals[gv]
    }

    /// Returns true if the given [GlobalValue] represents an address
    pub fn is_global_addr(&self, gv: GlobalValue) -> bool {
        match &self.globals[gv] {
            GlobalValueData::Symbol { .. } | GlobalValueData::IAddImm { .. } => true,
            GlobalValueData::Load { base, .. } => self.is_global_addr(*base),
        }
    }

    /// Returns the type of the given global value
    pub fn global_type(&self, gv: GlobalValue) -> Type {
        match &self.globals[gv] {
            GlobalValueData::Symbol { .. } => Type::Ptr(Box::new(Type::I8)),
            GlobalValueData::IAddImm { base, .. } => self.global_type(*base),
            GlobalValueData::Load { ref ty, .. } => ty.clone(),
        }
    }

    pub fn make_value(&mut self, data: ValueData) -> Value {
        self.values.push(data)
    }

    pub fn value_type(&self, v: Value) -> &Type {
        self.values[v].ty()
    }

    pub fn value_span(&self, v: Value) -> SourceSpan {
        match &self.values[v] {
            ValueData::Param { span, .. } => *span,
            ValueData::Inst { inst, .. } => self.inst_span(*inst),
        }
    }

    #[inline(always)]
    pub fn value_data(&self, v: Value) -> &ValueData {
        &self.values[v]
    }

    pub fn set_value_type(&mut self, v: Value, ty: Type) {
        self.values[v].set_type(ty)
    }

    pub fn get_value(&self, v: Value) -> ValueData {
        self.values[v].clone()
    }

    pub fn value_block(&self, v: Value) -> Block {
        match self.value_data(v) {
            ValueData::Inst { inst, .. } => self
                .inst_block(*inst)
                .expect("invalid value reference: instruction is not attached to a block"),
            ValueData::Param { block, .. } => *block,
        }
    }

    /// Get a reference to the metadata for an instruction
    #[inline(always)]
    pub fn inst_node(&self, inst: Inst) -> &InstNode {
        &self.insts[inst]
    }

    /// Get a reference to the data for an instruction
    #[inline(always)]
    pub fn inst(&self, inst: Inst) -> &Instruction {
        &self.insts[inst].data
    }

    /// Get a mutable reference to the metadata for an instruction
    #[inline(always)]
    pub fn inst_mut(&mut self, inst: Inst) -> &mut Instruction {
        &mut self.insts[inst].data
    }

    pub fn inst_span(&self, inst: Inst) -> SourceSpan {
        self.inst_node(inst).span()
    }

    pub fn inst_args(&self, inst: Inst) -> &[Value] {
        self.insts[inst].arguments(&self.value_lists)
    }

    pub fn inst_block(&self, inst: Inst) -> Option<Block> {
        let inst_data = &self.insts[inst];
        if inst_data.link.is_linked() {
            Some(inst_data.block)
        } else {
            None
        }
    }

    pub fn inst_results(&self, inst: Inst) -> &[Value] {
        self.results[inst].as_slice(&self.value_lists)
    }

    /// Append a new instruction to the end of `block`, using the provided instruction
    /// data, controlling type variable, and source span
    #[inline]
    pub fn append_inst(
        &mut self,
        block: Block,
        data: Instruction,
        ctrl_ty: Type,
        span: SourceSpan,
    ) -> Inst {
        self.insert_inst(InsertionPoint::after(ProgramPoint::Block(block)), data, ctrl_ty, span)
    }

    /// Insert a new instruction at `ip`, using the provided instruction
    /// data, controlling type variable, and source span
    pub fn insert_inst(
        &mut self,
        ip: InsertionPoint,
        data: Instruction,
        ctrl_ty: Type,
        span: SourceSpan,
    ) -> Inst {
        // Allocate the key for this instruction
        let id = self.insts.alloc_key();
        let block_id = match ip.at {
            ProgramPoint::Block(block) => block,
            ProgramPoint::Inst(inst) => {
                self.inst_block(inst).expect("cannot insert after detached instruction")
            }
        };
        // Store the instruction metadata
        self.insts.append(id, InstNode::new(id, block_id, Span::new(span, data)));
        // Manufacture values for all of the instruction results
        self.make_results(id, ctrl_ty);
        // Insert the instruction based on the insertion point provided
        let data = unsafe { UnsafeRef::from_raw(&self.insts[id]) };
        let block = &mut self.blocks[block_id];
        match ip {
            InsertionPoint {
                at: ProgramPoint::Block(_),
                action: Insert::After,
            } => {
                // Insert at the end of this block
                block.append(data);
            }
            InsertionPoint {
                at: ProgramPoint::Block(_),
                action: Insert::Before,
            } => {
                // Insert at the start of this block
                block.prepend(data);
            }
            InsertionPoint {
                at: ProgramPoint::Inst(inst),
                action,
            } => {
                let mut cursor = block.cursor_mut();
                while let Some(ix) = cursor.get() {
                    if ix.key == inst {
                        break;
                    }
                    cursor.move_next();
                }
                assert!(!cursor.is_null());
                match action {
                    // Insert just after `inst` in this block
                    Insert::After => cursor.insert_after(data),
                    // Insert just before `inst` in this block
                    Insert::Before => cursor.insert_before(data),
                }
            }
        }
        id
    }

    /// Create a new instruction which is a clone of `inst`, but detached from any block.
    ///
    /// NOTE: The instruction is in a temporarily invalid state, because if it has arguments,
    /// they will reference values from the scope of the original instruction, but the clone
    /// hasn't been inserted anywhere yet. It is up to the caller to ensure that the cloned
    /// instruction is updated appropriately once inserted.
    pub fn clone_inst(&mut self, inst: Inst) -> Inst {
        let id = self.insts.alloc_key();
        let span = self.insts[inst].data.span();
        let data = self.insts[inst].data.deep_clone(&mut self.value_lists);
        self.insts
            .append(id, InstNode::new(id, Block::default(), Span::new(span, data)));

        // Derive results for the cloned instruction using the results
        // of the original instruction
        let results = SmallVec::<[Value; 1]>::from_slice(self.inst_results(inst));
        for result in results.into_iter() {
            let ty = self.value_type(result).clone();
            self.append_result(id, ty);
        }
        id
    }

    /// Create a `ReplaceBuilder` that will replace `inst` with a new instruction in-place.
    pub fn replace(&mut self, inst: Inst) -> ReplaceBuilder {
        ReplaceBuilder::new(self, inst)
    }

    pub fn append_result(&mut self, inst: Inst, ty: Type) -> Value {
        let res = self.values.next_key();
        let num = self.results[inst].push(res, &mut self.value_lists);
        debug_assert!(num <= u16::MAX as usize, "too many result values");
        self.make_value(ValueData::Inst {
            ty,
            inst,
            num: num as u16,
        })
    }

    pub fn first_result(&self, inst: Inst) -> Value {
        self.results[inst].first(&self.value_lists).expect("instruction has no results")
    }

    pub fn has_results(&self, inst: Inst) -> bool {
        !self.results[inst].is_empty()
    }

    fn make_results(&mut self, inst: Inst, ctrl_ty: Type) {
        self.results[inst].clear(&mut self.value_lists);

        let opcode = self.insts[inst].opcode();
        if let Some(fdata) = self.call_signature(inst) {
            let results =
                SmallVec::<[Type; 2]>::from_iter(fdata.results().iter().map(|abi| abi.ty.clone()));
            for ty in results.into_iter() {
                self.append_result(inst, ty);
            }
        } else {
            match self.insts[inst].data.deref() {
                Instruction::InlineAsm(ref asm) => {
                    let results = asm.results.clone();
                    for ty in results.into_iter() {
                        self.append_result(inst, ty);
                    }
                }
                ix => {
                    let overflow = ix.overflow();
                    for ty in opcode.results(overflow, ctrl_ty).into_iter() {
                        self.append_result(inst, ty);
                    }
                }
            }
        }
    }

    pub(super) fn replace_results(&mut self, inst: Inst, ctrl_ty: Type) {
        let opcode = self.insts[inst].opcode();
        let old_results =
            SmallVec::<[Value; 1]>::from_slice(self.results[inst].as_slice(&self.value_lists));
        let mut new_results = SmallVec::<[Type; 1]>::default();
        if let Some(fdata) = self.call_signature(inst) {
            new_results.extend(fdata.results().iter().map(|p| p.ty.clone()));
        } else {
            match self.insts[inst].data.deref() {
                Instruction::InlineAsm(ref asm) => {
                    new_results.extend(asm.results.as_slice().iter().cloned());
                }
                ix => {
                    let overflow = ix.overflow();
                    new_results = opcode.results(overflow, ctrl_ty);
                }
            }
        }
        let old_results_len = old_results.len();
        let new_results_len = new_results.len();
        if old_results_len > new_results_len {
            self.results[inst].truncate(new_results_len, &mut self.value_lists);
        }
        for (index, ty) in new_results.into_iter().enumerate() {
            if index >= old_results_len {
                // We must allocate a new value for this result
                self.append_result(inst, ty);
            } else {
                // We're updating the old value with a new type
                let value = old_results[index];
                self.values[value].set_type(ty);
            }
        }
    }

    /// Replace uses of `value` with `replacement` in the arguments of `inst`
    pub fn replace_uses(&mut self, inst: Inst, value: Value, replacement: Value) {
        let ix = &mut self.insts[inst];
        match ix.data.deref_mut() {
            Instruction::Br(Br {
                ref mut successor, ..
            }) => {
                let args = successor.args.as_mut_slice(&mut self.value_lists);
                for arg in args.iter_mut() {
                    if arg == &value {
                        *arg = replacement;
                    }
                }
            }
            Instruction::CondBr(CondBr {
                ref mut cond,
                ref mut then_dest,
                ref mut else_dest,
                ..
            }) => {
                if cond == &value {
                    *cond = replacement;
                }
                let then_args = then_dest.args.as_mut_slice(&mut self.value_lists);
                for arg in then_args.iter_mut() {
                    if arg == &value {
                        *arg = replacement;
                    }
                }
                let else_args = else_dest.args.as_mut_slice(&mut self.value_lists);
                for arg in else_args.iter_mut() {
                    if arg == &value {
                        *arg = replacement;
                    }
                }
            }
            Instruction::Switch(Switch {
                ref mut arg,
                ref mut arms,
                default: default_succ,
                ..
            }) => {
                if arg == &value {
                    *arg = replacement;
                }
                let default_args = default_succ.args.as_mut_slice(&mut self.value_lists);
                for arg in default_args.iter_mut() {
                    if arg == &value {
                        *arg = replacement;
                    }
                }
                for arm in arms.iter_mut() {
                    let args = arm.successor.args.as_mut_slice(&mut self.value_lists);
                    for arg in args.iter_mut() {
                        if arg == &value {
                            *arg = replacement;
                        }
                    }
                }
            }
            ix => {
                for arg in ix.arguments_mut(&mut self.value_lists) {
                    if arg == &value {
                        *arg = replacement;
                    }
                }
            }
        }
    }

    /// Replace argument at `index` in the argument list of `inst`
    ///
    /// NOTE: This should not be used for successor arguments, as each successor gets its
    /// own distinct argument list, separate from the instruction argument list.
    pub fn replace_argument(&mut self, inst: Inst, index: usize, replacement: Value) {
        self.insts[inst].data.arguments_mut(&mut self.value_lists)[index] = replacement;
    }

    /// Replace the block argument at `index`, for the successor argument list of the
    /// successor at `succ_index`, in the set of successors for `inst`.
    pub fn replace_successor_argument(
        &mut self,
        inst: Inst,
        succ_index: usize,
        index: usize,
        replacement: Value,
    ) {
        let ix = &mut self.insts[inst];
        match ix.data.deref_mut() {
            Instruction::Br(Br {
                ref mut successor, ..
            }) => {
                debug_assert_eq!(succ_index, 0);
                successor.args.as_mut_slice(&mut self.value_lists)[index] = replacement;
            }
            Instruction::CondBr(CondBr {
                ref mut then_dest,
                ref mut else_dest,
                ..
            }) => match succ_index {
                0 => {
                    then_dest.args.as_mut_slice(&mut self.value_lists)[index] = replacement;
                }
                1 => {
                    else_dest.args.as_mut_slice(&mut self.value_lists)[index] = replacement;
                }
                _ => unreachable!("expected valid successor index for cond_br, got {succ_index}"),
            },
            Instruction::Switch(Switch {
                ref mut arms,
                default: ref mut default_succ,
                ..
            }) => {
                debug_assert!(succ_index < arms.len() + 1);
                if succ_index == arms.len() {
                    default_succ.args.as_mut_slice(&mut self.value_lists)[index] = replacement;
                }
                arms[succ_index].successor.args.as_mut_slice(&mut self.value_lists)[index] =
                    replacement;
            }
            ix => panic!("invalid instruction: expected branch instruction, got {ix:#?}"),
        }
    }

    pub fn pp_block(&self, pp: ProgramPoint) -> Block {
        match pp {
            ProgramPoint::Block(block) => block,
            ProgramPoint::Inst(inst) => self.inst_block(inst).expect("program point not in layout"),
        }
    }

    pub fn pp_cmp<A, B>(&self, a: A, b: B) -> core::cmp::Ordering
    where
        A: Into<ProgramPoint>,
        B: Into<ProgramPoint>,
    {
        let a = a.into();
        let b = b.into();
        debug_assert_eq!(self.pp_block(a), self.pp_block(b));
        let a_seq = match a {
            ProgramPoint::Block(_) => 0,
            ProgramPoint::Inst(inst) => {
                let block = self.insts[inst].block;
                self.blocks[block].insts().position(|i| i == inst).unwrap() + 1
            }
        };
        let b_seq = match b {
            ProgramPoint::Block(_) => 0,
            ProgramPoint::Inst(inst) => {
                let block = self.insts[inst].block;
                self.blocks[block].insts().position(|i| i == inst).unwrap() + 1
            }
        };
        a_seq.cmp(&b_seq)
    }

    pub fn call_signature(&self, inst: Inst) -> Option<&Signature> {
        match self.insts[inst].analyze_call(&self.value_lists) {
            CallInfo::NotACall => None,
            CallInfo::Direct(ref f, _) => Some(&self.imports[f].signature),
        }
    }

    pub fn analyze_call(&self, inst: Inst) -> CallInfo<'_> {
        self.insts[inst].analyze_call(&self.value_lists)
    }

    pub fn analyze_branch(&self, inst: Inst) -> BranchInfo<'_> {
        self.insts[inst].analyze_branch(&self.value_lists)
    }

    pub fn blocks(&self) -> impl Iterator<Item = (Block, &BlockData)> {
        Blocks {
            cursor: self.blocks.cursor(),
        }
    }

    /// Get the block identifier for the entry block
    #[inline(always)]
    pub fn entry_block(&self) -> Block {
        self.entry
    }

    /// Get a reference to the data for the entry block
    #[inline]
    pub fn entry(&self) -> &BlockData {
        &self.blocks[self.entry]
    }

    /// Get a mutable reference to the data for the entry block
    #[inline]
    pub fn entry_mut(&mut self) -> &mut BlockData {
        &mut self.blocks[self.entry]
    }

    pub(super) fn last_block(&self) -> Option<Block> {
        self.blocks.last().map(|b| b.key())
    }

    pub fn num_blocks(&self) -> usize {
        self.blocks.iter().count()
    }

    /// Get an immutable reference to the block data for `block`
    pub fn block(&self, block: Block) -> &BlockData {
        &self.blocks[block]
    }

    /// Get a mutable reference to the block data for `block`
    pub fn block_mut(&mut self, block: Block) -> &mut BlockData {
        &mut self.blocks[block]
    }

    pub fn block_args(&self, block: Block) -> &[Value] {
        self.blocks[block].params.as_slice(&self.value_lists)
    }

    pub fn block_insts(&self, block: Block) -> impl Iterator<Item = Inst> + '_ {
        self.blocks[block].insts()
    }

    pub fn block_cursor(&self, block: Block) -> InstructionCursor<'_> {
        self.blocks[block].front()
    }

    pub fn block_cursor_at(&self, inst: Inst) -> InstructionCursor<'_> {
        let block = self.inst_block(inst).expect("instruction is not linked to a block");
        let cursor = self.blocks[block].cursor_at_inst(inst);
        assert!(!cursor.is_null());
        cursor
    }

    pub fn last_inst(&self, block: Block) -> Option<Inst> {
        self.blocks[block].last()
    }

    pub fn is_block_linked(&self, block: Block) -> bool {
        self.blocks.contains(block)
    }

    pub fn is_block_empty(&self, block: Block) -> bool {
        self.blocks[block].is_empty()
    }

    pub fn create_block(&mut self) -> Block {
        let id = self.blocks.create();
        let data = BlockData::new(id);
        self.blocks.append(id, data);
        id
    }

    pub fn append_block(&mut self, block: Block) {
        self.blocks.append(block, BlockData::new(block));
    }

    /// Creates a new block, inserted into the function layout just after `block`
    pub fn create_block_after(&mut self, block: Block) -> Block {
        let id = self.blocks.create();
        let data = BlockData::new(id);
        assert!(
            self.blocks.get(block).is_some(),
            "cannot insert a new block after {block}, it is not linked"
        );
        self.blocks.insert_after(id, block, data);
        id
    }

    /// Removes `block` from the body of this function, without destroying it's data
    pub fn detach_block(&mut self, block: Block) {
        self.blocks.remove(block);
    }

    pub fn num_block_params(&self, block: Block) -> usize {
        self.blocks[block].params.len(&self.value_lists)
    }

    pub fn block_params(&self, block: Block) -> &[Value] {
        self.blocks[block].params.as_slice(&self.value_lists)
    }

    pub fn block_param(&self, block: Block, index: usize) -> &ValueData {
        self.blocks[block]
            .params
            .get(index, &self.value_lists)
            .map(|id| self.value_data(id))
            .expect("block argument index is out of bounds")
    }

    pub fn block_param_types(&self, block: Block) -> SmallVec<[Type; 1]> {
        self.block_params(block).iter().map(|&v| self.value_type(v).clone()).collect()
    }

    /// Clone the block parameters of `src` as a new set of values, derived from the data used to
    /// crate the originals, and use them to populate the block arguments of `dest`, in the same
    /// order.
    pub fn clone_block_params(&mut self, src: Block, dest: Block) {
        debug_assert_eq!(
            self.num_block_params(dest),
            0,
            "cannot clone block params to a block that already has params"
        );
        let num_params = self.num_block_params(src);
        for i in 0..num_params {
            let value = self.block_param(src, i);
            let ty = value.ty().clone();
            let span = value.span();
            self.append_block_param(dest, ty, span);
        }
    }

    pub fn append_block_param(&mut self, block: Block, ty: Type, span: SourceSpan) -> Value {
        let param = self.values.next_key();
        let num = self.blocks[block].params.push(param, &mut self.value_lists);
        debug_assert!(num <= u16::MAX as usize, "too many parameters on block");
        self.make_value(ValueData::Param {
            ty,
            num: num as u16,
            block,
            span,
        })
    }

    pub fn is_block_terminated(&self, block: Block) -> bool {
        if let Some(inst) = self.last_inst(block) {
            self.inst(inst).opcode().is_terminator()
        } else {
            false
        }
    }

    /// Removes `val` from `block`'s parameters by a standard linear time list removal which
    /// preserves ordering. Also updates the values' data.
    pub fn remove_block_param(&mut self, val: Value) {
        let (block, num) = if let ValueData::Param { num, block, .. } = self.values[val] {
            (block, num)
        } else {
            panic!("{} must be a block parameter", val);
        };
        self.blocks[block].params.remove(num as usize, &mut self.value_lists);
        for index in num..(self.num_block_params(block) as u16) {
            let value_data = &mut self.values
                [self.blocks[block].params.get(index as usize, &self.value_lists).unwrap()];
            let mut value_data_clone = value_data.clone();
            match &mut value_data_clone {
                ValueData::Param { ref mut num, .. } => {
                    *num -= 1;
                    *value_data = value_data_clone;
                }
                _ => panic!(
                    "{} must be a block parameter",
                    self.blocks[block].params.get(index as usize, &self.value_lists).unwrap()
                ),
            }
        }
    }

    /// Appends `value` as an argument to the `branch_inst` instruction arguments list if the
    /// destination block of the `branch_inst` is `dest`.
    /// Panics if `branch_inst` is not a branch instruction.
    pub fn append_branch_destination_argument(
        &mut self,
        branch_inst: Inst,
        dest: Block,
        value: Value,
    ) {
        match self.insts[branch_inst].data.deref_mut() {
            Instruction::Br(Br {
                ref mut successor, ..
            }) => {
                debug_assert_eq!(successor.destination, dest);
                successor.args.push(value, &mut self.value_lists);
            }
            Instruction::CondBr(CondBr {
                ref mut then_dest,
                ref mut else_dest,
                ..
            }) => {
                if then_dest.destination == dest {
                    then_dest.args.push(value, &mut self.value_lists);
                }
                if else_dest.destination == dest {
                    else_dest.args.push(value, &mut self.value_lists);
                }
            }
            Instruction::Switch(Switch {
                ref mut arms,
                default: ref mut default_succ,
                ..
            }) => {
                if default_succ.destination == dest {
                    default_succ.args.push(value, &mut self.value_lists);
                }
                for arm in arms.iter_mut() {
                    if arm.successor.destination == dest {
                        arm.successor.args.push(value, &mut self.value_lists);
                    }
                }
            }
            _ => panic!("{} must be a branch instruction", branch_inst),
        }
    }

    /// Try to locate a valid definition of `value` in the current block, looking up the block from
    /// `user`
    pub fn nearest_definition_in_block(&self, user: Inst, value: Value) -> Option<Value> {
        let mut cursor = self.block_cursor_at(user);
        // Move to the first instruction preceding this one, or the null cursor if this
        // is the first instruction in its containing block
        cursor.move_prev();

        while let Some(current_inst) = cursor.get() {
            match self.inst(current_inst.key) {
                Instruction::PrimOp(PrimOp {
                    op: Opcode::Reload,
                    args,
                }) => {
                    if args.as_slice(&self.value_lists).contains(&value) {
                        // We have found the closest definition of `value`, which
                        // is a reload from a spill slot
                        return Some(self.first_result(current_inst.key));
                    }
                }
                _ => {
                    if self.inst_results(current_inst.key).contains(&value) {
                        // We have reached the original definition of `value`
                        return Some(value);
                    }
                }
            }

            cursor.move_prev();
        }

        // Search block parameter list
        let current_block = self.inst_block(user).unwrap();
        match self.value_data(value) {
            ValueData::Param { block, .. } if block == &current_block => Some(value),
            _ => None,
        }
    }

    pub fn alloc_local(&mut self, ty: Type) -> LocalId {
        let id = self.locals.next_key();
        self.locals.push(Local { id, ty })
    }

    pub fn local_type(&self, id: LocalId) -> &Type {
        &self.locals[id].ty
    }

    pub fn locals(&self) -> impl Iterator<Item = &Local> + '_ {
        self.locals.values()
    }
}
impl Index<Inst> for DataFlowGraph {
    type Output = Instruction;

    fn index(&self, inst: Inst) -> &Self::Output {
        &self.insts[inst]
    }
}
impl IndexMut<Inst> for DataFlowGraph {
    fn index_mut(&mut self, inst: Inst) -> &mut Self::Output {
        &mut self.insts[inst]
    }
}

struct Blocks<'f> {
    cursor: intrusive_collections::linked_list::Cursor<'f, LayoutAdapter<Block, BlockData>>,
}
impl<'f> Iterator for Blocks<'f> {
    type Item = (Block, &'f BlockData);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.is_null() {
            return None;
        }
        let next = self.cursor.get().map(|data| (data.key(), data.value()));
        self.cursor.move_next();
        next
    }
}
