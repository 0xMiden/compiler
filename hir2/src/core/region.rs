use super::*;

pub type RegionRef = UnsafeIntrusiveEntityRef<Region>;
/// An intrusive, doubly-linked list of [Region]s
pub type RegionList = EntityList<Region>;
/// A cursor in a [RegionList]
pub type RegionCursor<'a> = EntityCursor<'a, Region>;
/// A mutable cursor in a [RegionList]
pub type RegionCursorMut<'a> = EntityCursorMut<'a, Region>;

pub struct Region {
    /// The operation this region is attached to.
    ///
    /// If `link.is_linked() == true`, this will always be set to a valid pointer
    owner: Option<OperationRef>,
    /// The list of [Block]s that comprise this region
    body: BlockList,
}
impl Region {
    /// Get the defining [Operation] for this region, if the region is attached to one.
    pub fn parent(&self) -> Option<OperationRef> {
        self.owner.clone()
    }

    /// Get a handle to the entry block for this region
    pub fn entry(&self) -> EntityRef<'_, Block> {
        self.body.front().get().unwrap()
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
