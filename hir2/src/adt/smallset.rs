use core::{borrow::Borrow, fmt};

use smallvec::SmallVec;

/// [SmallSet] is a set data structure that can store a specified number
/// of elements inline (i.e. on the stack) without allocating memory from the heap.
///
/// This data structure is designed with two goals in mind:
///
/// * Support efficient set operations over a small set of items
/// * Preserve the insertion order of those items
/// * Avoid allocating data on the heap for the typical case
///
/// Internally, [SmallSet] is implemented on top of [SmallVec], and uses linear search
/// to locate elements. This is only reasonably efficient on small sets, for anything
/// larger you should reach for quite efficient in general, and is particularly fast
/// when all of the data is stored inline, but may not be a good fit for all use cases.
///
/// Due to its design constraints, elements must implement [Eq].
pub struct SmallSet<T, const N: usize> {
    items: SmallVec<[T; N]>,
}
impl<T, const N: usize> Default for SmallSet<T, N> {
    fn default() -> Self {
        Self {
            items: Default::default(),
        }
    }
}
impl<T, const N: usize> Eq for SmallSet<T, N> where T: Eq {}
impl<T, const N: usize> PartialEq for SmallSet<T, N>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.items.eq(&other.items)
    }
}
impl<T, const N: usize> fmt::Debug for SmallSet<T, N>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.items.iter()).finish()
    }
}
impl<T, const N: usize> Clone for SmallSet<T, N>
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
impl<T, const N: usize> IntoIterator for SmallSet<T, N> {
    type IntoIter = smallvec::IntoIter<[T; N]>;
    type Item = T;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
impl<T, const N: usize> From<[T; N]> for SmallSet<T, N>
where
    T: Eq,
{
    #[inline]
    fn from(items: [T; N]) -> Self {
        Self::from_iter(items)
    }
}
impl<T, const N: usize> FromIterator<T> for SmallSet<T, N>
where
    T: Eq,
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
impl<T, const N: usize> SmallSet<T, N>
where
    T: Eq,
{
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.items.iter()
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }

    pub fn insert(&mut self, item: T) -> bool {
        if self.contains(&item) {
            return false;
        }
        self.items.push(item);
        true
    }

    pub fn remove<Q>(&mut self, item: &Q) -> Option<T>
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        match self.find(item) {
            Some(idx) => Some(self.items.remove(idx)),
            None => None,
        }
    }

    /// Remove all items from the set for which `predicate` returns false.
    pub fn retain<F>(&mut self, predicate: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        self.items.retain(predicate);
    }

    /// Clear the content of the set
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn contains<Q>(&self, item: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.find(item).is_some()
    }

    pub fn get<Q>(&self, item: &Q) -> Option<&T>
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        match self.find(item) {
            Some(idx) => Some(&self.items[idx]),
            None => None,
        }
    }

    pub fn get_mut<Q>(&mut self, item: &Q) -> Option<&mut T>
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        match self.find(item) {
            Some(idx) => Some(&mut self.items[idx]),
            None => None,
        }
    }

    /// Convert this set into a `SmallVec` containing the items of the set
    #[inline]
    pub fn into_vec(self) -> SmallVec<[T; N]> {
        self.items
    }

    #[inline]
    fn find<Q>(&self, item: &Q) -> Option<usize>
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.items.iter().position(|elem| elem.borrow() == item)
    }
}

impl<T, const N: usize> SmallSet<T, N>
where
    T: Clone + Eq,
{
    /// Obtain a new [SmallSet] containing the unique elements of both `self` and `other`
    pub fn union(&self, other: &Self) -> Self {
        let mut result = self.clone();

        for item in other.items.iter() {
            if result.contains(item) {
                continue;
            }
            result.items.push(item.clone());
        }

        result
    }

    /// Obtain a new [SmallSet] containing the unique elements of both `self` and `other`
    pub fn into_union(mut self, other: &Self) -> Self {
        for item in other.items.iter() {
            if self.contains(item) {
                continue;
            }
            self.items.push(item.clone());
        }

        self
    }

    /// Obtain a new [SmallSet] containing the elements in common between `self` and `other`
    pub fn intersection(&self, other: &Self) -> Self {
        let mut result = Self::default();

        for item in self.items.iter() {
            if other.contains(item) {
                result.items.push(item.clone());
            }
        }

        result
    }

    /// Obtain a new [SmallSet] containing the elements in common between `self` and `other`
    pub fn into_intersection(self, other: &Self) -> Self {
        let mut result = Self::default();

        for item in self.items.into_iter() {
            if other.contains(&item) {
                result.items.push(item);
            }
        }

        result
    }

    /// Obtain a new [SmallSet] containing the elements in `self` but not in `other`
    pub fn difference(&self, other: &Self) -> Self {
        let mut result = Self::default();

        for item in self.items.iter() {
            if other.contains(item) {
                continue;
            }
            result.items.push(item.clone());
        }

        result
    }

    /// Obtain a new [SmallSet] containing the elements in `self` but not in `other`
    pub fn into_difference(mut self, other: &Self) -> Self {
        Self {
            items: self.items.drain_filter(|item| !other.contains(item)).collect(),
        }
    }

    /// Obtain a new [SmallSet] containing the elements in `self` or `other`, but not in both
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        let mut result = Self::default();

        for item in self.items.iter() {
            if other.contains(item) {
                continue;
            }
            result.items.push(item.clone());
        }

        for item in other.items.iter() {
            if self.contains(item) {
                continue;
            }
            result.items.push(item.clone());
        }

        result
    }
}

impl<E, const N: usize> core::iter::Extend<E> for SmallSet<E, N>
where
    E: Eq,
{
    fn extend<T: IntoIterator<Item = E>>(&mut self, iter: T) {
        for item in iter {
            self.insert(item);
        }
    }
}
