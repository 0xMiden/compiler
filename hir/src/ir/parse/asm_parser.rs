use alloc::{boxed::Box, rc::Rc};
use core::cell::RefCell;

use crate::{
    Attribute, AttributeRef, BlockArgument, BlockArgumentRef, BlockRef, FxHashMap, Ident, OpResult,
    OperationName, OperationRef, SmallVec, Symbol, SymbolMap, SymbolPath, SymbolTable, Type,
    UnsafeIntrusiveEntityRef, ValueRef, adt::SmallDenseMap, diagnostics::SourceSpan,
    dialects::builtin::attributes::SymbolRefAttr, interner, smallvec,
};

/// A map from a SymbolPath to a range of uses
type SymbolUseMap =
    Rc<RefCell<FxHashMap<SymbolPath, SmallVec<[UnsafeIntrusiveEntityRef<SymbolRefAttr>; 1]>>>>;

/// This struct represents state from a parsed HIR textual format string.
///
/// It is useful for building additional analysis and language utilities on top of textual HIR. This
/// should generally not be used for traditional compilation.
#[derive(Default)]
pub struct AsmParserState {
    /// A mapping from operations in the input source file to their parser state
    operations: SmallVec<[OperationDefinition; 4]>,
    operation_to_idx: SmallDenseMap<OperationRef, usize, 4>,

    /// A mapping from blocks in the input source file to their parser state
    blocks: SmallVec<[BlockDefinition; 4]>,
    block_to_idx: SmallDenseMap<BlockRef, usize, 4>,

    /// A mapping from attribute aliases in the input source file to their parser state
    attr_aliases: SmallVec<[AttributeAliasDefinition; 4]>,
    attr_alias_to_idx: SmallDenseMap<interner::Symbol, usize, 4>,

    /// A mapping from type aliases in the input source file to their parser state
    type_aliases: SmallVec<[TypeAliasDefinition; 4]>,
    type_alias_to_idx: SmallDenseMap<interner::Symbol, usize, 4>,

    /// A set of value definitions that are placeholders for forward references.
    /// This map should be empty if the parser finishes successfully.
    placeholder_value_uses: SmallDenseMap<ValueRef, SmallVec<[SourceSpan; 4]>, 4>,

    /// The symbol table operations within the IR.
    symbol_table_operations: SmallVec<[(OperationRef, SymbolUseMap); 1]>,

    /// A stack of partial operation definitions that have been started but not yet finalized.
    partial_operations: SmallVec<[PartialOpDef; 2]>,

    /// A stack of symbol use scopes.
    ///
    /// This is used when collecting symbol table uses during parsing.
    symbol_use_scopes: SmallVec<[SymbolUseMap; 1]>,

    /// A global map of symbol users and the symbols they use
    symbol_uses: FxHashMap<OperationRef, SmallVec<[UnsafeIntrusiveEntityRef<SymbolRefAttr>; 1]>>,
}

impl AsmParserState {
    /// Initialize the state in preparation for populating more parser state under the given
    /// top-level operation.
    pub fn initialize(&mut self, top_level_op: OperationRef) {
        self.start_operation_definition(&top_level_op.borrow().name());

        // If the top-level operation is a symbol table, push a new symbol scope.
        let partial = self.partial_operations.last().unwrap();
        if partial.is_symbol_table() {
            self.symbol_use_scopes.push(partial.symbol_uses.clone());
        }
    }

    /// Finalize any in-progress parser state under the given top-level operation.
    pub fn finalize(&mut self, top_level_op: OperationRef) {
        let PartialOpDef {
            symbol_uses: symbol_table,
            is_symbol_table,
        } = self
            .partial_operations
            .pop()
            .expect("expected valid partial operation definition");

        // If this operation is a symbol table, resolve any symbol uses.
        if is_symbol_table {
            self.symbol_table_operations.push((top_level_op, symbol_table));
        }

        self.resolve_symbol_uses();
    }

    /// Start a definition for an operation with the given name.
    pub fn start_operation_definition(&mut self, name: &OperationName) {
        self.partial_operations.push(PartialOpDef::new(name));
    }

