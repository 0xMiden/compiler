//! This module is based on [cranelift_entity::SparseMap], but implemented in-tree
//! because the SparseMapValueTrait is not implemented for any standard library types
use cranelift_entity::{EntityRef, SecondaryMap};

pub trait SparseMapValue<K> {
    fn key(&self) -> K;
}
impl<K, V: SparseMapValue<K>> SparseMapValue<K> for Box<V> {
    fn key(&self) -> K {
        (**self).key()
    }
}
impl<K, V: SparseMapValue<K>> SparseMapValue<K> for alloc::rc::Rc<V> {
    fn key(&self) -> K {
        (**self).key()
    }
}
impl SparseMapValue<crate::ValueId> for crate::ValueId {
    fn key(&self) -> crate::ValueId {
        *self
    }
}
impl SparseMapValue<crate::BlockId> for crate::BlockId {
    fn key(&self) -> crate::BlockId {
        *self
    }
}

/// A sparse mapping of entity references.
///
/// A `SparseMap<K, V>` map provides:
///
/// - Memory usage equivalent to `SecondaryMap<K, u32>` + `Vec<V>`, so much smaller than
///   `SecondaryMap<K, V>` for sparse mappings of larger `V` types.
/// - Constant time lookup, slightly slower than `SecondaryMap`.
/// - A very fast, constant time `clear()` operation.
/// - Fast insert and erase operations.
/// - Stable iteration that is as fast as a `Vec<V>`.
///
/// # Compared to `SecondaryMap`
///
/// When should we use a `SparseMap` instead of a secondary `SecondaryMap`? First of all,
/// `SparseMap` does not provide the functionality of a `PrimaryMap` which can allocate and assign
/// entity references to objects as they are pushed onto the map. It is only the secondary entity
/// maps that can be replaced with a `SparseMap`.
///
/// - A secondary entity map assigns a default mapping to all keys. It doesn't distinguish between
///   an unmapped key and one that maps to the default value. `SparseMap` does not require `Default`
///   values, and it tracks accurately if a key has been mapped or not.
/// - Iterating over the contents of an `SecondaryMap` is linear in the size of the *key space*,
///   while iterating over a `SparseMap` is linear in the number of elements in the mapping. This is
///   an advantage precisely when the mapping is sparse.
/// - `SparseMap::clear()` is constant time and super-fast. `SecondaryMap::clear()` is linear in the
///   size of the key space. (Or, rather the required `resize()` call following the `clear()` is).
/// - `SparseMap` requires the values to implement `SparseMapValue<K>` which means that they must
///   contain their own key.
pub struct SparseMap<K, V>
where
    K: EntityRef,
    V: SparseMapValue<K>,
{
    sparse: SecondaryMap<K, u32>,
    dense: Vec<V>,
}
impl<K, V> Default for SparseMap<K, V>
where
    K: EntityRef,
    V: SparseMapValue<K>,
{
    fn default() -> Self {
        Self {
            sparse: SecondaryMap::new(),
            dense: Vec::new(),
        }
    }
}
impl<K, V> core::fmt::Debug for SparseMap<K, V>
where
    K: EntityRef + core::fmt::Debug,
    V: SparseMapValue<K> + core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_map().entries(self.values().map(|v| (v.key(), v))).finish()
    }
}
impl<K, V> SparseMap<K, V>
where
    K: EntityRef,
    V: SparseMapValue<K>,
{
    /// Create a new empty mapping.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of elements in the map.
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Returns true is the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    /// Remove all elements from the mapping.
    pub fn clear(&mut self) {
        self.dense.clear();
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get(&self, key: K) -> Option<&V> {
        if let Some(idx) = self.sparse.get(key).cloned() {
            if let Some(entry) = self.dense.get(idx as usize) {
                if entry.key() == key {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Returns a mutable reference to the value corresponding to the key.
    ///
    /// Note that the returned value must not be mutated in a way that would change its key. This
    /// would invalidate the sparse set data structure.
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        if let Some(idx) = self.sparse.get(key).cloned() {
            if let Some(entry) = self.dense.get_mut(idx as usize) {
                if entry.key() == key {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Return the index into `dense` of the value corresponding to `key`.
    fn index(&self, key: K) -> Option<usize> {
        if let Some(idx) = self.sparse.get(key).cloned() {
            let idx = idx as usize;
            if let Some(entry) = self.dense.get(idx) {
                if entry.key() == key {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// Return `true` if the map contains a value corresponding to `key`.
    pub fn contains_key(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    /// Insert a value into the map.
    ///
    /// If the map did not have this key present, `None` is returned.
    ///
    /// If the map did have this key present, the value is updated, and the old value is returned.
    ///
    /// It is not necessary to provide a key since the value knows its own key already.
    pub fn insert(&mut self, value: V) -> Option<V> {
        let key = value.key();

        // Replace the existing entry for `key` if there is one.
        if let Some(entry) = self.get_mut(key) {
            return Some(core::mem::replace(entry, value));
        }

        // There was no previous entry for `key`. Add it to the end of `dense`.
        let idx = self.dense.len();
        debug_assert!(idx <= u32::MAX as usize, "SparseMap overflow");
        self.dense.push(value);
        self.sparse[key] = idx as u32;
        None
    }

    /// Remove a value from the map and return it.
    pub fn remove(&mut self, key: K) -> Option<V> {
        if let Some(idx) = self.index(key) {
            let back = self.dense.pop().unwrap();

            // Are we popping the back of `dense`?
            if idx == self.dense.len() {
                return Some(back);
            }

            // We're removing an element from the middle of `dense`.
            // Replace the element at `idx` with the back of `dense`.
            // Repair `sparse` first.
            self.sparse[back.key()] = idx as u32;
            return Some(core::mem::replace(&mut self.dense[idx], back));
        }

        // Nothing to remove.
        None
    }

    /// Remove the last value from the map.
    pub fn pop(&mut self) -> Option<V> {
        self.dense.pop()
    }

    /// Get an iterator over the values in the map.
    ///
    /// The iteration order is entirely determined by the preceding sequence of `insert` and
    /// `remove` operations. In particular, if no elements were removed, this is the insertion
    /// order.
    pub fn values(&self) -> core::slice::Iter<V> {
        self.dense.iter()
    }

    /// Get the values as a slice.
    pub fn as_slice(&self) -> &[V] {
        self.dense.as_slice()
    }
}

/// Iterating over the elements of a set.
impl<'a, K, V> IntoIterator for &'a SparseMap<K, V>
where
    K: EntityRef,
    V: SparseMapValue<K>,
{
    type IntoIter = core::slice::Iter<'a, V>;
    type Item = &'a V;

    fn into_iter(self) -> Self::IntoIter {
        self.values()
    }
}
