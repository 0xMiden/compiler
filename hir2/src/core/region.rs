use super::{BlockList, EntityCursor, EntityCursorMut, EntityHandle, EntityList};

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
    owner: Option<EntityHandle<Operation>>,
    /// The list of [Block]s that comprise this region
    body: BlockList,
}
