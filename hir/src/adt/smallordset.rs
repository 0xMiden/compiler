use core::{borrow::Borrow, fmt};

use smallvec::SmallVec;

/// [SmallOrdSet] is a [BTreeSet]-like structure that can store a specified number
/// of elements inline (i.e. on the stack) without allocating memory from the heap.
///
/// This data structure is designed with two goals in mind:
///
/// * Support efficient set operations over a small set of items
/// * Maintains the underlying set in order (according to the `Ord` impl of the element type)
/// * Avoid allocating data on the heap for the typical case
///
/// Internally, [SmallOrdSet] is implemented on top of [SmallVec], and uses binary search
/// to locate elements. This is quite efficient in general, and is particularly fast
/// when all of the data is stored inline, but may not be a good fit for all use cases.
///
/// Due to its design constraints, it only supports elements which implement [Ord].
///
/// NOTE: This type differs from [SmallSet] in that [SmallOrdSet] uses the [Ord] implementation
/// of the element type for ordering, while [SmallSet] preserves the insertion order of elements.
/// Beyond that, the two types are meant to be essentially equivalent.
pub struct SmallOrdSet<T, const N: usize> {
    items: SmallVec<[T; N]>,
}
impl<T, const N: usize> Default for SmallOrdSet<T, N> {
    fn default() -> Self {
        Self {
            items: Default::default(),
        }
    }
}
impl<T, const N: usize> Eq for SmallOrdSet<T, N> where T: Eq {}
impl<T, const N: usize> PartialEq for SmallOrdSet<T, N>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.items.eq(&other.items)
    }
}
impl<T, const N: usize> fmt::Debug for SmallOrdSet<T, N>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.items.iter()).finish()
    }
}
impl<T, const N: usize> Clone for SmallOrdSet<T, N>
where
    T: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
        }
    }
}
impl<T, const N: usize> SmallOrdSet<T, N>
where
    T: Ord,
{
    pub fn from_vec(items: SmallVec<[T; N]>) -> Self {
        let mut set = Self { items };
        set.sort_and_dedup();
        set
    }

    #[inline]
    pub fn from_buf(buf: [T; N]) -> Self {
        Self::from_vec(buf.into())
    }
}
impl<T, const N: usize> IntoIterator for SmallOrdSet<T, N> {
    type IntoIter = smallvec::IntoIter<[T; N]>;
    type Item = T;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
impl<T, const N: usize> FromIterator<T> for SmallOrdSet<T, N>
where
    T: Ord,
{
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let mut set = Self::default();
        for item in iter {
            set.insert(item);
        }
        set
    }
}
impl<T, const N: usize> SmallOrdSet<T, N>
where
    T: Ord,
{
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.items.iter()
    }

    pub fn insert(&mut self, item: T) -> bool {
        match self.find(&item) {
            Ok(_) => false,
            Err(idx) => {
                self.items.insert(idx, item);
                true
            }
        }
    }

    pub fn remove<Q>(&mut self, item: &Q) -> Option<T>
    where
        T: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(item) {
            Ok(idx) => Some(self.items.remove(idx)),
            Err(_) => None,
        }
    }

    /// Clear the content of the set
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn contains<Q>(&self, item: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.find(item).is_ok()
    }

    pub fn get<Q>(&self, item: &Q) -> Option<&T>
    where
        T: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(item) {
            Ok(idx) => Some(&self.items[idx]),
            Err(_) => None,
        }
    }

    pub fn get_mut<Q>(&mut self, item: &Q) -> Option<&mut T>
    where
        T: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(item) {
            Ok(idx) => Some(&mut self.items[idx]),
            Err(_) => None,
        }
    }

    #[inline]
    fn find<Q>(&self, item: &Q) -> Result<usize, usize>
    where
        T: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.binary_search_by(|probe| Ord::cmp(probe.borrow(), item))
    }

    fn sort_and_dedup(&mut self) {
        self.items.sort_unstable();
        self.items.dedup();
    }
}
