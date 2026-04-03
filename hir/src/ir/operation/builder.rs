use alloc::{rc::Rc, vec, vec::Vec};

use midenc_session::diagnostics::Severity;

use super::state::PendingSuccessorInfo;
use crate::{
    AsCallableSymbolRef, AsSymbolRef, AttributeRef, AttributeRegistration, BlockRef, Builder,
    KeyedSuccessor, Op, OpBuilder, OperationRef, Report, Spanned, SuccessorInfo, Type,
    UnsafeIntrusiveEntityRef, ValueRef, attributes::IntoAttributeRef, interner, traits::Terminator,
};

/// This is the type-erased version of [OperationBuilder].
pub struct GenericOperationBuilder<'a, B: ?Sized = OpBuilder> {
    builder: &'a mut B,
    op: OperationRef,
}

impl<'a, B> GenericOperationBuilder<'a, B>
where
    B: ?Sized + Builder,
{
    /// Create a new [OperationBuilder] for `op` using the provided [Builder].
    ///
    /// The [super::Operation] underlying `op` must have been initialized correctly:
    ///
    /// * Allocated via the same context as `builder`
    /// * Initialized via [crate::Operation::uninit]
    /// * All op traits implemented by the concrete type must have been registered with its
    ///   [super::OperationName]
    /// * All fields of the concrete type must have been initialized to actual or default
    ///   values. This builder will invoke verification at the end, and if `T` is not correctly
    ///   initialized, it will result in undefined behavior.
    pub fn new(builder: &'a mut B, op: OperationRef) -> Self {
        Self { builder, op }
    }

    #[inline]
    pub fn context(&self) -> &crate::Context {
        self.builder.context()
    }

    #[inline]
    pub fn context_rc(&self) -> Rc<crate::Context> {
        self.builder.context_rc()
    }

    /// Set attribute `name` on this op to `value`
    #[inline]
    pub fn with_attr<A, V>(&mut self, name: impl Into<interner::Symbol>, value: V)
    where
        A: AttributeRegistration,
        <A as AttributeRegistration>::Value: From<V>,
    {
        let attr = self.context_rc().create_attribute::<A, V>(value);
        self.op.borrow_mut().set_attribute(name.into(), attr.as_attribute_ref());
    }

    /// Set attribute `name` on this op to `value`
    #[inline]
    pub fn with_attr_boxed(&mut self, name: impl Into<interner::Symbol>, attr: AttributeRef) {
        self.op.borrow_mut().set_attribute(name.into(), attr)
    }

    /// Set attribute `name` on this op to `value`
    #[inline]
    pub fn with_property<A, V>(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: V,
    ) -> Result<(), Report>
    where
        A: AttributeRegistration,
        <A as AttributeRegistration>::Value: From<V>,
    {
        let attr = self.context_rc().create_attribute::<A, V>(value);
        self.op.borrow_mut().set_property(name.into(), attr.into_attribute_ref())
    }

    /// Set attribute `name` on this op to `value`
    #[inline]
    pub fn with_property_boxed(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: AttributeRef,
    ) -> Result<(), Report> {
        self.op.borrow_mut().set_property(name.into(), value)
    }

    /// Set symbol `attr_name` on this op to `symbol`.
    ///
    /// Symbol references are stored as attributes, and have similar semantics to operands, i.e.
    /// they require tracking uses.
    #[inline]
    pub fn with_symbol(
        &mut self,
        attr_name: impl Into<interner::Symbol>,
        symbol: impl AsSymbolRef,
    ) {
        let mut op = self.op.borrow_mut();
        let attr_name = attr_name.into();
        if op.has_property(attr_name) {
            op.unsafe_set_symbol_property(attr_name, symbol);
        } else {
            op.set_symbol_attribute(attr_name, symbol);
        }
    }

    /// Like [Self::with_symbol], but further constrains the range of valid input symbols to those
    /// which are valid [crate::CallableOpInterface] implementations.
    #[inline]
    pub fn with_callable_symbol(
        &mut self,
        attr_name: impl Into<interner::Symbol>,
        callable: impl AsCallableSymbolRef,
    ) {
        let callable = callable.as_callable_symbol_ref();
        let mut op = self.op.borrow_mut();
        let attr_name = attr_name.into();
        if op.has_property(attr_name) {
            op.unsafe_set_symbol_property(attr_name, callable);
        } else {
            op.set_symbol_attribute(attr_name, callable);
        }
    }

    /// Add a new [crate::Region] to this operation.
    ///
    /// NOTE: You must ensure this is called _after_ [Self::with_operands], if the op implements the
    /// [crate::traits::NoRegionArguments] trait. Otherwise, the inserted region may not be valid
    /// for this op.
    pub fn create_region(&mut self) {
        let region = self.builder.context().create_region();
        let mut op = self.op.borrow_mut();
        op.regions.push_back(region);
    }

    // TODO: This needs to replicate the behavior of with_(keyed_)successor
    pub fn with_pending_successor(&mut self, succ: PendingSuccessorInfo) {
        let owner = self.op;
        let mut op = self.op.borrow_mut();
        // Record SuccessorInfo for this successor in the op
        let succ_index = u8::try_from(op.successors.len()).expect("too many successors");
        let successor = self.builder.context().make_block_operand(succ.block, owner, succ_index);
        op.successors.push(SuccessorInfo {
            block: successor,
            key: succ.key,
            operand_group: succ.operand_group,
        });
    }

    pub fn with_successor(
        &mut self,
        dest: BlockRef,
        arguments: impl IntoIterator<Item = ValueRef>,
    ) {
        let owner = self.op;
        // Insert operand group for this successor
        let mut op = self.op.borrow_mut();
        let operand_group = op.operands.push_group(
            arguments
                .into_iter()
                .enumerate()
                .map(|(index, arg)| self.builder.context().make_operand(arg, owner, index as u8)),
        );
        // Record SuccessorInfo for this successor in the op
        let succ_index = u8::try_from(op.successors.len()).expect("too many successors");
        let successor = self.builder.context().make_block_operand(dest, owner, succ_index);
        op.successors.push(SuccessorInfo {
            block: successor,
            key: None,
            operand_group: operand_group.try_into().expect("too many operand groups"),
        });
    }

    pub fn with_successors<I>(&mut self, succs: I)
    where
        I: IntoIterator<Item = (BlockRef, Vec<ValueRef>)>,
    {
        let owner = self.op;
        let mut op = self.op.borrow_mut();
        let mut group = vec![];
        for (i, (block, args)) in succs.into_iter().enumerate() {
            let block = self.builder.context().make_block_operand(block, owner, i as u8);
            let operands = args
                .into_iter()
                .map(|value_ref| self.builder.context().make_operand(value_ref, owner, 0));
            let operand_group = op.operands.push_group(operands);
            group.push(SuccessorInfo {
                block,
                key: None,
                operand_group: operand_group.try_into().expect("too many operand groups"),
            });
        }
        if op.successors.is_empty() {
            // Extend the empty default group
            op.successors.extend_group(0, group);
        } else {
            // Create new group
            op.successors.push_group(group);
        }
    }

    pub fn with_keyed_successors<I, S>(&mut self, succs: I)
    where
        S: KeyedSuccessor,
        I: IntoIterator<Item = S>,
    {
        let owner = self.op;
        let mut op = self.op.borrow_mut();
        let mut group = vec![];
        for (i, successor) in succs.into_iter().enumerate() {
            let (key, block, args) = successor.into_parts();
            let block = self.builder.context().make_block_operand(block, owner, i as u8);
            let operands = args
                .into_iter()
                .map(|value_ref| self.builder.context().make_operand(value_ref, owner, 0));
            let operand_group = op.operands.push_group(operands);
            group.push(SuccessorInfo {
                block,
                key: Some(key as AttributeRef),
                operand_group: operand_group.try_into().expect("too many operand groups"),
            });
        }
        if op.successors.is_empty() {
            // Extend the empty default group
            op.successors.extend_group(0, group);
        } else {
            // Create new group
            op.successors.push_group(group);
        }
    }

    /// Append operands to the set of operands given to this op so far.
    pub fn with_operands<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let owner = self.op;
        let operands = operands
            .into_iter()
            .enumerate()
            .map(|(index, value)| self.builder.context().make_operand(value, owner, index as u8));
        let mut op = self.op.borrow_mut();
        op.operands.extend(operands);
    }

    /// Append operands to the set of operands in operand group `group`
    pub fn with_operands_in_group<I>(&mut self, group: usize, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let owner = self.op;
        let operands = operands
            .into_iter()
            .enumerate()
            .map(|(index, value)| self.builder.context().make_operand(value, owner, index as u8));
        let mut op = self.op.borrow_mut();
        op.operands.extend_group(group, operands);
    }

    /// Allocate results for this op, with the provided types
    pub fn with_results(&mut self, types: impl IntoIterator<Item = Type>) {
        let span = self.op.borrow().span;
        let owner = self.op;
        let results = types
            .into_iter()
            .enumerate()
            .map(|(idx, ty)| self.builder.context().make_result(span, ty, owner, idx as u8));
        let mut op = self.op.borrow_mut();
        op.results.clear();
        op.results.extend(results);
    }

    /// Allocate a result for this op, with the provided type
    pub fn with_result(&mut self, ty: Type) {
        let span = self.op.borrow().span;
        let owner = self.op;
        let index = { self.op.borrow().num_results() };
        let result = self.builder.context().make_result(span, ty, owner, index as u8);
        let mut op = self.op.borrow_mut();
        op.results.push(result);
    }

    /// Consume this builder, verify the op, and return a handle to it, or an error if validation
    /// failed.
    pub fn build(mut self) -> Result<OperationRef, Report> {
        {
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
                    .session()
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message(::alloc::format!("invalid operation {}", op.name()))
                    .with_primary_label(
                        op.span(),
                        "this operation has successors, but does not implement the 'Terminator' \
                         trait",
                    )
                    .with_help("operations with successors must implement the 'Terminator' trait")
                    .into_report());
            }
        }

        // Insert op at current insertion point, if set
        if self.builder.insertion_point().is_valid() {
            self.builder.insert(self.op);
        }

        Ok(self.op)
    }
}

