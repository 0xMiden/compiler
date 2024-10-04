use alloc::{collections::BTreeMap, rc::Rc};
use core::{
    cell::{Cell, RefCell},
    mem::MaybeUninit,
};

use blink_alloc::Blink;
use midenc_session::Session;

use super::*;

/// Represents the shared state of the IR, used during a compilation session.
///
/// The primary purpose(s) of the context are:
///
/// * Provide storage/memory for all allocated IR entities for the lifetime of the session.
/// * Provide unique value and block identifiers for printing the IR
/// * Provide a uniqued constant pool
/// * Provide configuration used during compilation
///
/// # Safety
///
/// The [Context] _must_ live as long as any reference to an IR entity may be dereferenced.
pub struct Context {
    pub session: Rc<Session>,
    allocator: Rc<Blink>,
    registered_dialects: RefCell<BTreeMap<DialectName, Rc<dyn Dialect>>>,
    next_block_id: Cell<u32>,
    next_value_id: Cell<u32>,
    //pub constants: ConstantPool,
}

impl Default for Context {
    fn default() -> Self {
        use alloc::sync::Arc;

        use midenc_session::diagnostics::DefaultSourceManager;

        let target_dir = std::env::current_dir().unwrap();
        let options = midenc_session::Options::default();
        let source_manager = Arc::new(DefaultSourceManager::default());
        let session =
            Rc::new(Session::new([], None, None, target_dir, options, None, source_manager));
        Self::new(session)
    }
}

impl Context {
    /// Create a new [Context] for the given [Session]
    pub fn new(session: Rc<Session>) -> Self {
        let allocator = Rc::new(Blink::new());
        Self {
            session,
            allocator,
            registered_dialects: Default::default(),
            next_block_id: Cell::new(0),
            next_value_id: Cell::new(0),
            //constants: Default::default(),
        }
    }