    /// Finalize the most recently started operation definition.
    pub fn finalize_operation_definition(
        &mut self,
        mut op: OperationRef,
        at: SourceSpan,
        end: SourceSpan,
        result_groups: &[(usize, SourceSpan)],
    ) {
        let PartialOpDef {
            symbol_uses,
            is_symbol_table,
        } = self
            .partial_operations
            .pop()
            .expect("expected valid partial operation definition");

        // Build the full operation definition.
        let mut def = OperationDefinition::new(op, at, end);
        for (start, span) in result_groups {
            def.result_groups.push(ResultGroupDefinition::new(*span, *start));
        }

        self.operation_to_idx.insert(op, self.operations.len());
        self.operations.push(def);

        // If this operation is a symbol table, resolve any symbol uses.
        if is_symbol_table {
            // Populate symbol table first
            {
                let mut op = op.borrow_mut();
                let symbol_table = SymbolMap::build(&op);
                if let Some(op_symbol_table) = op.as_trait_mut::<dyn SymbolTable>() {
                    **op_symbol_table.symbol_manager_mut().symbols_mut() = symbol_table;
                }
            }
            self.symbol_table_operations.push((op, symbol_uses));
        } else {
            let mut symbol_uses = symbol_uses.borrow_mut();
            self.symbol_uses
                .entry(op)
                .or_default()
                .extend(symbol_uses.drain().flat_map(|(_, uses)| uses));
        }
    }

    /// Start a definition for a region nested under the current operation.
    pub fn start_region_definition(&mut self) {
        let PartialOpDef {
            symbol_uses: symbol_table,
            is_symbol_table,
        } = self
            .partial_operations
            .last()
            .expect("expected valid partial operation definition");

        // If the parent operation of this region is a symbol table, we also push a new symbol
        // scope.
        if *is_symbol_table {
            self.symbol_use_scopes.push(Rc::clone(symbol_table));
        }
    }

    /// Finalize the most recently started region definition.
    pub fn finalize_region_definition(&mut self) {
        let PartialOpDef {
            symbol_uses: symbol_table,
            is_symbol_table,
        } = self
            .partial_operations
            .last()
            .expect("expected valid partial operation definition");

        // If the parent operation of this region is a symbol table, pop the symbol scope for this
        // region.
        if *is_symbol_table {
            self.symbol_use_scopes.pop();
        }
    }

    /// Add a definition of the given block.
    pub fn add_block_definition(&mut self, block: BlockRef, span: SourceSpan) {
        use crate::adt::smallmap::Entry;

        match self.block_to_idx.entry(block) {
            Entry::Vacant(entry) => {
                entry.insert(self.blocks.len());
                self.blocks.push(BlockDefinition::new(block, span));
            }
            Entry::Occupied(entry) => {
                // If an entry already exists, this was a forward declaration that now has a proper
                // definition.
                self.blocks[*entry.get()].definition.span = span;
            }
        }
    }

    /// Add a definition of the given block argument.
    pub fn add_block_argument_definition(&mut self, arg: BlockArgumentRef, span: SourceSpan) {
        let block_arg = arg.borrow();
        let block = block_arg.owner();
        let index = *self.block_to_idx.get(&block).expect("expected owner block to have an entry");
        let def = &mut self.blocks[index];
        let arg_index = block_arg.index();

        if def.arguments.len() <= arg_index {
            def.arguments.resize(arg_index + 1, SourceDefinition::new(SourceSpan::UNKNOWN));
        }
        def.arguments[arg_index] = SourceDefinition::new(span);
    }

