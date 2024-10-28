pub mod smalldeque;
pub mod smallmap;
pub mod smallordset;
pub mod smallprio;
pub mod smallset;
pub mod sparsemap;

pub use self::{
    smalldeque::SmallDeque,
    smallmap::SmallMap,
    smallordset::SmallOrdSet,
    smallprio::SmallPriorityQueue,
    smallset::SmallSet,
    sparsemap::{SparseMap, SparseMapValue},
};

#[doc(hidden)]
pub trait SizedTypeProperties: Sized {
    const IS_ZST: bool = core::mem::size_of::<Self>() == 0;
}
impl<T> SizedTypeProperties for T {}