/// The [OperationBuilder] is a primitive for imperatively constructing an [super::Operation].
///
/// Currently, this is primarily used by our `#[operation]` macro infrastructure, to finalize
/// construction of the underlying [super::Operation] of an [Op] implementation, after both have
/// been allocated and initialized with only basic metadata. This builder is then used to add all of
/// the data under the op, e.g. operands, results, attributes, etc. Once complete, verification is
/// run on the constructed op.
///
/// Using this directly is possible, see [OperationBuilder::new] for details. You may also find it
/// useful to examine the expansion of the `#[operation]` macro for existing ops to understand what goes
/// on behind the scenes for most ops.
pub struct OperationBuilder<'a, T, B: ?Sized = OpBuilder> {
    builder: GenericOperationBuilder<'a, B>,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T, B> OperationBuilder<'a, T, B>
where
    T: Op,
    B: ?Sized + Builder,
{
    /// Create a new [OperationBuilder] for `op` using the provided [Builder].
    ///
    /// The [super::Operation] underlying `op` must have been initialized correctly:
    ///
    /// * Allocated via the same context as `builder`
    /// * Initialized via [crate::Operation::uninit]
    /// * All op traits implemented by `T` must have been registered with its [super::OperationName]
    /// * All fields of `T` must have been initialized to actual or default values. This builder
    ///   will invoke verification at the end, and if `T` is not correctly initialized, it will
    ///   result in undefined behavior.
    pub fn new(builder: &'a mut B, op: UnsafeIntrusiveEntityRef<T>) -> Self {
        let op = op.as_operation_ref();
        Self {
            builder: GenericOperationBuilder::new(builder, op),
            _marker: core::marker::PhantomData,
        }
    }

    /// Consume this builder, verify the op, and return a handle to it, or an error if validation
    /// failed.
    pub fn build(self) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
        let op = self.builder.build()?;
        match op.try_downcast_op::<T>() {
            Ok(op) => Ok(op),
            Err(_) => unreachable!("operation builder produced the wrong operation type"),
        }
    }
}

impl<'a, T, B> core::ops::Deref for OperationBuilder<'a, T, B>
where
    T: Op,
    B: ?Sized + Builder,
{
    type Target = GenericOperationBuilder<'a, B>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl<'a, T, B> core::ops::DerefMut for OperationBuilder<'a, T, B>
where
    T: Op,
    B: ?Sized + Builder,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.builder
    }
}