    /// Add a definition of the given attribute alias.
    pub fn add_attr_alias_definition(
        &mut self,
        name: interner::Symbol,
        span: SourceSpan,
        value: Option<AttributeRef>,
    ) {
        use crate::adt::smallmap::Entry;

        // Location aliases may be referenced before they are defined.
        match self.attr_alias_to_idx.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(self.attr_aliases.len());
                self.attr_aliases.push(AttributeAliasDefinition::new(name, span, value));
            }
            Entry::Occupied(entry) => {
                let mut attr = &mut self.attr_aliases[*entry.get()];
                attr.definition.span = span;
                attr.value = value;
            }
        }
    }

    /// Add a definition of the given type alias.
    pub fn add_type_alias_definition(
        &mut self,
        name: interner::Symbol,
        span: SourceSpan,
        value: Type,
    ) {
        assert!(
            self.type_alias_to_idx.insert_new(name, self.type_aliases.len()),
            "unexpected type alias redefinition"
        );
        self.type_aliases.push(TypeAliasDefinition::new(name, span, value));
    }

    /// Add a source uses of the given value.
    pub fn add_uses(&mut self, value: ValueRef, locations: &[SourceSpan]) {
        // Handle the case where the value is an operation result.
        let val = value.borrow();
        if let Some(result) = val.downcast_ref::<OpResult>() {
            // Check to see if a definition for the parent operation has been recorded.
            // If one hasn't, we treat the provided value as a placeholder value that will be
            // refined further later.
            let parent_op = result.owner();
            let Some(existing) = self.operation_to_idx.get(&parent_op) else {
                self.placeholder_value_uses[&value].extend_from_slice(locations);
                return;
            };

            // If a definition does exist, locate the value's result group and add the use. The
            // result groups are ordered by increasing start index, so we just need to find the last
            // group that has a smaller/equal start index.
            let result_index = result.index();
            let def = &mut self.operations[*existing];
            let result_group = def
                .result_groups
                .iter_mut()
                .rev()
                .find(|group| result_index >= group.start)
                .expect("expected valid result group for value use");
            result_group.definition.uses.extend_from_slice(locations);
        } else {
            // Otherwise, this is a block argument.
            let arg = val.downcast_ref::<BlockArgument>().unwrap();
            let existing = self
                .block_to_idx
                .get(&arg.owner())
                .expect("expected valid block definition for block argument");
            let def = &mut self.blocks[*existing];
            def.arguments[arg.index()].uses.extend_from_slice(locations);
        }
    }

    /// Add a source uses of the given block.
    pub fn add_block_uses(&mut self, block: BlockRef, locations: &[SourceSpan]) {
        use crate::adt::smallmap::Entry;

        match self.block_to_idx.entry(block) {
            Entry::Vacant(entry) => {
                entry.insert(self.blocks.len());
                let mut def = BlockDefinition::new(block, SourceSpan::UNKNOWN);
                def.definition.uses.extend_from_slice(locations);
                self.blocks.push(def);
            }
            Entry::Occupied(entry) => {
                self.blocks[*entry.get()].definition.uses.extend_from_slice(locations);
            }
        }
    }

    /// Add a source uses of the given attribute alias.
    pub fn add_attr_alias_uses(&mut self, name: interner::Symbol, locations: &[SourceSpan]) {
        use crate::adt::smallmap::Entry;

        match self.attr_alias_to_idx.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(self.attr_aliases.len());
                let mut def = AttributeAliasDefinition::new(name, SourceSpan::UNKNOWN, None);
                def.definition.uses.extend_from_slice(locations);
                self.attr_aliases.push(def);
            }
            Entry::Occupied(entry) => {
                self.attr_aliases[*entry.get()].definition.uses.extend_from_slice(locations);
            }
        }
    }

    /// Add a source uses of the given type alias.
    pub fn add_type_alias_uses(&mut self, name: interner::Symbol, locations: &[SourceSpan]) {
        let index =
            self.type_alias_to_idx.get(&name).expect("expected valid type alias definition");
        self.type_aliases[*index].definition.uses.extend_from_slice(locations);
    }

    /// Register that a symbol `path` was used by the current operation via `attr` at `loc`
    pub fn add_symbol_use(
        &mut self,
        path: &SymbolPath,
        attr: UnsafeIntrusiveEntityRef<SymbolRefAttr>,
        _loc: SourceSpan,
    ) {
        // Ignore this symbol if no scopes are active.
        if self.symbol_use_scopes.is_empty() {
            return;
        }

        self.symbol_use_scopes
            .last_mut()
            .unwrap()
            .borrow_mut()
            .entry(path.clone())
            .or_default()
            .push(attr);
    }

    /// Refine `old` to `new`.
    ///
    /// This is used to indicate that `old` was a placeholder, and the uses of it should really
    /// refer to `new`.
    pub fn refine_definition(&mut self, old: ValueRef, new: ValueRef) {
        let uses = self
            .placeholder_value_uses
            .remove(&old)
            .expect("expected `old` to be a placeholder");
        self.add_uses(new, &uses);
    }
}

