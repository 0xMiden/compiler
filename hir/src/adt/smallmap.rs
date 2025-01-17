use core::{
    borrow::Borrow,
    cmp::Ordering,
    fmt,
    ops::{Index, IndexMut},
};

use smallvec::SmallVec;

/// [SmallMap] is a [BTreeMap]-like structure that can store a specified number
/// of elements inline (i.e. on the stack) without allocating memory from the heap.
///
/// This data structure is designed with two goals in mind:
///
/// * Support efficient key/value operations over a small set of keys
/// * Preserve the order of keys
/// * Avoid allocating data on the heap for the typical case
///
/// Internally, [SmallMap] is implemented on top of [SmallVec], and uses binary search
/// to locate elements. This is quite efficient in general, and is particularly fast
/// when all of the data is stored inline, but may not be a good fit for all use cases.
///
/// Due to its design constraints, it only supports keys which implement [Ord].
pub struct SmallMap<K, V, const N: usize = 4> {
    items: SmallVec<[KeyValuePair<K, V>; N]>,
}
impl<K, V, const N: usize> SmallMap<K, V, N>
where
    K: Ord,
{
    /// Returns a new, empty [SmallMap]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this map is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of key/value pairs in this map
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Return an iterator over the key/value pairs in this map
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> {
        self.items.iter().map(|pair| (&pair.key, &pair.value))
    }

    /// Return an iterator over mutable key/value pairs in this map
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = (&K, &mut V)> {
        self.items.iter_mut().map(|pair| (&pair.key, &mut pair.value))
    }

    /// Returns true if `key` has been inserted in this map
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.find(key).is_ok()
    }

    /// Returns the value under `key` in this map, if it exists
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(key) {
            Ok(idx) => Some(&self.items[idx].value),
            Err(_) => None,
        }
    }

    /// Returns a mutable reference to the value under `key` in this map, if it exists
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(key) {
            Ok(idx) => Some(&mut self.items[idx].value),
            Err(_) => None,
        }
    }

    /// Inserts a new entry in this map using `key` and `value`.
    ///
    /// Returns the previous value, if `key` was already present in the map.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.entry(key) {
            Entry::Occupied(mut entry) => Some(core::mem::replace(entry.get_mut(), value)),
            Entry::Vacant(entry) => {
                entry.insert(value);
                None
            }
        }
    }

    /// Removes the value inserted under `key`, if it exists
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.find(key) {
            Ok(idx) => Some(self.items.remove(idx).value),
            Err(_) => None,
        }
    }

    /// Clear the content of the map
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Returns an [Entry] which can be used to combine `contains`+`insert` type operations.
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V, N> {
        match self.find(&key) {
            Ok(idx) => Entry::occupied(self, idx),
            Err(idx) => Entry::vacant(self, idx, key),
        }
    }

    #[inline]
    fn find<Q>(&self, item: &Q) -> Result<usize, usize>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.binary_search_by(|probe| Ord::cmp(probe.key.borrow(), item))
    }
}
impl<K, V, const N: usize> Default for SmallMap<K, V, N> {
    fn default() -> Self {
        Self {
            items: Default::default(),
        }
    }
}
impl<K, V, const N: usize> Eq for SmallMap<K, V, N>
where
    K: Eq,
    V: Eq,
{
}
impl<K, V, const N: usize> PartialEq for SmallMap<K, V, N>
where
    K: PartialEq,
    V: PartialEq,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.items
            .iter()
            .map(|pair| (&pair.key, &pair.value))
            .eq(other.items.iter().map(|pair| (&pair.key, &pair.value)))
    }
}
impl<K, V, const N: usize> fmt::Debug for SmallMap<K, V, N>
where
    K: fmt::Debug + Ord,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map()
            .entries(self.items.iter().map(|item| (&item.key, &item.value)))
            .finish()
    }
}
impl<K, V, const N: usize> Clone for SmallMap<K, V, N>
where
    K: Clone,
    V: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
        }
    }
}
impl<K, V, const N: usize> IntoIterator for SmallMap<K, V, N>
where
    K: Ord,
{
    type IntoIter = SmallMapIntoIter<K, V, N>;
    type Item = (K, V);

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        SmallMapIntoIter {
            iter: self.items.into_iter(),
        }
    }
}
impl<K, V, const N: usize> FromIterator<(K, V)> for SmallMap<K, V, N>
where
    K: Ord,
{
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let mut map = Self::default();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}
impl<K, V, Q, const N: usize> Index<&Q> for SmallMap<K, V, N>
where
    K: Borrow<Q> + Ord,
    Q: Ord + ?Sized,
{
    type Output = V;

    fn index(&self, key: &Q) -> &Self::Output {
        self.get(key).unwrap()
    }
}
impl<K, V, Q, const N: usize> IndexMut<&Q> for SmallMap<K, V, N>
where
    K: Borrow<Q> + Ord,
    Q: Ord + ?Sized,
{
    fn index_mut(&mut self, key: &Q) -> &mut Self::Output {
        self.get_mut(key).unwrap()
    }
}

