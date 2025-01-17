use std::{borrow::Borrow, fmt, hash::Hash, rc::Rc};

use midenc_hir::FxHashMap;

#[derive(Clone)]
pub struct ScopedMap<K, V>
where
    K: Eq + Hash,
{
    parent: Option<Rc<ScopedMap<K, V>>>,
    map: FxHashMap<K, V>,
}
impl<K, V> Default for ScopedMap<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            parent: None,
            map: Default::default(),
        }
    }
}
impl<K, V> ScopedMap<K, V>
where
    K: Eq + Hash,
{
    pub fn new(parent: Option<Rc<ScopedMap<K, V>>>) -> Self {
        Self {
            parent,
            map: Default::default(),
        }
    }

    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get(k).or_else(|| self.parent.as_ref().and_then(|p| p.get(k)))
    }

    pub fn insert(&mut self, k: K, v: V) {
        self.map.insert(k, v);
    }

    pub fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.map.extend(iter);
    }
}
impl<K, V> fmt::Debug for ScopedMap<K, V>
where
    K: Eq + Hash + fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ScopedMap")
            .field("parent", &self.parent)
            .field("map", &self.map)
            .finish()
    }
}
