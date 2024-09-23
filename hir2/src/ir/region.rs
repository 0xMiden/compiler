use super::*;

pub type RegionRef = UnsafeIntrusiveEntityRef<Region>;
/// An intrusive, doubly-linked list of [Region]s
pub type RegionList = EntityList<Region>;
/// A cursor in a [RegionList]
pub type RegionCursor<'a> = EntityCursor<'a, Region>;
/// A mutable cursor in a [RegionList]
pub type RegionCursorMut<'a> = EntityCursorMut<'a, Region>;

#[derive(Default)]
pub struct Region {
    /// The operation this region is attached to.
    ///
    /// If `link.is_linked() == true`, this will always be set to a valid pointer
    owner: Option<OperationRef>,
    /// The list of [Block]s that comprise this region
    body: BlockList,
}
impl Region {
    /// Returns true if this region is empty (has no blocks)
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// Get the defining [Operation] for this region, if the region is attached to one.
    pub fn parent(&self) -> Option<OperationRef> {
        self.owner.clone()
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

    /// Get a handle to the entry block for this region
    pub fn entry(&self) -> EntityRef<'_, Block> {
        self.body.front().into_borrow().unwrap()
    }

    /// Get a mutable handle to the entry block for this region
    pub fn entry_mut(&mut self) -> EntityMut<'_, Block> {
        self.body.front_mut().into_borrow_mut().unwrap()
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
