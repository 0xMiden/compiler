use crate::{
    traits::Terminator, AsCallableSymbolRef, AsSymbolRef, AttributeValue, BlockRef, Builder,
    KeyedSuccessor, Op, OpBuilder, OperationRef, Region, Report, Spanned, SuccessorInfo, Type,
    UnsafeIntrusiveEntityRef, ValueRef,
};

/// The [OperationBuilder] is a primitive for imperatively constructing an [Operation].
///
/// Currently, this is primarily used by our `#[operation]` macro infrastructure, to finalize
/// construction of the underlying [Operation] of an [Op] implementation, after both have been
/// allocated and initialized with only basic metadata. This builder is then used to add all of
/// the data under the op, e.g. operands, results, attributes, etc. Once complete, verification is
/// run on the constructed op.
///
/// Using this directly is possible, see [OperationBuilder::new] for details. You may also find it
/// useful to examine the expansion of the `#[operation]` macro for existing ops to understand what goes
/// on behind the scenes for most ops.
pub struct OperationBuilder<'a, T, B: ?Sized = OpBuilder> {
    builder: &'a mut B,
    op: OperationRef,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T, B> OperationBuilder<'a, T, B>
where
    T: Op,
    B: ?Sized + Builder,
{
    /// Create a new [OperationBuilder] for `op` using the provided [Builder].
    ///
    /// The [Operation] underlying `op` must have been initialized correctly:
    ///
    /// * Allocated via the same context as `builder`
    /// * Initialized via [crate::Operation::uninit]
    /// * All op traits implemented by `T` must have been registered with its [OperationName]
    /// * All fields of `T` must have been initialized to actual or default values. This builder
    ///   will invoke verification at the end, and if `T` is not correctly initialized, it will
    ///   result in undefined behavior.
    pub fn new(builder: &'a mut B, op: UnsafeIntrusiveEntityRef<T>) -> Self {
        let op = unsafe { UnsafeIntrusiveEntityRef::from_raw(op.borrow().as_operation()) };
        Self {
            builder,
            op,
            _marker: core::marker::PhantomData,
        }
    }

    /// Set attribute `name` on this op to `value`
    #[inline]
    pub fn with_attr<A>(&mut self, name: &'static str, value: A)
    where
        A: AttributeValue,
    {
        self.op.borrow_mut().set_attribute(name, Some(value));
    }

    /// Set symbol `attr_name` on this op to `symbol`.
    ///
    /// Symbol references are stored as attributes, and have similar semantics to operands, i.e.
    /// they require tracking uses.
    #[inline]
    pub fn with_symbol(&mut self, attr_name: &'static str, symbol: impl AsSymbolRef) {
        self.op.borrow_mut().set_symbol_attribute(attr_name, symbol);
    }

    /// Like [with_symbol], but further constrains the range of valid input symbols to those which
    /// are valid [CallableOpInterface] implementations.
    #[inline]
    pub fn with_callable_symbol(
        &mut self,
        attr_name: &'static str,
        callable: impl AsCallableSymbolRef,
    ) {
        let callable = callable.as_callable_symbol_ref();
        self.op.borrow_mut().set_symbol_attribute(attr_name, callable);
    }

    /// Add a new [Region] to this operation.
    ///
    /// NOTE: You must ensure this is called _after_ [Self::with_operands], and [Self::implements]
    /// if the op implements the [traits::NoRegionArguments] trait. Otherwise, the inserted region
    /// may not be valid for this op.
    pub fn create_region(&mut self) {
        let mut region = Region::default();
        unsafe {
            region.set_owner(Some(self.op.clone()));
        }
        let region = self.builder.context().alloc_tracked(region);
        let mut op = self.op.borrow_mut();
        op.regions.push_back(region);
    }

    pub fn with_successor(
        &mut self,
        dest: BlockRef,
        arguments: impl IntoIterator<Item = ValueRef>,
    ) {
        let owner = self.op.clone();
        // Insert operand group for this successor
        let mut op = self.op.borrow_mut();
        let operand_group =
            op.operands.push_group(arguments.into_iter().enumerate().map(|(index, arg)| {
                self.builder.context().make_operand(arg, owner.clone(), index as u8)
            }));
        // Record SuccessorInfo for this successor in the op
        let succ_index = u8::try_from(op.successors.len()).expect("too many successors");
        let successor = self.builder.context().make_block_operand(dest.clone(), owner, succ_index);
        op.successors.push_group([SuccessorInfo {
            block: successor,
            key: None,
            operand_group: operand_group.try_into().expect("too many operand groups"),
        }]);
    }

    pub fn with_successors<I>(&mut self, succs: I)
    where
        I: IntoIterator<Item = (BlockRef, Vec<ValueRef>)>,
    {
        let owner = self.op.clone();
        let mut op = self.op.borrow_mut();
        let mut group = vec![];
        for (i, (block, args)) in succs.into_iter().enumerate() {
            let block = self.builder.context().make_block_operand(block, owner.clone(), i as u8);
            let operands = args
                .into_iter()
                .map(|value_ref| self.builder.context().make_operand(value_ref, owner.clone(), 0));
            let operand_group = op.operands.push_group(operands);
            group.push(SuccessorInfo {
                block,
                key: None,
                operand_group: operand_group.try_into().expect("too many operand groups"),
            });
        }
        op.successors.push_group(group);
    }

    pub fn with_keyed_successors<I, S>(&mut self, succs: I)
    where
        S: KeyedSuccessor,
        I: IntoIterator<Item = S>,
    {
        let owner = self.op.clone();
        let mut op = self.op.borrow_mut();
        let mut group = vec![];
        for (i, successor) in succs.into_iter().enumerate() {
            let (key, block, args) = successor.into_parts();
            let block = self.builder.context().make_block_operand(block, owner.clone(), i as u8);
            let operands = args
                .into_iter()
                .map(|value_ref| self.builder.context().make_operand(value_ref, owner.clone(), 0));
            let operand_group = op.operands.push_group(operands);
            let key = Box::new(key);
            let key = unsafe { core::ptr::NonNull::new_unchecked(Box::into_raw(key)) };
            group.push(SuccessorInfo {
                block,
                key: Some(key.cast()),
                operand_group: operand_group.try_into().expect("too many operand groups"),
            });
        }
        op.successors.push_group(group);
    }

    /// Append operands to the set of operands given to this op so far.
    pub fn with_operands<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let owner = self.op.clone();
        let operands = operands.into_iter().enumerate().map(|(index, value)| {
            self.builder.context().make_operand(value, owner.clone(), index as u8)
        });
        let mut op = self.op.borrow_mut();
        op.operands.extend(operands);
    }

    /// Append operands to the set of operands in operand group `group`
    pub fn with_operands_in_group<I>(&mut self, group: usize, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let owner = self.op.clone();
        let operands = operands.into_iter().enumerate().map(|(index, value)| {
            self.builder.context().make_operand(value, owner.clone(), index as u8)
        });
        let mut op = self.op.borrow_mut();
        op.operands.extend_group(group, operands);
    }

    /// Allocate `n` results for this op, of unknown type, to be filled in later
    pub fn with_results(&mut self, n: usize) {
        let span = self.op.borrow().span;
        let owner = self.op.clone();
        let results = (0..n).map(|idx| {
            self.builder
                .context()
                .make_result(span, Type::Unknown, owner.clone(), idx as u8)
        });
        let mut op = self.op.borrow_mut();
        op.results.clear();
        op.results.extend(results);
    }

    /// Consume this builder, verify the op, and return a handle to it, or an error if validation
    /// failed.
    pub fn build(mut self) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
        let op = {
            let mut op = self.op.borrow_mut();

            // Infer result types and apply any associated validation
            if let Some(interface) = op.as_trait_mut::<dyn crate::traits::InferTypeOpInterface>() {
                interface.infer_return_types(self.builder.context())?;
            }

            // Verify things that would require negative trait impls
            if !op.implements::<dyn Terminator>() && op.has_successors() {
                return Err(self
                    .builder
                    .context()
                    .session
                    .diagnostics
                    .diagnostic(miden_assembly::diagnostics::Severity::Error)
                    .with_message("invalid operation")
                    .with_primary_label(
                        op.span(),
                        "this operation has successors, but does not implement the 'Terminator' \
                         trait",
                    )
                    .with_help("operations with successors must implement the 'Terminator' trait")
                    .into_report());
            }

            unsafe { UnsafeIntrusiveEntityRef::from_raw(op.container().cast()) }
        };

        // Run op-specific verification
        {
            let op: super::EntityRef<T> = op.borrow();
            //let op = op.borrow();
            op.verify(self.builder.context())?;
        }

        // Insert op at current insertion point
        self.builder.insert(self.op);

        Ok(op)
    }
}
