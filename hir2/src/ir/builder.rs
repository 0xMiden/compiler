use alloc::rc::Rc;

use crate::{
    BlockArgument, BlockRef, BuildableOp, Context, InsertionPoint, OperationRef, ProgramPoint,
    RegionRef, SourceSpan, Type, Value,
};

/// The [Builder] trait encompasses all of the functionality needed to construct and insert blocks
/// and operations into the IR.
pub trait Builder: Listener {
    fn context(&self) -> &Context;
    fn context_rc(&self) -> Rc<Context>;
    /// Returns the current insertion point of the builder
    fn insertion_point(&self) -> Option<&InsertionPoint>;
    /// Clears the current insertion point
    fn clear_insertion_point(&mut self) -> Option<InsertionPoint>;
    /// Restores the current insertion point to `ip`
    fn restore_insertion_point(&mut self, ip: Option<InsertionPoint>);
    /// Sets the current insertion point to `ip`
    fn set_insertion_point(&mut self, ip: InsertionPoint);

    /// Sets the insertion point to the specified program point, causing subsequent insertions to
    /// be placed before it.
    #[inline]
    fn set_insertion_point_before(&mut self, pp: ProgramPoint) {
        self.set_insertion_point(InsertionPoint::before(pp));
    }

    /// Sets the insertion point to the specified program point, causing subsequent insertions to
    /// be placed after it.
    #[inline]
    fn set_insertion_point_after(&mut self, pp: ProgramPoint) {
        self.set_insertion_point(InsertionPoint::after(pp));
    }

    /// Sets the insertion point to the node after the specified value is defined.
    ///
    /// If value has a defining operation, this sets the insertion point after that operation, so
    /// that all insertions are placed following the definition.
    ///
    /// Otherwise, a value must be a block argument, so the insertion point is placed at the start
    /// of the block, causing insertions to be placed starting at the front of the block.
    fn set_insertion_point_after_value(&mut self, value: &dyn Value) {
        let pp = if let Some(op) = value.get_defining_op() {
            ProgramPoint::Op(op)
        } else {
            let block_argument = value.downcast_ref::<BlockArgument>().unwrap();
            ProgramPoint::Block(block_argument.owner())
        };
        self.set_insertion_point_after(pp);
    }

    /// Sets the current insertion point to the start of `block`.
    ///
    /// Operations inserted will be placed starting at the beginning of the block.
    #[inline]
    fn set_insertion_point_to_start(&mut self, block: BlockRef) {
        self.set_insertion_point_before(block.into());
    }

    /// Sets the current insertion point to the end of `block`.
    ///
    /// Operations inserted will be placed starting at the end of the block.
    #[inline]
    fn set_insertion_point_to_end(&mut self, block: BlockRef) {
        self.set_insertion_point_after(block.into());
    }

    /// Return the block the current insertion point belongs to.
    ///
    /// NOTE: The insertion point is not necessarily at the end of the block.
    ///
    /// Returns `None` if the insertion point is unset, or is pointing at an operation which is
    /// detached from a block.
    fn insertion_block(&self) -> Option<BlockRef> {
        self.insertion_point().and_then(|ip| ip.at.block())
    }

    /// Add a new block with `args` arguments, and set the insertion point to the end of it.
    ///
    /// The block is inserted after the provided insertion point `ip`, or at the end of `parent` if
    /// not.
    ///
    /// Panics if `ip` is in a different region than `parent`, or if the position it refers to is no
    /// longer valid.
    fn create_block(&mut self, parent: RegionRef, ip: Option<BlockRef>, args: &[Type]) -> BlockRef {
        let mut block = self.context().create_block_with_params(args.iter().cloned());
        if let Some(at) = ip {
            let region = at.borrow().parent().unwrap();
            assert!(
                RegionRef::ptr_eq(&parent, &region),
                "insertion point region differs from 'parent'"
            );

            block.borrow_mut().insert_after(at);
        } else {
            block.borrow_mut().insert_at_end(parent);
        }

        self.notify_block_inserted(block.clone(), None, None);

        self.set_insertion_point_to_end(block.clone());

        block
    }

    /// Add a new block with `args` arguments, and set the insertion point to the end of it.
    ///
    /// The block is inserted before `before`.
    fn create_block_before(&mut self, before: BlockRef, args: &[Type]) -> BlockRef {
        let mut block = self.context().create_block_with_params(args.iter().cloned());
        block.borrow_mut().insert_before(before);
        self.notify_block_inserted(block.clone(), None, None);
        self.set_insertion_point_to_end(block.clone());
        block
    }

    /// Insert `op` at the current insertion point.
    ///
    /// If the insertion point is inserting after the current operation, then after calling this
    /// function, the insertion point will have been moved to the newly inserted operation. This
    /// ensures that subsequent calls to `insert` will place operations in the block in the same
    /// sequence as they were inserted. The other insertion point placements already have more or
    /// less intuitive behavior, e.g. inserting _before_ the current operation multiple times will
    /// result in operations being placed in the same sequence they were inserted, just before the
    /// current op.
    ///
    /// This function will panic if no insertion point is set.
    fn insert(&mut self, mut op: OperationRef) {
        let ip = self.insertion_point().expect("insertion point is unset").clone();
        match ip.at {
            ProgramPoint::Block(block) => match ip.placement {
                crate::Insert::Before => op.borrow_mut().insert_at_start(block),
                crate::Insert::After => op.borrow_mut().insert_at_end(block),
            },
            ProgramPoint::Op(other_op) => match ip.placement {
                crate::Insert::Before => op.borrow_mut().insert_before(other_op),
                crate::Insert::After => {
                    op.borrow_mut().insert_after(other_op.clone());
                    self.set_insertion_point_after(ProgramPoint::Op(other_op));
                }
            },
        }
        self.notify_operation_inserted(op, None);
    }
}