#[doc(hidden)]
pub struct SmallMapIntoIter<K, V, const N: usize> {
    iter: smallvec::IntoIter<[KeyValuePair<K, V>; N]>,
}
impl<K, V, const N: usize> ExactSizeIterator for SmallMapIntoIter<K, V, N> {
    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len()
    }
}
impl<K, V, const N: usize> Iterator for SmallMapIntoIter<K, V, N> {
    type Item = (K, V);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|pair| (pair.key, pair.value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.iter.count()
    }

    #[inline]
    fn last(mut self) -> Option<(K, V)> {
        self.iter.next_back().map(|pair| (pair.key, pair.value))
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.iter.nth(n).map(|pair| (pair.key, pair.value))
    }
}
impl<K, V, const N: usize> DoubleEndedIterator for SmallMapIntoIter<K, V, N> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back().map(|pair| (pair.key, pair.value))
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        self.iter.nth_back(n).map(|pair| (pair.key, pair.value))
    }
}

/// Represents an key/value pair entry in a [SmallMap]
pub enum Entry<'a, K, V, const N: usize> {
    Occupied(OccupiedEntry<'a, K, V, N>),
    Vacant(VacantEntry<'a, K, V, N>),
}
impl<'a, K, V, const N: usize> Entry<'a, K, V, N> {
    fn occupied(map: &'a mut SmallMap<K, V, N>, idx: usize) -> Self {
        Self::Occupied(OccupiedEntry { map, idx })
    }

    fn vacant(map: &'a mut SmallMap<K, V, N>, idx: usize, key: K) -> Self {
        Self::Vacant(VacantEntry { map, idx, key })
    }

    pub fn or_insert(self, default: V) -> &'a mut V {
        match self {
            Self::Occupied(entry) => entry.into_mut(),
            Self::Vacant(entry) => entry.insert(default),
        }
    }

    pub fn or_insert_with<F>(self, default: F) -> &'a mut V
    where
        F: FnOnce() -> V,
    {
        match self {
            Self::Occupied(entry) => entry.into_mut(),
            Self::Vacant(entry) => entry.insert(default()),
        }
    }

    pub fn key(&self) -> &K {
        match self {
            Self::Occupied(entry) => entry.key(),
            Self::Vacant(entry) => entry.key(),
        }
    }

    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut V),
    {
        match self {
            Self::Occupied(mut entry) => {
                f(entry.get_mut());
                Self::Occupied(entry)
            }
            vacant @ Self::Vacant(_) => vacant,
        }
    }
}

/// Represents an occupied entry in a [SmallMap]
pub struct OccupiedEntry<'a, K, V, const N: usize> {
    map: &'a mut SmallMap<K, V, N>,
    idx: usize,
}
impl<'a, K, V, const N: usize> OccupiedEntry<'a, K, V, N> {
    #[inline(always)]
    fn get_entry(&self) -> &KeyValuePair<K, V> {
        &self.map.items[self.idx]
    }

    pub fn remove_entry(self) -> V {
        self.map.items.remove(self.idx).value
    }

    pub fn key(&self) -> &K {
        &self.get_entry().key
    }

    pub fn get(&self) -> &V {
        &self.get_entry().value
    }

    pub fn get_mut(&mut self) -> &mut V {
        &mut self.map.items[self.idx].value
    }

    pub fn into_mut(self) -> &'a mut V {
        &mut self.map.items[self.idx].value
    }
}

/// Represents a vacant entry in a [SmallMap]
pub struct VacantEntry<'a, K, V, const N: usize> {
    map: &'a mut SmallMap<K, V, N>,
    idx: usize,
    key: K,
}
impl<'a, K, V, const N: usize> VacantEntry<'a, K, V, N> {
    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn into_key(self) -> K {
        self.key
    }

    pub fn insert_with<F>(self, f: F) -> &'a mut V
    where
        F: FnOnce() -> V,
    {
        self.map.items.insert(
            self.idx,
            KeyValuePair {
                key: self.key,
                value: f(),
            },
        );
        &mut self.map.items[self.idx].value
    }

    pub fn insert(self, value: V) -> &'a mut V {
        self.map.items.insert(
            self.idx,
            KeyValuePair {
                key: self.key,
                value,
            },
        );
        &mut self.map.items[self.idx].value
    }
}

struct KeyValuePair<K, V> {
    key: K,
    value: V,
}
impl<K, V> AsRef<V> for KeyValuePair<K, V> {
    #[inline]
    fn as_ref(&self) -> &V {
        &self.value
    }
}
impl<K, V> AsMut<V> for KeyValuePair<K, V> {
    #[inline]
    fn as_mut(&mut self) -> &mut V {
        &mut self.value
    }
}
impl<K, V> Clone for KeyValuePair<K, V>
where
    K: Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            value: self.value.clone(),
        }
    }
}
impl<K, V> Copy for KeyValuePair<K, V>
where
    K: Copy,
    V: Copy,
{
}

impl<K, V> Eq for KeyValuePair<K, V> where K: Eq {}

impl<K, V> PartialEq for KeyValuePair<K, V>
where
    K: PartialEq,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key.eq(&other.key)
    }
}

impl<K, V> Ord for KeyValuePair<K, V>
where
    K: Ord,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}
impl<K, V> PartialOrd for KeyValuePair<K, V>
where
    K: PartialOrd,
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.key.partial_cmp(&other.key)
    }
}
