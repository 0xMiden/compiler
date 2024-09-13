use core::{
    cell::{Cell, UnsafeCell},
    fmt,
    mem::MaybeUninit,
    ptr::NonNull,
};

use blink_alloc::Blink;
use cranelift_entity::{PrimaryMap, SecondaryMap};

use super::{
    entity::{EntityObj, TrackedEntityObj},
    *,
};
use crate::UnsafeRef;

pub struct Context {
    pub allocator: Blink,
    pub blocks: PrimaryMap<BlockId, Block>,
    pub values: PrimaryMap<ValueId, Value>,
    pub constants: ConstantPool,
}

impl Context {
    pub fn new() -> Self {
        let allocator = Blink::new();
        Self {
            allocator,
            blocks: PrimaryMap::new(),
            values: PrimaryMap::new(),
            constants: Default::default(),
        }
    }

    /// Allocate a new uninitialized entity of type `T`
    ///
    /// In general, you can probably prefer [Context::alloc] instead, but for use cases where you
    /// need to allocate the space for `T` first, and then perform initialization, this can be
    /// used.
    pub fn alloc_uninit<T>(&self) -> EntityHandle<MaybeUninit<T>> {
        let entity = self.allocator.uninit::<EntityObj<T>>();
        unsafe { EntityHandle::new_uninit(NonNull::new_unchecked(entity)) }
    }

    /// Allocate a new uninitialized entity of type `T`, which needs to be tracked in an intrusive
    /// doubly-linked list.
    ///
    /// In general, you can probably prefer [Context::alloc_tracked] instead, but for use cases
    /// where you need to allocate the space for `T` first, and then perform initialization,
    /// this can be used.
    pub fn alloc_uninit_tracked<T>(&self) -> TrackedEntityHandle<MaybeUninit<T>> {
        let entity = self.allocator.uninit::<TrackedEntityObj<T>>();
        unsafe { TrackedEntityHandle::new_uninit(NonNull::new_unchecked(entity)) }
    }

    /// Allocate a new `EntityHandle<T>`.
    ///
    /// [EntityHandle] is a smart-pointer type for IR entities, which behaves like a ref-counted
    /// pointer with dynamically-checked borrow checking rules. It is designed to play well with
    /// entities allocated from a [Context], and with the somewhat cyclical nature of the IR.
    pub fn alloc<T>(&self, value: T) -> EntityHandle<T> {
        let entity = self.allocator.put(EntityObj::new(value));
        unsafe { EntityHandle::new(NonNull::new_unchecked(entity)) }
    }

    /// Allocate a new `TrackedEntityHandle<T>`.
    ///
    /// [TrackedEntityHandle] is like [EntityHandle], except that it is specially designed for
    /// entities which are meant to be tracked in intrusive linked lists. For example, the blocks
    /// in a region, or the ops in a block. It does this without requiring the entity to know about
    /// the link at all, while still making it possible to access the link from the entity.
    pub fn alloc_tracked<T>(&self, value: T) -> TrackedEntityHandle<T> {
        let entity = self.allocator.put(TrackedEntityObj::new(value));
        unsafe { TrackedEntityHandle::new(NonNull::new_unchecked(entity)) }
    }

    pub fn create_op<T: Op>(&mut self, mut op: T) -> OpId {
        let key = self.ops.next_key();
        let op = self.allocator.put(op);
        let ptr = op as *mut T;
        {
            let operation = op.as_operation_mut();
            operation.key = key;
            operation.vtable.set_data_ptr(ptr);
        }
        let op = unsafe { NonNull::new_unchecked(op) };
        self.ops.push(op.cast());
        key
    }

    pub fn op(&self, id: OpId) -> &dyn Op {
        self.ops[id].as_ref()
    }
}