/// Accessors
impl AsmParserState {
    pub fn block_defs(&self) -> &[BlockDefinition] {
        &self.blocks
    }

    pub fn get_block_def(&self, block: BlockRef) -> Option<&BlockDefinition> {
        let index = self.block_to_idx.get(&block).copied()?;
        Some(&self.blocks[index])
    }

    pub fn op_defs(&self) -> &[OperationDefinition] {
        &self.operations
    }

    pub fn get_op_def(&self, op: OperationRef) -> Option<&OperationDefinition> {
        let index = self.operation_to_idx.get(&op).copied()?;
        Some(&self.operations[index])
    }

    pub fn attribute_alias_defs(&self) -> &[AttributeAliasDefinition] {
        &self.attr_aliases
    }

    pub fn get_attribute_alias_def(
        &self,
        alias: interner::Symbol,
    ) -> Option<&AttributeAliasDefinition> {
        let index = self.attr_alias_to_idx.get(&alias).copied()?;
        Some(&self.attr_aliases[index])
    }

    pub fn type_alias_defs(&self) -> &[TypeAliasDefinition] {
        &self.type_aliases
    }

    pub fn get_type_alias_def(&self, alias: interner::Symbol) -> Option<&TypeAliasDefinition> {
        let index = self.type_alias_to_idx.get(&alias).copied()?;
        Some(&self.type_aliases[index])
    }

    /// Resolve any symbol table uses in the IR
    pub fn resolve_symbol_uses(&mut self) {
        let mut symbol_ops = SmallVec::<[OperationRef; 4]>::new_const();
        let root_symbol_table = self.symbol_table_operations.last().map(|(op, _)| *op);
        for (op, symbol_uses) in self.symbol_table_operations.iter() {
            let operation = op.borrow();
            let context = operation.context_rc();
            let root_operation = root_symbol_table.unwrap();
            let symbol_uses = symbol_uses.borrow();
            for (path, uses) in symbol_uses.iter() {
                let Some(symbol_op) =
                    (if path.is_absolute() && !OperationRef::ptr_eq(op, &root_operation) {
                        let mut rst = root_operation.borrow();
                        let symbol_table = rst.as_symbol_table().unwrap();
                        symbol_table.symbol_manager().lookup_symbol_ref(path)
                    } else {
                        let symbol_table = operation.as_symbol_table().unwrap();
                        symbol_table.symbol_manager().lookup_symbol_ref(path)
                    })
                else {
                    continue;
                };

                for mut user in uses.iter().copied() {
                    let used = symbol_op
                        .borrow()
                        .as_symbol_ref()
                        .expect("resolved symbol references must point to symbols");
                    let symbol_use = context.alloc_tracked(crate::SymbolUse {
                        owner: *op,
                        attr: user,
                        used: Some(used),
                    });
                    user.borrow_mut().set_user(symbol_use);
                    if let Some(index) = self.operation_to_idx.get(&symbol_op).copied() {
                        self.operations[index].symbol_uses.push(symbol_use);
                    }
                }
            }
        }

        for (user, uses) in self.symbol_uses.drain() {
            let (context, nearest_symbol_table) = {
                let user_op = user.borrow();
                let context = user_op.context_rc();
                let symbol_table = match user_op.nearest_symbol_table() {
                    Some(symbol_table) => symbol_table,
                    None => {
                        if user_op.implements::<dyn SymbolTable>() {
                            user
                        } else {
                            continue;
                        }
                    }
                };
                (context, symbol_table)
            };
            for mut using_attr in uses {
                let path = using_attr.borrow().path().clone();
                let symbol_table = nearest_symbol_table.borrow();
                let Some(resolved) = symbol_table
                    .as_symbol_table()
                    .unwrap()
                    .symbol_manager()
                    .lookup_symbol_ref(&path)
                else {
                    continue;
                };

                let used = resolved
                    .borrow()
                    .as_symbol_ref()
                    .expect("resolved symbol references must point to symbols");
                let symbol_use = context.alloc_tracked(crate::SymbolUse {
                    owner: user,
                    attr: using_attr,
                    used: Some(used),
                });
                using_attr.borrow_mut().set_user(symbol_use);
                if let Some(index) = self.operation_to_idx.get(&resolved).copied() {
                    self.operations[index].symbol_uses.push(symbol_use);
                }
            }
        }

        for definition in self.operations.iter() {
            if definition.symbol_uses.is_empty() {
                continue;
            }

            let mut op = definition.op;
            let mut op_mut = op.borrow_mut();
            let mut symbol = op_mut.as_trait_mut::<dyn Symbol>().unwrap();
            for user in definition.symbol_uses.iter().copied() {
                symbol.insert_use(user);
            }
        }
    }
}

