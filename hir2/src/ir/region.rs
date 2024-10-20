use super::*;

pub type RegionRef = UnsafeIntrusiveEntityRef<Region>;
/// An intrusive, doubly-linked list of [Region]s
pub type RegionList = EntityList<Region>;
/// A cursor in a [RegionList]
pub type RegionCursor<'a> = EntityCursor<'a, Region>;
/// A mutable cursor in a [RegionList]
pub type RegionCursorMut<'a> = EntityCursorMut<'a, Region>;

/// A region is a container for [Block], in one of two forms:
///
/// * Graph-like, in which the region consists of a single block, and the order of operations in
///   that block does not dictate any specific control flow semantics. It is up to the containing
///   operation to define.
/// * SSA-form, in which the region consists of one or more blocks that must obey the usual rules
///   of SSA dominance, and where operations in a block reflect the order in which those operations
///   are to be executed. Values defined by an operation must dominate any uses of those values in
///   the region.
///
/// The first block in a region is the _entry_ block, and its argument list corresponds to the
/// arguments expected by the region itself.
///
/// A region is only valid when it is attached to an [Operation], whereas the inverse is not true,
/// i.e. an operation without a parent region is a top-level operation, e.g. `Module`.
#[derive(Default)]
pub struct Region {
    /// The operation this region is attached to.
    ///
    /// If `link.is_linked() == true`, this will always be set to a valid pointer
    owner: Option<OperationRef>,
    /// The list of [Block]s that comprise this region
    body: BlockList,
}

impl Entity for Region {}
impl EntityWithParent for Region {
    type Parent = Operation;

    fn on_inserted_into_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().owner = Some(parent);
    }

    fn on_removed_from_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        _parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().owner = None;
    }

    fn on_transfered_to_new_parent(
        _from: UnsafeIntrusiveEntityRef<Self::Parent>,
        to: UnsafeIntrusiveEntityRef<Self::Parent>,
        transferred: impl IntoIterator<Item = UnsafeIntrusiveEntityRef<Self>>,
    ) {
        for mut transferred_region in transferred {
            transferred_region.borrow_mut().owner = Some(to.clone());
        }
    }
}

/// Blocks
impl Region {
    /// Returns true if this region is empty (has no blocks)
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// Get a handle to the entry block for this region
    pub fn entry(&self) -> EntityRef<'_, Block> {
        self.body.front().into_borrow().unwrap()
    }

    /// Get a mutable handle to the entry block for this region
    pub fn entry_mut(&mut self) -> EntityMut<'_, Block> {
        self.body.front_mut().into_borrow_mut().unwrap()
    }

    /// Get the [BlockRef] of the entry block of this region, if it has one
    #[inline]
    pub fn entry_block_ref(&self) -> Option<BlockRef> {
        self.body.front().as_pointer()
    }

    /// Get the list of blocks comprising the body of this region
    pub fn body(&self) -> &BlockList {
        &self.body
    }

    /// Get a mutable reference to the list of blocks comprising the body of this region
    pub fn body_mut(&mut self) -> &mut BlockList {
        &mut self.body
    }
}

/// Metadata
impl Region {
    #[inline]
    pub fn as_region_ref(&self) -> RegionRef {
        unsafe { RegionRef::from_raw(self) }
    }

    /// Returns true if this region is an ancestor of `other`, i.e. it contains it.
    ///
    /// NOTE: This returns true if `self == other`, see [Self::is_proper_ancestor] if you do not
    /// want this behavior.
    pub fn is_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        &this == other || Self::is_proper_ancestor_of(&this, other)
    }

    /// Returns true if this region is a proper ancestor of `other`, i.e. `other` is contained by it
    ///
    /// NOTE: This returns false if `self == other`, see [Self::is_ancestor] if you do not want this
    /// behavior.
    pub fn is_proper_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        Self::is_proper_ancestor_of(&this, other)
    }

    fn is_proper_ancestor_of(this: &RegionRef, other: &RegionRef) -> bool {
        if this == other {
            return false;
        }

        let mut parent = other.borrow().parent_region();
        while let Some(parent_region) = parent.take() {
            if this == &parent_region {
                return true;
            }
            parent = parent_region.borrow().parent_region();
        }

        false
    }

    /// Returns true if this region may be a graph region without SSA dominance
    pub fn may_be_graph_region(&self) -> bool {
        if let Some(owner) = self.owner.as_ref() {
            owner
                .borrow()
                .as_trait::<dyn RegionKindInterface>()
                .is_some_and(|rki| rki.has_graph_regions())
        } else {
            true
        }
    }

    /// Returns true if this region has only one block
    pub fn has_one_block(&self) -> bool {
        !self.body.is_empty()
            && BlockRef::ptr_eq(
                &self.body.front().as_pointer().unwrap(),
                &self.body.back().as_pointer().unwrap(),
            )
    }

    /// Get the defining [Operation] for this region, if the region is attached to one.
    pub fn parent(&self) -> Option<OperationRef> {
        self.owner.clone()
    }

    /// Get the region which contains the parent operation of this region, if there is one.
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.owner.as_ref().and_then(|op| op.borrow().parent_region())
    }

    /// Set the owner of this region.
    ///
    /// Returns the previous owner.
    ///
    /// # Safety
    ///
    /// It is dangerous to set this field unless doing so as part of allocating the [Region] or
    /// moving the [Region] from one op to another. If it is set to a different entity than actually
    /// owns the region, it will result in undefined behavior or panics when we attempt to access
    /// the owner via the region.
    ///
    /// You must ensure that the owner given _actually_ owns the region. Similarly, if you are
    /// unsetting the owner, you must ensure that no entity _thinks_ it owns this region.
    pub unsafe fn set_owner(&mut self, owner: Option<OperationRef>) -> Option<OperationRef> {
        match owner {
            None => self.owner.take(),
            Some(owner) => self.owner.replace(owner),
        }
    }
}

/// Mutation
impl Region {
    /// Push `block` to the start of this region
    #[inline]
    pub fn push_front(&mut self, block: BlockRef) {
        self.body.push_front(block);
    }

    /// Push `block` to the end of this region
    #[inline]
    pub fn push_back(&mut self, block: BlockRef) {
        self.body.push_back(block);
    }

    /// Drop any references to blocks in this region - this is used to break cycles when cleaning
    /// up regions.
    pub fn drop_all_references(&mut self) {
        todo!()
    }
}
