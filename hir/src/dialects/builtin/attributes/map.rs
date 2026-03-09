use super::*;

#[derive(Clone)]
pub struct DictAttr<K, V> {
    values: Vec<(K, V)>,
}
impl<K, V> Default for DictAttr<K, V> {
    fn default() -> Self {
        Self { values: vec![] }
    }
}
impl<K, V> DictAttr<K, V>
where
    K: Ord,
    V: Clone,
{
    pub fn insert(&mut self, key: K, value: V) {
        match self.values.binary_search_by(|(k, _)| key.cmp(k)) {
            Ok(index) => {
                self.values[index].1 = value;
            }
            Err(index) => {
                self.values.insert(index, (key, value));
            }
        }
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())).is_ok()
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())) {
            Ok(index) => Some(&self.values[index].1),
            Err(_) => None,
        }
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())) {
            Ok(index) => Some(self.values.remove(index).1),
            Err(_) => None,
        }
    }

    pub fn iter(&self) -> core::slice::Iter<'_, (K, V)> {
        self.values.iter()
    }
}
impl<K, V> Eq for DictAttr<K, V>
where
    K: Eq,
    V: Eq,
{
}
impl<K, V> PartialEq for DictAttr<K, V>
where
    K: PartialEq,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
impl<K, V> fmt::Debug for DictAttr<K, V>
where
    K: fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.values.iter().map(|entry| (&entry.0, &entry.1)))
            .finish()
    }
}
impl<K, V> crate::formatter::PrettyPrint for DictAttr<K, V>
where
    K: crate::formatter::PrettyPrint,
    V: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let entries = self.values.iter().fold(Document::Empty, |acc, (k, v)| match acc {
            Document::Empty => k.render() + const_text(" = ") + v.render(),
            _ => acc + const_text(", ") + k.render() + const_text(" = ") + v.render(),
        });
        if self.values.is_empty() {
            const_text("{}")
        } else {
            const_text("{") + entries + const_text("}")
        }
    }
}
impl<K, V> crate::print::AttrPrinter for DictAttr<K, V>
where
    K: crate::formatter::PrettyPrint,
    V: crate::formatter::PrettyPrint,
{
    fn print(
        &self,
        _flags: &crate::OpPrintingFlags,
        _context: &crate::Context,
    ) -> crate::formatter::Document {
        use crate::formatter::PrettyPrint;
        self.render()
    }
}
impl<K, V> core::hash::Hash for DictAttr<K, V>
where
    K: core::hash::Hash,
    V: core::hash::Hash,
{
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        <Vec<(K, V)> as core::hash::Hash>::hash(&self.values, state);
    }
}
impl<K, V> AttributeValue for DictAttr<K, V>
where
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
    V: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
{
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self as &mut dyn Any
    }

    #[inline]
    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(self.clone())
    }
}