struct PartialOpDef {
    /// If this operation is a symbol table, this map contains symbol uses within the operation
    symbol_uses: SymbolUseMap,
    is_symbol_table: bool,
}

impl PartialOpDef {
    pub fn new(name: &OperationName) -> Self {
        let is_symbol_table = name.implements::<dyn SymbolTable>();
        Self {
            symbol_uses: Default::default(),
            is_symbol_table,
        }
    }

    #[inline]
    pub const fn is_symbol_table(&self) -> bool {
        self.is_symbol_table
    }
}

/// This struct represents a definition within the source manager, containing it's defining location
/// and locations of any uses.
///
/// SourceDefinitions are only provided for entities that have uses within an input file, e.g. SSA
/// values, blocks, and symbols.
#[derive(Clone)]
pub struct SourceDefinition {
    pub span: SourceSpan,
    pub uses: SmallVec<[SourceSpan; 2]>,
}

impl SourceDefinition {
    pub fn new(span: SourceSpan) -> Self {
        Self {
            span,
            uses: SmallVec::new_const(),
        }
    }
}

/// This struct represents the information for an operation definition within an input file.
pub struct OperationDefinition {
    /// The operation representing this definition
    pub op: OperationRef,
    /// The source location of the start of the operation definition, i.e. the location of its name
    pub at: SourceSpan,
    /// The full source span of the operation definition
    pub span: SourceSpan,
    /// Source definitions for any result groups of this operation
    pub result_groups: SmallVec<[ResultGroupDefinition; 1]>,
    /// The uses of this operation as a symbol, if it is a symbol operation
    pub symbol_uses: SmallVec<[crate::SymbolUseRef; 1]>,
}

impl OperationDefinition {
    pub fn new(op: OperationRef, at: SourceSpan, end: SourceSpan) -> Self {
        Self {
            op,
            at,
            span: SourceSpan::new(at.source_id(), at.start()..end.end()),
            result_groups: SmallVec::new_const(),
            symbol_uses: SmallVec::new_const(),
        }
    }
}

pub struct ResultGroupDefinition {
    /// The result index that starts this group
    pub start: usize,
    /// The source definition of the result group
    pub definition: SourceDefinition,
}

impl ResultGroupDefinition {
    pub fn new(span: SourceSpan, start: usize) -> Self {
        Self {
            start,
            definition: SourceDefinition::new(span),
        }
    }
}

pub struct BlockDefinition {
    /// The block representing this definition
    pub block: BlockRef,
    /// The source location for the block, i.e. the location of its name and its uses
    pub definition: SourceDefinition,
    /// Source definitions for any arguments of this block
    pub arguments: SmallVec<[SourceDefinition; 1]>,
}

impl BlockDefinition {
    pub fn new(block: BlockRef, span: SourceSpan) -> Self {
        Self {
            block,
            definition: SourceDefinition::new(span),
            arguments: SmallVec::new_const(),
        }
    }
}

pub struct AttributeAliasDefinition {
    /// The name of the attribute alias
    pub name: interner::Symbol,
    /// The source location of the alias
    pub definition: SourceDefinition,
    /// The value of the alias
    pub value: Option<AttributeRef>,
}

impl AttributeAliasDefinition {
    pub fn new(name: interner::Symbol, span: SourceSpan, value: Option<AttributeRef>) -> Self {
        Self {
            name,
            definition: SourceDefinition::new(span),
            value,
        }
    }
}

pub struct TypeAliasDefinition {
    /// The name of the type alias
    pub name: interner::Symbol,
    /// The source location of the alias
    pub definition: SourceDefinition,
    /// The value of the alias
    pub value: Type,
}

impl TypeAliasDefinition {
    pub fn new(name: interner::Symbol, span: SourceSpan, value: Type) -> Self {
        Self {
            name,
            definition: SourceDefinition::new(span),
            value,
        }
    }
}