pub trait BuilderExt: Builder {
    /// Returns a specialized builder for a concrete [Op], `T`, which can be called like a closure
    /// with the arguments required to create an instance of the specified operation.
    ///
    /// # How it works
    ///
    /// The set of arguments which are valid for the specialized builder returned by `create`, are
    /// determined by what implementations of the [BuildableOp] trait exist for `T`. The specific
    /// impl that is chosen will depend on the types of the arguments given to it. Typically, there
    /// should only be one implementation, or if there are multiple, they should not overlap in
    /// ways that may confuse type inference, or you will be forced to specify the full type of the
    /// argument pack.
    ///
    /// This mechanism for constructing ops using arbitrary arguments is essentially a workaround
    /// for the lack of variadic generics in Rust, and isn't quite as nice as what you can acheive
    /// in C++ with varidadic templates and `std::forward` and such, but is close enough so that
    /// the ergonomics are still a significant improvement over the alternative approaches.
    ///
    /// The nice thing about this is that we can generate all of the boilerplate, and hide all of
    /// the sensitive/unsafe parts of initializing operations. Alternative approaches require
    /// exposing more unsafe APIs for use by builders, whereas this approach can conceal those
    /// details within this crate.
    ///
    /// ## Example
    ///
    /// ```text,ignore
    /// // Get an OpBuilder
    /// let builder = context.builder();
    /// // Obtain a builder for AddOp
    /// let add_builder = builder.create::<AddOp, _>(span);
    /// // Consume the builder by creating the op with the given arguments
    /// let add = add_builder(lhs, rhs, Overflow::Wrapping).expect("invalid add op");
    /// ```
    ///
    /// Or, simplified/collapsed:
    ///
    /// ```text,ignore
    /// let builder = context.builder();
    /// let add = builder.create::<AddOp, _>(span)(lhs, rhs, Overflow::Wrapping)
    ///     .expect("invalid add op");
    /// ```
    #[inline(always)]
    fn create<T, Args>(&mut self, span: SourceSpan) -> <T as BuildableOp<Args>>::Builder<'_, Self>
    where
        Args: core::marker::Tuple,
        T: BuildableOp<Args>,
    {
        <T as BuildableOp<Args>>::builder(self, span)
    }
}

pub struct OpBuilder {
    context: Rc<Context>,
    listener: Option<Box<dyn Listener>>,
    ip: Option<InsertionPoint>,
}

impl OpBuilder {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            listener: None,
            ip: None,
        }
    }

    /// Sets the listener of this builder to `listener`
    pub fn with_listener(&mut self, listener: impl Listener) -> &mut Self {
        self.listener = Some(Box::new(listener));
        self
    }
}

impl Listener for OpBuilder {
    fn kind(&self) -> ListenerType {
        self.listener.as_ref().map(|l| l.kind()).unwrap_or(ListenerType::Builder)
    }

    fn notify_block_inserted(
        &mut self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<InsertionPoint>,
    ) {
        if let Some(listener) = self.listener.as_deref_mut() {
            listener.notify_block_inserted(block, prev, ip);
        }
    }

    fn notify_operation_inserted(&mut self, op: OperationRef, prev: Option<InsertionPoint>) {
        if let Some(listener) = self.listener.as_deref_mut() {
            listener.notify_operation_inserted(op, prev);
        }
    }
}

impl Builder for OpBuilder {
    #[inline(always)]
    fn context(&self) -> &Context {
        self.context.as_ref()
    }

    #[inline(always)]
    fn context_rc(&self) -> Rc<Context> {
        self.context.clone()
    }

    #[inline(always)]
    fn insertion_point(&self) -> Option<&InsertionPoint> {
        self.ip.as_ref()
    }

    #[inline]
    fn clear_insertion_point(&mut self) -> Option<InsertionPoint> {
        self.ip.take()
    }

    #[inline]
    fn restore_insertion_point(&mut self, ip: Option<InsertionPoint>) {
        self.ip = ip;
    }

    #[inline(always)]
    fn set_insertion_point(&mut self, ip: InsertionPoint) {
        self.ip = Some(ip);
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ListenerType {
    Builder,
    Rewriter,
}

pub trait Listener: 'static {
    fn kind(&self) -> ListenerType;
    /// Notify the listener that the specified operation was inserted.
    ///
    /// * If the operation was moved, then `prev` is the previous location of the op
    /// * If the operation was unlinked before it was inserted, then `prev` is `None`
    fn notify_operation_inserted(&mut self, op: OperationRef, prev: Option<InsertionPoint>);
    /// Notify the listener that the specified block was inserted.
    ///
    /// * If the block was moved, then `prev` and `ip` represent the previous location of the block.
    /// * If the block was unlinked before it was inserted, then `prev` and `ip` are `None`
    fn notify_block_inserted(
        &mut self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<InsertionPoint>,
    );
}

pub struct InsertionGuard<'a> {
    builder: &'a mut OpBuilder,
    ip: Option<InsertionPoint>,
}
impl<'a> InsertionGuard<'a> {
    #[allow(unused)]
    pub fn new(builder: &'a mut OpBuilder, ip: InsertionPoint) -> Self {
        Self {
            builder,
            ip: Some(ip),
        }
    }
}
impl Drop for InsertionGuard<'_> {
    fn drop(&mut self) {
        self.builder.restore_insertion_point(self.ip.take());
    }
}
