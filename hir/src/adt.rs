pub mod arena;
pub mod smalldeque;
pub mod smallmap;
pub mod smallordset;
pub mod smallprio;
pub mod smallset;

pub use self::{
    arena::Arena,
    smalldeque::SmallDeque,
    smallmap::{SmallDenseMap, SmallOrdMap},
    smallordset::SmallOrdSet,
    smallprio::SmallPriorityQueue,
    smallset::SmallSet,
};

#[doc(hidden)]
pub trait SizedTypeProperties: Sized {
    const IS_ZST: bool = core::mem::size_of::<Self>() == 0;
}
impl<T> SizedTypeProperties for T {}
