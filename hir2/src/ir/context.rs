use alloc::rc::Rc;
use core::{cell::Cell, mem::MaybeUninit};

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
            next_block_id: Cell::new(0),
            next_value_id: Cell::new(0),
            //constants: Default::default(),
        }
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

    /// Create a new [OpResult] with the given type, owner, and index
    ///
    /// NOTE: This does not attach the result to the operation, it is expected that the caller will
    /// do so.
    pub fn make_result(&self, ty: Type, owner: OperationRef, index: u8) -> OpResultRef {
        let id = self.alloc_value_id();
        self.alloc(OpResult::new(id, ty, owner, index))
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