    pub fn registered_dialects(
        &self,
    ) -> core::cell::Ref<'_, BTreeMap<DialectName, Rc<dyn Dialect>>> {
        self.registered_dialects.borrow()
    }

    pub fn get_or_register_dialect<T: DialectRegistration>(&self) -> Rc<dyn Dialect> {
        use alloc::collections::btree_map::Entry;

        let mut registered_dialects = self.registered_dialects.borrow_mut();
        let dialect_name = DialectName::new(T::NAMESPACE);
        match registered_dialects.entry(dialect_name) {
            Entry::Occupied(entry) => Rc::clone(entry.get()),
            Entry::Vacant(entry) => {
                let dialect = Rc::new(T::init()) as Rc<dyn Dialect>;
                entry.insert(Rc::clone(&dialect));
                dialect
            }
        }
    }

    /// Get a new [OpBuilder] for this context
    pub fn builder(self: Rc<Self>) -> OpBuilder {
        OpBuilder::new(Rc::clone(&self))
    }

    /// Create a new, detached and empty [Block] with no parameters
    pub fn create_block(&self) -> BlockRef {
        let block = Block::new(self.alloc_block_id());
        self.alloc_tracked(block)
    }

    /// Create a new, detached and empty [Block], with parameters corresponding to the given types
    pub fn create_block_with_params<I>(&self, tys: I) -> BlockRef
    where
        I: IntoIterator<Item = Type>,
    {
        let block = Block::new(self.alloc_block_id());
        let mut block = self.alloc_tracked(block);
        let owner = block.clone();
        let args = tys.into_iter().enumerate().map(|(index, ty)| {
            let id = self.alloc_value_id();
            let arg = BlockArgument::new(
                SourceSpan::default(),
                id,
                ty,
                owner.clone(),
                index.try_into().expect("too many block arguments"),
            );
            self.alloc(arg)
        });
        block.borrow_mut().arguments_mut().extend(args);
        block
    }

    /// Append a new [BlockArgument] to `block`, with the given type and source location
    ///
    /// Returns the block argument as a `dyn Value` reference
    pub fn append_block_argument(
        &self,
        mut block: BlockRef,
        ty: Type,
        span: SourceSpan,
    ) -> ValueRef {
        let next_index = block.borrow().num_arguments();
        let id = self.alloc_value_id();
        let arg = BlockArgument::new(
            span,
            id,
            ty,
            block.clone(),
            next_index.try_into().expect("too many block arguments"),
        );
        let arg = self.alloc(arg);
        block.borrow_mut().arguments_mut().push(arg.clone());
        arg.upcast()
    }

    /// Create a new [OpOperand] with the given value, owner, and index.
    ///
    /// NOTE: This inserts the operand as a user of `value`, but does _not_ add the operand to
    /// `owner`'s operand storage, the caller is expected to do that. This makes this function a
    /// more useful primitive.
    pub fn make_operand(&self, mut value: ValueRef, owner: OperationRef, index: u8) -> OpOperand {
        let op_operand = self.alloc_tracked(OpOperandImpl::new(value.clone(), owner, index));
        let mut value = value.borrow_mut();
        value.insert_use(op_operand.clone());
        op_operand
    }

    /// Create a new [BlockOperand] with the given block, owner, and index.
    ///
    /// NOTE: This inserts the block operand as a user of `block`, but does _not_ add the block
    /// operand to `owner`'s successor storage, the caller is expected to do that. This makes this
    /// function a more useful primitive.
    pub fn make_block_operand(
        &self,
        mut block: BlockRef,
        owner: OperationRef,
        index: u8,
    ) -> BlockOperandRef {
        let block_operand = self.alloc_tracked(BlockOperand::new(block.clone(), owner, index));
        let mut block = block.borrow_mut();
        block.insert_use(block_operand.clone());
        block_operand
    }

    /// Create a new [OpResult] with the given type, owner, and index
    ///
    /// NOTE: This does not attach the result to the operation, it is expected that the caller will
    /// do so.
    pub fn make_result(
        &self,
        span: SourceSpan,
        ty: Type,
        owner: OperationRef,
        index: u8,
    ) -> OpResultRef {
        let id = self.alloc_value_id();
        self.alloc(OpResult::new(span, id, ty, owner, index))
    }

    /// Allocate a new uninitialized entity of type `T`
    ///
    /// In general, you can probably prefer [Context::alloc] instead, but for use cases where you
    /// need to allocate the space for `T` first, and then perform initialization, this can be
    /// used.
    pub fn alloc_uninit<T: 'static>(&self) -> UnsafeEntityRef<MaybeUninit<T>> {
        UnsafeEntityRef::new_uninit(&self.allocator)
    }

    /// Allocate a new uninitialized entity of type `T`, which needs to be tracked in an intrusive
    /// doubly-linked list.
    ///
    /// In general, you can probably prefer [Context::alloc_tracked] instead, but for use cases
    /// where you need to allocate the space for `T` first, and then perform initialization,
    /// this can be used.
    pub fn alloc_uninit_tracked<T: 'static>(&self) -> UnsafeIntrusiveEntityRef<MaybeUninit<T>> {
        UnsafeIntrusiveEntityRef::new_uninit(&self.allocator)
    }

    /// Allocate a new `EntityHandle<T>`.
    ///
    /// [EntityHandle] is a smart-pointer type for IR entities, which behaves like a ref-counted
    /// pointer with dynamically-checked borrow checking rules. It is designed to play well with
    /// entities allocated from a [Context], and with the somewhat cyclical nature of the IR.
    pub fn alloc<T: 'static>(&self, value: T) -> UnsafeEntityRef<T> {
        UnsafeEntityRef::new(value, &self.allocator)
    }

    /// Allocate a new `TrackedEntityHandle<T>`.
    ///
    /// [TrackedEntityHandle] is like [EntityHandle], except that it is specially designed for
    /// entities which are meant to be tracked in intrusive linked lists. For example, the blocks
    /// in a region, or the ops in a block. It does this without requiring the entity to know about
    /// the link at all, while still making it possible to access the link from the entity.
    pub fn alloc_tracked<T: 'static>(&self, value: T) -> UnsafeIntrusiveEntityRef<T> {
        UnsafeIntrusiveEntityRef::new(value, &self.allocator)
    }

    fn alloc_block_id(&self) -> BlockId {
        let id = self.next_block_id.get();
        self.next_block_id.set(id + 1);
        BlockId::from_u32(id)
    }

    fn alloc_value_id(&self) -> ValueId {
        let id = self.next_value_id.get();
        self.next_value_id.set(id + 1);
        ValueId::from_u32(id)
    }
}
